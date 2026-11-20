//! Main-thread bridge to the Win32 and AppKit window hosts.

use super::{Artwork, Canvas, Frame, copy_black_client};
use crate::playback::{Cycle, Repeat};
use crate::settings::WindowAppearance;
use std::ffi::c_void;
use std::io;
use std::time::Instant;

type Update = unsafe extern "C" fn(*mut c_void, *mut *const u32) -> i32;

unsafe extern "C" {
    fn goodhand_run_platform(
        context: *mut c_void,
        width: u32,
        height: u32,
        min_width: u32,
        min_height: u32,
        update: Update,
    ) -> i32;
}

struct Presenter {
    art: Artwork,
    canvas: Canvas,
    pixels: Box<[u32]>,
    clock: Option<Instant>,
    error: Option<io::Error>,
}

pub(crate) fn run(repeat: Repeat, appearance: WindowAppearance, cycle: Cycle) -> io::Result<()> {
    if appearance != WindowAppearance::Safe {
        return Err(io::Error::other(
            "this desktop backend supports only the black (safe) window appearance",
        ));
    }
    let mut art = Artwork::new(repeat, appearance, cycle)?;
    art.update(0)?;
    let canvas = art.canvas();
    let (width, height) = canvas.client_at(1.0);
    let (min_width, min_height) = canvas.minimum();
    let mut presenter = Presenter {
        pixels: vec![0; canvas.width * canvas.height].into_boxed_slice(),
        art,
        canvas,
        clock: None,
        error: None,
    };
    copy_black_client(&presenter.art.argb, canvas, &mut presenter.pixels);

    // The presenter and its fixed allocation outlive the native event loop.
    // The host calls back on this thread only and finishes using the client
    // pixels before the next callback may update that allocation.
    let status = unsafe {
        goodhand_run_platform(
            std::ptr::from_mut(&mut presenter).cast(),
            width as u32,
            height as u32,
            min_width as u32,
            min_height as u32,
            advance,
        )
    };
    if let Some(error) = presenter.error {
        return Err(error);
    }
    if status != 0 {
        return Err(io::Error::other("could not open the native oops window"));
    }
    Ok(())
}

unsafe extern "C" fn advance(context: *mut c_void, output: *mut *const u32) -> i32 {
    // Do not unwind into the host. A renderer failure terminates the event
    // loop; its message is returned to the regular command entry point.
    let presenter: &mut Presenter = unsafe { &mut *context.cast() };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(clock) = presenter.clock else {
            presenter.clock = Some(Instant::now());
            return Ok(1);
        };
        match presenter.art.follow(clock.elapsed())? {
            Frame::Unchanged => Ok(0),
            Frame::Changed => {
                copy_black_client(&presenter.art.argb, presenter.canvas, &mut presenter.pixels);
                Ok(1)
            }
            Frame::Finished => Ok(-1),
        }
    }));
    match result {
        Ok(Ok(status)) => {
            unsafe { output.write(presenter.pixels.as_ptr()) };
            status
        }
        Ok(Err(error)) => {
            presenter.error = Some(error);
            -1
        }
        Err(_) => {
            presenter.error = Some(io::Error::other("native artwork rendering panicked"));
            -1
        }
    }
}
