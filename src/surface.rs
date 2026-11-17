//! Place separate occupied gray on a pixel surface, never terminal cells.
//!
//! This performs no shading, palette recovery, resampling, fitting, or I/O.
//! The reference profile matches the offline renderer's uniform 2x2 dots.

use crate::frame::ShapeFrame;
use core::fmt;

/// Pixel-only placement of a fixed logical grid. Every pair is [x, y].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterSpec {
    pub width: usize,
    pub height: usize,
    pub origin: [usize; 2],
    pub pitch: [usize; 2],
    pub dot_size: [usize; 2],
}

impl RasterSpec {
    /// 1000x640 pixel canvas, not a 1000x640 terminal-cell rectangle.
    pub const REFERENCE: Self = Self {
        width: 1000,
        height: 640,
        origin: [2, 42],
        pitch: [5, 5],
        dot_size: [2, 2],
    };

    pub fn required_len(self) -> Result<usize, SurfaceError> {
        if self.width == 0
            || self.height == 0
            || self.pitch.contains(&0)
            || self.dot_size.contains(&0)
            || self.dot_size[0] > self.pitch[0]
            || self.dot_size[1] > self.pitch[1]
        {
            return Err(SurfaceError::InvalidRaster);
        }
        self.width
            .checked_mul(self.height)
            .ok_or(SurfaceError::SizeOverflow)
    }

    fn validate_grid(self, shape: ShapeFrame<'_>) -> Result<(), SurfaceError> {
        for (axis, points) in [shape.width(), shape.height()].into_iter().enumerate() {
            let end = (usize::from(points) - 1)
                .checked_mul(self.pitch[axis])
                .and_then(|span| self.origin[axis].checked_add(span))
                .and_then(|last| last.checked_add(self.dot_size[axis]))
                .ok_or(SurfaceError::SizeOverflow)?;
            if end > [self.width, self.height][axis] {
                return Err(SurfaceError::GridOutsideCanvas);
            }
        }
        Ok(())
    }
}

/// Clear and redraw the required prefix of a caller-owned gray8 pixel buffer.
///
/// Occupied gray follows shape's row/run order; 0 is a valid occupied value.
/// The whole logical grid must fit even if its outer points are unoccupied.
/// All inputs are checked before any write. Errors leave the destination alone;
/// success leaves its unused tail alone. No allocation or shape mutation occurs.
/// This full redraw clears P pixels, then writes dot area for each occupied point.
pub fn write_surface(
    shape: ShapeFrame<'_>,
    occupied_gray: &[u8],
    spec: RasterSpec,
    destination: &mut [u8],
) -> Result<(), SurfaceError> {
    let required = spec.required_len()?;
    spec.validate_grid(shape)?;
    let expected = shape
        .rows()
        .flat_map(|row| row.runs())
        .map(|run| usize::from(run.length()))
        .sum();
    if occupied_gray.len() != expected {
        return Err(SurfaceError::GrayCountMismatch {
            expected,
            actual: occupied_gray.len(),
        });
    }
    if destination.len() < required {
        return Err(SurfaceError::BufferTooShort {
            required,
            actual: destination.len(),
        });
    }
    let destination = &mut destination[..required];
    destination.fill(0);
    let mut tone = 0;
    for row in shape.rows() {
        let y = spec.origin[1] + usize::from(row.y()) * spec.pitch[1];
        for run in row.runs() {
            for point_x in run.start()..run.end() {
                let gray = occupied_gray[tone];
                tone += 1;
                let x = spec.origin[0] + usize::from(point_x) * spec.pitch[0];
                for dy in 0..spec.dot_size[1] {
                    let start = (y + dy) * spec.width + x;
                    destination[start..start + spec.dot_size[0]].fill(gray);
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceError {
    InvalidRaster,
    SizeOverflow,
    GridOutsideCanvas,
    GrayCountMismatch { expected: usize, actual: usize },
    BufferTooShort { required: usize, actual: usize },
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid reference surface input: {self:?}")
    }
}

impl std::error::Error for SurfaceError {}
