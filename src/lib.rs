//! Goodhand's continuous arm model and separate readers for archived 2D art.
//!
//! Occupancy says where the hand exists, not how light or dark its surface is.
//! The continuous model samples geometry and broad base light in code. Archived
//! shape and gray readers preserve their fixed-grid data without resampling.
//! The library performs no I/O or terminal placement.

#![forbid(unsafe_code)]

pub mod arm_model;
pub mod frame;
pub mod sequence;
pub mod surface;
pub mod tone;

pub use arm_model::{render_arm, ArmAttrs, ArmRenderError, ArmSampler, ArmView};
pub use frame::{FrameError, MaskBufferTooShort, Row, Rows, Run, Runs, ShapeFrame};
pub use sequence::{DecodeError, Sequence};
pub use surface::{write_surface, RasterSpec, SurfaceError};
pub use tone::{ToneError, ToneSequence};
