//! Apply the reviewed W5 value hierarchy without changing glyph geometry.
use super::{Layout, Size};
use crate::dots::{DotCell, Renderer};
use crate::settings::Settings;

pub(super) fn supports(layout: Layout) -> bool {
    let default = Settings::default();
    layout.display.size
        == (Size {
            cols: 116,
            rows: 41,
        })
        && (layout.cols, layout.rows, layout.aspect) == (115, 27, 0.5)
        && layout.settings.zoom == default.zoom
        && layout.settings.offset == default.offset
        && layout.settings.brightness == default.brightness
        && layout.settings.cycle == default.cycle
        && layout.settings.renderer == Renderer::Braille
}

pub(super) fn apply(layout: Layout, pose: u32, cells: &mut [Option<DotCell>]) {
    if !supports(layout) {
        return;
    }
    let Some(view) = pose.checked_sub(START).and_then(|n| VIEWS.get(n as usize)) else {
        return;
    };
    remap(
        cells,
        usize::from(layout.cols),
        usize::from(layout.rows),
        CENTER,
        *view,
        LIMITS,
        BANDS,
    );
}

include!("w5_coefficients.rs");

#[derive(Clone, Copy)]
struct Band<'a> {
    row: usize,
    stops: &'a [(i16, i16)],
}

#[derive(Clone, Copy)]
struct View {
    gain: f64,
    horizontal_scale: f64,
    center: f64,
}

#[derive(Clone, Copy)]
struct Limits {
    dark_floor: u8,
    bearing_start: u8,
    highlight_limit: u8,
}

