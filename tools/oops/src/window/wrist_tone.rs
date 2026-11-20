//! Local wrist highlight control for the reviewed motion on the opaque pane.
use super::{Artwork, RasterSpec};
use std::io;

const COLS: usize = crate::archive_profile::WIDTH as usize;
const ROWS: usize = crate::archive_profile::HEIGHT as usize;
const FIRST: u32 = 288;
const LAST: u32 = 319;
const CELLS: usize = COLS * ROWS;
const LIMIT: u8 = 7;
const SPEC: RasterSpec = RasterSpec::REFERENCE;

pub(super) struct WristTone(Vec<Vec<u8>>);

impl WristTone {
    pub(super) fn prepare(art: &mut Artwork) -> io::Result<Self> {
        // Derive the local gain once from the exact reviewed pixels. The
        // rate cap reduces isolated flicker between neighboring poses.
        let mut frames = Vec::with_capacity((LAST - FIRST + 1) as usize);
        for pose in FIRST..=LAST {
            art.update(pose)?;
            frames.push(raw(art, pose));
        }
        for i in 1..frames.len() {
            let (a, b) = frames.split_at_mut(i);
            cap(&mut b[0], &a[i - 1]);
        }
        for i in (1..frames.len()).rev() {
            let (a, b) = frames.split_at_mut(i);
            cap(&mut a[i - 1], &b[0]);
        }
        Ok(Self(frames))
    }

    pub(super) fn apply(&self, pose: u32, pixels: &mut [u32]) {
        let Some(delta) = pose
            .checked_sub(FIRST)
            .and_then(|offset| self.0.get(offset as usize))
        else {
            return;
        };
        for (i, &d) in delta.iter().enumerate() {
            if d == 0 {
                continue;
            }
            let origin = pixel(i % COLS, i / COLS);
            for y in 0..SPEC.dot_size[1] {
                let start = origin + y * SPEC.width;
                for argb in &mut pixels[start..start + SPEC.dot_size[0]] {
                    let [a, r, g, b] = argb.to_be_bytes();
                    *argb = u32::from_be_bytes([
                        a,
                        r.saturating_sub(d),
                        g.saturating_sub(d),
                        b.saturating_sub(d),
                    ]);
                }
            }
        }
    }
}

fn cap(vector: &mut [u8], neighbor: &[u8]) {
    for (v, &n) in vector.iter_mut().zip(neighbor) {
        *v = (*v).min(n.saturating_add(LIMIT));
    }
}

fn pixel(x: usize, y: usize) -> usize {
    (SPEC.origin[1] + y * SPEC.pitch[1]) * SPEC.width + SPEC.origin[0] + x * SPEC.pitch[0]
}

fn smooth(value: f64) -> f64 {
    let t = value.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn raw(art: &Artwork, pose: u32) -> Vec<u8> {
    let mut out = vec![0; CELLS];
    let p = f64::from(pose);
    let phase_weight = smooth((p - 290.) / 11.) * smooth((318. - p) / 11.);
    for x in 68..158 {
        let fx = x as f64;
        let along = smooth((fx - 67.) / 24.) * smooth((158. - fx) / 44.);
        for y in 1..ROWS - 1 {
            let i = pixel(x, y);
            if [
                i,
                i - SPEC.pitch[0],
                i + SPEC.pitch[0],
                i - SPEC.pitch[1] * SPEC.width,
                i + SPEC.pitch[1] * SPEC.width,
            ]
            .iter()
            .any(|&j| art.alpha[j] == 0)
            {
                continue;
            }
            let v = art.argb[i].to_be_bytes();
            let gray = f64::from(v[1]);
            debug_assert_eq!(v[1], v[2]);
            debug_assert_eq!(v[1], v[3]);
            let across = y as f64 - (59.25 + 0.258 * (fx - 72.));
            let plane = smooth((across + 5.) / 6.) * smooth((14. - across) / 6.);
            let err = ((gray - 174.).max(0.) * 0.78).min(40.);
            out[y * COLS + x] = ((plane * along) * err * phase_weight).round_ties_even() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::{Cycle, CycleDuration, Repeat};
    use crate::settings::WindowAppearance;
    #[test]
    fn reviewed_wrist_retains_coverage_and_caps_the_neighbor_change() {
        let cycle = Cycle {
            duration: CycleDuration::WINDOW_DEFAULT,
            ..Cycle::default()
        };
        let mut art = Artwork::new(Repeat::Once, WindowAppearance::Safe, cycle).unwrap();
        let wrist = art.wrist.take().unwrap();
        assert!(wrist.0[0].iter().all(|&x| x == 0) && wrist.0[31].iter().all(|&x| x == 0));
        assert_eq!(
            wrist.0.iter().filter(|f| f.iter().any(|&d| d != 0)).count(),
            26
        );
        let count = wrist.0[16].iter().filter(|&&x| x > 0).count();
        assert_eq!(count, 797);
        assert_eq!(wrist.0[16].iter().max(), Some(&39));
        assert!(wrist.0.windows(2).all(|pair| {
            pair[0]
                .iter()
                .zip(&pair[1])
                .all(|(&a, &b)| a.abs_diff(b) <= LIMIT)
        }));
        art.update(304).unwrap();
        let existing = art.alpha.clone();
        let base = art.argb.clone();
        wrist.apply(304, &mut art.argb);
        assert_eq!(art.alpha, existing);
        assert_eq!(
            art.argb.iter().zip(base).filter(|&(&a, b)| a != b).count(),
            4 * 797
        );
    }
}
