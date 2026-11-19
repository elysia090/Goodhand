//! Terminal projection and encoding, independent of which scene supplies the dots.
#[cfg(goodhand_embedded_playback)]
use crate::archived_scene::Archive;
use crate::dots::DotCell;
use crate::projection::Projection;
use crate::settings::{CellAspect, MAX_ASPECT, MIN_ASPECT, Settings};
#[cfg(not(goodhand_embedded_playback))]
use goodhand::ArmSampler;
#[cfg(all(test, goodhand_embedded_playback))]
use goodhand::RasterSpec;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;
#[cfg(goodhand_embedded_playback)]
pub const CONTENT_LABEL: &str = crate::archived_scene::LABEL;
#[cfg(not(goodhand_embedded_playback))]
pub const CONTENT_LABEL: &str = "procedural arm";

#[cfg(all(test, goodhand_embedded_playback))]
const FRAME_SIZE: (f64, f64) = (
    RasterSpec::REFERENCE.width as f64,
    RasterSpec::REFERENCE.height as f64,
);
#[cfg(not(goodhand_embedded_playback))]
const FRAME_SIZE: (f64, f64) = (ArmSampler::FRAME_WIDTH, ArmSampler::FRAME_HEIGHT);

const MAX_COLS: u16 = 220;
const MAX_ROWS: u16 = 80;
// Cancel any partial escape sequence, restore our black canvas, then redraw
// on the already-owned alternate screen. Do not enter it again.
const RESYNC: &[u8] = b"\x18\x1b[0m\x1b[48;2;0;0;0m\x1b[2J";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Size {
    pub fn fits(self) -> bool {
        self.cols >= MIN_WIDTH && self.rows >= MIN_HEIGHT
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Display {
    pub size: Size,
    pub pixels: (u16, u16),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PitchSource {
    Measured,
    Assumed,
    Configured,
}

impl PitchSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Measured => "tty",
            Self::Assumed => "assumed",
            Self::Configured => "configured",
        }
    }
}

/// Physical rows bounded by the saved top and the live row below the art.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineRows {
    pub top: u16,
    pub below: u16,
}

pub fn reservation(size: Size) -> u16 {
    // Leave room for the command and neighboring shell output, even
    // when the command launched at the viewport bottom.
    ((u32::from(size.rows) * 3 / 4) as u16).min(MAX_ROWS)
}

