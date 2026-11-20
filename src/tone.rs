//! Borrowed gray8 values in each shape frame's occupied row/run order.
//!
//! GHT8 v1 uses a 16-byte little-endian header: magic `GHT8`, version u16,
//! width u16, height u16, fps u16, frame_count u32. It is followed by
//! frame_count + 1 u32 payload-relative offsets and uncompressed u8 gray values.
//! Every occupied point has one value, including black (0). Background has none.
//!
//! Validation proves matching dimensions, fps, frame count, and occupied counts,
//! not that two assets share provenance or the same shape hash. That association
//! and mask acceptance belong to offline provenance. There is no second clock.

use crate::sequence::{Sequence, MAX_ASSET_BYTES};
use core::fmt;

const HEADER_LEN: usize = 16;
const FORMAT_VERSION: u16 = 1;

/// A fully validated, allocation-free view of a separate occupied-gray asset.
#[derive(Clone, Copy, Debug)]
pub struct ToneSequence<'a> {
    frame_count: u32,
    offsets: &'a [u8],
    payload: &'a [u8],
}

impl<'a> ToneSequence<'a> {
    /// Validate all metadata, offsets, and per-frame counts before exposure.
    ///
    /// Gray values themselves are not quantized or interpreted as occupancy.
    /// Frame selection uses the supplied shape sequence's existing clock.
    pub fn from_bytes(bytes: &'a [u8], shape: Sequence<'_>) -> Result<Self, ToneError> {
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(ToneError::AssetTooLarge);
        }
        if bytes.len() < HEADER_LEN {
            return Err(ToneError::TruncatedHeader);
        }
        if &bytes[..4] != b"GHT8" {
            return Err(ToneError::BadMagic);
        }
        let version = read_u16(bytes, 4);
        if version != FORMAT_VERSION {
            return Err(ToneError::UnsupportedVersion(version));
        }
        let dimensions = (read_u16(bytes, 6), read_u16(bytes, 8));
        let expected_dimensions = (shape.width(), shape.height());
        if dimensions != expected_dimensions {
            return Err(ToneError::DimensionsMismatch {
                expected: expected_dimensions,
                actual: dimensions,
            });
        }
        let fps = read_u16(bytes, 10);
        if fps != shape.fps() {
            return Err(ToneError::FpsMismatch {
                expected: shape.fps(),
                actual: fps,
            });
        }
        let frame_count = read_u32(bytes, 12);
        if frame_count != shape.frame_count() {
            return Err(ToneError::FrameCountMismatch {
                expected: shape.frame_count(),
                actual: frame_count,
            });
        }
        // Equality with the validated shape count bounds this arithmetic.
        let table_end = HEADER_LEN + (frame_count as usize + 1) * 4;
        let offsets = bytes
            .get(HEADER_LEN..table_end)
            .ok_or(ToneError::TruncatedOffsetTable)?;
        let sequence = Self {
            frame_count,
            offsets,
            payload: &bytes[table_end..],
        };
        let mut previous = 0;
        for index in 0..=frame_count {
            let offset = sequence.offset(index);
            if offset as usize > sequence.payload.len() {
                return Err(ToneError::OffsetOutOfRange { index, offset });
            }
            if index == 0 && offset != 0 {
                return Err(ToneError::FirstOffsetNotZero);
            }
            if offset < previous {
                return Err(ToneError::NonMonotoneOffsets { index });
            }
            previous = offset;
        }
        if previous as usize != sequence.payload.len() {
            return Err(ToneError::FinalOffsetMismatch);
        }
        for (index, frame) in shape.frames().enumerate() {
            let expected = frame
                .rows()
                .flat_map(|row| row.runs())
                .map(|run| usize::from(run.length()))
                .sum();
            let actual = sequence.frame_bytes(index as u32).len();
            if expected != actual {
                return Err(ToneError::OccupiedCountMismatch {
                    index: index as u32,
                    expected,
                    actual,
                });
            }
        }
        Ok(sequence)
    }

    pub fn frame_count(self) -> u32 {
        self.frame_count
    }

    /// Borrow gray values in occupied top-to-bottom, left-to-right order.
    pub fn frame(self, index: u32) -> Option<&'a [u8]> {
        (index < self.frame_count).then(|| self.frame_bytes(index))
    }

    fn frame_bytes(self, index: u32) -> &'a [u8] {
        &self.payload[self.offset(index) as usize..self.offset(index + 1) as usize]
    }

    fn offset(self, index: u32) -> u32 {
        read_u32(self.offsets, index as usize * 4)
    }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneError {
    AssetTooLarge,
    TruncatedHeader,
    BadMagic,
    UnsupportedVersion(u16),
    DimensionsMismatch {
        expected: (u16, u16),
        actual: (u16, u16),
    },
    FpsMismatch {
        expected: u16,
        actual: u16,
    },
    FrameCountMismatch {
        expected: u32,
        actual: u32,
    },
    TruncatedOffsetTable,
    FirstOffsetNotZero,
    OffsetOutOfRange {
        index: u32,
        offset: u32,
    },
    NonMonotoneOffsets {
        index: u32,
    },
    FinalOffsetMismatch,
    OccupiedCountMismatch {
        index: u32,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for ToneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid occupied-gray sequence: {self:?}")
    }
}

impl std::error::Error for ToneError {}
