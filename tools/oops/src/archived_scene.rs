//! Display projection of an already-reviewed raster; no geometry or shading.
use crate::archive_profile::{self, ARCHIVE_FPS, HEIGHT, POSES, WIDTH};
use crate::playback::{FPS, TICKS};
use goodhand::{RasterSpec, Sequence, ToneSequence};
use std::io;

#[cfg(goodhand_embedded_playback)]
pub const LABEL: &str = "embedded review";

pub struct Archive {
    shape: Sequence<'static>,
    tone: ToneSequence<'static>,
    last_pose: Option<u32>,
    points: Vec<Option<u8>>,
}

impl Archive {
    #[cfg(goodhand_embedded_playback)]
    pub fn embedded() -> io::Result<Self> {
        let shape = include_bytes!(concat!(env!("OUT_DIR"), "/shape.ghs"));
        let tone = include_bytes!(concat!(env!("OUT_DIR"), "/tone.ght"));
        Self::from_bytes(shape, tone)
    }

    fn from_bytes(shape: &'static [u8], tone: &'static [u8]) -> io::Result<Self> {
        if u64::from(ARCHIVE_FPS) != FPS {
            return Err(io::Error::other(
                "archive frame rate does not match the player",
            ));
        }
        let (shape, tone) = archive_profile::validate(shape, tone)?;
        Ok(Self {
            shape,
            tone,
            last_pose: None,
            points: vec![None; usize::from(WIDTH) * usize::from(HEIGHT)],
        })
    }

    fn select(&mut self, tick: u64) -> io::Result<()> {
        let index = pose_at_tick(tick).ok_or_else(|| io::Error::other("invalid playback tick"))?;
        if self.last_pose == Some(index) {
            return Ok(());
        }
        let shape = self
            .shape
            .frame(index)
            .ok_or_else(|| io::Error::other("archive is missing a shape frame"))?;
        let mut tone = self
            .tone
            .frame(index)
            .ok_or_else(|| io::Error::other("archive is missing a tone frame"))?;
        self.points.fill(None);
        for row in shape.rows() {
            let base = usize::from(row.y()) * usize::from(WIDTH);
            for run in row.runs() {
                let start = base + usize::from(run.start());
                let end = base + usize::from(run.end());
                let (values, rest) = tone.split_at(end - start);
                for (point, value) in self.points[start..end].iter_mut().zip(values) {
                    *point = Some(*value);
                }
                tone = rest;
            }
        }
        self.last_pose = Some(index);
        Ok(())
    }

    pub fn project(&mut self, tick: u64, view: View, output: &mut [Option<u8>]) -> io::Result<()> {
        if view.cols == 0
            || view.rows == 0
            || view.cols.checked_mul(view.rows) != Some(output.len())
            || !view.aspect.is_finite()
            || view.aspect <= 0.0
            || !view.zoom.is_finite()
            || view.zoom <= 0.0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid archive viewport",
            ));
        }
        self.select(tick)?;
        let spec = RasterSpec::REFERENCE;
        let (w, h) = (spec.width as f64, spec.height as f64);
        // A single physical scale on both axes. Fitting the full source
        // canvas keeps its margins; zoom crops around the same center.
        let dy = (h / view.rows as f64).max(w / (view.cols as f64 * view.aspect)) / view.zoom;
        let dx = dy * view.aspect;
        // Each terminal bullet represents a logical dot's region. Region
        // boundaries lie halfway between dot centers, not at a tone cutoff.
        let source_index = |point: f64, axis: usize, count: u16| {
            let pitch = spec.pitch[axis] as f64;
            let origin =
                spec.origin[axis] as f64 + (spec.dot_size[axis] as f64 - 1.0) * 0.5 - pitch * 0.5;
            let index = ((point - origin) / pitch).floor();
            (index >= 0.0 && index < f64::from(count)).then_some(index as usize)
        };
        let center = |index, count, step, size| {
            size / 2.0 + (index as f64 + 0.5 - count as f64 / 2.0) * step
        };
        for (r, row) in output.chunks_exact_mut(view.cols).enumerate() {
            let y = source_index(center(r, view.rows, dy, h), 1, HEIGHT);
            for (c, cell) in row.iter_mut().enumerate() {
                let x = source_index(center(c, view.cols, dx, w), 0, WIDTH);
                *cell = y
                    .zip(x)
                    .and_then(|(y, x)| self.points[y * usize::from(WIDTH) + x]);
            }
        }
        Ok(())
    }
}

pub struct View {
    pub cols: usize,
    pub rows: usize,
    pub aspect: f64,
    pub zoom: f64,
}

