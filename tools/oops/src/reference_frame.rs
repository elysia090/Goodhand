//! Native raster and occupancy, independent of window compositing.
use crate::archived_scene::Archive;
use goodhand::{RasterSpec, write_surface};
use std::io;

#[cfg(goodhand_native_motion)]
struct MotionArchive(goodhand::Sequence<'static>, goodhand::ToneSequence<'static>);

#[cfg(goodhand_native_motion)]
impl MotionArchive {
    fn embedded() -> io::Result<Self> {
        let shape = include_bytes!(concat!(env!("OUT_DIR"), "/native-motion-shape.ghs"));
        let tone = include_bytes!(concat!(env!("OUT_DIR"), "/native-motion-tone.ght"));
        let (shape, tone) = crate::archive_profile::validate_native_motion(shape, tone)?;
        Ok(Self(shape, tone))
    }

    fn frame(&self, pose: u32) -> io::Result<(goodhand::ShapeFrame<'static>, &'static [u8])> {
        self.0.frame(pose).zip(self.1.frame(pose)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "native phase outside the motion archive",
            )
        })
    }
}

pub(crate) struct ReferenceFrame {
    archive: Archive,
    #[cfg(goodhand_native_motion)]
    motion: Option<MotionArchive>,
    opaque: Vec<u8>,
}

impl ReferenceFrame {
    pub(crate) fn new(cycle: crate::playback::Cycle) -> io::Result<Self> {
        #[cfg(not(goodhand_native_motion))]
        let _ = cycle;
        let archive = Archive::embedded()?;
        #[cfg(goodhand_native_motion)]
        let motion = (cycle.duration.seconds() == crate::archive_profile::NATIVE_MOTION_SECONDS)
            .then(MotionArchive::embedded)
            .transpose()?;
        let out = Self {
            archive,
            #[cfg(goodhand_native_motion)]
            motion,
            opaque: Vec::new(),
        };
        #[cfg(goodhand_native_motion)]
        if let Some(motion) = &out.motion {
            use crate::archive_profile::NATIVE_MOTION_POSES;
            if out.occupied_bounds()? != occupied_bounds(NATIVE_MOTION_POSES, |i| motion.frame(i))?
            {
                return Err(io::Error::other(
                    "native motion archive changes the reviewed aperture",
                ));
            }
        }
        Ok(out)
    }

    pub(crate) fn tick_for(
        &self,
        timeline: crate::playback::Timeline,
        elapsed: std::time::Duration,
    ) -> Option<u64> {
        #[cfg(goodhand_native_motion)]
        if self.motion.is_some() {
            return motion_tick(timeline, elapsed);
        }
        timeline.sample(elapsed).map(|slot| slot.phase.tick)
    }

    pub(crate) fn pose_at_tick(&self, cycle: crate::playback::Cycle, tick: u64) -> Option<u32> {
        #[cfg(goodhand_native_motion)]
        if self.motion.is_some() {
            return motion_pose_at_tick(cycle, tick);
        }
        crate::archived_scene::pose_at_tick(cycle, tick)
    }

    /// Bounds over the full turn, in source pixels; the upper bound is exclusive.
    pub(crate) fn occupied_bounds(&self) -> io::Result<([i32; 2], [i32; 2])> {
        occupied_bounds(crate::archive_profile::POSES, |pose| {
            self.archive.frame(pose)
        })
    }

    pub(crate) fn render(
        &mut self,
        pose: u32,
        pixels: &mut [u8],
        alpha: Option<&mut [u8]>,
    ) -> io::Result<()> {
        #[cfg(goodhand_native_motion)]
        if let Some(motion) = &self.motion {
            // The additional phases already carry W5 point tones. Do not apply
            // the anchor correction again or derive coverage from gray values.
            let (shape, gray) = motion.frame(pose)?;
            return write_raster(shape, gray, &mut self.opaque, pixels, alpha);
        }
        let (shape, stock) = self.archive.frame(pose)?;
        #[cfg(goodhand_w5_profile)]
        let corrected = shading::correct(&mut self.archive, pose, shape, stock)?;
        #[cfg(goodhand_w5_profile)]
        let stock = corrected.as_deref().unwrap_or(stock);
        write_raster(shape, stock, &mut self.opaque, pixels, alpha)
    }
}

