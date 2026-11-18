//! Terminal projection/encoding only. The Goodhand sampler owns geometry,
//! view orientation and base tone. This program never activates attributes.
use crate::playback::TICKS;
use goodhand::ArmSampler;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

pub const MIN_WIDTH: u16 = 92;
pub const MIN_HEIGHT: u16 = 45;
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub cols: u16,
    pub rows: u16,
    pub aspect: f64,
    pub measured_pitch: bool,
    left: u16,
    top: u16,
}

impl Layout {
    pub fn fit(display: Display) -> Self {
        debug_assert!(display.size.fits());
        let Size { cols, rows } = display.size;
        let (px, py) = display.pixels;
        let measured = (f64::from(px) * f64::from(rows)) / (f64::from(py) * f64::from(cols));
        let measured_pitch = (0.30..=0.85).contains(&measured)
            && px >= cols.saturating_mul(3)
            && py >= rows.saturating_mul(5);
        let aspect = if measured_pitch { measured } else { 0.5 };

        let available_cols = (cols - 1).min(MAX_COLS);
        let available_rows = (rows - 3).min(MAX_ROWS);
        let ratio = ArmSampler::FRAME_WIDTH / (ArmSampler::FRAME_HEIGHT * aspect);
        let picture_cols = available_cols.min((f64::from(available_rows) * ratio).floor() as u16);
        let picture_rows = available_rows.min((f64::from(available_cols) / ratio).floor() as u16);
        Self {
            cols: picture_cols,
            rows: picture_rows,
            aspect,
            measured_pitch,
            left: 1 + (cols - 1 - picture_cols) / 2,
            top: 2 + (rows - 3 - picture_rows) / 2,
        }
    }

    fn step(self) -> (f64, f64) {
        let dy = (ArmSampler::FRAME_HEIGHT / f64::from(self.rows))
            .max(ArmSampler::FRAME_WIDTH / (self.aspect * f64::from(self.cols)));
        (dy * self.aspect, dy)
    }
}

#[derive(Default)]
pub struct Painter {
    old: Vec<Option<u8>>,
    new: Vec<Option<u8>>,
    old_layout: Option<Layout>,
    needs_resync: bool,
}

impl Painter {
    pub fn paint(
        &mut self,
        tick: u64,
        display: Display,
        packet: &mut Vec<u8>,
    ) -> io::Result<(Layout, Duration)> {
        let layout = Layout::fit(display);
        let len = usize::from(layout.cols) * usize::from(layout.rows);
        self.new.resize(len, None);
        let trace_start = Instant::now();
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
            packet.extend_from_slice(b"oops / procedural arm");
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
        cup(packet, display.size.rows, 1);
        Ok((layout, trace_time))
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
        let fallback = Layout::fit(d);
        assert_eq!((fallback.cols, fallback.rows), (164, 49));
        assert!(!fallback.measured_pitch);
        let measured = Layout::fit(Display {
            pixels: (1155, 825),
            ..d
        });
        assert!(measured.measured_pitch);
        assert!((measured.aspect - 7.0 / 15.0).abs() < 1e-12);
        let (dx, dy) = measured.step();
        assert!((dx / dy - measured.aspect).abs() < 1e-12);
        assert_eq!(
            Layout::fit(Display {
                size: Size {
                    cols: 291,
                    rows: 100
                },
                ..d
            })
            .cols,
            220
        );
    }

    #[test]
    fn partial_submission_restarts_from_a_clean_canvas_until_committed() {
        let d = Display {
            size: Size { cols: 92, rows: 45 },
            pixels: (0, 0),
        };
        let mut current = Painter::default();
        let mut packet = Vec::new();
        let (layout, _) = current.paint(0, d, &mut packet).unwrap();
        current.submitted(layout);
        current.paint(1, d, &mut packet).unwrap();
        current.interrupted_submission();
        // This attempted redraw can itself be skipped, or write only a
        // prefix. Neither should commit a partial screen to diff history.
        current.paint(100, d, &mut packet).unwrap();
        assert!(packet.starts_with(RESYNC));

        let mut fresh = Vec::new();
        Painter::default().paint(112, d, &mut fresh).unwrap();
        let (layout, _) = current.paint(112, d, &mut packet).unwrap();
        assert_eq!(packet.strip_prefix(RESYNC), Some(fresh.as_slice()));
        current.submitted(layout);
        current.paint(113, d, &mut packet).unwrap();
        assert!(!packet.starts_with(RESYNC));
    }

    #[test]
    fn geometry_changes_without_assets_at_both_angles() {
        let d = Display {
            size: Size { cols: 92, rows: 45 },
            pixels: (0, 0),
        };
        let mut p = Painter::default();
        let mut first = Vec::new();
        let (layout, _) = p.paint(0, d, &mut first).unwrap();
        let old = p.new.clone();
        p.submitted(layout);
        let mut next = Vec::new();
        p.paint(180, d, &mut next).unwrap();
        assert_ne!(old, p.new);
        assert!(next.windows(3).any(|w| w == "•".as_bytes()));
        assert!(!next.contains(&b'\n'));
    }
}
