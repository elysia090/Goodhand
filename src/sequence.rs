//! Validate a borrowed row-run asset once, then select frames by absolute time.

use crate::frame::{validate_frame, FrameError, ShapeFrame};
use core::{fmt, time::Duration};

const HEADER_LEN: usize = 16;
pub const FORMAT_VERSION: u16 = 1;
pub const MAX_DIMENSION: u16 = 255;
pub const MAX_FPS: u16 = 1_000;
pub const MAX_FRAME_COUNT: u32 = 4_096;
pub const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;

/// An allocation-free, fully validated view of an immutable shape asset.
#[derive(Clone, Copy, Debug)]
pub struct Sequence<'a> {
    width: u16,
    height: u16,
    fps: u16,
    frame_count: u32,
    offsets: &'a [u8],
    payload: &'a [u8],
}

impl<'a> Sequence<'a> {
    /// Validate every offset, row, and run before exposing any frame.
    ///
    /// Parsing takes O(asset bytes), no heap allocation, and does not mutate
    /// the source. Frame access afterwards reads only two offsets. The format
    /// and conservative resource limits are documented in the README.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(DecodeError::AssetTooLarge);
        }
        if bytes.len() < HEADER_LEN {
            return Err(DecodeError::TruncatedHeader);
        }
        if &bytes[..4] != b"GHSH" {
            return Err(DecodeError::BadMagic);
        }
        let version = read_u16(bytes, 4);
        if version != FORMAT_VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let width = read_u16(bytes, 6);
        let height = read_u16(bytes, 8);
        if width == 0 || width > MAX_DIMENSION || height == 0 || height > MAX_DIMENSION {
            return Err(DecodeError::InvalidDimensions { width, height });
        }
        let fps = read_u16(bytes, 10);
        if fps == 0 || fps > MAX_FPS {
            return Err(DecodeError::InvalidFps(fps));
        }
        let frame_count = read_u32(bytes, 12);
        if frame_count == 0 || frame_count > MAX_FRAME_COUNT {
            return Err(DecodeError::InvalidFrameCount(frame_count));
        }

        // The validated cap bounds all table-size and frame-index arithmetic.
        let table_end = HEADER_LEN + (frame_count as usize + 1) * 4;
        let offsets = bytes
            .get(HEADER_LEN..table_end)
            .ok_or(DecodeError::TruncatedOffsetTable)?;
        let payload = &bytes[table_end..];
        let sequence = Self {
            width,
            height,
            fps,
            frame_count,
            offsets,
            payload,
        };

        let mut previous = 0;
        for index in 0..=frame_count {
            let offset = sequence.offset(index);
            if offset as usize > payload.len() {
                return Err(DecodeError::OffsetOutOfRange { index, offset });
            }
            if index == 0 && offset != 0 {
                return Err(DecodeError::FirstOffsetNotZero);
            }
            if offset < previous {
                return Err(DecodeError::NonMonotoneOffsets { index });
            }
            previous = offset;
        }
        if previous as usize != payload.len() {
            return Err(DecodeError::FinalOffsetMismatch);
        }
        for index in 0..frame_count {
            validate_frame(width, height, sequence.frame_bytes(index))
                .map_err(|error| DecodeError::InvalidFrame { index, error })?;
        }
        Ok(sequence)
    }

    pub fn width(self) -> u16 {
        self.width
    }

    pub fn height(self) -> u16 {
        self.height
    }

    pub fn fps(self) -> u16 {
        self.fps
    }

    pub fn frame_count(self) -> u32 {
        self.frame_count
    }

    /// Compute floor(elapsed * fps) modulo frame_count, without accumulated dt.
    ///
    /// Reducing seconds before multiplication bounds every intermediate, even
    /// for Duration::MAX. Integer nanoseconds define exact interval boundaries.
    pub fn frame_index_at(self, elapsed: Duration) -> u32 {
        let count = u64::from(self.frame_count);
        let fps = u64::from(self.fps);
        let whole = (elapsed.as_secs() % count) * fps;
        let fractional = u64::from(elapsed.subsec_nanos()) * fps / 1_000_000_000;
        ((whole + fractional) % count) as u32
    }

    pub fn frame(self, index: u32) -> Option<ShapeFrame<'a>> {
        (index < self.frame_count).then(|| self.validated_frame(index))
    }

    pub fn frame_at(self, elapsed: Duration) -> ShapeFrame<'a> {
        self.validated_frame(self.frame_index_at(elapsed))
    }

    pub fn frames(self) -> impl ExactSizeIterator<Item = ShapeFrame<'a>> + DoubleEndedIterator {
        (0..self.frame_count as usize).map(move |index| self.validated_frame(index as u32))
    }

    fn validated_frame(self, index: u32) -> ShapeFrame<'a> {
        ShapeFrame::from_validated_bytes(self.width, self.height, self.frame_bytes(index))
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
pub enum DecodeError {
    AssetTooLarge,
    TruncatedHeader,
    BadMagic,
    UnsupportedVersion(u16),
    InvalidDimensions { width: u16, height: u16 },
    InvalidFps(u16),
    InvalidFrameCount(u32),
    TruncatedOffsetTable,
    FirstOffsetNotZero,
    OffsetOutOfRange { index: u32, offset: u32 },
    NonMonotoneOffsets { index: u32 },
    FinalOffsetMismatch,
    InvalidFrame { index: u32, error: FrameError },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid shape sequence: {self:?}")
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidFrame { error, .. } => Some(error),
            _ => None,
        }
    }
}
