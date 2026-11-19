//! Display projection of an already-reviewed raster; no geometry or shading.
use crate::archive_profile::{self, ARCHIVE_FPS, HEIGHT, POSES, WIDTH};
use crate::playback::{Cycle, Direction, FPS};
use crate::projection::Projection;
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

    fn select(&mut self, index: u32) -> io::Result<()> {
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

    pub fn project(
        &mut self,
        cycle: Cycle,
        tick: u64,
        view: Projection,
        output: &mut [Option<u8>],
    ) -> io::Result<()> {
        if view.cols * view.rows != output.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive viewport does not match output length",
            ));
        }
        let index =
            pose_at_tick(cycle, tick).ok_or_else(|| io::Error::other("invalid playback tick"))?;
        self.select(index)?;
        let spec = RasterSpec::REFERENCE;
        let (w, h) = (spec.width as f64, spec.height as f64);
        // Each terminal bullet represents a logical dot's region. Region
        // boundaries lie halfway between dot centers, not at a tone cutoff.
        let source_index = |point: f64, axis: usize, count: u16| {
            let pitch = spec.pitch[axis] as f64;
            let origin =
                spec.origin[axis] as f64 + (spec.dot_size[axis] as f64 - 1.0) * 0.5 - pitch * 0.5;
            let index = ((point - origin) / pitch).floor();
            (index >= 0.0 && index < f64::from(count)).then_some(index as usize)
        };
        for (r, row) in output.chunks_exact_mut(view.cols).enumerate() {
            let y = source_index(h / 2.0 + view.y(r), 1, HEIGHT);
            for (c, cell) in row.iter_mut().enumerate() {
                let x = source_index(w / 2.0 + view.x(c), 0, WIDTH);
                *cell = y
                    .zip(x)
                    .and_then(|(y, x)| self.points[y * usize::from(WIDTH) + x]);
            }
        }
        Ok(())
    }
}

