//! Goodhand's neutral 2D shapes, separate reference gray, and pixel surfaces.
//!
//! Occupancy says where the hand exists, not how light or dark its surface is.
//! This core neither reconstructs depth/normals nor decodes video, invents light,
//! places terminal cells, or performs I/O. Runtime assets are prepared offline.

#![forbid(unsafe_code)]

pub mod frame;
pub mod sequence;
pub mod surface;
pub mod tone;

pub use frame::{FrameError, MaskBufferTooShort, Row, Rows, Run, Runs, ShapeFrame};
pub use sequence::{DecodeError, Sequence};
pub use surface::{write_surface, RasterSpec, SurfaceError};
pub use tone::{ToneError, ToneSequence};