pub fn clear_rows(packet: &mut Vec<u8>, rows: InlineRows) {
    for row in rows.top..rows.below {
        cup(packet, row, 1);
        // At column 1, EL clears this whole owned row. Unlike EL2,
        // VTE does not fill the row to full width with blank cells.
        packet.extend_from_slice(b"\x1b[K");
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub cols: u16,
    pub rows: u16,
    pub aspect: f64,
    pub pitch: PitchSource,
    pub settings: Settings,
    display: Display,
    frame_size: (f64, f64),
    left: u16,
    top: u16,
    inline: Option<InlineRows>,
}

impl Layout {
    #[cfg(test)]
    pub fn fit(display: Display, settings: Settings) -> Self {
        Self::for_surface(display, settings, FRAME_SIZE, None)
    }

    pub fn for_surface(
        display: Display,
        settings: Settings,
        frame_size: (f64, f64),
        inline: Option<InlineRows>,
    ) -> Self {
        debug_assert!(display.size.fits());
        let Size { cols, rows } = display.size;
        let (px, py) = display.pixels;
        let (aspect, pitch) = match settings.cell_aspect {
            CellAspect::Fixed(value) => (value, PitchSource::Configured),
            CellAspect::Auto => {
                let measured =
                    (f64::from(px) * f64::from(rows)) / (f64::from(py) * f64::from(cols));
                if (MIN_ASPECT..=MAX_ASPECT).contains(&measured)
                    && u32::from(px) >= u32::from(cols) * 3
                    && u32::from(py) >= u32::from(rows) * 5
                {
                    (measured, PitchSource::Measured)
                } else {
                    (0.5, PitchSource::Assumed)
                }
            }
        };

        let available_cols = (cols - 1).min(MAX_COLS);
        let inline_height = reservation(display.size);
        let available_rows = inline.map_or((rows - 3).min(MAX_ROWS), |area| {
            (area.below - area.top).min(inline_height)
        });
        let ratio = frame_size.0 / (frame_size.1 * aspect);
        let picture_cols = available_cols.min((f64::from(available_rows) * ratio).floor() as u16);
        let picture_rows = available_rows.min((f64::from(available_cols) / ratio).floor() as u16);
        Self {
            cols: picture_cols,
            rows: picture_rows,
            aspect,
            pitch,
            settings,
            display,
            frame_size,
            left: 1 + (cols - 1 - picture_cols) / 2,
            top: inline.map_or_else(|| 2 + (rows - 3 - picture_rows) / 2, |area| area.top),
            inline,
        }
    }

    fn projection(self) -> io::Result<Projection> {
        let [x, y] = self.settings.renderer.subdivisions();
        Projection::new(
            usize::from(self.cols) * x,
            usize::from(self.rows) * y,
            self.frame_size,
            self.aspect * y as f64 / x as f64,
            self.settings.zoom.factor(),
            self.settings.offset,
        )
    }
}

pub struct Painter {
    #[cfg(goodhand_embedded_playback)]
    archive: Archive,
    old: Vec<Option<DotCell>>,
    new: Vec<Option<DotCell>>,
    samples: Vec<Option<u8>>,
    #[cfg(not(goodhand_embedded_playback))]
    procedural_grid: Vec<Option<u8>>,
    old_layout: Option<Layout>,
    needs_resync: bool,
    drawn_right: u16,
}

impl Painter {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            #[cfg(goodhand_embedded_playback)]
            archive: Archive::embedded()?,
            old: Vec::new(),
            new: Vec::new(),
            samples: Vec::new(),
            #[cfg(not(goodhand_embedded_playback))]
            procedural_grid: Vec::new(),
            old_layout: None,
            needs_resync: false,
            drawn_right: 0,
        })
    }

    pub fn frame_size(&self) -> (f64, f64) {
        #[cfg(goodhand_embedded_playback)]
        return self.archive.frame_size();
        #[cfg(not(goodhand_embedded_playback))]
        FRAME_SIZE
    }

    pub fn paint(
        &mut self,
        tick: u64,
        layout: Layout,
        packet: &mut Vec<u8>,
    ) -> io::Result<Duration> {
        let trace_start = Instant::now();
        let projection = layout.projection()?;
        self.samples.resize(projection.cols * projection.rows, None);
        let settings = layout.settings;
        #[cfg(goodhand_embedded_playback)]
        self.archive
            .project(settings.cycle, tick, projection, &mut self.samples)?;
        #[cfg(not(goodhand_embedded_playback))]
        {
            let sampler = ArmSampler::new(settings.cycle.yaw(tick)).map_err(io::Error::other)?;
            project_procedural(
                &sampler,
                projection,
                &mut self.samples,
                &mut self.procedural_grid,
            );
        }
        let cols = usize::from(layout.cols);
        self.new.resize(cols * usize::from(layout.rows), None);
        settings.renderer.pack(&self.samples, cols, &mut self.new);
        // Adjust art gray only, never the presence of a dot or the UI colors.
        if settings.brightness != crate::settings::Brightness::default() {
            for cell in self.new.iter_mut().flatten() {
                cell.gray = settings.brightness.apply(cell.gray);
            }
        }
        let trace_time = trace_start.elapsed();

        packet.clear();
        let same_layout = !self.needs_resync && self.old_layout == Some(layout);
        if !same_layout {
            if let Some(area) = layout.inline {
                if self.needs_resync || self.old_layout.is_some() {
                    packet.extend_from_slice(b"\x18\x1b[0m");
                    clear_rows(packet, area);
                }
            } else if self.needs_resync {
                packet.extend_from_slice(RESYNC);
            } else if self.old_layout.is_some() {
                packet.extend_from_slice(b"\x1b[2J");
            }
        }

        let mut active = None;
        let mut right = 0;
        let inline = layout.inline.is_some();
        for (row, pixels) in self.new.chunks_exact(cols).enumerate() {
            let limit = if inline {
                pixels
                    .iter()
                    .rposition(Option::is_some)
                    .map_or(0, |col| col + 1)
            } else {
                cols
            };
            if limit > 0 {
                right = right.max(layout.left + limit as u16 - 1);
            }
            let y = layout.top + row as u16;
            let changed = |col| {
                let old = if same_layout {
                    self.old[row * cols + col]
                } else {
                    None
                };
                old != pixels[col]
            };
            let mut col = 0;
            while col < limit {
                if !changed(col) {
                    col += 1;
                    continue;
                }
                cup(packet, y, layout.left + col as u16);
                loop {
                    match pixels[col] {
                        Some(cell) => {
                            if active != Some(cell.gray) {
                                gray(packet, cell.gray);
                                active = Some(cell.gray);
                            }
                            let mut utf8 = [0; 4];
                            packet.extend_from_slice(cell.mark.encode_utf8(&mut utf8).as_bytes());
                        }
                        None => packet.push(b' '),
                    }
                    col += 1;
                    if col == limit || !(changed(col) || col + 1 < limit && changed(col + 1)) {
                        break;
                    }
                }
            }
            if inline
                && same_layout
                && self.old[row * cols + limit..(row + 1) * cols]
                    .iter()
                    .any(Option::is_some)
            {
                // Trim old inactive tail instead of printing spaces.
                // Otherwise VTE can wrap an invisible historical tail.
                let erase_at = if limit == 0 {
                    1
                } else {
                    layout.left + limit as u16
                };
                cup(packet, y, erase_at);
                packet.extend_from_slice(b"\x1b[K");
            }
        }
        self.drawn_right = right;

        if layout.inline.is_some() {
            packet.extend_from_slice(b"\x1b[0m");
        }
        cup(
            packet,
            layout
                .inline
                .map_or(layout.display.size.rows, |area| area.below),
            1,
        );
        Ok(trace_time)
    }

    pub fn interrupted_submission(&mut self) {
        self.needs_resync = true;
    }

    pub fn submitted(&mut self, layout: Layout) -> u16 {
        std::mem::swap(&mut self.old, &mut self.new);
        self.old_layout = Some(layout);
        self.needs_resync = false;
        self.drawn_right
    }
}

