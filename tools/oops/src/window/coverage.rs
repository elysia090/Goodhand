//! Device-pixel area sampling for the opaque, unmodified archived dots.
use super::{BACKDROP_RGB, Canvas, Placement, WIDTH, argb};
use crate::archive_profile;
use goodhand::RasterSpec;
use std::sync::OnceLock;

// Keep one scratch surface, no more than 32 MiB. Larger or unusual targets
// keep the existing Cairo renderer rather than changing the shape or fit.
const MAX_PIXELS: usize = 8 * 1024 * 1024;
const SPEC: RasterSpec = RasterSpec::REFERENCE;
const COLS: usize = archive_profile::WIDTH as usize;
const ROWS: usize = archive_profile::HEIGHT as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    canvas: Canvas,
    logical: (i32, i32),
    device: i32,
}

#[derive(Clone, Copy)]
struct Span {
    lo: i64,
    hi: i64,
}

impl Span {
    fn clipped(start: i64, length: i64, end: i64) -> Self {
        Self {
            lo: start.clamp(0, end),
            hi: (start + length).clamp(0, end),
        }
    }

    // Numerator of the intersection with each physical pixel.
    fn pieces(self, den: i64) -> impl Iterator<Item = (usize, i64)> {
        let first = self.lo / den;
        let last = if self.lo < self.hi {
            (self.hi + den - 1) / den
        } else {
            first
        };
        (first..last).map(move |pixel| {
            (
                pixel as usize,
                self.hi.min((pixel + 1) * den) - self.lo.max(pixel * den),
            )
        })
    }
}

struct Plan {
    key: Key,
    width: usize,
    height: usize,
    den: i64,
    columns: [Span; COLS],
    rows: [Span; ROWS],
}

impl Plan {
    fn new(key: Key) -> Option<Self> {
        let (w, h) = key.logical;
        if !(1..=8).contains(&key.device) || w <= 0 || h <= 0 {
            return None;
        }
        let width = (w as usize).checked_mul(key.device as usize)?;
        let height = (h as usize).checked_mul(key.device as usize)?;
        if width.checked_mul(height)? > MAX_PIXELS {
            return None;
        }

        let aperture = key.canvas;
        let ah = aperture.height as i64;
        let device = i64::from(key.device);
        let den = 2 * ah;
        let scaled_height = i64::from(h) * device;

        // Adjacent dots cannot touch one destination pixel when the gap
        // between their exact footprints is at least one physical pixel.
        if 3 * scaled_height < ah {
            return None;
        }

        // Placement::raster_origin and scale, in physical pixels relative to
        // the aperture. The common denominator avoids drifting between dots.
        let x = device
            * (ah * i64::from(w)
                + (2 * i64::from(aperture.offset_x) - WIDTH as i64) * i64::from(h));
        let y = -2 * i64::from(aperture.crop_top) * scaled_height;
        if scaled_height % ah == 0 && x % den == 0 && y % den == 0 {
            // Exact 1× (and aligned whole-pixel magnification) stays on the
            // original zero-resampling Cairo path.
            return None;
        }
        let step = 2 * scaled_height;
        let axis = |offset, origin, pitch, dot, index, limit| {
            Span::clipped(offset + (origin + index * pitch) as i64 * step, dot * step, limit)
        };
        let columns = std::array::from_fn(|i| {
            axis(
                x,
                SPEC.origin[0],
                SPEC.pitch[0],
                SPEC.dot_size[0] as i64,
                i,
                width as i64 * den,
            )
        });
        let rows = std::array::from_fn(|i| {
            axis(
                y,
                SPEC.origin[1],
                SPEC.pitch[1],
                SPEC.dot_size[1] as i64,
                i,
                height as i64 * den,
            )
        });
        Some(Self {
            key,
            width,
            height,
            den,
            columns,
            rows,
        })
    }

