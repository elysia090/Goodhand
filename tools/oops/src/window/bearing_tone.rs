//! A faint supporting gray inside the lower forearm, for the black pane only.
//! This alters dot RGB, never coverage, and does not supply window placement.
use super::interior_matte::InteriorMatte;
use crate::archive_profile::{HEIGHT, WIDTH};
use goodhand::RasterSpec;

const COLS: usize = WIDTH as usize;
const ROWS: usize = HEIGHT as usize;
const SPEC: RasterSpec = RasterSpec::REFERENCE;
const MAX_GAIN: f64 = 9.;

#[derive(Clone, Copy)]
struct Section {
    top: usize,
    bottom: usize,
    count: usize,
}

struct Forearm {
    columns: [Section; COLS],
    origin: [f64; 2],
    span: [f64; 2],
    base: f64,
    points_right: bool,
    broad: f64,
    narrow: f64,
}

impl Forearm {
    fn from_coverage(coverage: &InteriorMatte) -> Option<Self> {
        let mut columns = [Section {
            top: ROWS,
            bottom: 0,
            count: 0,
        }; COLS];
        let mut rows = [(0_usize, 0_usize); ROWS]; // number of dots, sum of their x coordinates
        let mut lo = [COLS, ROWS];
        let mut hi = [0, 0];
        for (y, (count, sum)) in rows.iter_mut().enumerate() {
            for (x, section) in columns.iter_mut().enumerate() {
                if coverage.dot_depth(x, y) == 0 {
                    continue;
                }
                *count += 1;
                *sum += x;
                section.top = section.top.min(y);
                section.bottom = y;
                section.count += 1;
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x), hi[1].max(y)];
            }
        }
        if lo[0] >= hi[0] || lo[1] >= hi[1] {
            return None;
        }
        let width = (hi[0] - lo[0]) as f64;
        let height = (hi[1] - lo[1]) as f64;
        let mut bottom = [0.; 2];
        let mut top = [0.; 2];
        for (y, &(count, sum)) in rows.iter().enumerate().take(hi[1] + 1).skip(lo[1]) {
            let z = (y - lo[1]) as f64 / height;
            let low = smooth(0.74, 0.92, z);
            let high = 1. - smooth(0.04, 0.19, z);
            for (acc, weight) in [(&mut bottom, low), (&mut top, high)] {
                acc[0] += weight * sum as f64;
                acc[1] += weight * count as f64;
            }
        }
        let base = bottom[0] / bottom[1];
        let tip = top[0] / top[1];
        let direction = (tip - base) / width;
        // With a narrow view or ambiguous direction, blend out the
        // long-shaft mask before its left/right definition can flip.
        let broad = smooth(84., 133., width) * smooth(0.13, 0.42, direction.abs());
        Some(Self {
            columns,
            origin: lo.map(|a| a as f64),
            span: [width, height],
            base,
            points_right: direction >= 0.,
            broad,
            narrow: smooth(45., 60., width),
        })
    }

    fn strength(&self, x: usize, y: usize) -> f64 {
        let [width, height] = self.span;
        let h = (x as f64 - self.origin[0]) / width;
        let z = (y as f64 - self.origin[1]) / height;
        let from_base = if self.points_right { h } else { 1. - h };
        let reach = smooth(0.05, 0.13, from_base) * (1. - smooth(0.42, 0.535, from_base));
        let column = self.columns[x];
        let length = column.bottom - column.top;
        let fraction = (y - column.top) as f64 / length.max(1) as f64;
        let fill = column.count as f64 / (1 + length) as f64;
        let wide = reach
            * smooth(0.49, 0.66, fraction)
            * (1. - smooth(0.81, 0.96, fraction))
            * smooth(0.78, 0.97, fill)
            * smooth(0.47, 0.58, z);

        // When foreshortened, support stays in the bottom bulk, away
        // from overlapping fingers and the palm above.
        let radial = ((x as f64 - self.base) / (width * 0.325)).hypot((z - 0.845) / 0.18);
        let narrow = (1. - smooth(0.48, 1., radial)) * smooth(0.75, 0.825, z) * self.narrow;
        self.broad * wide + (1. - self.broad) * narrow
    }
}

pub(super) fn apply(coverage: &InteriorMatte, gray: &[u8], pixels: &mut [u32]) {
    let Some(forearm) = Forearm::from_coverage(coverage) else {
        return;
    };
    for y in 0..ROWS {
        for x in 0..COLS {
            let depth = coverage.dot_depth(x, y);
            if depth <= 2 {
                continue;
            }
            let col = SPEC.origin[0] + SPEC.pitch[0] * x;
            let row = SPEC.origin[1] + SPEC.pitch[1] * y;
            let first = row * SPEC.width + col;
            let gray = gray[first];
            if !(25..106).contains(&gray) {
                continue;
            }
            let value = f64::from(gray);
            let tone = smooth(24., 39., value) * (1. - smooth(74., 106., value));
            let gain = (MAX_GAIN * tone * smooth(2., 5., f64::from(depth)) * forearm.strength(x, y))
                .round_ties_even() as u8;
            if gain == 0 {
                continue;
            }
            for dv in 0..SPEC.dot_size[1] {
                let first = first + dv * SPEC.width;
                for pixel in &mut pixels[first..first + SPEC.dot_size[0]] {
                    let [a, r, g, b] = pixel.to_be_bytes();
                    *pixel = u32::from_be_bytes([
                        a,
                        r.saturating_add(gain),
                        g.saturating_add(gain),
                        b.saturating_add(gain),
                    ]);
                }
            }
        }
    }
}

fn smooth(lo: f64, hi: f64, value: f64) -> f64 {
    let t = ((value - lo) / (hi - lo)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
