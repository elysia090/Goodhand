//! Corner controls use local GTK pointer events and never touch artwork.
use super::sys::*;
use crate::settings::WindowAppearance;
use std::f64::consts::TAU;
use std::ffi::CStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Button {
    Resize,
    Close,
    Appearance(WindowAppearance),
}

impl Button {
    fn center(self, width: i32) -> f64 {
        f64::from(width)
            - match self {
                Self::Resize => 48.,
                Self::Close => 20.,
                Self::Appearance(_) => unreachable!("appearance uses the other corner"),
            }
    }
}

/// GdkWindowEdge discriminants. The other corners are reserved for controls.
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResizeEdge {
    NorthWest = 0,
    North = 1,
    West = 3,
    East = 4,
    South = 6,
    SouthEast = 7,
}

impl ResizeEdge {
    pub(super) fn cursor(self) -> &'static CStr {
        match self {
            Self::NorthWest => c"nw-resize",
            Self::North => c"n-resize",
            Self::West => c"w-resize",
            Self::East => c"e-resize",
            Self::South => c"s-resize",
            Self::SouthEast => c"se-resize",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Zone {
    #[default]
    Hidden,
    Window,
    Appearance,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(super) struct Controls {
    zone: Zone,
    in_panel: bool,
    over: Option<Button>,
    down: Option<Button>,
}

type Point = Option<(i32, i32, f64, f64)>;
const Y: f64 = 20.;
const THEMES: [WindowAppearance; 3] = [
    WindowAppearance::Framed,
    WindowAppearance::Clear,
    WindowAppearance::Safe,
];

fn theme_x(index: usize) -> f64 {
    24. + 24. * index as f64
}

fn hit(point: Point) -> (Zone, Option<Button>) {
    let Some((width, height, x, y)) = point else {
        return (Zone::Hidden, None);
    };
    let from_bottom = f64::from(height) - y;
    if (-9. ..=54.).contains(&y) && (f64::from(width) - 110. ..=f64::from(width) + 9.).contains(&x)
    {
        let over = if (6. ..34.).contains(&y) {
            [Button::Resize, Button::Close]
                .into_iter()
                .find(|&button| (x - button.center(width)).abs() < 13.)
        } else {
            None
        };
        return (Zone::Window, over);
    }
    if (-9. ..=54.).contains(&from_bottom) && (-9. ..=102.).contains(&x) {
        let over = if (6. ..34.).contains(&from_bottom) {
            THEMES.iter().enumerate().find_map(|(i, &appearance)| {
                ((x - theme_x(i)).abs() < 12.).then_some(Button::Appearance(appearance))
            })
        } else {
            None
        };
        return (Zone::Appearance, over);
    }
    (Zone::Hidden, None)
}

impl Controls {
    pub(super) fn pointer(&mut self, point: Point) -> bool {
        let (zone, over) = hit(point);
        let old = (self.zone, self.over, self.show_rail());
        self.in_panel = point.is_some_and(|(w, h, x, y)| {
            (0.0..f64::from(w)).contains(&x) && (0.0..f64::from(h)).contains(&y)
        });
        (self.zone, self.over) = (zone, over);
        old != (self.zone, self.over, self.show_rail())
    }

    pub(super) fn show_rail(&self) -> bool {
        self.in_panel || self.zone != Zone::Hidden || self.down.is_some()
    }

    pub(super) fn resize_edge(point: Point) -> Option<ResizeEdge> {
        use ResizeEdge::*;
        let (width, height, x, y) = point?;
        let (w, h) = (f64::from(width), f64::from(height));
        if hit(point).0 != Zone::Hidden || !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
            return None;
        }
        const CORNER: f64 = 18.;
        const EDGE: f64 = 8.;
        if x < CORNER && y < CORNER {
            Some(NorthWest)
        } else if w - x <= CORNER && h - y <= CORNER {
            Some(SouthEast)
        } else if y < EDGE {
            Some(North)
        } else if h - y <= EDGE {
            Some(South)
        } else if x < EDGE {
            Some(West)
        } else if w - x <= EDGE {
            Some(East)
        } else {
            None
        }
    }

    pub(super) fn may_drag(point: Point) -> bool {
        let Some((w, _, x, y)) = point else {
            return false;
        };
        // Title-strip gesture, outside the resize/close hit zones.
        (0.0..f64::from(w)).contains(&x)
            && (0.0..48.0).contains(&y)
            && hit(point).0 == Zone::Hidden
            && Self::resize_edge(point).is_none()
    }

    pub(super) fn clear(&mut self) -> bool {
        let changed = *self != Self::default();
        *self = Self::default();
        changed
    }

    pub(super) fn press(&mut self, point: Point) -> bool {
        self.down = (self.zone == hit(point).0 && self.zone != Zone::Hidden)
            .then(|| hit(point).1)
            .flatten();
        self.down.is_some()
    }

    pub(super) fn release(&mut self, point: Point) -> (bool, Option<Button>) {
        let down = self.down.take();
        let (_, over) = hit(point);
        (down.is_some(), down.filter(|d| Some(*d) == over))
    }

    pub(super) unsafe fn draw(
        &self,
        ctx: Ptr,
        width: i32,
        height: i32,
        maximized: bool,
        appearance: WindowAppearance,
    ) {
        if self.zone == Zone::Hidden {
            return;
        }
        unsafe {
            if self.zone == Zone::Appearance {
                self.draw_appearance(ctx, height, appearance);
                return;
            }
            for button in [Button::Resize, Button::Close] {
                let x = button.center(width);
                let hot = self.over == Some(button);
                let down = self.down == Some(button) && hot;
                cairo_new_sub_path(ctx);
                cairo_arc(ctx, x, Y, 8.5, 0., TAU);
                match (button, down, hot) {
                    (Button::Close, true, _) => cairo_set_source_rgba(ctx, 0.62, 0.16, 0.18, 1.),
                    (Button::Close, _, true) => cairo_set_source_rgba(ctx, 0.94, 0.40, 0.41, 1.),
                    (Button::Close, _, _) => cairo_set_source_rgba(ctx, 0.85, 0.30, 0.33, 1.),
                    (_, true, _) => cairo_set_source_rgba(ctx, 0.23, 0.24, 0.27, 1.),
                    (_, _, true) => cairo_set_source_rgba(ctx, 0.43, 0.44, 0.47, 1.),
                    (_, _, _) => cairo_set_source_rgba(ctx, 0.30, 0.31, 0.34, 1.),
                }
                cairo_fill(ctx);
                cairo_set_source_rgba(ctx, 0.98, 0.98, 0.98, 1.);
                cairo_set_line_width(ctx, 1.);
                match button {
                    Button::Close => {
                        cairo_move_to(ctx, x - 3., Y - 3.);
                        cairo_line_to(ctx, x + 3., Y + 3.);
                        cairo_move_to(ctx, x + 3., Y - 3.);
                        cairo_line_to(ctx, x - 3., Y + 3.);
                    }
                    Button::Resize if maximized => {
                        cairo_move_to(ctx, x - 1.5, Y - 4.5);
                        cairo_line_to(ctx, x + 4.5, Y - 4.5);
                        cairo_line_to(ctx, x + 4.5, Y + 1.5);
                        cairo_rectangle(ctx, x - 4.5, Y - 1.5, 6., 6.);
                    }
                    Button::Resize => cairo_rectangle(ctx, x - 3.5, Y - 3.5, 7., 7.),
                    Button::Appearance(_) => unreachable!("separate lower-left picker"),
                }
                cairo_stroke(ctx);
            }
        }
    }

    /// Three separate targets, not a cycle. Accent only the active dash.
    unsafe fn draw_appearance(&self, ctx: Ptr, height: i32, active: WindowAppearance) {
        let y = f64::from(height) - Y + 0.5;
        unsafe {
            for (index, &choice) in THEMES.iter().enumerate() {
                let center = theme_x(index);
                let hot = self.over == Some(Button::Appearance(choice));
                let (length, stroke, level, opacity) = if choice == active {
                    (13., 2., 0.78, 0.94)
                } else if hot {
                    (11., 1., 0.67, 0.90)
                } else {
                    (9., 1., 0.51, 0.78)
                };
                cairo_set_source_rgba(ctx, level, level, level, opacity);
                cairo_set_line_width(ctx, stroke);
                cairo_move_to(ctx, center - length / 2.0, y);
                cairo_line_to(ctx, center + length / 2.0, y);
                cairo_stroke(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(x: f64, y: f64) -> Point {
        Some((1080, 576, x, y))
    }

    #[test]
    fn hover_holds_from_corner_to_buttons_and_hides_away() {
        let mut c = Controls::default();
        assert!(!c.show_rail());
        assert!(c.pointer(at(570., 300.)) && c.zone == Zone::Hidden && c.show_rail());
        assert!(!c.pointer(at(571., 300.)));
        assert!(c.pointer(at(990., 47.)) && c.zone == Zone::Window && c.over.is_none());
        assert!(
            c.pointer(at(1060., 20.)) && c.zone == Zone::Window && c.over == Some(Button::Close)
        );
        assert!(
            c.pointer(at(1032., 20.)) && c.zone == Zone::Window && c.over == Some(Button::Resize)
        );
        assert!(c.clear() && c.zone == Zone::Hidden && !c.show_rail());
    }

    #[test]
    fn dragging_does_not_steal_a_control_or_press_in_the_art() {
        assert!(Controls::may_drag(at(540., 24.)));
        assert!(!Controls::may_drag(at(1060., 20.)));
        assert!(!Controls::may_drag(at(540., 300.)));
        assert!(!Controls::may_drag(at(-0.1, 24.)));
        assert!(!Controls::may_drag(None));
    }

    #[test]
    fn resize_corners_and_edges_leave_visible_controls_and_move_strip_free() {
        use ResizeEdge::*;
        for (point, expected) in [
            (at(12., 12.), Some(NorthWest)),
            (at(1069., 565.), Some(SouthEast)),
            (at(550., 4.), Some(North)),
            (at(550., 574.), Some(South)),
            (at(2., 222.), Some(West)),
            (at(1078., 222.), Some(East)),
            (at(1060., 20.), None),
            (at(24., 556.), None),
            (at(540., 24.), None),
            (at(550., -1.), None),
            (at(1080., 260.), None),
        ] {
            assert_eq!(Controls::resize_edge(point), expected);
        }
        assert!(Controls::may_drag(at(540., 24.)));
        assert!(!Controls::may_drag(at(540., 4.)));
    }

    #[test]
    fn only_a_visible_button_released_in_its_own_hit_area_activates() {
        let mut c = Controls::default();
        assert!(!c.press(at(1060., 20.)));
        c.pointer(at(1060., 20.));
        assert!(c.press(at(1060., 20.)));
        assert_eq!(c.release(at(1032., 20.)), (true, None));
        assert!(c.press(at(1060., 20.)));
        assert_eq!(c.release(at(1060., 20.)), (true, Some(Button::Close)));
        assert!(c.press(at(1032., 20.)));
        assert_eq!(c.release(at(1032., 20.)), (true, Some(Button::Resize)));
    }
    #[test]
    fn the_three_lower_left_segments_are_direct_and_ordered() {
        let mut c = Controls::default();
        for (i, &appearance) in THEMES.iter().enumerate() {
            let point = at(theme_x(i), 556.);
            c.pointer(point);
            assert_eq!(c.zone, Zone::Appearance);
            assert!(c.press(point));
            assert_eq!(
                c.release(point),
                (true, Some(Button::Appearance(appearance)))
            );
        }
    }
}