    fn paint(&self, art: &[u32], out: &mut [u32]) {
        let color = Colors::get();
        let pixel_area = self.den * self.den;
        let inv_area = 1.0 / pixel_area as f64;
        out.fill(argb(BACKDROP_RGB, true));
        for (row, &v) in self.rows.iter().enumerate() {
            let start = (SPEC.origin[1] + row * SPEC.pitch[1]) * SPEC.width + SPEC.origin[0];
            for (column, &u) in self.columns.iter().enumerate() {
                let dot = art[start + column * SPEC.pitch[0]];
                if dot == 0 {
                    continue;
                }
                // The baseline's occupied dots are opaque uniform gray. Its
                // stored value is preserved exactly when coverage is complete.
                let gray = color.gray[(dot & 255) as usize];
                for (y, wy) in v.pieces(self.den) {
                    for (x, wx) in u.pieces(self.den) {
                        let q = wx * wy;
                        out[y * self.width + x] = if q == pixel_area {
                            dot
                        } else {
                            color.blend(gray, q as f64 * inv_area)
                        };
                    }
                }
            }
        }
    }
}

struct Colors {
    gray: [f64; 256],
    cuts: [f64; 255],
    ground: [f64; 3],
}

impl Colors {
    fn get() -> &'static Self {
        static TABLE: OnceLock<Colors> = OnceLock::new();
        TABLE.get_or_init(|| {
            fn linear(srgb: f64) -> f64 {
                if srgb <= 0.04045 {
                    srgb / 12.92
                } else {
                    ((srgb + 0.055) / 1.055).powf(2.4)
                }
            }
            let gray = std::array::from_fn(|i| linear(i as f64 / 255.));
            Self {
                gray,
                cuts: std::array::from_fn(|i| linear((i as f64 + 0.5) / 255.)),
                ground: BACKDROP_RGB.map(|v| gray[v as usize]),
            }
        })
    }

    fn blend(&self, gray: f64, coverage: f64) -> u32 {
        argb(
            self.ground.map(|back| {
                // Box weights are linear light, not a blend of encoded bytes.
                // This returns the sRGB8 value without per-pixel powers.
                let mixed = back + coverage * (gray - back);
                self.cuts.partition_point(|&cut| cut <= mixed) as u8
            }),
            true,
        )
    }
}

#[derive(Default)]
pub(super) struct Raster {
    plan: Option<Plan>,
    pixels: Vec<u32>,
    pose: Option<u32>,
}

pub(super) struct Frame<'a> {
    pub width: i32,
    pub height: i32,
    pub pixels: &'a mut [u32],
}