fn pose_at_tick(cycle: Cycle, tick: u64) -> Option<u32> {
    let ticks = cycle.ticks();
    if tick >= ticks {
        return None;
    }
    // Invert the half-up review boundaries: pose p starts at
    // round(p * ticks / POSES). 30 seconds preserves the exact 3/2 holds.
    let pose = ((u64::from(POSES) * (2 * tick + 1) - 1) / (2 * ticks)) as u32;
    Some(match cycle.direction {
        Direction::Reverse => pose,
        Direction::Forward => (POSES - pose) % POSES,
    })
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
    fn default_pose_schedule_keeps_every_original_hold() {
        let cycle = Cycle::default();
        assert_eq!(cycle.ticks(), 720);
        let mut tick = 0;
        for pose in 0..POSES {
            for _ in 0..if pose % 2 == 0 { 3 } else { 2 } {
                assert_eq!(pose_at_tick(cycle, tick), Some(pose));
                tick += 1;
            }
        }
        assert_eq!(tick, 720);
        assert_eq!(pose_at_tick(cycle, 720), None);
        assert_eq!(pose_at_tick(cycle, u64::MAX), None);
    }

    #[test]
    fn all_durations_keep_each_pose_and_both_directions_start_at_zero() {
        let poses = u64::from(POSES);
        for seconds in 12..=60 {
            let reverse = Cycle {
                duration: seconds.to_string().parse().unwrap(),
                ..Cycle::default()
            };
            let forward = Cycle {
                direction: Direction::Forward,
                ..reverse
            };
            let ticks = reverse.ticks();
            let boundary = |pose| (pose * ticks + poses / 2) / poses;
            for pose in 0..poses {
                let start = boundary(pose);
                let end = boundary(pose + 1);
                assert!(start < end);
                for tick in start..end {
                    assert_eq!(pose_at_tick(reverse, tick), Some(pose as u32));
                    assert_eq!(
                        pose_at_tick(forward, tick),
                        Some(((poses - pose) % poses) as u32)
                    );
                }
            }
            assert_eq!(pose_at_tick(forward, 0), Some(0));
            assert_eq!(pose_at_tick(forward, ticks - 1), Some(1));
            assert_eq!(pose_at_tick(forward, ticks), None);
        }
    }

    fn grid(cols: usize, rows: usize, aspect: f64, zoom: f64, x: &str, y: &str) -> Projection {
        Projection::new(
            cols,
            rows,
            (1000.0, 640.0),
            aspect,
            zoom,
            [x.parse().unwrap(), y.parse().unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn review_projection_preserves_black_occupancy_and_maps_native_grid_centers() {
        let mut archive = fixture();
        let cycle = Cycle::default();
        // The first source row lies below eight rows of output margin.
        let view = grid(200, 128, 1.0, 1.0, "0", "0");
        let mut pixels = vec![None; 200 * 128];
        archive.project(cycle, 0, view, &mut pixels).unwrap();
        assert_eq!(
            pixels[9 * 200 + 7..9 * 200 + 11],
            [None, Some(0), Some(79), None]
        );
        assert_eq!(pixels.iter().filter(|p| p.is_some()).count(), 2);
        archive.project(cycle, 2, view, &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(79));
        archive.project(cycle, 3, view, &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(190));
        // Forward starts at zero, runs through pose 287, then ends at pose 1.
        let forward = Cycle {
            direction: Direction::Forward,
            ..cycle
        };
        archive.project(forward, 3, view, &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(79));
        archive.project(forward, 719, view, &mut pixels).unwrap();
        assert_eq!(pixels[9 * 200 + 9], Some(190));
        assert!(archive.project(cycle, 720, view, &mut pixels).is_err());
    }

    #[test]
    fn crop_and_translation_do_not_stretch_either_axis() {
        let mut archive = fixture();
        let mut pixels = vec![None; 200 * 64];
        let cycle = Cycle::default();
        let view = grid(200, 64, 0.5, 1.0, "0", "0");
        archive.project(cycle, 0, view, &mut pixels).unwrap();
        assert_eq!(
            pixels[4 * 200 + 7..4 * 200 + 11],
            [None, Some(0), Some(79), None]
        );
        let shifted = grid(200, 64, 0.5, 1.0, "25", "25");
        archive.project(cycle, 0, shifted, &mut pixels).unwrap();
        assert_eq!(
            pixels[20 * 200 + 57..20 * 200 + 61],
            [None, Some(0), Some(79), None]
        );
        assert_eq!(pixels.iter().filter(|p| p.is_some()).count(), 2);
        let cropped = grid(200, 64, 0.5, 1.0, "-25", "-25");
        archive.project(cycle, 0, cropped, &mut pixels).unwrap();
        assert!(pixels.iter().all(Option::is_none));

        let zoomed_out = grid(200, 128, 1.0, 0.5, "0", "0");
        let mut smaller = vec![None; 200 * 128];
        archive.project(cycle, 0, zoomed_out, &mut smaller).unwrap();
        assert_eq!(
            smaller[36 * 200 + 53..36 * 200 + 56],
            [None, Some(79), None]
        );
    }

    #[test]
    fn invalid_viewport_returns_without_overwriting_output() {
        let mut archive = fixture();
        let mut out = [Some(37)];
        for (cols, rows, aspect, zoom) in
            [(0, 1, 0.5, 1.0), (1, 1, f64::NAN, 1.0), (1, 1, 0.5, 0.0)]
        {
            assert!(
                Projection::new(
                    cols,
                    rows,
                    (1000.0, 640.0),
                    aspect,
                    zoom,
                    Default::default()
                )
                .is_err()
            );
        }
        assert!(
            archive
                .project(
                    Cycle::default(),
                    0,
                    grid(2, 1, 0.5, 1.0, "0", "0"),
                    &mut out
                )
                .is_err()
        );
        assert_eq!(out, [Some(37)]);
    }

    #[cfg(goodhand_embedded_playback)]
    #[test]
    fn selected_build_has_a_complete_archive() {
        let archive = Archive::embedded().unwrap();
        assert_eq!(archive.shape.frame_count(), POSES);
        assert_eq!(archive.tone.frame_count(), POSES);
    }
}
