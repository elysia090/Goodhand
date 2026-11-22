//! GTK3, Cairo and GLib ABI used by the optional Linux window feature.
use std::ffi::{c_char, c_double, c_int, c_uint, c_ulong, c_void};
pub(super) type Ptr = *mut c_void;
pub(super) type Callback = unsafe extern "C" fn();

#[link(name = "libgtk-3.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    pub fn gtk_init_check(argc: *mut c_int, argv: *mut *mut *mut c_char) -> c_int;
    pub fn gtk_window_new(kind: c_int) -> Ptr;
    pub fn gtk_window_set_title(window: Ptr, title: *const c_char);
    pub fn gtk_window_is_maximized(window: Ptr) -> c_int;
    pub fn gtk_window_unmaximize(window: Ptr);
    pub fn gtk_widget_set_size_request(widget: Ptr, width: c_int, height: c_int);
    pub fn gtk_window_set_default_size(window: Ptr, width: c_int, height: c_int);
    pub fn gtk_window_resize(window: Ptr, width: c_int, height: c_int);
    pub fn gtk_widget_set_app_paintable(widget: Ptr, value: c_int);
    pub fn gtk_widget_get_screen(widget: Ptr) -> Ptr;
    pub fn gtk_widget_set_visual(widget: Ptr, visual: Ptr);
    pub fn gtk_widget_show_all(widget: Ptr);
    pub fn gtk_widget_get_allocated_width(widget: Ptr) -> c_int;
    pub fn gtk_widget_get_allocated_height(widget: Ptr) -> c_int;
    pub fn gtk_widget_queue_draw(widget: Ptr);
    pub fn gtk_widget_add_tick_callback(
        widget: Ptr,
        callback: unsafe extern "C" fn(Ptr, Ptr, Ptr) -> c_int,
        data: Ptr,
        destroy: Option<unsafe extern "C" fn(Ptr)>,
    ) -> c_uint;
    pub fn gtk_widget_remove_tick_callback(widget: Ptr, id: c_uint);
    pub fn gtk_widget_destroy(widget: Ptr);
    pub fn gtk_main();
    pub fn gtk_main_quit();
}

#[link(name = "libgdk-3.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    pub fn gdk_screen_is_composited(screen: Ptr) -> c_int;
    pub fn gdk_screen_get_rgba_visual(screen: Ptr) -> Ptr;
    pub fn gdk_event_get_keyval(event: Ptr, keyval: *mut c_uint) -> c_int;

}

#[link(name = "libgobject-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    pub fn g_signal_connect_data(
        instance: Ptr,
        signal: *const c_char,
        callback: Callback,
        data: Ptr,
        destroy: Option<unsafe extern "C" fn(Ptr, Ptr)>,
        flags: c_uint,
    ) -> c_ulong;
}

#[link(name = "libglib-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    pub fn g_timeout_add(
        interval: c_uint,
        callback: unsafe extern "C" fn(Ptr) -> c_int,
        data: Ptr,
    ) -> c_uint;
    pub fn g_source_remove(tag: c_uint) -> c_int;
}

pub(super) const ARGB32: c_int = 0;
pub(super) const SOURCE: c_int = 1;
pub(super) const OVER: c_int = 2;
pub(super) const NEAREST: c_int = 3;

#[cfg(goodhand_baseline_artwork)]
#[derive(Default)]
#[repr(C)]
pub(super) struct CairoMatrix {
    pub xx: c_double,
    pub yx: c_double,
    pub xy: c_double,
    pub yy: c_double,
    pub x0: c_double,
    pub y0: c_double,
}

#[link(name = "libcairo.so.2", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    pub fn cairo_image_surface_create_for_data(
        data: *mut u8,
        format: c_int,
        width: c_int,
        height: c_int,
        stride: c_int,
    ) -> Ptr;
    pub fn cairo_surface_status(surface: Ptr) -> c_int;
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_get_group_target(context: Ptr) -> Ptr;
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_surface_get_type(surface: Ptr) -> c_int;
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_get_matrix(context: Ptr, matrix: *mut CairoMatrix);
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_surface_get_device_scale(surface: Ptr, x: *mut c_double, y: *mut c_double);
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_surface_get_device_offset(surface: Ptr, x: *mut c_double, y: *mut c_double);
    #[cfg(goodhand_baseline_artwork)]
    pub fn cairo_surface_set_device_scale(surface: Ptr, x: c_double, y: c_double);
    #[cfg(all(test, goodhand_baseline_artwork))]
    pub fn cairo_create(surface: Ptr) -> Ptr;
    #[cfg(all(test, goodhand_baseline_artwork))]
    pub fn cairo_destroy(context: Ptr);
    #[cfg(all(test, goodhand_baseline_artwork))]
    pub fn cairo_surface_flush(surface: Ptr);
    pub fn cairo_save(context: Ptr);
    pub fn cairo_restore(context: Ptr);
    pub fn cairo_set_operator(context: Ptr, operator: c_int);
    pub fn cairo_set_source_rgba(context: Ptr, r: c_double, g: c_double, b: c_double, a: c_double);
    pub fn cairo_paint(context: Ptr);
    pub fn cairo_translate(context: Ptr, x: c_double, y: c_double);
    pub fn cairo_scale(context: Ptr, x: c_double, y: c_double);
    pub fn cairo_set_source_surface(context: Ptr, surface: Ptr, x: c_double, y: c_double);
    pub fn cairo_get_source(context: Ptr) -> Ptr;
    pub fn cairo_pattern_set_filter(pattern: Ptr, filter: c_int);
    pub fn cairo_rectangle(context: Ptr, x: c_double, y: c_double, w: c_double, h: c_double);
    pub fn cairo_clip(context: Ptr);
    pub fn cairo_fill(context: Ptr);
    pub fn cairo_surface_destroy(surface: Ptr);
}
