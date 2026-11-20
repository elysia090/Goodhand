//! A visual mask over the real Xfwm frame. It takes no input and never
//! changes decorations, the client layout, or the window manager's settings.
use super::BACKDROP_RGB;
use super::sys::{Ptr, gtk_widget_get_screen};
use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulong};
use std::ptr::null;
use std::time::{Duration, Instant};

type Xid = c_ulong;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rectangle {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

// Public Xlib ABIs, used by XCreateWindow and XGetWindowAttributes.
#[repr(C)]
#[derive(Default)]
struct SetAttributes {
    background_pixmap: Xid,
    background_pixel: c_ulong,
    border_pixmap: Xid,
    border_pixel: c_ulong,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    colormap: Xid,
    cursor: Xid,
}
#[repr(C)]
struct Visual {
    ext_data: Ptr,
    id: Xid,
    class: c_int,
    red: c_ulong,
    green: c_ulong,
    blue: c_ulong,
    bits_per_rgb: c_int,
    entries: c_int,
}
#[repr(C)]
struct Attributes {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    depth: c_int,
    visual: *const Visual,
    root: Xid,
    class: c_int,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    colormap: Xid,
    map_installed: c_int,
    map_state: c_int,
    all_event_masks: c_long,
    your_event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    screen: Ptr,
}

#[link(name = "libX11.so.6", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn XQueryTree(
        d: Ptr,
        w: Xid,
        root: *mut Xid,
        parent: *mut Xid,
        children: *mut *mut Xid,
        count: *mut c_uint,
    ) -> c_int;
    fn XGetWindowAttributes(d: Ptr, w: Xid, attributes: *mut Attributes) -> c_int;
    fn XCreateWindow(
        d: Ptr,
        parent: Xid,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border: c_uint,
        depth: c_int,
        class: c_uint,
        visual: *const Visual,
        value_mask: c_ulong,
        attrs: *const SetAttributes,
    ) -> Xid;
    fn XDestroyWindow(d: Ptr, w: Xid) -> c_int;
    fn XResizeWindow(d: Ptr, w: Xid, width: c_uint, height: c_uint) -> c_int;
    fn XMapRaised(d: Ptr, w: Xid) -> c_int;
    fn XUnmapWindow(d: Ptr, w: Xid) -> c_int;
    fn XQueryPointer(
        d: Ptr,
        w: Xid,
        root: *mut Xid,
        child: *mut Xid,
        x: *mut c_int,
        y: *mut c_int,
        wx: *mut c_int,
        wy: *mut c_int,
        mask: *mut c_uint,
    ) -> c_int;
    fn XFree(value: Ptr) -> c_int;
}

#[link(name = "libXext.so.6", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn XShapeQueryVersion(d: Ptr, major: *mut c_int, minor: *mut c_int) -> c_int;
    fn XShapeCombineRectangles(
        d: Ptr,
        w: Xid,
        kind: c_int,
        dx: c_int,
        dy: c_int,
        rects: *const Rectangle,
        n: c_int,
        operation: c_int,
        ordering: c_int,
    );
}
#[link(name = "libgdk-3.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn gdk_x11_display_get_type() -> usize;
    fn gdk_x11_display_get_xdisplay(display: Ptr) -> Ptr;
    fn gdk_x11_window_get_xid(window: Ptr) -> Xid;
    fn gdk_x11_screen_get_window_manager_name(screen: Ptr) -> *const c_char;
    fn gdk_x11_display_error_trap_push(display: Ptr);
    fn gdk_x11_display_error_trap_pop(display: Ptr) -> c_int;
}
#[link(name = "libgtk-3.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn gtk_widget_get_display(widget: Ptr) -> Ptr;
    fn gtk_widget_get_window(widget: Ptr) -> Ptr;
    fn gtk_widget_get_settings(widget: Ptr) -> Ptr;
}
#[link(name = "libgobject-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn g_type_check_instance_is_a(instance: Ptr, type_: usize) -> c_int;
    fn g_object_get(instance: Ptr, first: *const c_char, ...);
}
#[link(name = "libglib-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn g_free(value: Ptr);
}

