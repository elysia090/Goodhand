use goodhand::{Sequence, ToneSequence};
use std::io;

pub const WIDTH: u16 = 200;
pub const HEIGHT: u16 = 120;
pub const POSES: u32 = 288;
pub const ARCHIVE_FPS: u16 = 24;
#[allow(dead_code)]
pub const NATIVE_MOTION_POSES: u32 = NATIVE_MOTION_FPS as u32 * NATIVE_MOTION_SECONDS as u32;
#[allow(dead_code)]
pub const NATIVE_MOTION_SECONDS: u8 = 30;

#[allow(dead_code)]
pub const NATIVE_MOTION_FPS: u16 = 18;

pub fn validate<'a>(
    shape_bytes: &'a [u8],
    tone_bytes: &'a [u8],
) -> io::Result<(Sequence<'a>, ToneSequence<'a>)> {
    validate_count(shape_bytes, tone_bytes, POSES, ARCHIVE_FPS)
}

#[allow(dead_code)]
pub fn validate_native_motion<'a>(
    shape_bytes: &'a [u8],
    tone_bytes: &'a [u8],
) -> io::Result<(Sequence<'a>, ToneSequence<'a>)> {
    validate_count(
        shape_bytes,
        tone_bytes,
        NATIVE_MOTION_POSES,
        NATIVE_MOTION_FPS,
    )
}

fn validate_count<'a>(
    shape_bytes: &'a [u8],
    tone_bytes: &'a [u8],
    poses: u32,
    fps: u16,
) -> io::Result<(Sequence<'a>, ToneSequence<'a>)> {
    let shape = Sequence::from_bytes(shape_bytes).map_err(io::Error::other)?;
    if (
        shape.width(),
        shape.height(),
        shape.frame_count(),
        shape.fps(),
    ) != (WIDTH, HEIGHT, poses, fps)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected a {WIDTH}×{HEIGHT}, {poses}-pose, {fps} Hz archive"),
        ));
    }
    let tone = ToneSequence::from_bytes(tone_bytes, shape).map_err(io::Error::other)?;
    Ok((shape, tone))
}