impl Raster {
    pub(super) fn frame(
        &mut self,
        canvas: Canvas,
        view: Placement,
        device: i32,
        pose: u32,
        source: &[u32],
    ) -> Option<Frame<'_>> {
        let key = Key {
            canvas,
            logical: view.size,
            device,
        };
        if !self.plan.as_ref().is_some_and(|p| p.key == key) {
            let plan = Plan::new(key)?;
            let len = plan.width * plan.height;
            self.pixels
                .try_reserve_exact(len.saturating_sub(self.pixels.len()))
                .ok()?;
            self.pixels.resize(len, 0);
            self.plan = Some(plan);
            self.pose = None;
        }
        let plan = self.plan.as_ref()?;
        if self.pose != Some(pose) {
            plan.paint(source, &mut self.pixels);
            self.pose = Some(pose);
        }
        Some(Frame {
            width: plan.width as i32,
            height: plan.height as i32,
            pixels: &mut self.pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn black_canvas() -> Canvas {
        super::super::Apertures::new(([47, 137], [954, 549]))
            .unwrap()
            .black
    }

    fn plan(canvas: Canvas, width: i32, height: i32, device: i32) -> Option<Plan> {
        Plan::new(Key {
            canvas,
            logical: Placement::new(canvas, width, height).size,
            device,
        })
    }

    #[test]
    fn whole_pixel_scales_and_oversize_keep_the_old_renderer() {
        let canvas = black_canvas();
        assert!(plan(canvas, 1016, 445, 1).is_none());
        assert!(plan(canvas, 1016, 445, 2).is_none());
        assert!(plan(canvas, 595, 290, 0).is_none());
        assert!(plan(canvas, 400, 100, 1).is_none());
        assert!(plan(canvas, 10000, 4500, 1).is_none());
    }

    #[test]
    fn all_interior_dots_have_exact_coverage_in_the_narrow_tiles() {
        let canvas = black_canvas();
        for (w, h, device) in [(595, 290, 1), (597, 290, 1), (597, 290, 2), (960, 1040, 1)] {
            let p = plan(canvas, w, h, device).unwrap();
            let side = 4 * i64::from(p.key.logical.1) * i64::from(device);
            let check = |span: Span| {
                assert_eq!(span.pieces(p.den).map(|(_, weight)| weight).sum::<i64>(), side);
            };
            // x/y indices in the exact full-turn occupied bbox [47,954) × [137,549).
            p.columns[9..=190].iter().copied().for_each(check);
            p.rows[19..=101].iter().copied().for_each(check);
        }
    }

    #[test]
    fn fractional_dots_preserve_light_area_after_eight_bit_rounding() {
        let canvas = black_canvas();
        let color = Colors::get();
        let mut art = vec![0; WIDTH * super::super::HEIGHT];
        let row = 55;
        let gray_for = |column| [0, 76, 160, 255][column % 4];
        for col in 20..180 {
            let x = SPEC.origin[0] + col * SPEC.pitch[0];
            let y = SPEC.origin[1] + row * SPEC.pitch[1];
            art[y * WIDTH + x] = argb([gray_for(col); 3], true);
        }
        for (w, h, device) in [(595, 290, 1), (597, 290, 1), (597, 290, 2)] {
            let p = plan(canvas, w, h, device).unwrap();
            let side = 4 * i64::from(p.key.logical.1) * i64::from(device);
            let px = (side as f64 / p.den as f64).powi(2);
            let mut out = vec![0; p.width * p.height];
            p.paint(&art, &mut out);
            assert_eq!(out[0], argb(BACKDROP_RGB, true));
            for col in 20..180 {
                let mut sum = [0.0; 3];
                for (y, _) in p.rows[row].pieces(p.den) {
                    for (x, _) in p.columns[col].pieces(p.den) {
                        let bytes = out[y * p.width + x].to_be_bytes();
                        assert_eq!(bytes[0], 255);
                        for c in 0..3 {
                            sum[c] += color.gray[usize::from(bytes[c + 1])] - color.ground[c];
                        }
                    }
                }
                for (c, actual) in sum.into_iter().enumerate() {
                    let expect = px * (color.gray[usize::from(gray_for(col))] - color.ground[c]);
                    assert!((actual - expect).abs() < 0.035, "{w}×{h}@{device}, column {col}");
                }
            }
        }
    }

    #[test]
    fn same_pose_reuses_buffer_but_pose_and_geometry_changes_repaint() {
        let canvas = black_canvas();
        let a = Placement::new(canvas, 595, 290);
        let b = Placement::new(canvas, 597, 290);
        let mut art = vec![0; WIDTH * super::super::HEIGHT];
        let dot = (SPEC.origin[1] + 55 * SPEC.pitch[1]) * WIDTH + SPEC.origin[0] + 100 * SPEC.pitch[0];
        art[dot] = argb([180; 3], true);
        let mut raster = Raster::default();
        let pixels = raster.frame(canvas, a, 1, 4, &art).unwrap().pixels.to_vec();
        let black = argb(BACKDROP_RGB, true);
        assert!(pixels.iter().any(|&p| p != black));
        art[dot] = 0;
        assert_eq!(raster.frame(canvas, a, 1, 4, &art).unwrap().pixels, pixels);
        let changed = raster.frame(canvas, a, 1, 5, &art).unwrap();
        assert!(changed.pixels.iter().all(|&p| p == black));
        art[dot] = argb([180; 3], true);
        let resized = raster.frame(canvas, b, 1, 5, &art).unwrap();
        assert_eq!((resized.width, resized.height), b.size);
        assert!(resized.pixels.iter().any(|&p| p != black));
    }
}
