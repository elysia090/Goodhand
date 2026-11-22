//! Linux GTK3 presentation.
#[path = "chrome_x11.rs"]
mod chrome_x11;
#[path = "sys.rs"]
mod sys;

use self::sys::*;
#[cfg(goodhand_baseline_artwork)]
use super::coverage;
use super::{Artwork, BACKDROP_RGB, Canvas, Frame, HEIGHT, MIN_SCALE, Placement, WIDTH};
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
    #[cfg(goodhand_baseline_artwork)]
    area: coverage::Raster,
    time: Option<Instant>,
    once_timeout: Option<c_uint>,
    initial_size: Option<(c_int, c_int)>,
    translucent: bool,
    chrome: chrome_x11::Chrome,
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
            #[cfg(goodhand_baseline_artwork)]
            area: coverage::Raster::default(),
            time: None,
            once_timeout: None,
            initial_size: None,
            translucent: false,
            chrome: chrome_x11::Chrome::default(),
            error: None,
        })
    }

    fn select_appearance(&mut self, appearance: WindowAppearance) {
        let previous = self.art.canvas();
        let scale = self.placement().scale.max(MIN_SCALE);
        let regrade = (self.art.appearance == WindowAppearance::Safe)
            != (appearance == WindowAppearance::Safe);
        self.art.appearance = appearance;
        if appearance != WindowAppearance::Safe {
            self.chrome.hide();
        }
        if regrade {
            self.art.paint();
        }
        let canvas = self.art.canvas();
        if previous != canvas {
            let (min_width, min_height) = self.art.client_sizes().minimum;
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
        let sizes = state.art.client_sizes();
        let (min_width, min_height) = sizes.minimum;
        gtk_widget_set_size_request(handle, min_width, min_height);
        let (width, height) = sizes.initial;
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
            (c"focus-in-event", focus as Signal),
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
        state.chrome.destroy();
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

unsafe extern "C" fn focus(_widget: Ptr, _event: Ptr, data: Ptr) -> c_int {
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    state.chrome.reveal_for_keyboard();
    0
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
        state.chrome.reveal_for_keyboard();
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
    state.chrome.update(
        state.handle,
        now,
        state.art.appearance == WindowAppearance::Safe,
    );
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

#[cfg(goodhand_baseline_artwork)]
unsafe fn device_scale(canvas: Ptr) -> Option<i32> {
    // GTK gives the draw signal logical widget coordinates. Sample on the
    // actual Cairo target's device grid, not monitor DPI or the window size.
    let mut matrix = CairoMatrix::default();
    let (mut sx, mut sy, mut x0, mut y0) = (0., 0., 0., 0.);
    unsafe {
        let target = cairo_get_group_target(canvas);
        const RECORDING: c_int = 16;
        if cairo_surface_get_type(target) == RECORDING {
            // Recording can retain the borrowed image beyond this draw.
            return None;
        }
        cairo_get_matrix(canvas, &mut matrix);
        cairo_surface_get_device_scale(target, &mut sx, &mut sy);
        cairo_surface_get_device_offset(target, &mut x0, &mut y0);
    }
    let on_pixel = |v: f64| (v - v.round()).abs() < 1e-9;
    if matrix.xx == 1.
        && matrix.yy == 1.
        && matrix.xy == 0.
        && matrix.yx == 0.
        && sx == sy
        && (1. ..=8.).contains(&sx)
        && sx.fract() == 0.
        && on_pixel(matrix.x0 * sx + x0)
        && on_pixel(matrix.y0 * sy + y0)
    {
        Some(sx as i32)
    } else {
        // Mirroring, unusual device transforms, or a non-pixel-aligned
        // target must not sample the scratch image a second time.
        None
    }
}

#[cfg(goodhand_baseline_artwork)]
unsafe fn paint_area(canvas: Ptr, view: Placement, device: i32, area: coverage::Frame<'_>) -> bool {
    // The caller-owned bytes outlive this image and its Cairo source pattern.
    unsafe {
        let image = cairo_image_surface_create_for_data(
            area.pixels.as_mut_ptr().cast(),
            ARGB32,
            area.width,
            area.height,
            4 * area.width,
        );
        let ok = cairo_surface_status(image) == 0;
        if ok {
            cairo_surface_set_device_scale(image, f64::from(device), f64::from(device));
            let (x, y) = (view.origin.0.into(), view.origin.1.into());
            cairo_save(canvas);
            cairo_set_operator(canvas, OVER);
            cairo_set_source_surface(canvas, image, x, y);
            cairo_pattern_set_filter(cairo_get_source(canvas), NEAREST);
            cairo_rectangle(canvas, x, y, view.size.0.into(), view.size.1.into());
            cairo_fill(canvas);
            cairo_restore(canvas);
        }
        cairo_surface_destroy(image);
        ok
    }
}

unsafe fn paint_nearest(canvas: Ptr, layout: Canvas, view: Placement, pixels: &mut [u32]) -> bool {
    // Fill and restore before releasing the image borrowed from the raster.
    unsafe {
        let image = cairo_image_surface_create_for_data(
            pixels.as_mut_ptr().cast(),
            ARGB32,
            WIDTH as c_int,
            HEIGHT as c_int,
            (4 * WIDTH) as c_int,
        );
        let ok = cairo_surface_status(image) == 0;
        if ok {
            cairo_save(canvas);
            cairo_translate(canvas, view.origin.0.into(), view.origin.1.into());
            cairo_set_operator(canvas, OVER);
            cairo_rectangle(canvas, 0., 0., view.size.0.into(), view.size.1.into());
            cairo_clip(canvas);
            let (x, y) = view.raster_origin(layout);
            cairo_translate(canvas, x, y);
            cairo_scale(canvas, view.scale, view.scale);
            cairo_set_source_surface(canvas, image, 0., 0.);
            cairo_pattern_set_filter(cairo_get_source(canvas), NEAREST);
            cairo_rectangle(canvas, 0., 0., WIDTH as f64, HEIGHT as f64);
            cairo_fill(canvas);
            cairo_restore(canvas);
        }
        cairo_surface_destroy(image);
        ok
    }
}

#[cfg(goodhand_baseline_artwork)]
unsafe fn paint_preferred(
    canvas: Ptr,
    layout: Canvas,
    view: Placement,
    art: &mut Artwork,
    area: &mut coverage::Raster,
) -> bool {
    unsafe {
        let sampled = art.appearance == WindowAppearance::Safe
            && device_scale(canvas).is_some_and(|device| {
                area
                    .frame(layout, view, device, art.pose, &art.argb)
                    .is_some_and(|frame| paint_area(canvas, view, device, frame))
            });
        sampled || paint_nearest(canvas, layout, view, &mut art.argb)
    }
}

unsafe extern "C" fn draw(_widget: Ptr, canvas: Ptr, data: Ptr) -> c_int {
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    unsafe {
        let view = state.placement();
        let layout = state.art.canvas();
        let ([red, green, blue], alpha) = match state.art.appearance {
            WindowAppearance::Safe => (BACKDROP_RGB.map(|v| f64::from(v) / 255.), 1.0),
            WindowAppearance::Framed => ([0.04; 3], 0.14),
            WindowAppearance::Clear => ([0.; 3], 0.),
        };
        cairo_save(canvas);
        cairo_set_operator(canvas, SOURCE);
        cairo_set_source_rgba(canvas, red, green, blue, alpha);
        cairo_paint(canvas);
        #[cfg(goodhand_baseline_artwork)]
        let ok = view.scale <= 0.
            || paint_preferred(canvas, layout, view, &mut state.art, &mut state.area);
        #[cfg(not(goodhand_baseline_artwork))]
        let ok = view.scale <= 0. || paint_nearest(canvas, layout, view, &mut state.art.argb);
        if !ok {
            state.error = Some(io::Error::other(
                "Cairo could not create the raster surface",
            ));
            gtk_main_quit();
        }
        cairo_restore(canvas);
    }
    1
}

#[cfg(all(test, goodhand_baseline_artwork))]
mod tests {
    use super::*;
    use crate::playback::CycleDuration;
    use std::hint::black_box;

    struct Target {
        context: Ptr,
        image: Ptr,
        pixels: Vec<u32>,
    }

    impl Target {
        fn new(w: i32, h: i32, device: i32) -> Self {
            let (width, height) = (w * device, h * device);
            let mut pixels = vec![0_u32; (width * height) as usize];
            // Borrow the buffer until both the context and surface are dropped.
            unsafe {
                let image = cairo_image_surface_create_for_data(
                    pixels.as_mut_ptr().cast(),
                    ARGB32,
                    width,
                    height,
                    4 * width,
                );
                assert_eq!(cairo_surface_status(image), 0);
                cairo_surface_set_device_scale(image, f64::from(device), f64::from(device));
                let context = cairo_create(image);
                assert_eq!(device_scale(context), Some(device));
                Self {
                    context,
                    image,
                    pixels,
                }
            }
        }

        fn draw(
            &mut self,
            layout: Canvas,
            view: Placement,
            art: &mut Artwork,
            area: Option<&mut coverage::Raster>,
        ) {
            unsafe {
                cairo_save(self.context);
                cairo_set_operator(self.context, SOURCE);
                let [r, g, b] = BACKDROP_RGB.map(|v| f64::from(v) / 255.);
                cairo_set_source_rgba(self.context, r, g, b, 1.);
                cairo_paint(self.context);
                assert!(match area {
                    Some(area) => paint_preferred(self.context, layout, view, art, area),
                    None => paint_nearest(self.context, layout, view, &mut art.argb),
                });
                cairo_restore(self.context);
                cairo_surface_flush(self.image);
            }
        }
    }

    impl Drop for Target {
        fn drop(&mut self) {
            unsafe {
                cairo_destroy(self.context);
                cairo_surface_destroy(self.image);
            }
        }
    }

    fn baseline() -> Artwork {
        Artwork::new(
            Repeat::Loop,
            WindowAppearance::Safe,
            Cycle {
                duration: CycleDuration::WINDOW_DEFAULT,
                ..Cycle::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn offscreen_cairo_preserves_one_x_and_blits_exact_physical_pixels() {
        let mut art = baseline();
        let layout = art.canvas();
        let mut area = coverage::Raster::default();
        for pose in [0, 36, 108, 180, 252] {
            art.update(pose).unwrap();
            for (w, h, device) in [(1016, 445, 1), (565, 305, 1), (595, 290, 1), (597, 290, 1), (597, 290, 2)] {
                let view = Placement::new(layout, w, h);
                let mut nearest = Target::new(w, h, device);
                nearest.draw(layout, view, &mut art, None);
                let mut target = Target::new(w, h, device);
                target.draw(layout, view, &mut art, Some(&mut area));
                if w == 1016 {
                    assert_eq!(target.pixels, nearest.pixels);
                    continue;
                }
                assert_ne!(target.pixels, nearest.pixels);
                let frame = area.frame(layout, view, device, pose, &art.argb).unwrap();
                let black = super::super::argb(BACKDROP_RGB, true);
                let (x0, y0) = (view.origin.0 * device, view.origin.1 * device);
                for row in 0..h * device {
                    for col in 0..w * device {
                        let (x, y) = (col - x0, row - y0);
                        let expected = if (0..frame.width).contains(&x)
                            && (0..frame.height).contains(&y)
                        {
                            frame.pixels[(y * frame.width + x) as usize]
                        } else {
                            black
                        };
                        assert_eq!(target.pixels[(row * w * device + col) as usize], expected);
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "run explicitly for a release-mode, no-display Cairo comparison"]
    fn benchmark_cairo_draw() {
        let mut art = baseline();
        let layout = art.canvas();
        for (w, h, device) in [
            (595, 290, 1),
            (597, 290, 1),
            (597, 290, 2),
            (960, 1040, 1),
        ] {
            let view = Placement::new(layout, w, h);
            let mut a = Target::new(w, h, device);
            let mut b = Target::new(w, h, device);
            let mut area = coverage::Raster::default();
            let mut old = Vec::new();
            let mut new = Vec::new();
            for pose in 0..crate::archive_profile::POSES {
                art.update(pose).unwrap(); // identical work for both paths
                let before = Instant::now();
                a.draw(layout, view, &mut art, None);
                let t_old = before.elapsed();
                let before = Instant::now();
                b.draw(layout, view, &mut art, Some(&mut area));
                let t_new = before.elapsed();
                black_box(&a.pixels);
                black_box(&b.pixels);
                // Exclude the first allocation and palette initialization.
                if pose > 0 {
                    old.push(t_old);
                    new.push(t_new);
                }
            }
            for (what, mut samples) in [("nearest", old), ("area", new)] {
                samples.sort_unstable();
                let ms = |p: usize| samples[p].as_secs_f64() * 1000.;
                eprintln!(
                    "{w}×{h}@{device} {what}: p50 {:.3} ms, p95 {:.3} ms, max {:.3} ms",
                    ms(samples.len() / 2),
                    ms(samples.len() * 95 / 100),
                    ms(samples.len() - 1),
                );
            }
        }
    }
}