#[cfg(not(goodhand_embedded_playback))]
fn project_procedural(
    sampler: &ArmSampler,
    view: Projection,
    output: &mut [Option<u8>],
    scratch: &mut Vec<Option<u8>>,
) {
    // Bound ray tracing to the legacy maximum. The authored archive
    // is sampled directly and never uses this developer-scene limit.
    let source = view.limit_samples(usize::from(MAX_COLS) * usize::from(MAX_ROWS));
    let reduced = source.cols != view.cols || source.rows != view.rows;
    let trace = if reduced {
        scratch.resize(source.cols * source.rows, None);
        &mut scratch[..]
    } else {
        &mut *output
    };
    for (row, pixels) in trace.chunks_exact_mut(source.cols).enumerate() {
        let y = -source.y(row);
        for (col, pixel) in pixels.iter_mut().enumerate() {
            *pixel = sampler.sample(source.x(col), y);
        }
    }
    if reduced {
        // Reuse the traced sample nearest each output dot's center.
        let nearest = |i, fine, coarse| (2 * i + 1) * coarse / (2 * fine);
        for (row, pixels) in output.chunks_exact_mut(view.cols).enumerate() {
            let start = nearest(row, view.rows, source.rows) * source.cols;
            for (col, pixel) in pixels.iter_mut().enumerate() {
                *pixel = scratch[start + nearest(col, view.cols, source.cols)];
            }
        }
    }
}