/// Changes selected gray codes only. The caller supplies a reviewed
/// profile and ensures it is appropriate for its input and projection.
fn remap(
    cells: &mut [Option<DotCell>],
    width: usize,
    height: usize,
    reference_center: f64,
    view: View,
    limits: Limits,
    bands: &[Band<'_>],
) {
    if width < 3 || height < 3 || width.checked_mul(height) != Some(cells.len()) || view.gain == 0.0
    {
        return;
    }
    let display_col = |col: i16| {
        ((f64::from(col) - reference_center) * view.horizontal_scale + view.center)
            .round_ties_even() as isize
    };
    // Do not own a scratch buffer or allocate per frame. Every defined
    // band for the reviewed profile has at most eight control stops.
    for band in bands {
        if !(1..height - 1).contains(&band.row) || band.stops.len() < 2 {
            continue;
        }
        let (Some(first), Some(last)) = (band.stops.first(), band.stops.last()) else {
            continue;
        };
        let first = display_col(first.0).max(1) as usize;
        let last = display_col(last.0).min(width as isize - 2);
        if last < first as isize {
            continue;
        }
        let row = band.row;
        let mut lo = 0;
        for col in first..=last as usize {
            let pos = col as isize;
            // Select the rightmost coincident control stop, as in the
            // authored reference interpolation after viewport rounding.
            while lo + 1 < band.stops.len() && display_col(band.stops[lo + 1].0) <= pos {
                lo += 1;
            }
            let a = band.stops[lo];
            let level = if let Some(&b) = band.stops.get(lo + 1) {
                let x = display_col(a.0);
                let length = display_col(b.0) - x;
                debug_assert!(length > 0);
                let slope = f64::from(b.1 - a.1) / length as f64;
                slope * (pos - x) as f64 + f64::from(a.1)
            } else {
                f64::from(a.1)
            };
            let change = (view.gain * level).round_ties_even() as i16;
            if change == 0 || !has_single_dot_halo(cells, width, row, col) {
                continue;
            }
            if let Some(cell) = &mut cells[row * width + col] {
                cell.gray = apply_limit(cell.gray, change, limits);
            }
        }
    }
}

fn apply_limit(gray: u8, change: i16, limits: Limits) -> u8 {
    let gray = i16::from(gray);
    let change = if change < 0 {
        // Preserve the authored deepest darks even when the band crosses them.
        change.max((i16::from(limits.dark_floor) - gray).min(0))
    } else {
        // Lift the bearing half-tone only; zero at both range boundaries.
        change
            .min((gray - i16::from(limits.bearing_start)).max(0))
            .min((i16::from(limits.highlight_limit) - gray).max(0))
    };
    (gray + change) as u8
}

fn has_single_dot_halo(cells: &[Option<DotCell>], width: usize, row: usize, col: usize) -> bool {
    let i = row * width + col;
    let has = |at: usize, required: u8| {
        cells[at].is_some_and(|cell| cell.pattern.get() & required == required)
    };
    // A full center and four directly adjacent single-dot edges. Corners
    // are excluded: the reference protects four translated rectangles.
    const FULL: u8 = 0xff;
    const BOTTOM_ROW: u8 = (1 << 6) | (1 << 7);
    const TOP_ROW: u8 = (1 << 0) | (1 << 3);
    const RIGHT_COLUMN: u8 = (1 << 3) | (1 << 4) | (1 << 5) | (1 << 7);
    const LEFT_COLUMN: u8 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 6);
    has(i, FULL)
        && has(i - width, BOTTOM_ROW)
        && has(i + width, TOP_ROW)
        && has(i - 1, RIGHT_COLUMN)
        && has(i + 1, LEFT_COLUMN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU8;

    fn cell(bits: u8, gray: u8) -> Option<DotCell> {
        NonZeroU8::new(bits).map(|pattern| DotCell { pattern, gray })
    }

    #[test]
    fn only_a_fully_supported_interior_changes() {
        let mut subject = [
            None,
            cell(0xc0, 20),
            None,
            cell(0xb8, 20),
            cell(255, 112),
            cell(0x47, 20),
            None,
            cell(0x09, 20),
            None,
        ];
        let original = subject;
        let profile = [Band {
            row: 1,
            stops: &[(0, 0), (1, 10), (2, 0)],
        }];
        let view = View {
            gain: 1.0,
            horizontal_scale: 1.0,
            center: 0.0,
        };
        let limits = Limits {
            dark_floor: 77,
            bearing_start: 96,
            highlight_limit: 144,
        };
        remap(&mut subject, 3, 3, 0.0, view, limits, &profile);
        assert_eq!(subject[4], cell(255, 122));
        for (n, v) in original.into_iter().enumerate() {
            if n != 4 {
                assert_eq!(subject[n], v);
            }
        }
        subject[5] = cell(0x07, 20);
        let held = subject;
        remap(&mut subject, 3, 3, 0.0, view, limits, &profile);
        assert_eq!(subject, held);
    }
    // Synthetic terminal cells are sufficient to validate the posture and
    // viewport contract without shipping reference frames or private tests.
    fn fixture_layout(cols: u16, rows: u16, settings: Settings) -> Layout {
        let display = super::super::Display {
            size: Size { cols, rows },
            pixels: (0, 0),
        };
        Layout::for_surface(display, settings, (230.0, 108.0), None)
    }

    #[test]
    fn the_main_cut_is_a_value_change_not_a_new_glyph() {
        let layout = fixture_layout(116, 41, Settings::default());
        assert!(supports(layout));
        let mut subject = vec![cell(255, 119); 115 * 27];
        let before = subject.clone();
        apply(layout, 168, &mut subject);
        assert_eq!(subject[16 * 115 + 76], cell(255, 95));
        assert_eq!(subject[19 * 115 + 80], cell(255, 130));
        for (i, (old, new)) in before.iter().zip(&subject).enumerate() {
            assert_eq!(old.map(|v| v.pattern), new.map(|v| v.pattern));
            let row = i / 115;
            let col = i % 115;
            if !(14..=21).contains(&row) || !(57..96).contains(&col) {
                assert_eq!(old, new);
            }
        }
    }

    #[test]
    fn only_the_reviewed_pose_interval_can_change() {
        use crate::archived_scene::pose_at_tick;
        let layout = fixture_layout(116, 41, Settings::default());
        let before = vec![cell(255, 119); 115 * 27];
        let mut changed = std::collections::BTreeSet::new();
        for tick in 0..layout.settings.cycle.ticks() {
            let Some(pose) = pose_at_tick(layout.settings.cycle, tick) else {
                unreachable!()
            };
            let mut subject = before.clone();
            apply(layout, pose, &mut subject);
            if subject != before {
                changed.insert(pose);
            }
        }
        assert!(changed.into_iter().eq(157..180));
    }

    #[test]
    fn other_apertures_and_playback_controls_keep_stock() {
        let stock = Settings::default();
        let mut forward = stock;
        forward.cycle.direction = crate::playback::Direction::Forward;
        let mut duration = stock;
        duration.cycle.duration = "24".parse().unwrap();
        let mut zoom = stock;
        zoom.zoom = "105".parse().unwrap();
        let mut offset = stock;
        offset.offset[0] = "1".parse().unwrap();
        let mut brightness = stock;
        brightness.brightness = "90".parse().unwrap();
        let mut aspect = stock;
        aspect.cell_aspect = "0.53".parse().unwrap();
        let mut dots = stock;
        dots.renderer = Renderer::Dots;
        let mut layouts = vec![
            fixture_layout(115, 41, stock),
            fixture_layout(116, 40, stock),
        ];
        layouts.extend(
            [forward, duration, zoom, offset, brightness, aspect, dots]
                .map(|settings| fixture_layout(116, 41, settings)),
        );
        let before = vec![cell(255, 119); 115 * 27];
        for layout in layouts {
            assert!(!supports(layout));
            let mut subject = before.clone();
            apply(layout, 168, &mut subject);
            assert_eq!(subject, before);
        }
    }
}
