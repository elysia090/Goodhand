//! Linux GTK3 presentation.
#[path = "sys.rs"]
mod sys;

use self::sys::*;
use super::{Artwork, Frame, HEIGHT, MIN_SCALE, Placement, WIDTH};
use crate::playback::{Cycle, Repeat};
use crate::settings::WindowAppearance;
use std::ffi::{c_int, c_uint};
use std::io;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

type Signal = unsafe extern "C" fn(Ptr, Ptr, Ptr) -> c_int;

struct Window {
    handle: Ptr,
    art: Artwork,
    time: Option<Instant>,
    once_timeout: Option<c_uint>,
    initial_size: Option<(c_int, c_int)>,
    translucent: bool,
    error: Option<io::Error>,
}

impl Window {
    fn new(
        handle: Ptr,
        repeat: Repeat,
        appearance: WindowAppearance,
        cycle: Cycle,
    ) -> io::Result<Self> {
        Ok(Self {
            handle,
            art: Artwork::new(repeat, appearance, cycle)?,
            time: None,
            once_timeout: None,
            initial_size: None,
            translucent: false,
            error: None,
        })
    }

    fn select_appearance(&mut self, appearance: WindowAppearance) {
        let previous = self.art.canvas();
        let scale = self.placement().scale.max(MIN_SCALE);
        let regrade = (self.art.appearance == WindowAppearance::Safe)
            != (appearance == WindowAppearance::Safe);
        self.art.appearance = appearance;
        if regrade {
            self.art.paint();
        }
        let canvas = self.art.canvas();
        if previous != canvas {
            let (min_width, min_height) = canvas.minimum();
            unsafe {
                gtk_widget_set_size_request(self.handle, min_width, min_height);
                // Preserve artwork scale when switching a floating panel.
                if gtk_window_is_maximized(self.handle) == 0 {
                    let (width, height) = canvas.client_at(scale);
                    gtk_window_resize(self.handle, width, height);
                }
            }
        }
    }

    fn placement(&self) -> Placement {
        // The GTK object stays live on this event-loop thread.
        let (width, height) = unsafe {
            (
                gtk_widget_get_allocated_width(self.handle),
                gtk_widget_get_allocated_height(self.handle),
            )
        };
        Placement::new(self.art.canvas(), width, height)
    }
}

pub(crate) fn run(repeat: Repeat, appearance: WindowAppearance, cycle: Cycle) -> io::Result<()> {
    // All GTK state and callbacks live on this thread. Remove the callbacks
    // before releasing their data (the Box). The FFI adapter is
    // not linked by the source-only terminal build.
    unsafe {
        if gtk_init_check(null_mut(), null_mut()) == 0 {
            return Err(io::Error::other(
                "no GTK display; use oops --terminal for text playback",
            ));
        }
        let handle = gtk_window_new(0);
        if handle.is_null() {
            return Err(io::Error::other("GTK window creation failed"));
        }
        // Keep GTK's decorated top-level. The window manager owns chrome,
        // movement, maximization and interactive resizing.
        let screen = gtk_widget_get_screen(handle);
        let visual = gdk_screen_get_rgba_visual(screen);
        let translucent = !visual.is_null() && gdk_screen_is_composited(screen) != 0;
        if translucent {
            gtk_widget_set_visual(handle, visual);
        } else if appearance != WindowAppearance::Safe {
            gtk_widget_destroy(handle);
            return Err(io::Error::other(
                "no composited RGBA surface; select the black window or use oops --terminal",
            ));
        }
        let mut state =
            match Window::new(handle, repeat, appearance, cycle).and_then(|mut state| {
                state.art.update(0)?;
                Ok(Box::new(state))
            }) {
                Ok(state) => state,
                Err(error) => {
                    gtk_widget_destroy(handle);
                    return Err(error);
                }
            };
        gtk_window_set_title(handle, c"oops".as_ptr());
        gtk_widget_set_app_paintable(handle, 1);
        let canvas = state.art.canvas();
        let (min_width, min_height) = canvas.minimum();
        gtk_widget_set_size_request(handle, min_width, min_height);
        let (width, height) = canvas.client_at(1.0);
        gtk_window_set_default_size(handle, width, height);
        state.initial_size = Some((width, height));
        state.translucent = translucent;
        let data = std::ptr::from_mut(state.as_mut()).cast();
        // GTK expects a type-erased GCallback; these three signals all
        // invoke the C signature (widget, event/context, user_data) -> gint.
        for (name, callback) in [
            (c"draw", draw as Signal),
            (c"delete-event", close as Signal),
            (c"key-press-event", key as Signal),
        ] {
            g_signal_connect_data(
                handle,
                name.as_ptr(),
                std::mem::transmute::<Signal, Callback>(callback),
                data,
                None,
                0,
            );
        }
        gtk_widget_show_all(handle);
        let callback = gtk_widget_add_tick_callback(handle, tick, data, None);
        if callback == 0 {
            gtk_widget_destroy(handle);
            return Err(io::Error::other("GTK playback frame clock could not start"));
        }
        gtk_main();
        if let Some(timeout) = state.once_timeout.take() {
            g_source_remove(timeout);
        }
        gtk_widget_remove_tick_callback(handle, callback);
        gtk_widget_destroy(handle);
        if let Some(error) = state.error.take() {
            return Err(error);
        }
        Ok(())
    }
}