const SHAPE_BOUNDING: c_int = 0;
const SHAPE_INPUT: c_int = 2;
const SHAPE_SET: c_int = 0;
const UNSORTED: c_int = 0;
const CW_BACK_PIXEL: c_ulong = 1 << 1;
const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;
const VIEWABLE: c_int = 2;
const BUTTONS: c_uint = 0x1f00;

fn outside_client([w, h]: [c_int; 2], [x, y, iw, ih]: [c_int; 4]) -> Option<[Rectangle; 4]> {
    if w > i16::MAX.into()
        || h > i16::MAX.into()
        || w < 1
        || h < 1
        || x < 0
        || y < 0
        || iw < 1
        || ih < 1
        || x.checked_add(iw)? > w
        || y.checked_add(ih)? > h
    {
        return None;
    }
    let rect = |x, y, width, height| Rectangle {
        x,
        y,
        width,
        height,
    };
    let (x, y, right, bottom, w, h) = (
        x as i16,
        y as i16,
        (x + iw) as i16,
        (y + ih) as i16,
        w as i16,
        h as i16,
    );
    Some([
        rect(0, 0, w as u16, y as u16),
        rect(0, y, x as u16, ih as u16),
        rect(right, y, (w - right) as u16, ih as u16),
        rect(0, bottom, w as u16, (h - bottom) as u16),
    ])
}

struct X11 {
    gdk: Ptr,
    display: Ptr,
    client: Xid,
}

impl X11 {
    fn new(widget: Ptr) -> Option<Self> {
        unsafe {
            let gdk = gtk_widget_get_display(widget);
            let gwindow = gtk_widget_get_window(widget);
            if gdk.is_null()
                || gwindow.is_null()
                || g_type_check_instance_is_a(gdk, gdk_x11_display_get_type()) == 0
            {
                return None;
            }
            let display = gdk_x11_display_get_xdisplay(gdk);
            let client = gdk_x11_window_get_xid(gwindow);
            if display.is_null() || client == 0 {
                return None;
            }
            let mut version = (0, 0);
            (XShapeQueryVersion(display, &mut version.0, &mut version.1) != 0 && version >= (1, 1))
                .then_some(Self {
                    gdk,
                    display,
                    client,
                })
        }
    }

    fn trap<T>(&self, work: impl FnOnce() -> T) -> Option<T> {
        // Keep expected races (closing the window, a frame being replaced)
        // out of GDK's fatal X error path. Never install a global handler.
        unsafe { gdk_x11_display_error_trap_push(self.gdk) };
        let result = work();
        (unsafe { gdk_x11_display_error_trap_pop(self.gdk) } == 0).then_some(result)
    }

    fn parent(&self) -> Option<(Xid, Xid)> {
        let (mut root, mut frame, mut count, mut children) = (0, 0, 0, std::ptr::null_mut());
        let ok = unsafe {
            XQueryTree(
                self.display,
                self.client,
                &mut root,
                &mut frame,
                &mut children,
                &mut count,
            )
        };
        if !children.is_null() {
            unsafe { XFree(children.cast()) };
        }
        (ok != 0 && frame != 0 && frame != root).then_some((root, frame))
    }

    fn attrs(&self, w: Xid) -> Option<Attributes> {
        let mut value = std::mem::MaybeUninit::uninit();
        (unsafe { XGetWindowAttributes(self.display, w, value.as_mut_ptr()) } != 0)
            .then(|| unsafe { value.assume_init() })
    }