#[cfg(any(goodhand_native_motion, test))]
pub(super) fn motion_pose_at_tick(cycle: crate::playback::Cycle, tick: u64) -> Option<u32> {
    use crate::archive_profile::{NATIVE_MOTION_POSES, NATIVE_MOTION_SECONDS};
    use crate::playback::Direction;

    let ticks = u64::from(NATIVE_MOTION_POSES);
    if cycle.duration.seconds() != NATIVE_MOTION_SECONDS || tick >= ticks {
        return None;
    }
    // Each 18 Hz tick grows the source phase by precisely 8/15ths of a
    // reviewed pose. Forward uses the same archive lattice in reverse order.
    Some(match cycle.direction {
        Direction::Reverse => tick as u32,
        Direction::Forward => ((ticks - tick) % ticks) as u32,
    })
}

#[cfg(any(goodhand_native_motion, test))]
pub(super) fn motion_tick(
    timeline: crate::playback::Timeline,
    elapsed: std::time::Duration,
) -> Option<u64> {
    use crate::archive_profile::NATIVE_MOTION_FPS;
    use crate::playback::Repeat;

    // Compare the original absolute epoch; never chain rounded 55.56-ms
    // timers. Reduce before multiplication to bound large durations.
    let seconds = timeline.cycle.length().as_secs();
    if timeline.repeat == Repeat::Once && elapsed.as_secs() >= seconds {
        return None;
    }
    let fps = u64::from(NATIVE_MOTION_FPS);
    Some(
        (elapsed.as_secs() % seconds) * fps
            + u64::from(elapsed.subsec_nanos()) * fps / 1_000_000_000,
    )
}

fn occupied_bounds(
    count: u32,
    frame: impl Fn(u32) -> io::Result<(goodhand::ShapeFrame<'static>, &'static [u8])>,
) -> io::Result<([i32; 2], [i32; 2])> {
    let spec = RasterSpec::REFERENCE;
    let mut lo = [usize::MAX; 2];
    let mut hi = [0; 2];
    for pose in 0..count {
        let (shape, _) = frame(pose)?;
        for row in shape.rows() {
            let y = usize::from(row.y());
            for run in row.runs() {
                lo = [lo[0].min(usize::from(run.start())), lo[1].min(y)];
                hi = [hi[0].max(usize::from(run.end())), hi[1].max(y + 1)];
            }
        }
    }
    if lo[0] == usize::MAX {
        return Err(io::Error::other("empty reference artwork"));
    }
    Ok((
        std::array::from_fn(|i| (spec.origin[i] + lo[i] * spec.pitch[i]) as i32),
        std::array::from_fn(|i| {
            (spec.origin[i] + (hi[i] - 1) * spec.pitch[i] + spec.dot_size[i]) as i32
        }),
    ))
}

fn write_raster(
    shape: goodhand::ShapeFrame<'_>,
    gray: &[u8],
    opaque: &mut Vec<u8>,
    pixels: &mut [u8],
    alpha: Option<&mut [u8]>,
) -> io::Result<()> {
    let spec = RasterSpec::REFERENCE;
    write_surface(shape, gray, spec, pixels).map_err(io::Error::other)?;
    if let Some(alpha) = alpha {
        opaque.resize(gray.len(), 255);
        write_surface(shape, opaque, spec, alpha).map_err(io::Error::other)?;
    }
    Ok(())
}