unsafe extern "C" fn close(_widget: Ptr, _event: Ptr, _data: Ptr) -> c_int {
    unsafe { gtk_main_quit() };
    1
}

unsafe extern "C" fn key(widget: Ptr, event: Ptr, data: Ptr) -> c_int {
    let mut code: c_uint = 0;
    unsafe {
        if gdk_event_get_keyval(event, &mut code) == 0 {
            return 0;
        }
        if matches!(code, 113 | 0xff1b) {
            gtk_main_quit();
            return 1;
        }
        let state = &mut *data.cast::<Window>();
        if code == u32::from(b't')
            && state.translucent
            && gdk_screen_is_composited(gtk_widget_get_screen(widget)) != 0
        {
            let next = match state.art.appearance {
                WindowAppearance::Safe => WindowAppearance::Framed,
                WindowAppearance::Framed => WindowAppearance::Clear,
                WindowAppearance::Clear => WindowAppearance::Safe,
            };
            state.select_appearance(next);
            gtk_widget_queue_draw(widget);
            return 1;
        }
    }
    0
}

unsafe extern "C" fn stop_once(data: Ptr) -> c_int {
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    state.once_timeout = None;
    unsafe { gtk_main_quit() };
    // Remove this one-shot source.
    0
}

unsafe extern "C" fn tick(_widget: Ptr, _clock: Ptr, data: Ptr) -> c_int {
    // GTK invokes updates ahead of drawing on the same thread. The absolute
    // clock doesn't accumulate refresh quantization or queue stale artwork.
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    let now = Instant::now();
    if state.time.is_none() && state.art.timeline.repeat == Repeat::Once {
        // A hidden/minimized widget can stop receiving frame-clock callbacks.
        // Keep the one-turn exit independent of desktop visibility.
        let milliseconds = state.art.timeline.cycle.length().as_millis() as c_uint;
        let timeout = unsafe { g_timeout_add(milliseconds, stop_once, data) };
        if timeout == 0 {
            state.error = Some(io::Error::other("GTK one-turn timer could not start"));
            unsafe { gtk_main_quit() };
            return 1;
        }
        state.once_timeout = Some(timeout);
    }
    let elapsed = state.time.get_or_insert(now).elapsed();
    // Honor the requested floating start size on a desktop that fits the
    // mapped window to its work area. Never override later user resizing.
    if let Some((w, h)) = state.initial_size {
        if elapsed > Duration::from_millis(300) {
            state.initial_size = None;
        } else if unsafe { gtk_window_is_maximized(state.handle) } != 0 {
            state.initial_size = None;
            unsafe {
                gtk_window_unmaximize(state.handle);
                gtk_window_resize(state.handle, w, h);
            }
        }
    }
    match state.art.follow(elapsed) {
        Ok(Frame::Changed) => unsafe { gtk_widget_queue_draw(state.handle) },
        Ok(Frame::Unchanged) => {}
        Ok(Frame::Finished) => unsafe { gtk_main_quit() },
        Err(error) => {
            state.error = Some(error);
            unsafe { gtk_main_quit() };
        }
    }
    1
}

unsafe extern "C" fn draw(_widget: Ptr, canvas: Ptr, data: Ptr) -> c_int {
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    // Borrowed image memory stays valid through paint. Restoring the Cairo
    // state releases its source pattern before the image and slice can change.
    unsafe {
        let view = state.placement();
        let layout = state.art.canvas();
        let (width, height) = view.size;
        let scale = view.scale;
        let (shade, alpha) = match state.art.appearance {
            WindowAppearance::Safe => (0., 1.0),
            WindowAppearance::Framed => (0.04, 0.14),
            WindowAppearance::Clear => (0., 0.),
        };
        cairo_save(canvas);
        cairo_set_operator(canvas, SOURCE);
        cairo_set_source_rgba(canvas, shade, shade, shade, alpha);
        cairo_paint(canvas);
        if scale > 0.0 {
            let image = cairo_image_surface_create_for_data(
                state.art.argb.as_mut_ptr().cast(),
                ARGB32,
                WIDTH as c_int,
                HEIGHT as c_int,
                (4 * WIDTH) as c_int,
            );
            if cairo_surface_status(image) == 0 {
                cairo_translate(canvas, view.origin.0.into(), view.origin.1.into());
                cairo_set_operator(canvas, OVER);
                cairo_save(canvas);
                cairo_rectangle(canvas, 0., 0., width.into(), height.into());
                cairo_clip(canvas);
                let (x, y) = view.raster_origin(layout);
                cairo_translate(canvas, x, y);
                cairo_scale(canvas, scale, scale);
                cairo_set_source_surface(canvas, image, 0., 0.);
                cairo_pattern_set_filter(cairo_get_source(canvas), NEAREST);
                cairo_rectangle(canvas, 0., 0., WIDTH as f64, HEIGHT as f64);
                cairo_fill(canvas);
                cairo_restore(canvas);
            } else {
                state.error = Some(io::Error::other(
                    "Cairo could not create the raster surface",
                ));
                gtk_main_quit();
            }
            cairo_surface_destroy(image);
        }
        cairo_restore(canvas);
    }
    1
}
