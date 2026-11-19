//! Optional GTK desktop transport; no contribution to raster geometry or shade.
mod controls;
mod interior_matte;
mod sys;

use self::controls::{Button, Controls, ResizeEdge};
use self::interior_matte::InteriorMatte;
use self::sys::*;
use crate::archived_scene::pose_at_tick;
use crate::black_grade;
use crate::playback::{Cycle, Repeat, Timeline};
use crate::reference_frame::ReferenceFrame;
use crate::settings::WindowAppearance;
use goodhand::RasterSpec;
use std::ffi::{c_int, c_uint};
use std::io;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

const WIDTH: usize = RasterSpec::REFERENCE.width;
const HEIGHT: usize = RasterSpec::REFERENCE.height;
const PANEL_WIDTH: usize = 1120;
const PANEL_HEIGHT: usize = 560;
const BLACK_WIDTH: usize = 1016;
const BLACK_HEIGHT: usize = 462;
const BLACK_CLIENT: [usize; 2] = [1024, 576];
const BLACK_UP: i32 = 8;
const MIN_SCALE: f64 = 0.75;
const PANEL_CORNER: f64 = 7.0;
const BLACK_CORNER: f64 = 5.0;
const START_PLACEMENT_GRACE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Eq, PartialEq)]
struct Canvas {
    width: usize,
    height: usize,
    crop_top: i32,
    inset: [usize; 2],
}

impl Canvas {
    fn frame(
        (lo, hi): ([i32; 2], [i32; 2]),
        [width, height]: [usize; 2],
        lift: i32,
        vertical_gutter: i32,
    ) -> Option<Self> {
        let left = (WIDTH as i32 - width as i32) / 2;
        let top = (lo[1] + hi[1] - height as i32).div_euclid(2) + lift;
        if lo[0] < left
            || hi[0] > left + width as i32
            || lo[1] - top < vertical_gutter
            || top + height as i32 - hi[1] < vertical_gutter
        {
            return None;
        }
        Some(Self {
            width,
            height,
            crop_top: top,
            inset: [0, 0],
        })
    }

    fn client_at(self, scale: f64) -> (c_int, c_int) {
        (
            (scale * self.width as f64 + (2 * self.inset[0]) as f64).round() as c_int,
            (scale * self.height as f64 + (2 * self.inset[1]) as f64).round() as c_int,
        )
    }

    fn minimum(self) -> (c_int, c_int) {
        // Round the limiting axis up before deriving its width, so the
        // pixel-quantized aspect fit never drops below 0.75×.
        let height = (3 * self.height).div_ceil(4);
        let width = (height * self.width).div_ceil(self.height);
        (
            (width + 2 * self.inset[0]) as c_int,
            (height + 2 * self.inset[1]) as c_int,
        )
    }
}

struct Apertures {
    standard: Canvas,
    black: Canvas,
}

impl Apertures {
    fn new(bounds: ([i32; 2], [i32; 2])) -> io::Result<Self> {
        let standard =
            Canvas::frame(bounds, [PANEL_WIDTH, PANEL_HEIGHT], 0, 16).ok_or_else(|| {
                io::Error::other("this archive cannot fit the wide panel without cropping")
            })?;
        // Fixed compact presentation for the reviewed black artwork. If
        // another embedded archive is too large, keep the wider-safe framing.
        let black = Canvas::frame(bounds, [BLACK_WIDTH, BLACK_HEIGHT], BLACK_UP, 6)
            .map(|mut aperture| {
                // The 16:9 starting client contains a visibly wide aperture.
                // Transparent insets are fixed so a resize changes the
                // fitted image without first spending the letterbox slack.
                aperture.inset = [
                    (BLACK_CLIENT[0] - BLACK_WIDTH) / 2,
                    (BLACK_CLIENT[1] - BLACK_HEIGHT) / 2,
                ];
                aperture
            })
            .unwrap_or(standard);
        Ok(Self { standard, black })
    }

    fn for_appearance(&self, appearance: WindowAppearance) -> Canvas {
        match appearance {
            WindowAppearance::Safe => self.black,
            WindowAppearance::Clear | WindowAppearance::Framed => self.standard,
        }
    }
}
#[derive(Clone, Copy)]
struct Placement {
    size: (c_int, c_int),
    scale: f64,
    origin: (c_int, c_int),
}

