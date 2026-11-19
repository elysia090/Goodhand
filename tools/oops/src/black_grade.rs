//! The reviewed foreground grade for a black canvas.
//!
//! These are scalar tone and hue controls, not artwork. Prepare once per
//! painter; no per-pixel grading or background detection.

type Color = [f64; 3];

fn value([r, g, b]: Color) -> f64 {
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

fn smooth(x: f64, lo: f64, hi: f64) -> f64 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(x: f64, stops: &[(f64, f64)]) -> f64 {
    let end = stops.partition_point(|&(level, _)| level <= x);
    if end == 0 {
        return stops[0].1;
    }
    if end == stops.len() {
        return stops[end - 1].1;
    }
    let (a, from) = stops[end - 1];
    let (b, to) = stops[end];
    from + (x - a) * ((to - from) / (b - a))
}

fn add(a: Color, b: Color) -> Color {
    std::array::from_fn(|i| a[i] + b[i])
}

fn quantized(color: Color) -> Color {
    color.map(f64::round_ties_even)
}

fn map_ordered(palette: &mut [Color; 256], transform: impl Fn(Color) -> Color) {
    let mut previous = [0.0_f64; 3];
    for rgb in palette {
        let next = transform(*rgb);
        *rgb = std::array::from_fn(|i| previous[i].max(next[i]));
        previous = *rgb;
    }
}

fn paper_ramp(gray: usize) -> Color {
    const LEVELS: &[(f64, f64)] = &[
        (0.0, 0.0),
        (8.0, 8.0),
        (16.0, 19.0),
        (25.0, 43.0),
        (34.0, 59.0),
        (48.0, 77.0),
        (68.0, 99.0),
        (100.0, 127.0),
        (150.0, 166.0),
        (190.0, 198.0),
        (230.0, 230.0),
        (255.0, 250.0),
    ];
    const PAPER: [Color; 7] = [
        [16.0, 15.0, 15.0],
        [40.0, 39.0, 38.0],
        [87.0, 86.0, 83.0],
        [135.0, 133.0, 128.0],
        [183.0, 181.0, 172.0],
        [230.0, 228.0, 217.0],
        [255.0, 252.0, 240.0],
    ];
    let gray = gray as f64;
    let light = lerp(gray, LEVELS);
    let strength = 0.55 * (light / 65.0).clamp(0.0, 1.0);
    let preserve_black = 0.50 * ((gray - 12.0) / 14.0).clamp(0.0, 1.0);
    std::array::from_fn(|i| {
        let offsets = PAPER.map(|rgb| (value(rgb), rgb[i] - value(rgb)));
        let paper = (light + lerp(light, &offsets) * strength)
            .clamp(0.0, 255.0)
            .round_ties_even();
        (gray + (paper - gray) * preserve_black + 0.5).floor()
    })
}

fn neutral_guide() -> [Color; 256] {
    let mut neutral = std::array::from_fn(paper_ramp);
    // Soft shoulder before the hue roles are applied. Keep the levels
    // ordered through the historical rounding used in the reviewed grade.
    map_ordered(&mut neutral, |rgb| {
        let t = ((value(rgb) - 116.0) / 132.0).clamp(0.0, 1.0);
        let shoulder = (17.0 * t * t * (3.0 - 2.0 * t)).round_ties_even();
        rgb.map(|v| v - shoulder)
    });
    let mut middle = neutral;
    map_ordered(&mut middle, |rgb| {
        let s = smooth(value(rgb), 7.0, 43.0);
        add(rgb, quantized([-2.0 * s, s, -3.0 * s]))
    });
    for (rgb, green) in neutral.iter_mut().zip(middle) {
        let light = value(*rgb);
        let dark = smooth(light, 5.0, 21.0) * (1.0 - smooth(light, 29.0, 67.0));
        let high = smooth(light, 123.0, 198.0);
        let shadow = [1.0, -1.0, 4.0];
        let lit = [1.0, 0.0, -3.0];
        *rgb = quantized(add(
            *rgb,
            std::array::from_fn(|i| {
                (green[i] - rgb[i]) * (1.0 - dark - high) + dark * shadow[i] + high * lit[i]
            }),
        ));
    }
    map_ordered(&mut neutral, |rgb| rgb);
    neutral
}

fn roles(light: f64) -> Color {
    let cool = smooth(light, 35.0, 69.0);
    let warm = smooth(light, 115.0, 164.0);
    let guard = smooth(light, 4.0, 19.0);
    let gold = [2.3, 0.5, -11.7];
    let blue_green = [-6.0, 1.0, 8.0];
    let russet = [9.0, -2.0, -6.7];
    let chroma: Color = std::array::from_fn(|i| {
        (((1.0 - cool) * gold[i] + cool * blue_green[i]) * (1.0 - warm) + warm * russet[i]) * guard
    });
    let dip = 3.0 * smooth(light, 5.0, 20.0) * (1.0 - smooth(light, 36.0, 77.0));
    let lift = 6.0 * smooth(light, 93.0, 159.0) * (1.0 - smooth(light, 213.0, 243.0));
    let level = light + lift - dip;
    // Keep the complete hue vector inside gamut rather than clipping R/G/B
    // independently, which would unpredictably change dark-cell lightness.
    let limit = chroma.iter().fold(1.0_f64, |limit, &tint| {
        if tint > 0.0 {
            limit.min((255.0 - level) / tint)
        } else if tint < 0.0 {
            limit.min(level / -tint)
        } else {
            limit
        }
    });
    quantized(chroma.map(|tint| level + tint * limit))
}

pub(super) fn palette() -> [[u8; 3]; 256] {
    neutral_guide().map(|guide| {
        let light = value(guide);
        let rgb = roles(light);
        let high = smooth(light, 123.0, 166.0);
        let rgb = add(rgb, quantized([-high, high, -5.0 * high]));
        let middle = smooth(light, 48.0, 76.0) * (1.0 - smooth(light, 100.0, 128.0));
        let rgb = add(rgb, quantized([middle, 4.0 * middle, -2.0 * middle]));
        let shade = value(rgb);
        let soft = smooth(shade, 135.0, 171.0);
        let rgb = quantized(rgb.map(|v| v + 0.22 * soft * (shade - 2.5 - v) - 1.8 * soft));
        let straw = smooth(value(rgb), 51.0, 102.0);
        let rgb = add(rgb, quantized([0.0, 0.2 * straw, -2.0 * straw]));
        let shade = value(rgb);
        let dark = smooth(shade, 4.0, 17.0) * (1.0 - smooth(shade, 30.0, 54.0));
        add(rgb, quantized([dark, -0.25 * dark, -0.50 * dark])).map(|c| c as u8)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_grade_keeps_black_and_tonal_order() {
        let rgb = palette();
        assert_eq!(rgb[0], [0; 3]);
        assert_eq!(rgb[20], [23, 20, 8]);
        assert_eq!(rgb[70], [81, 91, 90]);
        assert_eq!(rgb[160], [171, 164, 153]);
        assert!(
            rgb.windows(2)
                .all(|pair| { value(pair[0].map(f64::from)) <= value(pair[1].map(f64::from)) })
        );
    }
}
