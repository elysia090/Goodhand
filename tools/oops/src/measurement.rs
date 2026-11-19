use crate::arm_scene::Layout;
use crate::settings::Settings;
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
    pub traces: usize,
    pub stale: usize,
    pub late_write: usize,
    pub dropped_write: usize,
    pub partial_write: usize,
    pub bytes: usize,
    pub layout: Option<Layout>,
    timings: Vec<Timings>,
}

impl Measurement {
    pub fn new(enabled: bool, settings: Settings) -> Self {
        Self {
            enabled,
            settings,
            traces: 0,
            stale: 0,
            late_write: 0,
            dropped_write: 0,
            partial_write: 0,
            bytes: 0,
            layout: None,
            timings: Vec::with_capacity(if enabled {
                settings.cycle.ticks() as usize
            } else {
                0
            }),
        }
    }

    pub fn record(&mut self, timing: Timings) {
        if self.enabled {
            self.timings.push(timing);
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
        Some(format!(
            "oops: grid={layout} written={}/{} rendered={} stale={} late_write={} dropped_write={} partial_write={} bytes={} trace_p95/max_us={} encode_p95/max_us={} write_p95/max_us={} {}\n",
            self.timings.len(),
            self.settings.cycle.ticks(),
            self.traces,
            self.stale,
            self.late_write,
            self.dropped_write,
            self.partial_write,
            self.bytes,
            timing(|t| t.trace),
            timing(|t| t.encode),
            timing(|t| t.write),
            self.settings.playback_status(),
        ))
    }
}