impl Placement {
    fn new(canvas: Canvas, width: c_int, height: c_int) -> Self {
        // Fit the selected aperture's aspect without stretching the artwork.
        // Integer arithmetic preserves the original 2:1 path exactly.
        let available_width = (width - (2 * canvas.inset[0]) as c_int).max(0);
        let available_height = (height - (2 * canvas.inset[1]) as c_int).max(0);
        let width_limited_height =
            (i64::from(available_width) * canvas.height as i64 / canvas.width as i64) as c_int;
        let panel_height = available_height.min(width_limited_height);
        let panel_width =
            (i64::from(panel_height) * canvas.width as i64 / canvas.height as i64) as c_int;
        let size = (panel_width, panel_height);
        Self {
            size,
            scale: f64::from(panel_height) / canvas.height as f64,
            origin: ((width - size.0) / 2, (height - size.1) / 2),
        }
    }

    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (x - f64::from(self.origin.0), y - f64::from(self.origin.1))
    }

    fn raster_origin(self, canvas: Canvas) -> (f64, f64) {
        (
            (f64::from(self.size.0) - WIDTH as f64 * self.scale) / 2.0,
            -f64::from(canvas.crop_top) * self.scale,
        )
    }
}

type Signal = unsafe extern "C" fn(Ptr, Ptr, Ptr) -> c_int;

struct Window {
    handle: Ptr,
    renderer: ReferenceFrame,
    apertures: Apertures,
    appearance: WindowAppearance,
    light: Vec<u8>,
    alpha: Vec<u8>,
    argb: Vec<u32>,
    matte: InteriorMatte,
    foreground: [u32; 256],
    timeline: Timeline,
    time: Instant,
    startup_placement: bool,
    pose: u32,
    error: Option<io::Error>,
    controls: Controls,
    resize_cursor: Option<ResizeEdge>,
}

impl Window {
    fn new(
        handle: Ptr,
        repeat: Repeat,
        appearance: WindowAppearance,
        cycle: Cycle,
    ) -> io::Result<Self> {
        let renderer = ReferenceFrame::new()?;
        let apertures = Apertures::new(renderer.occupied_bounds()?)?;
        Ok(Self {
            handle,
            renderer,
            apertures,
            appearance,
            light: vec![0; WIDTH * HEIGHT],
            alpha: vec![0; WIDTH * HEIGHT],
            argb: vec![0; WIDTH * HEIGHT],
            matte: InteriorMatte::new(),
            foreground: black_grade::palette().map(|rgb| argb(rgb, true)),
            timeline: Timeline { cycle, repeat },
            time: Instant::now(),
            startup_placement: true,
            pose: 0,
            error: None,
            controls: Controls::default(),
            resize_cursor: None,
        })
    }

    fn canvas(&self) -> Canvas {
        self.apertures.for_appearance(self.appearance)
    }

    fn select_appearance(&mut self, appearance: WindowAppearance) {
        let previous = self.canvas();
        let scale = self.placement().scale.max(MIN_SCALE);
        self.appearance = appearance;
        let canvas = self.canvas();
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
            self.controls.clear();
            self.update_resize_cursor(None);
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
        Placement::new(self.canvas(), width, height)
    }

    fn control_point(&self, event: Ptr) -> Option<(i32, i32, f64, f64)> {
        let (mut x, mut y) = (0., 0.);
        if unsafe { gdk_event_get_coords(event, &mut x, &mut y) } == 0 {
            return None;
        }
        let view = self.placement();
        if view.scale == 0.0 {
            return None;
        }
        let (x, y) = view.point(x, y);
        Some((view.size.0, view.size.1, x, y))
    }

    fn update_resize_cursor(&mut self, edge: Option<ResizeEdge>) {
        if edge == self.resize_cursor {
            return;
        }
        // The widget and its display live for the event loop's lifetime.
        unsafe {
            let window = gtk_widget_get_window(self.handle);
            if window.is_null() {
                return;
            }
            let cursor = edge.map_or(null_mut(), |edge| {
                gdk_cursor_new_from_name(gdk_window_get_display(window), edge.cursor().as_ptr())
            });
            gdk_window_set_cursor(window, cursor);
            if !cursor.is_null() {
                g_object_unref(cursor);
            }
        }
        self.resize_cursor = edge;
    }