    fn pointer(&self, w: Xid) -> Option<(Xid, bool)> {
        let (mut root, mut child, mut x, mut y, mut wx, mut wy, mut mask) = (0, 0, 0, 0, 0, 0, 0);
        let ok = unsafe {
            XQueryPointer(
                self.display,
                w,
                &mut root,
                &mut child,
                &mut x,
                &mut y,
                &mut wx,
                &mut wy,
                &mut mask,
            )
        };
        (ok != 0).then_some((child, mask & BUTTONS != 0))
    }
}

#[derive(Default)]
pub(super) struct Chrome {
    x11: Option<X11>,
    checked: bool,
    next_check: Option<Instant>,
    keyboard_grace: Option<Instant>,
    mask: Option<Mask>,
}
struct Mask {
    frame: Xid,
    id: Xid,
    size: [c_int; 2],
    outline: [Rectangle; 4],
    visible: bool,
}

impl Chrome {
    pub(super) fn reveal_for_keyboard(&mut self) {
        self.keyboard_grace = Some(Instant::now() + Duration::from_millis(1500));
        self.next_check = None;
        self.hide();
    }

    pub(super) fn hide(&mut self) {
        if let (Some(x), Some(mask)) = (&self.x11, &mut self.mask) {
            if mask.visible {
                let _ = x.trap(|| unsafe { XUnmapWindow(x.display, mask.id) });
                mask.visible = false;
            }
        }
    }

    pub(super) fn update(&mut self, widget: Ptr, now: Instant, use_mask: bool) {
        // Do not couple artwork phase changes to pointer position. The only
        // idle poll is cheap and bounded; no repaint when nothing changes.
        if !use_mask {
            self.hide();
            return;
        }
        if self.next_check.is_some_and(|deadline| now < deadline) {
            return;
        }
        self.next_check = Some(now + Duration::from_millis(32));
        if !self.checked {
            self.x11 = X11::new(widget);
            self.checked = true;
        }
        let Some(x) = &self.x11 else {
            return;
        };
        if !supported(widget) || self.keyboard_grace.is_some_and(|end| now < end) {
            self.hide();
            return;
        }
        let next = x.trap(|| reconcile(x, &mut self.mask));
        if next.flatten().is_none() {
            // If a queried frame changed or vanished, unmask rather than
            // guessing. Any old child is destroyed, never moved elsewhere.
            self.destroy();
        }
    }

    pub(super) fn destroy(&mut self) {
        if let (Some(x), Some(mask)) = (&self.x11, self.mask.take()) {
            let _ = x.trap(|| unsafe { XDestroyWindow(x.display, mask.id) });
        }
    }
}

fn supported(widget: Ptr) -> bool {
    // Opt in only to the tested reparenting WM. Wayland/other X11 WMs
    // retain their own visible native decoration. HighContrast stays visible.
    unsafe {
        let name = gdk_x11_screen_get_window_manager_name(gtk_widget_get_screen(widget));
        if name.is_null()
            || !CStr::from_ptr(name)
                .to_bytes()
                .eq_ignore_ascii_case(b"xfwm4")
        {
            return false;
        }
        let settings = gtk_widget_get_settings(widget);
        if settings.is_null() {
            return false;
        }
        let mut theme: *mut c_char = std::ptr::null_mut();
        g_object_get(
            settings,
            c"gtk-theme-name".as_ptr(),
            &mut theme,
            null::<c_char>(),
        );
        if theme.is_null() {
            return false;
        }
        let compact: Vec<_> = CStr::from_ptr(theme)
            .to_bytes()
            .iter()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(u8::to_ascii_lowercase)
            .collect();
        g_free(theme.cast());
        !compact.windows(12).any(|part| part == b"highcontrast")
    }
}

