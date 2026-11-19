//! Pack projected samples into single display cells.
use std::num::NonZeroU8;
use std::str::FromStr;
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Renderer {
    #[default]
    Braille,
    Dots,
}

impl FromStr for Renderer {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "braille" => Ok(Self::Braille),
            "dots" => Ok(Self::Dots),
            _ => Err("expected braille or dots"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DotCell {
    pub pattern: NonZeroU8,
    pub gray: u8,
}

impl Renderer {
    pub fn label(self) -> &'static str {
        match self {
            Self::Braille => "braille",
            Self::Dots => "dots",
        }
    }

    pub fn subdivisions(self) -> [usize; 2] {
        match self {
            Self::Dots => [1, 1],
            Self::Braille => [2, 4],
        }
    }

    pub fn glyph(self, pattern: NonZeroU8) -> [u8; 3] {
        match self {
            Self::Dots => *b"\xe2\x80\xa2",
            Self::Braille => {
                // U+2800 plus the Braille pattern always encodes this way.
                let bits = pattern.get();
                [0xe2, 0xa0 | bits >> 6, 0x80 | bits & 0x3f]
            }
        }
    }

    pub fn pack(self, samples: &[Option<u8>], cols: usize, out: &mut [Option<DotCell>]) {
        match self {
            Self::Dots => {
                debug_assert_eq!(samples.len(), out.len());
                for (cell, gray) in out.iter_mut().zip(samples) {
                    *cell = gray.map(|gray| DotCell {
                        pattern: NonZeroU8::MIN,
                        gray,
                    });
                }
            }
            Self::Braille => {
                debug_assert_eq!(samples.len(), out.len() * 8);
                let stride = cols * 2;
                for (samples, cells) in samples
                    .chunks_exact(stride * 4)
                    .zip(out.chunks_exact_mut(cols))
                {
                    for (col, cell) in cells.iter_mut().enumerate() {
                        *cell = braille(samples, stride, col * 2);
                    }
                }
            }
        }
    }
}

fn braille(samples: &[Option<u8>], stride: usize, col: usize) -> Option<DotCell> {
    // Unicode's fourth row uses bits 6 and 7; the first three rows
    // are column-major. One terminal foreground is shared by all dots.
    const BITS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];
    let mut mask = 0u8;
    let mut sum = 0u16;
    let mut count = 0u16;
    for (row, bits) in BITS.into_iter().enumerate() {
        for (dx, bit) in bits.into_iter().enumerate() {
            if let Some(level) = samples[row * stride + col + dx] {
                mask |= 1 << bit;
                sum += u16::from(level);
                count += 1;
            }
        }
    }
    Some(DotCell {
        pattern: NonZeroU8::new(mask)?,
        gray: ((sum + count / 2) / count) as u8,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_storage_is_compact_and_every_glyph_has_the_expected_codepoint() {
        assert_eq!(std::mem::size_of::<Option<DotCell>>(), 2);
        for bits in 1..=u8::MAX {
            let pattern = NonZeroU8::new(bits).unwrap();
            let bytes = Renderer::Braille.glyph(pattern);
            let glyph = char::from_u32(0x2800 + u32::from(bits)).unwrap();
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), glyph.to_string());
        }
        assert_eq!(Renderer::Dots.glyph(NonZeroU8::MIN), *b"\xe2\x80\xa2");
    }

    #[test]
    fn braille_places_all_eight_bits_and_rounds_only_occupied_grays() {
        for (cell, code) in [1, 8, 2, 16, 4, 32, 64, 128].into_iter().enumerate() {
            let mut samples = [None; 8];
            samples[cell] = Some(0);
            assert_eq!(
                braille(&samples, 2, 0),
                Some(DotCell {
                    pattern: NonZeroU8::new(code).unwrap(),
                    gray: 0
                }),
            );
        }
        assert_eq!(braille(&[None; 8], 2, 0), None);
        let mut samples = [None; 8];
        samples[2] = Some(43);
        samples[7] = Some(100);
        assert_eq!(
            braille(&samples, 2, 0),
            Some(DotCell {
                pattern: NonZeroU8::new(0x82).unwrap(),
                gray: 72
            })
        );
    }

    #[test]
    fn braille_rows_and_neighbors_are_not_interleaved() {
        let mut samples = [None; 32];
        samples[20] = Some(21);
        samples[7] = Some(61);
        let mut cells = [None; 4];
        Renderer::Braille.pack(&samples, 2, &mut cells);
        assert_eq!(
            cells,
            [
                None,
                Some(DotCell {
                    pattern: NonZeroU8::new(0x10).unwrap(),
                    gray: 61
                }),
                Some(DotCell {
                    pattern: NonZeroU8::new(0x02).unwrap(),
                    gray: 21
                }),
                None,
            ]
        );
    }
}
