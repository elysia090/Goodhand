//! Immutable row runs and optional expansion into a caller-owned 0/1 mask.

use core::fmt;

/// A half-open occupied interval in one logical grid row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    start: u8,
    len: u8,
}

impl Run {
    pub fn start(self) -> u16 {
        u16::from(self.start)
    }

    pub fn length(self) -> u16 {
        u16::from(self.len)
    }

    pub fn end(self) -> u16 {
        self.start() + self.length()
    }
}

/// Borrowed occupied intervals in one row, ordered from left to right.
#[derive(Clone, Debug)]
pub struct Runs<'a> {
    encoded: core::slice::ChunksExact<'a, u8>,
}

impl Iterator for Runs<'_> {
    type Item = Run;

    fn next(&mut self) -> Option<Self::Item> {
        self.encoded.next().map(|pair| Run {
            start: pair[0],
            len: pair[1],
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.encoded.size_hint()
    }
}

impl ExactSizeIterator for Runs<'_> {}
impl core::iter::FusedIterator for Runs<'_> {}

/// One row of a validated shape. Empty rows are present too.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    y: u16,
    encoded_runs: &'a [u8],
}

impl<'a> Row<'a> {
    pub fn y(self) -> u16 {
        self.y
    }

    pub fn runs(self) -> Runs<'a> {
        Runs {
            encoded: self.encoded_runs.chunks_exact(2),
        }
    }
}

/// Borrowed rows in top-to-bottom image-coordinate order.
#[derive(Clone, Debug)]
pub struct Rows<'a> {
    remaining: &'a [u8],
    next_y: u16,
    height: u16,
}

impl<'a> Iterator for Rows<'a> {
    type Item = Row<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_y == self.height {
            return None;
        }

        // Only Sequence's fully validated, immutably borrowed bytes reach here.
        let count = usize::from(self.remaining[0]);
        let (encoded_runs, remaining) = self.remaining[1..].split_at(count * 2);
        let row = Row {
            y: self.next_y,
            encoded_runs,
        };
        self.remaining = remaining;
        self.next_y += 1;
        Some(row)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::from(self.height - self.next_y);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Rows<'_> {}
impl core::iter::FusedIterator for Rows<'_> {}

/// A validated frame's neutral 2D occupancy, borrowed from its encoded asset.
///
/// A present point may eventually have any tone, including black. Absence is
/// not a black surface. Logical points are not terminal character cells.
#[derive(Clone, Copy, Debug)]
pub struct ShapeFrame<'a> {
    width: u16,
    height: u16,
    encoded_rows: &'a [u8],
}

impl<'a> ShapeFrame<'a> {
    pub(crate) fn from_validated_bytes(width: u16, height: u16, encoded_rows: &'a [u8]) -> Self {
        Self {
            width,
            height,
            encoded_rows,
        }
    }

    pub fn width(self) -> u16 {
        self.width
    }

    pub fn height(self) -> u16 {
        self.height
    }

    pub fn mask_len(self) -> usize {
        usize::from(self.width) * usize::from(self.height)
    }

    pub fn rows(self) -> Rows<'a> {
        Rows {
            remaining: self.encoded_rows,
            next_y: 0,
            height: self.height,
        }
    }

    /// Write a row-major mask: 0 = absent, 1 = occupied, never a gray level.
    ///
    /// Reuses the caller's allocation. Only the first `mask_len()` bytes are
    /// written; an undersized buffer is unchanged. Work is O(N + rows + runs)
    /// for N grid points, including clearing all N destination bytes.
    pub fn write_mask(self, output: &mut [u8]) -> Result<(), MaskBufferTooShort> {
        let required = self.mask_len();
        if output.len() < required {
            return Err(MaskBufferTooShort {
                required,
                actual: output.len(),
            });
        }
        let output = &mut output[..required];
        output.fill(0);
        for row in self.rows() {
            let base = usize::from(row.y()) * usize::from(self.width);
            for run in row.runs() {
                output[base + usize::from(run.start())..base + usize::from(run.end())].fill(1);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskBufferTooShort {
    pub required: usize,
    pub actual: usize,
}

impl fmt::Display for MaskBufferTooShort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mask needs {} bytes, received {}",
            self.required, self.actual
        )
    }
}

impl std::error::Error for MaskBufferTooShort {}

/// A frame-local validation failure. Row and run indices are zero-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    TruncatedRow { row: u16 },
    TruncatedRun { row: u16, run: u8 },
    ZeroLengthRun { row: u16, run: u8 },
    RunOutsideGrid { row: u16, run: u8 },
    NonCanonicalRun { row: u16, run: u8 },
    TrailingBytes,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid row-run frame: {self:?}")
    }
}

impl std::error::Error for FrameError {}

pub(crate) fn validate_frame(width: u16, height: u16, bytes: &[u8]) -> Result<(), FrameError> {
    let mut cursor = 0;
    for row in 0..height {
        let count = *bytes.get(cursor).ok_or(FrameError::TruncatedRow { row })?;
        cursor += 1;
        let mut previous_end = None;
        for run in 0..count {
            let pair = bytes
                .get(cursor..cursor + 2)
                .ok_or(FrameError::TruncatedRun { row, run })?;
            cursor += 2;
            let start = u16::from(pair[0]);
            let len = u16::from(pair[1]);
            if len == 0 {
                return Err(FrameError::ZeroLengthRun { row, run });
            }
            let end = start + len;
            if end > width {
                return Err(FrameError::RunOutsideGrid { row, run });
            }
            // end is exclusive: equality would be adjacent, not canonical.
            if previous_end.is_some_and(|previous| start <= previous) {
                return Err(FrameError::NonCanonicalRun { row, run });
            }
            previous_end = Some(end);
        }
    }
    if cursor != bytes.len() {
        return Err(FrameError::TrailingBytes);
    }
    Ok(())
}