#[cfg(goodhand_w5_profile)]
mod shading {
    //! Lift the reviewed change, not the mean-gray character image.
    use super::*;
    use crate::archive_profile::{HEIGHT, WIDTH};
    use crate::arm_scene::w5_tones::{REFERENCE_COLS, REFERENCE_ROWS, apply_reference};
    use crate::dots::Renderer;
    use crate::projection::Projection;
    use crate::settings::Offset;
    use goodhand::ShapeFrame;

    const COLS: usize = REFERENCE_COLS;
    const ROWS: usize = REFERENCE_ROWS;

    pub(super) fn correct(
        archive: &mut Archive,
        pose: u32,
        shape: ShapeFrame<'_>,
        stock: &[u8],
    ) -> io::Result<Option<Vec<u8>>> {
        let (sx, sy) = (2 * COLS, 4 * ROWS);
        let view = Projection::new(
            sx,
            sy,
            archive.frame_size(),
            1.0,
            1.0,
            [Offset::default(); 2],
        )?;
        let mut samples = vec![None; sx * sy];
        archive.project_pose(pose, view, &mut samples)?;
        let mut cells = vec![None; COLS * ROWS];
        Renderer::Braille.pack(&samples, COLS, &mut cells);
        let stock_cells = cells.clone();
        apply_reference(pose, &mut cells);
        let delta: Vec<i16> = stock_cells
            .iter()
            .zip(cells)
            .map(|(stock, graded)| match (stock, graded) {
                (Some(old), Some(new)) => i16::from(new.gray) - i16::from(old.gray),
                _ => 0,
            })
            .collect();
        if delta.iter().all(|&d| d == 0) {
            return Ok(None);
        }

        let (center, span, source_x, source_y) = archive.grid_lattice();
        let width = usize::from(WIDTH);
        let height = usize::from(HEIGHT);
        let mut covered = vec![false; width * height];
        for (cell, &d) in delta.iter().enumerate() {
            if d != 0 {
                let (col, row) = (cell % COLS, cell / COLS);
                for &j in &source_y[row * 4..row * 4 + 4] {
                    for &i in &source_x[col * 2..col * 2 + 2] {
                        if i < width && j < height {
                            covered[j * width + i] = true;
                        }
                    }
                }
            }
        }
        let step = (span[0] / sx as f64).max(span[1] / sy as f64);
        let mut output = Vec::with_capacity(stock.len());
        let mut from = stock.iter().copied();
        for row in shape.rows() {
            let y = usize::from(row.y());
            for run in row.runs() {
                for x in usize::from(run.start())..usize::from(run.end()) {
                    let gray = i16::from(from.next().expect("validated occupied gray count"));
                    let correction = if covered[y * width + x] {
                        let cell_x =
                            (x as f64 + 0.5 - center[0]) / (2.0 * step) + (COLS - 1) as f64 / 2.0;
                        let cell_y =
                            (y as f64 + 0.5 - center[1]) / (4.0 * step) + (ROWS - 1) as f64 / 2.0;
                        interpolate(&delta, cell_x, cell_y)
                    } else {
                        0
                    };
                    output.push((gray + correction).clamp(0, 255) as u8);
                }
            }
        }
        Ok(Some(output))
    }

    /// Interpolate adjustments; each original point retains its own input gray.
    fn interpolate(delta: &[i16], x: f64, y: f64) -> i16 {
        let (a, b) = (x.floor(), y.floor());
        let (u, v) = (x - a, y - b);
        let read = |c: i32, r: i32| {
            let c = c.clamp(0, COLS as i32 - 1) as usize;
            let r = r.clamp(0, ROWS as i32 - 1) as usize;
            f64::from(delta[r * COLS + c])
        };
        let (a, b) = (a as i32, b as i32);
        let blended = (1.0 - u) * (1.0 - v) * read(a, b)
            + u * (1.0 - v) * read(a + 1, b)
            + (1.0 - u) * v * read(a, b + 1)
            + u * v * read(a + 1, b + 1);
        blended.round_ties_even() as i16
    }
}
