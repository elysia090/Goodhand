//! Bounded decoder for the VTE reply to CSI 6 n (position, not screen text).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
}

#[derive(Default)]
pub struct Decoder {
    bytes: [u8; 14],
    len: usize,
}

impl Decoder {
    pub fn feed(&mut self, byte: u8) -> Option<Cursor> {
        if byte == 0x1b {
            self.len = 1;
            return None;
        }
        if self.len == 0 {
            return None;
        }
        if self.len == 1 {
            self.len = usize::from(byte == b'[') * 2;
            return None;
        }
        if byte == b'R' {
            let body = std::str::from_utf8(&self.bytes[2..self.len]).ok()?;
            self.len = 0;
            let (row, col) = body.split_once(';')?;
            let row = row.parse().ok()?;
            let col = col.parse().ok()?;
            return (row > 0 && col > 0).then_some(Cursor { row, col });
        }
        if self.len == self.bytes.len() || !(byte.is_ascii_digit() || byte == b';') {
            self.len = 0;
            return None;
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_positive_in_range_pairs_and_resynchronizes() {
        let mut decoder = Decoder::default();
        for bad in [
            b"\x1b[0;1R".as_slice(),
            b"\x1b[65536;1R",
            b"\x1b[1;0R",
            b"\x1b[1;2;3R",
            b"\x1b[;;;;;;99999999;1R",
            b"[1;2R",
        ] {
            assert!(
                bad.iter().filter_map(|&b| decoder.feed(b)).next().is_none(),
                "{bad:?}"
            );
        }
        let parsed: Vec<_> = b"noise\x1b[4;\x1b[65535;3R\x1b[1;80R"
            .iter()
            .filter_map(|&b| decoder.feed(b))
            .collect();
        assert_eq!(
            parsed,
            [Cursor { row: 65535, col: 3 }, Cursor { row: 1, col: 80 }]
        );
    }
}
