//! Material dark-theme neutral tones on the cool, nearly black pane. Do not alter the archive.

type Color = [u8; 3];

#[cfg(any(test, goodhand_desktop_artwork))]
pub(super) const BACKDROP_RGB: Color = [12, 16, 18];

// Approved dot gray values are the input, not the old display transfer.
// Against #0C1012 the lit form, midtone and recurring highlight stops are
// about 3.64:1, 7.31:1 and 14.48:1 respectively. Dark detail is intentional.
// The three reference gray foregrounds are 38%, 60% and 87% white
// composited on Material 2 Dark's #121212. Our pane itself remains #0C1012.
const NEUTRAL: [(u32, u8); 10] = [
    (0, 0),
    (13, 20),
    (40, 58),
    (62, 108),
    (82, 137),
    (110, 160),
    (130, 192),
    (148, 224),
    (160, 239),
    (255, 255),
];

pub(super) fn palette() -> [Color; 256] {
    std::array::from_fn(|gray| [neutral(gray as u32); 3])
}

fn neutral(gray: u32) -> u8 {
    for stops in NEUTRAL.windows(2) {
        let [(lo, a), (hi, b)] = [stops[0], stops[1]];
        if gray <= hi {
            let span = hi - lo;
            return ((u32::from(a) * (hi - gray) + u32::from(b) * (gray - lo) + span / 2) / span)
                as u8;
        }
    }
    NEUTRAL[NEUTRAL.len() - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color) -> f64 {
        c.into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(byte, weight)| {
                let s = f64::from(byte) / 255.;
                weight
                    * if s <= 0.04045 {
                        s / 12.92
                    } else {
                        ((s + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }

    #[test]
    fn neutral_ramp_preserves_black_and_luminance_order() {
        let p = palette();
        assert_eq!(p[0], [0; 3]);
        assert_eq!(p[255], [255; 3]);
        for [r, g, b] in p {
            assert!(r == g && g == b);
        }
        assert!(p.windows(2).all(|s| luminance(s[0]) <= luminance(s[1])));
    }

    #[test]
    fn lit_form_medium_and_key_have_designed_contrast() {
        let p = palette();
        assert_eq!([p[62], p[110], p[148]], [[108; 3], [160; 3], [224; 3]]);
        let background = luminance(BACKDROP_RGB);
        for (gray, low, high) in [(62, 3.6, 3.7), (110, 7.0, 7.7), (148, 14.2, 14.8)] {
            let contrast = (luminance(p[gray]) + 0.05) / (background + 0.05);
            assert!((low..high).contains(&contrast), "{gray}: {contrast}");
        }
    }
}
