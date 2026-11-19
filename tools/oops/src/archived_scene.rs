//! Display projection of an already-reviewed raster; no geometry or shading.
use crate::archive_profile::{self, ARCHIVE_FPS, HEIGHT, POSES, WIDTH};
use crate::playback::{Cycle, Direction, FPS};
use crate::projection::Projection;
use goodhand::{RasterSpec, Sequence, ToneSequence};
use std::io;

#[cfg(goodhand_embedded_playback)]
pub const LABEL: &str = "embedded review";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Framing {
    size: (f64, f64),
    center: (f64, f64),
}

pub struct Archive {
    shape: Sequence<'static>,
    tone: ToneSequence<'static>,
    framing: Framing,
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
            framing: occupied_framing(shape).unwrap_or_else(native_framing),
            last_pose: None,
            points: vec![None; usize::from(WIDTH) * usize::from(HEIGHT)],
        })
    }

    #[cfg(goodhand_embedded_playback)]
    pub fn frame_size(&self) -> (f64, f64) {
        self.framing.size
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
        let (center_x, center_y) = self.framing.center;
        // Each output sample represents a logical dot's region. Region
        // boundaries lie halfway between dot centers, not at a tone cutoff.
        let source_index = |point: f64, axis: usize, count: u16| {
            let pitch = spec.pitch[axis] as f64;
            let index = ((point - tile_origin(spec, axis)) / pitch).floor();
            (index >= 0.0 && index < f64::from(count)).then_some(index as usize)
        };
        for (r, row) in output.chunks_exact_mut(view.cols).enumerate() {
            let y = source_index(center_y + view.y(r), 1, HEIGHT);
            for (c, cell) in row.iter_mut().enumerate() {
                let x = source_index(center_x + view.x(c), 0, WIDTH);
                *cell = y
                    .zip(x)
                    .and_then(|(y, x)| self.points[y * usize::from(WIDTH) + x]);
            }
        }
        Ok(())
    }
}

fn native_framing() -> Framing {
    let spec = RasterSpec::REFERENCE;
    let size = (spec.width as f64, spec.height as f64);
    Framing {
        size,
        center: (size.0 / 2.0, size.1 / 2.0),
    }
}

fn tile_origin(spec: RasterSpec, axis: usize) -> f64 {
    spec.origin[axis] as f64 + (spec.dot_size[axis] as f64 - 1.0) / 2.0
        - spec.pitch[axis] as f64 / 2.0
}

fn occupied_framing(shape: Sequence<'_>) -> Option<Framing> {
    // Use full source sampling tiles, not tone, with one fixed margin
    // throughout the turn. Run ends and bottom rows are exclusive.
    const MARGIN_TILES: f64 = 3.0;
    let (mut left, mut top) = (shape.width(), shape.height());
    let (mut right, mut bottom) = (0, 0);
    for pose in 0..shape.frame_count() {
        for row in shape.frame(pose)?.rows() {
            for run in row.runs() {
                left = left.min(run.start());
                right = right.max(run.end());
                top = top.min(row.y());
                bottom = bottom.max(row.y() + 1);
            }
        }
    }
    if left >= right || top >= bottom {
        return None;
    }
    let spec = RasterSpec::REFERENCE;
    let axis = |axis: usize, lo: u16, hi: u16| {
        let pitch = spec.pitch[axis] as f64;
        let center = tile_origin(spec, axis) + (f64::from(lo) + f64::from(hi)) * pitch / 2.0;
        let span = (f64::from(hi - lo) + 2.0 * MARGIN_TILES) * pitch;
        (span, center)
    };
    let (width, x) = axis(0, left, right);
    let (height, y) = axis(1, top, bottom);
    Some(Framing {
        size: (width, height),
        center: (x, y),
    })
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
        let mut archive = Archive::from_bytes(shape.leak(), tone.leak()).unwrap();
        // The projection tests specify a native-grid camera explicitly.
        archive.framing = native_framing();
        archive
    }

    #[test]
    fn framing_uses_occupied_extremes_from_the_entire_turn() {
        let mut first = vec![0; usize::from(HEIGHT)];
        first.splice(1..2, [1, 8, 2]);
        let mut late = vec![0; usize::from(HEIGHT)];
        late.splice(105..106, [1, 150, 4]);
        let mut entries = vec![first; POSES as usize];
        entries[POSES as usize - 1] = late;
        let data = asset(b"GHSH", &entries);
        let framing = occupied_framing(Sequence::from_bytes(&data).unwrap()).unwrap();
        assert_eq!(framing.size, (760.0, 555.0));
        assert_eq!(framing.center, (405.0, 307.5));
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
    fn complete_archive_fits_the_default_view_for_every_pose() {
        use crate::arm_scene::{Display, InlineRows, Layout, Size, reservation};
        use crate::settings::{CellAspect, Settings};

        let archive = Archive::embedded().unwrap();
        assert_eq!(archive.shape.frame_count(), POSES);
        assert_eq!(archive.tone.frame_count(), POSES);
        let frame = archive.framing;
        let spec = RasterSpec::REFERENCE;
        let (tile_x, tile_y) = (tile_origin(spec, 0), tile_origin(spec, 1));
        let (pitch_x, pitch_y) = (spec.pitch[0] as f64, spec.pitch[1] as f64);
        for (cols, rows) in [(80, 24), (116, 41), (120, 30), (132, 43), (290, 97)] {
            let size = Size { cols, rows };
            let display = Display {
                size,
                pixels: (0, 0),
            };
            let inline = Some(InlineRows {
                top: 1,
                below: 1 + reservation(size),
            });
            for surface in [None, inline] {
                for aspect in [0.30, 0.50, 0.85] {
                    let settings = Settings {
                        cell_aspect: CellAspect::Fixed(aspect),
                        ..Settings::default()
                    };
                    let layout = Layout::for_surface(display, settings, frame.size, surface);
                    let view = Projection::new(
                        usize::from(layout.cols),
                        usize::from(layout.rows),
                        frame.size,
                        aspect,
                        1.0,
                        settings.offset,
                    )
                    .unwrap();
                    let (dx, dy) = view.step();
                    let (x, y) = frame.center;
                    let left = x + view.x(0) - dx / 2.0;
                    let top = y + view.y(0) - dy / 2.0;
                    let right = x + view.x(view.cols - 1) + dx / 2.0;
                    let bottom = y + view.y(view.rows - 1) + dy / 2.0;
                    for pose in 0..POSES {
                        for row in archive.shape.frame(pose).unwrap().rows() {
                            for run in row.runs() {
                                assert!(left < tile_x + pitch_x * f64::from(run.start()));
                                assert!(right > tile_x + pitch_x * f64::from(run.end()));
                                assert!(top < tile_y + pitch_y * f64::from(row.y()));
                                assert!(bottom > tile_y + pitch_y * (f64::from(row.y()) + 1.0));
                            }
                        }
                    }
                }
            }
        }
    }
}