pub(super) fn pose_at_tick(tick: u64) -> Option<u32> {
    // Invert the half-up pose boundaries used by the fixed-grid review.
    // Unlike a linear interpolation, this holds the original poses for
    // alternating three and two 24 Hz ticks in the exact 30-second cycle.
    (tick < TICKS).then(|| ((u64::from(POSES) * (2 * tick + 1) - 1) / (2 * TICKS)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(magic: &[u8; 4], entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(magic);
        for value in [1, WIDTH, HEIGHT, ARCHIVE_FPS] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&POSES.to_le_bytes());
        let mut offset = 0u32;
        bytes.extend_from_slice(&offset.to_le_bytes());
        for entry in entries {
            offset += entry.len() as u32;
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        for entry in entries {
            bytes.extend_from_slice(entry);
        }
        bytes
    }

    fn fixture() -> Archive {
        let mut data = vec![0; usize::from(HEIGHT)];
        data.splice(1..2, [1, 8, 2]);
        let shape = asset(b"GHSH", &vec![data; POSES as usize]);
        let mut frames = vec![vec![0, 79]; POSES as usize];
        frames[1] = vec![0, 190];
        let tone = asset(b"GHT8", &frames);
        Archive::from_bytes(shape.leak(), tone.leak()).unwrap()
    }

    #[test]
    fn every_pose_uses_the_exact_review_clock_and_ends_at_tick_720() {
        assert_eq!(TICKS, 720);
        let mut progression = Vec::new();
        for tick in 0..TICKS {
            progression.push(pose_at_tick(tick).unwrap());
        }
        let mut next = 0;
        for p in 0..POSES {
            let hold = if p % 2 == 0 { 3 } else { 2 };
            assert_eq!(progression[next..next + hold], vec![p; hold]);
            next += hold;
        }
        assert_eq!(next, 720);
        assert_eq!(pose_at_tick(720), None);
        assert_eq!(pose_at_tick(u64::MAX), None);
    }

    #[test]
    fn review_projection_preserves_black_occupancy_and_maps_native_grid_centers() {
        let mut archive = fixture();
        // 1000×640 canvas sampled at 5px square cells. The first source
        // row is below eight canvas rows of margin, and row 1 is at 9.
        let view = || View {
            cols: 200,
            rows: 128,
            aspect: 1.0,
            zoom: 1.0,
        };
        let mut pixels = vec![None; 200 * 128];
        archive.project(0, view(), &mut pixels).unwrap();
        assert_eq!(
            pixels[9 * 200 + 7..9 * 200 + 11],
            [None, Some(0), Some(79), None]
        );
        assert_eq!(pixels.iter().filter(|p| p.is_some()).count(), 2);
        archive.project(2, view(), &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(79));
        archive.project(3, view(), &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(190));
        // Resizing/revisiting earlier ticks cannot retain a stale pose.
        archive.project(0, view(), &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(79));
        assert!(archive.project(720, view(), &mut pixels).is_err());
    }

    #[test]
    fn non_square_cells_and_zoom_use_one_physical_scale_about_the_canvas_center() {
        let mut archive = fixture();
        let mut pixels = vec![None; 200 * 64];
        let view = View {
            cols: 200,
            rows: 64,
            aspect: 0.5,
            zoom: 1.0,
        };
        archive.project(0, view, &mut pixels).unwrap();
        assert_eq!(
            pixels[4 * 200 + 7..4 * 200 + 11],
            [None, Some(0), Some(79), None]
        );
        let zoomed_out = View {
            cols: 200,
            rows: 128,
            aspect: 1.0,
            zoom: 0.5,
        };
        let mut smaller = vec![None; 200 * 128];
        archive.project(0, zoomed_out, &mut smaller).unwrap();
        assert_eq!(
            smaller[36 * 200 + 53..36 * 200 + 56],
            [None, Some(79), None]
        );
    }

    #[test]
    fn invalid_viewport_returns_without_overwriting_output() {
        let mut archive = fixture();
        let mut out = [Some(37)];
        for (cols, rows, aspect, zoom) in [
            (0, 1, 0.5, 1.0),
            (2, 1, 0.5, 1.0),
            (1, 1, f64::NAN, 1.0),
            (1, 1, 0.5, 0.0),
        ] {
            let view = View {
                cols,
                rows,
                aspect,
                zoom,
            };
            assert!(archive.project(0, view, &mut out).is_err());
            assert_eq!(out, [Some(37)]);
        }
    }

    #[cfg(goodhand_embedded_playback)]
    #[test]
    fn selected_build_has_a_complete_archive() {
        let archive = Archive::embedded().unwrap();
        assert_eq!(archive.shape.frame_count(), POSES);
        assert_eq!(archive.tone.frame_count(), POSES);
    }
}
