use goodhand::{Sequence, ToneSequence};
use std::io;

pub const WIDTH: u16 = 200;
pub const HEIGHT: u16 = 120;
pub const POSES: u32 = 288;
pub const ARCHIVE_FPS: u16 = 24;

pub fn validate<'a>(
    shape_bytes: &'a [u8],
    tone_bytes: &'a [u8],
) -> io::Result<(Sequence<'a>, ToneSequence<'a>)> {
    let shape = Sequence::from_bytes(shape_bytes).map_err(io::Error::other)?;
    if (
        shape.width(),
        shape.height(),
        shape.frame_count(),
        shape.fps(),
    ) != (WIDTH, HEIGHT, POSES, ARCHIVE_FPS)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a 200×120, 288-pose, 24 Hz review archive",
        ));
    }
    let tone = ToneSequence::from_bytes(tone_bytes, shape).map_err(io::Error::other)?;
    Ok((shape, tone))
}
