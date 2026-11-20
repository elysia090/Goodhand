//! Darken only the gaps in the occupied interior. Source dots stay opaque
//! and unchanged, and this layer is invisible over pure black.
use crate::archive_profile::{HEIGHT, WIDTH};
use goodhand::RasterSpec;

const SPEC: RasterSpec = RasterSpec::REFERENCE;
const COLS: usize = WIDTH as usize;
const ROWS: usize = HEIGHT as usize;
const GRID_STRIDE: usize = COLS + 2;
const GRID_LEN: usize = GRID_STRIDE * (ROWS + 2);
// Zero out the outermost occupied band, then feather in over the next
// few dots. The padded border is always empty, even for a full raster.
const FEATHER: [f32; 6] = [0., 0., 0.12, 0.48, 0.85, 1.];
const MAX_DEPTH: u8 = (FEATHER.len() - 1) as u8;
pub(super) const PEAK: u8 = 160;

pub(super) struct InteriorMatte {
    depth: Vec<u8>,
    alpha: Vec<f32>,
}

impl InteriorMatte {
    pub(super) fn new() -> Self {
        Self {
            depth: vec![0; GRID_LEN],
            alpha: vec![0.; GRID_LEN],
        }
    }

    pub(super) fn dot_depth(&self, x: usize, y: usize) -> u8 {
        self.depth[grid(x + 1, y + 1)]
    }

    pub(super) fn apply(&mut self, gray: &[u8], occupied: &[u8], destination: &mut [u32]) {
        for y in 1..=ROWS {
            for x in 1..=COLS {
                self.depth[grid(x, y)] = if occupied[raster(x, y)] == 0 {
                    0
                } else {
                    MAX_DEPTH
                };
            }
        }
        // Chebyshev distance to empty: two directional passes, with distance
        // capped once the matte has reached its full interior strength.
        for y in 1..=ROWS {
            for x in 1..=COLS {
                let i = grid(x, y);
                let near = self.depth[i - 1]
                    .min(self.depth[i - GRID_STRIDE - 1])
                    .min(self.depth[i - GRID_STRIDE])
                    .min(self.depth[i - GRID_STRIDE + 1]);
                self.depth[i] = self.depth[i].min(near + 1);
            }
        }
        for y in (1..=ROWS).rev() {
            for x in (1..=COLS).rev() {
                let i = grid(x, y);
                let near = self.depth[i + 1]
                    .min(self.depth[i + GRID_STRIDE - 1])
                    .min(self.depth[i + GRID_STRIDE])
                    .min(self.depth[i + GRID_STRIDE + 1]);
                self.depth[i] = self.depth[i].min(near + 1);
            }
        }
        for y in 1..=ROWS {
            for x in 1..=COLS {
                let i = grid(x, y);
                let depth = self.depth[i];
                self.alpha[i] = if depth < 2 {
                    0.
                } else {
                    let mut sum = 0;
                    for j in y - 1..=y + 1 {
                        for k in x - 1..=x + 1 {
                            sum += u16::from(gray[raster(k, j)]);
                        }
                    }
                    let average = f32::from(sum) / 9.;
                    // Emphasize a continuous dark-to-middle plane, never
                    // punch gaps at a high-contrast fold or knuckle.
                    let t = ((average - 32.) / 101.).clamp(0., 1.);
                    let tone = 1. - 0.40 * t * t * (3. - 2. * t);
                    f32::from(PEAK) * FEATHER[usize::from(depth)] * tone
                };
            }
        }

        for y in 1..ROWS {
            for x in 1..COLS {
                let i = grid(x, y);
                let corners = [i, i + 1, i + GRID_STRIDE, i + GRID_STRIDE + 1];
                // No darkening across a gap between fingers, through a hole,
                // or outside the convex hull of this group of four dots.
                if corners.iter().any(|&j| self.depth[j] == 0) {
                    continue;
                }
                let [ul, ur, ll, lr] = corners.map(|j| self.alpha[j]);
                if ul.max(ur).max(ll).max(lr) < 0.5 {
                    continue;
                }
                let start = raster(x, y);
                for dy in 0..SPEC.pitch[1] {
                    let v = dy as f32 / SPEC.pitch[1] as f32;
                    let l = lerp(ul, ll, v);
                    let r = lerp(ur, lr, v);
                    let row = start + dy * SPEC.width;
                    for dx in 0..SPEC.pitch[0] {
                        let pixel = row + dx;
                        if occupied[pixel] == 0 {
                            let u = dx as f32 / SPEC.pitch[0] as f32;
                            let a = lerp(l, r, u).round_ties_even() as u8;
                            // Cairo ARGB32 needs premultiplied color. Black
                            // is zero RGB with any opacity.
                            destination[pixel] = u32::from(a) << 24;
                        }
                    }
                }
            }
        }
    }
}

fn grid(x: usize, y: usize) -> usize {
    GRID_STRIDE * y + x
}

// Coordinates are 1-based to share the padded lattice's axes.
fn raster(x: usize, y: usize) -> usize {
    let col = SPEC.origin[0] + SPEC.pitch[0] * (x - 1);
    let row = SPEC.origin[1] + SPEC.pitch[1] * (y - 1);
    row * SPEC.width + col
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matte_keeps_a_hole_clear_and_never_writes_an_occupied_mark() {
        let len = SPEC.required_len().unwrap();
        let mut gray = vec![0; len];
        let mut coverage = vec![0; len];
        let mut display = vec![0; len];
        for y in 5..=18 {
            for x in 9..=22 {
                if (x, y) == (16, 12) {
                    continue;
                }
                let dot = raster(x, y);
                for dy in 0..SPEC.dot_size[1] {
                    for dx in 0..SPEC.dot_size[0] {
                        let p = dot + dy * SPEC.width + dx;
                        gray[p] = 55;
                        coverage[p] = 255;
                        display[p] = 0xff_37_37_37;
                    }
                }
            }
        }
        let mut matte = InteriorMatte::new();
        matte.apply(&gray, &coverage, &mut display);
        assert_eq!(display[raster(16, 12)], 0);
        assert_eq!(display[raster(15, 11) + 3 + 3 * SPEC.width], 0);
        assert_eq!(display[raster(8, 9) + 3], 0);
        let inside = display[raster(13, 9) + 3];
        assert!(inside >> 24 > 10);
        for (&pixel, &covered) in display.iter().zip(&coverage) {
            if covered == 255 {
                assert_eq!(pixel, 0xff_37_37_37);
            } else {
                assert_eq!(pixel & 0xff_ff_ff, 0);
                assert!((pixel >> 24) <= u32::from(PEAK));
            }
        }
    }
}
