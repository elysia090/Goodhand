//! Terminal projection and encoding, independent of which scene supplies the dots.
#[cfg(goodhand_embedded_playback)]
use crate::archived_scene::Archive;
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
        packet.extend_from_slice(b"\x1b[2K");
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
        Projection::new(
            usize::from(self.cols),
            usize::from(self.rows),
            self.frame_size,
            self.aspect,
            self.settings.zoom.factor(),
            self.settings.offset,
        )
    }
}

pub struct Painter {
    #[cfg(goodhand_embedded_playback)]
    archive: Archive,
    old: Vec<Option<u8>>,
    new: Vec<Option<u8>>,
    old_layout: Option<Layout>,
    needs_resync: bool,
}

impl Painter {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            #[cfg(goodhand_embedded_playback)]
            archive: Archive::embedded()?,
            old: Vec::new(),
            new: Vec::new(),
            old_layout: None,
            needs_resync: false,
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
        self.new.resize(projection.cols * projection.rows, None);
        let settings = layout.settings;
        #[cfg(goodhand_embedded_playback)]
        self.archive
            .project(settings.cycle, tick, projection, &mut self.new)?;
        #[cfg(not(goodhand_embedded_playback))]
        {
            let sampler = ArmSampler::new(settings.cycle.yaw(tick)).map_err(io::Error::other)?;
            for (row, pixels) in self.new.chunks_exact_mut(projection.cols).enumerate() {
                let y = -projection.y(row);
                for (col, pixel) in pixels.iter_mut().enumerate() {
                    *pixel = sampler.sample(projection.x(col), y);
                }
            }
        }
        // Adjust art gray only, never the presence of a dot or the UI colors.
        if settings.brightness != crate::settings::Brightness::default() {
            for level in self.new.iter_mut().flatten() {
                *level = settings.brightness.apply(*level);
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
        let cols = usize::from(layout.cols);
        for (row, pixels) in self.new.chunks_exact(cols).enumerate() {
            let changed = |col| {
                let old = if same_layout {
                    self.old[row * cols + col]
                } else {
                    None
                };
                old != pixels[col]
            };
            let mut col = 0;
            while col < cols {
                if !changed(col) {
                    col += 1;
                    continue;
                }
                cup(packet, layout.top + row as u16, layout.left + col as u16);
                loop {
                    match pixels[col] {
                        Some(level) => {
                            if active != Some(level) {
                                gray(packet, level);
                                active = Some(level);
                            }
                            packet.extend_from_slice("•".as_bytes());
                        }
                        None => packet.push(b' '),
                    }
                    col += 1;
                    if col == cols || !(changed(col) || col + 1 < cols && changed(col + 1)) {
                        break;
                    }
                }
            }
        }

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

    pub fn submitted(&mut self, layout: Layout) {
        std::mem::swap(&mut self.old, &mut self.new);
        self.old_layout = Some(layout);
        self.needs_resync = false;
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
        let (dx, dy) = measured.projection().unwrap().step();
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
                let (fit_x, fit_y) = fit.projection().unwrap().step();
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
                    let (dx, dy) = layout.projection().unwrap().step();
                    assert!((dx / dy - aspect).abs() < 1e-12);
                    assert!((dx * zoom.factor() - fit_x).abs() < 1e-12);
                    assert!((dy * zoom.factor() - fit_y).abs() < 1e-12);
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
        assert!(next.windows(3).any(|w| w == "•".as_bytes()));
        assert!(!next.contains(&b'\n'));
    }
}
