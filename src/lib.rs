//! Goodhand's neutral 2D shape sequence and absolute-time frame selection.
//!
//! Occupancy says where the hand exists, not how light or dark its surface is.
//! This core neither reconstructs depth/normals nor decodes video, assigns tone,
//! places terminal cells, or performs I/O. Runtime assets are prepared offline.

#![forbid(unsafe_code)]

pub mod frame;
pub mod sequence;

pub use frame::{FrameError, MaskBufferTooShort, Row, Rows, Run, Runs, ShapeFrame};
pub use sequence::{DecodeError, Sequence};