fn reconcile(x: &X11, current: &mut Option<Mask>) -> Option<()> {
    let (root, frame) = x.parent()?;
    if current.as_ref().is_some_and(|mask| mask.frame != frame) {
        return None;
    }
    let (under, dragging) = x.pointer(root)?;
    if under == frame || dragging {
        if let Some(mask) = current.as_mut().filter(|mask| mask.visible) {
            unsafe { XUnmapWindow(x.display, mask.id) };
            mask.visible = false;
        }
        return Some(());
    }
    let view = x.attrs(frame)?;
    let client = x.attrs(x.client)?;
    if view.map_state != VIEWABLE
        || client.map_state != VIEWABLE
        || view.border_width != 0
        || client.border_width != 0
        || !matches!(view.depth, 24 | 32)
        || view.visual.is_null()
        || view.x < i16::MIN.into()
        || view.y < i16::MIN.into()
    {
        return None;
    }
    let visual = unsafe { &*view.visual };
    if visual.class != 4
        || [visual.red, visual.green, visual.blue] != [0xff0000, 0x00ff00, 0x0000ff]
    {
        return None;
    }
    let size = [view.width, view.height];
    let outline = outside_client(size, [client.x, client.y, client.width, client.height])?;
    if outline
        .iter()
        .all(|rect| rect.width == 0 || rect.height == 0)
    {
        return None;
    }
    if let Some(mask) = current.as_mut() {
        if mask.size != size {
            unsafe { XResizeWindow(x.display, mask.id, size[0] as c_uint, size[1] as c_uint) };
        }
        if mask.outline != outline {
            set_outline(x, mask.id, &outline);
        }
        if !mask.visible {
            unsafe { XMapRaised(x.display, mask.id) };
        }
        mask.size = size;
        mask.outline = outline;
        mask.visible = true;
    } else {
        let argb = u32::from_be_bytes([255, BACKDROP_RGB[0], BACKDROP_RGB[1], BACKDROP_RGB[2]]);
        let attrs = SetAttributes {
            background_pixel: c_ulong::from(argb),
            override_redirect: 1,
            ..SetAttributes::default()
        };
        let id = unsafe {
            // CopyFromParent visual, depth and class; the verified frame
            // clips its own round corners, no additional OS top level.
            XCreateWindow(
                x.display,
                frame,
                0,
                0,
                size[0] as c_uint,
                size[1] as c_uint,
                0,
                0,
                0,
                null(),
                CW_BACK_PIXEL | CW_OVERRIDE_REDIRECT,
                &attrs,
            )
        };
        if id == 0 {
            return None;
        }
        unsafe {
            XShapeCombineRectangles(
                x.display,
                id,
                SHAPE_INPUT,
                0,
                0,
                null(),
                0,
                SHAPE_SET,
                UNSORTED,
            );
        }
        set_outline(x, id, &outline);
        unsafe { XMapRaised(x.display, id) };
        *current = Some(Mask {
            frame,
            id,
            size,
            outline,
            visible: true,
        });
    }
    Some(())
}

fn set_outline(x: &X11, id: Xid, outline: &[Rectangle; 4]) {
    let shapes: Vec<_> = outline
        .iter()
        .copied()
        .filter(|r| r.width != 0 && r.height != 0)
        .collect();
    unsafe {
        XShapeCombineRectangles(
            x.display,
            id,
            SHAPE_BOUNDING,
            0,
            0,
            shapes.as_ptr(),
            shapes.len() as c_int,
            SHAPE_SET,
            UNSORTED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_covers_only_native_frame() {
        let shapes = outside_client([671, 326], [4, 32, 663, 290]).unwrap();
        for y in 0..326 {
            for x in 0..671 {
                let covers = shapes
                    .iter()
                    .filter(|r| {
                        r.x <= x
                            && x - r.x < r.width as i16
                            && r.y <= y
                            && y - r.y < r.height as i16
                    })
                    .count();
                assert_eq!(
                    covers,
                    usize::from(!(4..667).contains(&x) || !(32..322).contains(&y))
                );
            }
        }
        assert!(outside_client([671, 326], [4, 32, 670, 290]).is_none());
        assert!(outside_client([671, 326], [4, 32, 663, i32::MAX]).is_none());
    }
}
