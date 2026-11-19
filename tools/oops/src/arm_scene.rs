//! Terminal projection and encoding, independent of which scene supplies the dots.
#[cfg(goodhand_embedded_playback)]
use crate::archived_scene::{Archive, View};
#[cfg(not(goodhand_embedded_playback))]
use crate::playback::TICKS;
use crate::settings::{CellAspect, MAX_ASPECT, MIN_ASPECT, Settings, Zoom};
#[cfg(not(goodhand_embedded_playback))]
use goodhand::ArmSampler;
#[cfg(goodhand_embedded_playback)]
use goodhand::RasterSpec;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

pub const MIN_WIDTH: u16 = 92;
pub const MIN_HEIGHT: u16 = 45;
#[cfg(goodhand_embedded_playback)]
pub const CONTENT_LABEL: &str = crate::archived_scene::LABEL;
#[cfg(not(goodhand_embedded_playback))]
pub const CONTENT_LABEL: &str = "procedural arm";

#[cfg(goodhand_embedded_playback)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub cols: u16,
    pub rows: u16,
    pub aspect: f64,
    pub pitch: PitchSource,
    pub zoom: Zoom,
    display: Display,
    left: u16,
    top: u16,
}

impl Layout {
    pub fn fit(display: Display, settings: Settings) -> Self {
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
        let available_rows = (rows - 3).min(MAX_ROWS);
        let ratio = FRAME_SIZE.0 / (FRAME_SIZE.1 * aspect);
        let picture_cols = available_cols.min((f64::from(available_rows) * ratio).floor() as u16);
        let picture_rows = available_rows.min((f64::from(available_cols) / ratio).floor() as u16);
        Self {
            cols: picture_cols,
            rows: picture_rows,
            aspect,
            pitch,
            zoom: settings.zoom,
            display,
            left: 1 + (cols - 1 - picture_cols) / 2,
            top: 2 + (rows - 3 - picture_rows) / 2,
        }
    }

    #[cfg(any(test, not(goodhand_embedded_playback)))]
    fn step(self) -> (f64, f64) {
        // Start from a uniform physical fit, then apply one scale to both axes.
        // The integer cell window is independent of zoom and still bounds work.
        let dy = (FRAME_SIZE.1 / f64::from(self.rows))
            .max(FRAME_SIZE.0 / (self.aspect * f64::from(self.cols)))
            / self.zoom.factor();
        (dy * self.aspect, dy)
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

    pub fn paint(
        &mut self,
        tick: u64,
        layout: Layout,
        packet: &mut Vec<u8>,
    ) -> io::Result<Duration> {
        let len = usize::from(layout.cols) * usize::from(layout.rows);
        self.new.resize(len, None);
        let trace_start = Instant::now();
        #[cfg(goodhand_embedded_playback)]
        self.archive.project(
            tick,
            View {
                cols: usize::from(layout.cols),
                rows: usize::from(layout.rows),
                aspect: layout.aspect,
                zoom: layout.zoom.factor(),
            },
            &mut self.new,
        )?;
        #[cfg(not(goodhand_embedded_playback))]
        {
            let angle = -360.0 * tick as f64 / TICKS as f64;
            let sampler = ArmSampler::new(angle).map_err(io::Error::other)?;
            let (dx, dy) = layout.step();
            let center_x = (f64::from(layout.cols) - 1.0) / 2.0;
            let center_y = (f64::from(layout.rows) - 1.0) / 2.0;
            for (row, pixels) in self
                .new
                .chunks_exact_mut(usize::from(layout.cols))
                .enumerate()
            {
                let y = (center_y - row as f64) * dy;
                for (col, pixel) in pixels.iter_mut().enumerate() {
                    let x = (col as f64 - center_x) * dx;
                    *pixel = sampler.sample(x, y);
                }
            }
        }
        let trace_time = trace_start.elapsed();

        packet.clear();
        let same_layout = !self.needs_resync && self.old_layout == Some(layout);
        if !same_layout {
            if self.needs_resync {
                packet.extend_from_slice(RESYNC);
            } else if self.old_layout.is_some() {
                packet.extend_from_slice(b"\x1b[2J");
            }
            cup(packet, layout.top - 1, layout.left);
            gray(packet, 160);
            write!(packet, "oops / {CONTENT_LABEL}").expect("Vec writes cannot fail");
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

        cup(packet, layout.top + layout.rows, layout.left);
        gray(packet, 135);
        write!(packet, "30 seconds / q or Ctrl-C / tick {tick:03}")
            .expect("Vec writes cannot fail");
        cup(packet, layout.display.size.rows, 1);
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

fn cup(out: &mut Vec<u8>, row: u16, col: u16) {
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
        let (dx, dy) = measured.step();
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
                let (fit_x, fit_y) = fit.step();
                assert!(fit_x * f64::from(fit.cols) >= FRAME_SIZE.0 - 1e-12);
                assert!(fit_y * f64::from(fit.rows) >= FRAME_SIZE.1 - 1e-12);
                for zoom in ["50", "99", "100", "101", "200"] {
                    let zoom: Zoom = zoom.parse().unwrap();
                    let layout = Layout::fit(
                        display,
                        Settings {
                            zoom,
                            cell_aspect: CellAspect::Fixed(aspect),
                        },
                    );
                    assert_eq!(layout, Layout { zoom, ..fit });
                    let (dx, dy) = layout.step();
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
        };
        let mut painter = Painter::new().unwrap();
        let mut packet = Vec::new();
        let first = Layout::fit(before, settings);
        let second = Layout::fit(after, settings);
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
        let layout = Layout::fit(d, Settings::default());
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
        let layout = Layout::fit(d, Settings::default());
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
