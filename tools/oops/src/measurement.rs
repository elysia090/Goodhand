use crate::arm_scene::Layout;
use crate::playback::{Phase, Repeat};
use crate::settings::Settings;
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Timings {
    pub trace: Duration,
    pub encode: Duration,
    pub write: Duration,
}

pub struct Measurement {
    enabled: bool,
    settings: Settings,
    repeat: Repeat,
    traces: u64,
    stale: u64,
    late_write: u64,
    dropped_write: u64,
    partial_write: u64,
    bytes: u64,
    written: u64,
    holds: u64,
    layout: Option<Layout>,
    timings: VecDeque<Timings>,
    first: Option<Phase>,
    last: Option<Phase>,
    first_join: Option<(Phase, Phase)>,
    max_gap: u64,
    order_errors: u64,
}

impl Measurement {
    pub fn new(enabled: bool, settings: Settings, repeat: Repeat) -> Self {
        Self {
            enabled,
            settings,
            repeat,
            traces: 0,
            stale: 0,
            late_write: 0,
            dropped_write: 0,
            partial_write: 0,
            bytes: 0,
            written: 0,
            holds: 0,
            layout: None,
            first: None,
            last: None,
            first_join: None,
            max_gap: 0,
            order_errors: 0,
            timings: VecDeque::with_capacity(if enabled {
                settings.cycle.ticks() as usize
            } else {
                0
            }),
        }
    }

    pub fn record_trace(&mut self) {
        self.traces = self.traces.saturating_add(1);
    }

    pub fn record_stale(&mut self) {
        self.stale = self.stale.saturating_add(1);
    }

    pub fn record_drop(&mut self, partial: bool) {
        self.dropped_write = self.dropped_write.saturating_add(1);
        self.partial_write = self.partial_write.saturating_add(u64::from(partial));
    }

    pub fn record_frame(
        &mut self,
        phase: Phase,
        layout: Layout,
        bytes: usize,
        late: bool,
        timing: Timings,
    ) {
        if !self.enabled {
            return;
        }
        self.record_phase(phase);
        self.written = self.written.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes as u64);
        self.layout = Some(layout);
        self.late_write = self.late_write.saturating_add(u64::from(late));
        // Quantiles are a bounded recent sample, counters cover the full run.
        if self.timings.len() == self.settings.cycle.ticks() as usize {
            self.timings.pop_front();
        }
        self.timings.push_back(timing);
    }

    pub fn record_hold(&mut self, phase: Phase) {
        if self.enabled {
            self.record_phase(phase);
            self.holds = self.holds.saturating_add(1);
        }
    }

    fn record_phase(&mut self, phase: Phase) {
        self.first.get_or_insert(phase);
        if let Some(previous) = self.last.replace(phase) {
            if previous >= phase {
                self.order_errors = self.order_errors.saturating_add(1);
            } else {
                if previous.turn != phase.turn && self.first_join.is_none() {
                    self.first_join = Some((previous, phase));
                }
                let gap = u128::from(phase.turn - previous.turn)
                    * u128::from(self.settings.cycle.ticks())
                    + u128::from(phase.tick)
                    - u128::from(previous.tick);
                self.max_gap = self.max_gap.max(u64::try_from(gap).unwrap_or(u64::MAX));
            }
        }
    }

    pub fn report(&self) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let layout = self.layout.map_or("none".into(), |l| {
            format!(
                "{}x{} aspect={:.4} pitch={} zoom={}%",
                l.cols,
                l.rows,
                l.aspect,
                l.pitch.label(),
                l.settings.zoom.percent(),
            )
        });
        let timing = |read: fn(&Timings) -> Duration| {
            let mut times: Vec<_> = self
                .timings
                .iter()
                .map(|item| read(item).as_micros())
                .collect();
            times.sort_unstable();
            let p95 = times
                .get((times.len() * 95).div_ceil(100).saturating_sub(1))
                .copied()
                .unwrap_or(0);
            format!("{}/{}", p95, times.last().copied().unwrap_or(0))
        };
        let handled = self.written.saturating_add(self.holds);
        let handled = match self.repeat {
            Repeat::Once => format!("{handled}/{}", self.settings.cycle.ticks()),
            Repeat::Loop => handled.to_string(),
        };
        let phase_label = |phase: Option<Phase>| {
            phase.map_or_else(|| "none".into(), |p| format!("{}:{}", p.turn, p.tick))
        };
        let first = phase_label(self.first);
        let last = phase_label(self.last);
        let join = self.first_join.map_or_else(
            || "none".into(),
            |(before, after)| format!("{}-{}", phase_label(Some(before)), phase_label(Some(after))),
        );
        Some(format!(
            "oops: grid={layout} written={} held={} handled={handled} rendered={} stale={} late_write={} dropped_write={} partial_write={} bytes={} trace_p95/max_us={} encode_p95/max_us={} prep_p95/max_us={} write_p95/max_us={} timing_samples={} {} playback={} first={first} last={last} join={join} max_gap={} order_errors={}\n",
            self.written,
            self.holds,
            self.traces,
            self.stale,
            self.late_write,
            self.dropped_write,
            self.partial_write,
            self.bytes,
            timing(|t| t.trace),
            timing(|t| t.encode),
            timing(|t| t.trace + t.encode),
            timing(|t| t.write),
            self.timings.len(),
            self.settings.playback_status(),
            self.repeat,
            self.max_gap,
            self.order_errors,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arm_scene::{Display, Size};

    #[test]
    fn timing_window_is_bounded_and_counters_saturate() {
        let settings = Settings::default();
        let mut stats = Measurement::new(true, settings, Repeat::Loop);
        let layout = Layout::fit(
            Display {
                size: Size { cols: 92, rows: 45 },
                pixels: (0, 0),
            },
            settings,
        );
        for i in 0..2 * settings.cycle.ticks() {
            stats.record_frame(
                Phase {
                    turn: i / settings.cycle.ticks(),
                    tick: i % settings.cycle.ticks(),
                },
                layout,
                100,
                false,
                Timings {
                    trace: Duration::from_micros(i),
                    encode: Duration::ZERO,
                    write: Duration::ZERO,
                },
            );
        }
        assert_eq!(stats.written, 1440);
        assert_eq!(stats.timings.len(), 720);
        assert_eq!(
            stats.timings.front().unwrap().trace,
            Duration::from_micros(720)
        );
        stats.traces = u64::MAX;
        stats.record_trace();
        assert_eq!(stats.traces, u64::MAX);
    }
}