pub fn cup(out: &mut Vec<u8>, row: u16, col: u16) {
    write!(out, "\x1b[{row};{col}H").expect("Vec writes cannot fail");
}

fn gray(out: &mut Vec<u8>, gray: u8) {
    write!(out, "\x1b[38;2;{gray};{gray};{gray}m").expect("Vec writes cannot fail");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dots::Renderer;

    fn cell_pitch(layout: Layout) -> (f64, f64) {
        let (dx, dy) = layout.projection().unwrap().step();
        let [x, y] = layout.settings.renderer.subdivisions();
        (dx * x as f64, dy * y as f64)
    }

    #[test]
    fn centered_layout_uses_known_cell_pitch_or_explicit_fallback() {
        let d = Display {
            size: Size {
                cols: 165,
                rows: 55,
            },
            pixels: (0, 0),
        };
        let fallback = Layout::fit(d, Settings::default());
        let expected = if cfg!(goodhand_embedded_playback) {
            (162, 52)
        } else {
            (164, 49)
        };
        assert_eq!((fallback.cols, fallback.rows), expected);
        assert_eq!(fallback.pitch, PitchSource::Assumed);
        let measured = Layout::fit(
            Display {
                pixels: (1155, 825),
                ..d
            },
            Settings::default(),
        );
        assert_eq!(measured.pitch, PitchSource::Measured);
        assert!((measured.aspect - 7.0 / 15.0).abs() < 1e-12);
        let (dx, dy) = cell_pitch(measured);
        assert!((dx / dy - measured.aspect).abs() < 1e-12);
        assert_eq!(
            Layout::fit(
                Display {
                    size: Size {
                        cols: 291,
                        rows: 100
                    },
                    ..d
                },
                Settings::default()
            )
            .cols,
            220
        );
    }

    #[test]
    fn projection_fits_at_all_supported_aspects_and_scales_about_its_center() {
        for (cols, rows) in [(92, 45), (92, 250), (300, 45), (65535, 65535)] {
            for aspect in [MIN_ASPECT, 0.5, MAX_ASPECT] {
                let display = Display {
                    size: Size { cols, rows },
                    pixels: (0, 0),
                };
                let fit = Layout::fit(
                    display,
                    Settings {
                        cell_aspect: CellAspect::Fixed(aspect),
                        ..Settings::default()
                    },
                );
                let center_slack = |near, far| u16::abs_diff(near, far);
                assert!(center_slack(fit.left - 1, (cols - fit.cols) - fit.left) <= 1);
                assert!(center_slack(fit.top - 2, (rows - 1 - fit.rows) - fit.top) <= 1);
                assert!(fit.cols <= MAX_COLS && fit.rows <= MAX_ROWS);
                assert_eq!(fit.pitch, PitchSource::Configured);
                let (fit_x, fit_y) = cell_pitch(fit);
                assert!(fit_x * f64::from(fit.cols) >= FRAME_SIZE.0 - 1e-12);
                assert!(fit_y * f64::from(fit.rows) >= FRAME_SIZE.1 - 1e-12);
                for zoom in ["50", "99", "100", "101", "200"] {
                    let zoom: crate::settings::Zoom = zoom.parse().unwrap();
                    let layout = Layout::fit(
                        display,
                        Settings {
                            zoom,
                            cell_aspect: CellAspect::Fixed(aspect),
                            ..Settings::default()
                        },
                    );
                    assert_eq!(
                        layout,
                        Layout {
                            settings: Settings {
                                zoom,
                                ..fit.settings
                            },
                            ..fit
                        }
                    );
                    let (dx, dy) = cell_pitch(layout);
                    assert!((dx / dy - aspect).abs() < 1e-12);
                    assert!((dx * zoom.factor() - fit_x).abs() < 1e-12);
                    assert!((dy * zoom.factor() - fit_y).abs() < 1e-12);
                    let legacy = Layout {
                        settings: Settings {
                            renderer: Renderer::Dots,
                            ..layout.settings
                        },
                        ..layout
                    };
                    let (dot_x, dot_y) = cell_pitch(legacy);
                    assert!((dx - dot_x).abs() < 1e-12);
                    assert!((dy - dot_y).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn raw_viewport_change_invalidates_diff_even_if_cell_projection_is_unchanged() {
        let before = Display {
            size: Size {
                cols: 165,
                rows: 55,
            },
            pixels: (1155, 825),
        };
        // Calibrated aspect is independent of the reported pixel dimensions.
        // This resize still invalidates the old surface in the tty.
        let after = Display {
            pixels: (1320, 825),
            ..before
        };
        let settings = Settings {
            zoom: "125".parse().unwrap(),
            cell_aspect: CellAspect::Fixed(0.5),
            ..Settings::default()
        };
        let mut painter = Painter::new().unwrap();
        let mut packet = Vec::new();
        let first = Layout::for_surface(before, settings, painter.frame_size(), None);
        let second = Layout::for_surface(after, settings, painter.frame_size(), None);
        assert_eq!(
            Layout {
                display: after,
                ..first
            },
            second
        );

        painter.paint(0, first, &mut packet).unwrap();
        let original = packet.clone();
        painter.submitted(first);
        painter.paint(0, second, &mut packet).unwrap();
        assert_eq!(packet.strip_prefix(b"\x1b[2J"), Some(original.as_slice()));
        painter.submitted(second);
        painter.paint(0, second, &mut packet).unwrap();
        assert!(!packet.starts_with(b"\x1b[2J"));
    }

    #[test]
    fn partial_submission_restarts_from_a_clean_canvas_until_committed() {
        let d = Display {
            size: Size { cols: 92, rows: 45 },
            pixels: (0, 0),
        };
        let mut current = Painter::new().unwrap();
        let mut packet = Vec::new();
        let layout = Layout::for_surface(d, Settings::default(), current.frame_size(), None);
        current.paint(0, layout, &mut packet).unwrap();
        current.submitted(layout);
        current.paint(1, layout, &mut packet).unwrap();
        current.interrupted_submission();
        // This attempted redraw can itself be skipped, or write only a
        // prefix. Neither should commit a partial screen to diff history.
        current.paint(100, layout, &mut packet).unwrap();
        assert!(packet.starts_with(RESYNC));

        let mut fresh = Vec::new();
        Painter::new()
            .unwrap()
            .paint(112, layout, &mut fresh)
            .unwrap();
        current.paint(112, layout, &mut packet).unwrap();
        assert_eq!(packet.strip_prefix(RESYNC), Some(fresh.as_slice()));
        current.submitted(layout);
        current.paint(113, layout, &mut packet).unwrap();
        assert!(!packet.starts_with(RESYNC));
    }

    #[test]
    fn playback_updates_the_picture_at_different_ticks() {
        let d = Display {
            size: Size { cols: 92, rows: 45 },
            pixels: (0, 0),
        };
        let mut p = Painter::new().unwrap();
        let mut first = Vec::new();
        let layout = Layout::for_surface(d, Settings::default(), p.frame_size(), None);
        p.paint(0, layout, &mut first).unwrap();
        let old = p.new.clone();
        p.submitted(layout);
        let mut next = Vec::new();
        p.paint(180, layout, &mut next).unwrap();
        assert_ne!(old, p.new);
        assert!(
            std::str::from_utf8(&next)
                .unwrap()
                .chars()
                .any(|mark| ('\u{2801}'..='\u{28ff}').contains(&mark))
        );
        assert!(!next.contains(&b'\n'));
    }
}