    fn update(&mut self, pose: u32) -> io::Result<()> {
        self.renderer
            .render(pose, &mut self.light, Some(&mut self.alpha))?;
        for (pixel, (&gray, &alpha)) in self.argb.iter_mut().zip(self.light.iter().zip(&self.alpha))
        {
            *pixel = if alpha != 0 {
                self.foreground[usize::from(gray)]
            } else {
                0
            };
        }
        self.matte.apply(&self.light, &self.alpha, &mut self.argb);
        self.pose = pose;
        Ok(())
    }
}

fn argb([r, g, b]: [u8; 3], present: bool) -> u32 {
    if present {
        u32::from_be_bytes([255, r, g, b])
    } else {
        0
    }
}

pub(crate) fn run(repeat: Repeat, appearance: WindowAppearance, cycle: Cycle) -> io::Result<()> {
    // All GTK state and callbacks live on this thread. Signals and timer are
    // destroyed before their data (the Box) is released. The FFI adapter is
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
        {
            let screen = gtk_widget_get_screen(handle);
            let visual = gdk_screen_get_rgba_visual(screen);
            if visual.is_null() || gdk_screen_is_composited(screen) == 0 {
                gtk_widget_destroy(handle);
                return Err(io::Error::other(
                    "no composited RGBA surface; use oops --terminal",
                ));
            }
            gtk_widget_set_visual(handle, visual);
            // Use the application rail and hover controls, not OS chrome.
            gtk_window_set_decorated(handle, 0);
        }
        let mut state =
            match Window::new(handle, repeat, appearance, cycle).and_then(|mut state| {
                state.update(0)?;
                Ok(Box::new(state))
            }) {
                Ok(state) => state,
                Err(error) => {
                    gtk_widget_destroy(handle);
                    return Err(error);
                }
            };
        gtk_window_set_title(handle, c"Goodhand".as_ptr());
        gtk_widget_set_app_paintable(handle, 1);
        let canvas = state.canvas();
        let (min_width, min_height) = canvas.minimum();
        gtk_widget_set_size_request(handle, min_width, min_height);
        let (width, height) = canvas.client_at(1.0);
        gtk_window_set_default_size(handle, width, height);
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
        {
            // Documented GdkEventMask bits: motion, primary press/release,
            // enter and leave. Never query global pointer state.
            const POINTER_EVENTS: c_int = (1 << 2) | (1 << 8) | (1 << 9) | (1 << 12) | (1 << 13);
            gtk_widget_add_events(handle, POINTER_EVENTS);
            for (name, callback) in [
                (c"motion-notify-event", pointer as Signal),
                (c"enter-notify-event", pointer as Signal),
                (c"leave-notify-event", leave as Signal),
                (c"button-press-event", press as Signal),
                (c"button-release-event", release as Signal),
                (c"window-state-event", leave as Signal),
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
        }
        gtk_widget_show_all(handle);
        state.time = Instant::now();
        let timeout = g_timeout_add(6, tick, data);
        if timeout == 0 {
            gtk_widget_destroy(handle);
            return Err(io::Error::other("GTK playback timer could not start"));
        }
        gtk_main();
        g_source_remove(timeout);
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

unsafe extern "C" fn key(_widget: Ptr, event: Ptr, _data: Ptr) -> c_int {
    let mut code: c_uint = 0;
    unsafe {
        if gdk_event_get_keyval(event, &mut code) != 0 && matches!(code, 113 | 0xff1b) {
            gtk_main_quit();
            return 1;
        }
    }
    0
}

unsafe extern "C" fn pointer(widget: Ptr, event: Ptr, data: Ptr) -> c_int {
    let state = unsafe { &mut *data.cast::<Window>() };
    let next = state.control_point(event);
    let edge = if unsafe { gtk_window_is_maximized(widget) } == 0 {
        Controls::resize_edge(next)
    } else {
        None
    };
    state.update_resize_cursor(edge);
    if state.controls.pointer(next) {
        unsafe { gtk_widget_queue_draw(widget) };
    }
    0
}

unsafe extern "C" fn leave(widget: Ptr, _event: Ptr, data: Ptr) -> c_int {
    let state = unsafe { &mut *data.cast::<Window>() };
    state.update_resize_cursor(None);
    if state.controls.clear() {
        unsafe { gtk_widget_queue_draw(widget) };
    }
    0
}

unsafe extern "C" fn press(widget: Ptr, event: Ptr, data: Ptr) -> c_int {
    let mut button = 0;
    if unsafe { gdk_event_get_button(event, &mut button) } == 0 || button != 1 {
        return 0;
    }
    let state = unsafe { &mut *data.cast::<Window>() };
    let point = state.control_point(event);
    if state.controls.press(point) {
        unsafe { gtk_widget_queue_draw(widget) };
        return 1;
    }
    if unsafe { gtk_window_is_maximized(widget) } == 0 {
        let edge = Controls::resize_edge(point);
        if edge.is_some() || Controls::may_drag(point) {
            let (mut x, mut y) = (0., 0.);
            // Use the initiating event's root coordinates and timestamp;
            // GTK's window manager handles the move/resize gesture.
            unsafe {
                if gdk_event_get_root_coords(event, &mut x, &mut y) != 0 {
                    let (x, y) = (x.round() as i32, y.round() as i32);
                    let time = gdk_event_get_time(event);
                    if let Some(edge) = edge {
                        gtk_window_begin_resize_drag(widget, edge as c_int, 1, x, y, time);
                    } else {
                        gtk_window_begin_move_drag(widget, 1, x, y, time);
                    }
                    return 1;
                }
            }
        }
    }
    0
}

unsafe extern "C" fn release(widget: Ptr, event: Ptr, data: Ptr) -> c_int {
    let mut button = 0;
    if unsafe { gdk_event_get_button(event, &mut button) } == 0 || button != 1 {
        return 0;
    }
    let state = unsafe { &mut *data.cast::<Window>() };
    let point = state.control_point(event);
    let (handled, action) = state.controls.release(point);
    unsafe {
        match action {
            Some(Button::Close) => gtk_main_quit(),
            Some(Button::Resize) => {
                // A user choice overrides the initial placement preference.
                state.startup_placement = false;
                if gtk_window_is_maximized(widget) != 0 {
                    gtk_window_unmaximize(widget);
                } else {
                    gtk_window_maximize(widget);
                }
                gtk_widget_queue_draw(widget);
            }
            Some(Button::Appearance(appearance)) => {
                state.select_appearance(appearance);
                gtk_widget_queue_draw(widget);
            }
            None if handled => gtk_widget_queue_draw(widget),
            None => (),
        }
    }
    i32::from(handled)
}

unsafe extern "C" fn tick(data: Ptr) -> c_int {
    // GLib invokes timers serially on the GTK event-loop thread.
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    let elapsed = state.time.elapsed();
    if state.startup_placement {
        // Some window managers initially maximize even an undecorated
        // window with a natural size. Request floating once after map,
        // never override maximization during normal playback.
        if elapsed > START_PLACEMENT_GRACE {
            state.startup_placement = false;
        } else if unsafe { gtk_window_is_maximized(state.handle) } != 0 {
            state.startup_placement = false;
            unsafe { gtk_window_unmaximize(state.handle) };
        }
    }
    let Some(slot) = state.timeline.sample(elapsed) else {
        unsafe { gtk_main_quit() };
        return 1;
    };
    let Some(pose) = pose_at_tick(state.timeline.cycle, slot.phase.tick) else {
        return 1;
    };
    if pose != state.pose {
        if let Err(error) = state.update(pose) {
            state.error = Some(error);
            unsafe { gtk_main_quit() };
        } else {
            unsafe { gtk_widget_queue_draw(state.handle) };
        }
    }
    1
}

unsafe extern "C" fn draw(widget: Ptr, canvas: Ptr, data: Ptr) -> c_int {
    let state: &mut Window = unsafe { &mut *data.cast::<Window>() };
    // Borrowed image memory stays valid through paint. Restoring the Cairo
    // state releases its source pattern before the image and slice can change.
    unsafe {
        let maximized = gtk_window_is_maximized(widget) != 0;
        let view = state.placement();
        let layout = state.canvas();
        let (width, height) = view.size;
        let scale = view.scale;
        let corner = match state.appearance {
            WindowAppearance::Safe => BLACK_CORNER,
            WindowAppearance::Clear | WindowAppearance::Framed => PANEL_CORNER,
        };
        cairo_save(canvas);
        cairo_set_operator(canvas, SOURCE);
        cairo_set_source_rgba(canvas, 0., 0., 0., 0.);
        cairo_paint(canvas);
        if scale > 0.0 {
            let image = cairo_image_surface_create_for_data(
                state.argb.as_mut_ptr().cast(),
                ARGB32,
                WIDTH as c_int,
                HEIGHT as c_int,
                (4 * WIDTH) as c_int,
            );
            if cairo_surface_status(image) == 0 {
                cairo_translate(canvas, view.origin.0.into(), view.origin.1.into());
                if state.appearance != WindowAppearance::Clear {
                    // Fill precisely one neutral pane underneath unchanged
                    // occupied marks. Nothing reaches outside the panel.
                    let (shade, alpha) = match state.appearance {
                        WindowAppearance::Safe => (0., 1.0),
                        WindowAppearance::Framed => (0.04, 0.14),
                        WindowAppearance::Clear => unreachable!(),
                    };
                    cairo_set_operator(canvas, OVER);
                    panel_path(canvas, width.into(), height.into(), corner);
                    cairo_set_source_rgba(canvas, shade, shade, shade, alpha);
                    cairo_fill(canvas);
                }
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
                if state.controls.show_rail() {
                    draw_panel_rail(canvas, width.into(), height.into(), corner);
                }
                state
                    .controls
                    .draw(canvas, width, height, maximized, state.appearance);
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

/// Same rounded path for the pane underlay and its one-pixel boundary.
unsafe fn panel_path(context: Ptr, width: f64, height: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let (left, top) = (0.5, 0.5);
    let (right, bottom) = (width - 0.5, height - 0.5);
    unsafe {
        cairo_new_sub_path(context);
        cairo_arc(context, right - r, top + r, r, -FRAC_PI_2, 0.0);
        cairo_arc(context, right - r, bottom - r, r, 0.0, FRAC_PI_2);
        cairo_arc(context, left + r, bottom - r, r, FRAC_PI_2, PI);
        cairo_arc(context, left + r, top + r, r, PI, PI + FRAC_PI_2);
        cairo_close_path(context);
    }
}

unsafe fn draw_panel_rail(context: Ptr, width: f64, height: f64, corner: f64) {
    unsafe {
        cairo_set_operator(context, OVER);
        panel_path(context, width, height, corner);
        cairo_set_line_width(context, 1.0);
        cairo_set_source_rgba(context, 0.60, 0.60, 0.60, 0.34);
        cairo_stroke(context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_black_aperture_fits_native_art_in_the_sixteen_by_nine_client() {
        let a = Apertures::new(([47, 137], [954, 549])).unwrap();
        let black = a.for_appearance(WindowAppearance::default());
        assert_eq!(
            a.for_appearance(WindowAppearance::Clear).client_at(1.0),
            (1120, 560)
        );
        assert_eq!(
            (black.width, black.height, black.crop_top),
            (1016, 462, 120)
        );
        assert_eq!(black.client_at(1.0), (1024, 576));
        let p = Placement::new(black, 1024, 576);
        assert_eq!((p.size, p.scale, p.origin), ((1016, 462), 1.0, (4, 57)));
        assert_eq!(p.raster_origin(black), (8., -120.));
        let narrower = Placement::new(black, 1023, 576);
        let shorter = Placement::new(black, 1024, 575);
        assert!(narrower.scale < 1.0 && shorter.scale < 1.0);
        let (min_width, min_height) = black.minimum();
        assert_eq!((min_width, min_height), (772, 461));
        assert!(Placement::new(black, min_width, min_height).scale >= MIN_SCALE);
    }

    #[test]
    fn transparent_geometry_is_unchanged_and_oversize_black_art_stays_wide() {
        let a = Apertures::new(([47, 90], [954, 590])).unwrap();
        let clear = a.for_appearance(WindowAppearance::Clear);
        assert_eq!(clear.client_at(1.0), (1120, 560));
        assert_eq!(clear.minimum(), (840, 420));
        assert!(a.for_appearance(WindowAppearance::Framed) == clear);
        assert!(a.for_appearance(WindowAppearance::Safe) == clear);
    }

    #[test]
    fn resize_fits_reference_pane_in_any_viewport_without_stretching() {
        let canvas = Canvas {
            width: PANEL_WIDTH,
            height: PANEL_HEIGHT,
            crop_top: 40,
            inset: [0, 0],
        };
        for (width, height, panelsize, origin, scale) in [
            (1120, 560, (1120, 560), (0, 0), 1.0),
            (840, 420, (840, 420), (0, 0), 0.75),
            (900, 500, (900, 450), (0, 25), 45. / 56.),
            (1100, 430, (860, 430), (120, 0), 43. / 56.),
            (1600, 800, (1600, 800), (0, 0), 10. / 7.),
        ] {
            let p = Placement::new(canvas, width, height);
            assert_eq!((p.size, p.origin, p.scale), (panelsize, origin, scale));
            let (x, y) = p.raster_origin(canvas);
            assert!((x + WIDTH as f64 * scale / 2.0 - f64::from(p.size.0) / 2.0).abs() < 1e-12);
            assert_eq!(y + f64::from(canvas.crop_top) * scale, 0.);
            assert_eq!(p.point(f64::from(origin.0), f64::from(origin.1)), (0., 0.));
        }
    }

    #[test]
    fn nearest_downscale_keeps_a_sample_in_each_source_mark() {
        let spec = RasterSpec::REFERENCE;
        let a = Apertures::new(([47, 137], [954, 549])).unwrap();
        for canvas in [a.standard, a.black] {
            for height in (3 * canvas.height).div_ceil(4)..=canvas.height {
                let width = (height * canvas.width).div_ceil(canvas.height);
                let p = Placement::new(
                    canvas,
                    (width + 2 * canvas.inset[0]) as i32,
                    (height + 2 * canvas.inset[1]) as i32,
                );
                let ox = p.raster_origin(canvas);
                for (axis, offset, count) in [(0, ox.0, 200), (1, ox.1, 120)] {
                    for point in 0..count {
                        let source = spec.origin[axis] + point * spec.pitch[axis];
                        let start = offset + source as f64 * p.scale;
                        let end = start + spec.dot_size[axis] as f64 * p.scale;
                        // Every footprint has an output pixel center before
                        // clipping. Canvas has already verified the artwork fits.
                        let first_center = (start - 0.5).ceil() + 0.5;
                        assert!(first_center < end, "{axis}, {point}, {height}");
                    }
                }
            }
        }
    }

    #[test]
    fn black_stays_opaque_and_the_matte_never_changes_black_compositing() {
        assert_eq!(argb([0; 3], true), 0xff00_0000);
        assert_eq!(argb([49, 50, 51], true), 0xff31_3233);
        assert_eq!(argb([0; 3], false), 0);

        // The raster path is independent of the GTK handle.
        let mut state = Window::new(
            null_mut(),
            Repeat::Loop,
            WindowAppearance::Clear,
            Cycle::default(),
        )
        .unwrap();
        state.update(0).unwrap();
        assert_eq!((state.canvas().width, state.canvas().height), (1120, 560));
        let local = Placement::new(state.canvas(), 1120, 560);
        assert_eq!((local.scale, local.origin), (1.0, (0, 0)));
        assert!(state.alpha.contains(&0));
        assert!(state.alpha.contains(&255));
        let palette = black_grade::palette();
        for ((&pixel, &gray), &alpha) in state.argb.iter().zip(&state.light).zip(&state.alpha) {
            assert!(alpha == 0 || alpha == 255);
            if alpha != 0 {
                assert_eq!(pixel, argb(palette[usize::from(gray)], true));
            } else {
                // Compositing over black is identical to the original image.
                assert_eq!(pixel & 0xff_ff_ff, 0);
                assert!(pixel >> 24 <= u32::from(interior_matte::PEAK));
            }
        }
    }
}
