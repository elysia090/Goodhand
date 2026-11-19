use crate::arm_scene::Layout;
use crate::playback::Repeat;
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
    layout: Option<Layout>,
    timings: VecDeque<Timings>,
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
            layout: None,
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

    pub fn record_frame(&mut self, layout: Layout, bytes: usize, late: bool, timing: Timings) {
        if !self.enabled {
            return;
        }
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
        let written = match self.repeat {
            Repeat::Once => format!("{}/{}", self.written, self.settings.cycle.ticks()),
            Repeat::Loop => self.written.to_string(),
        };
        Some(format!(
            "oops: grid={layout} written={written} rendered={} stale={} late_write={} dropped_write={} partial_write={} bytes={} trace_p95/max_us={} encode_p95/max_us={} write_p95/max_us={} timing_samples={} {} playback={}\n",
            self.traces,
            self.stale,
            self.late_write,
            self.dropped_write,
            self.partial_write,
            self.bytes,
            timing(|t| t.trace),
            timing(|t| t.encode),
            timing(|t| t.write),
            self.timings.len(),
            self.settings.playback_status(),
            self.repeat,
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
