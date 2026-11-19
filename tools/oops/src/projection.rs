//! Shared uniform physical transform from cell centers to scene coordinates.
use crate::settings::Offset;
use std::io;

#[derive(Clone, Copy)]
pub struct Projection {
    pub cols: usize,
    pub rows: usize,
    step: [f64; 2],
    center: [f64; 2],
}

impl Projection {
    pub fn new(
        cols: usize,
        rows: usize,
        frame: (f64, f64),
        aspect: f64,
        zoom: f64,
        offset: [Offset; 2],
    ) -> io::Result<Self> {
        if cols == 0
            || rows == 0
            || cols.checked_mul(rows).is_none()
            || ![frame.0, frame.1]
                .into_iter()
                .all(|span| span.is_finite() && span > 0.0)
            || !aspect.is_finite()
            || aspect <= 0.0
            || !zoom.is_finite()
            || zoom <= 0.0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid projection grid",
            ));
        }
        let width = cols as f64;
        let height = rows as f64;
        let dy = (frame.1 / height).max(frame.0 / (aspect * width)) / zoom;
        let dx = dy * aspect;
        if !dx.is_finite() || !dy.is_finite() || dx <= 0.0 || dy <= 0.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid projection scale",
            ));
        }
        Ok(Self {
            cols,
            rows,
            step: [dx, dy],
            center: [
                (width - 1.0) * 0.5 + width * offset[0].fraction(),
                (height - 1.0) * 0.5 + height * offset[1].fraction(),
            ],
        })
    }

    #[cfg(any(test, not(goodhand_embedded_playback)))]
    pub fn limit_samples(self, max: usize) -> Self {
        let samples = self.cols * self.rows;
        if samples <= max {
            return self;
        }
        // Retain the complete footprint and its center while reducing the
        // number of expensive procedural traces at large viewport sizes.
        let scale = (max as f64 / samples as f64).sqrt();
        let cols = ((self.cols as f64 * scale) as usize).clamp(1, max);
        let rows = ((self.rows as f64 * scale) as usize).clamp(1, max / cols);
        let resize = [
            self.cols as f64 / cols as f64,
            self.rows as f64 / rows as f64,
        ];
        Self {
            cols,
            rows,
            step: [self.step[0] * resize[0], self.step[1] * resize[1]],
            center: [
                (self.center[0] + 0.5) / resize[0] - 0.5,
                (self.center[1] + 0.5) / resize[1] - 0.5,
            ],
        }
    }

    pub fn x(self, col: usize) -> f64 {
        (col as f64 - self.center[0]) * self.step[0]
    }

    /// Positive y points down; the procedural model uses the opposite sign.
    pub fn y(self, row: usize) -> f64 {
        (row as f64 - self.center[1]) * self.step[1]
    }

    #[cfg(test)]
    pub fn step(self) -> (f64, f64) {
        (self.step[0], self.step[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiting_samples_keeps_the_same_outer_edges_and_offsets() {
        for (cols, rows) in [(460, 320), (71, 30), (2, 11_000)] {
            let fine = Projection::new(
                cols,
                rows,
                (1000.0, 640.0),
                0.7,
                1.3,
                ["14".parse().unwrap(), "-30".parse().unwrap()],
            )
            .unwrap();
            let coarse = fine.limit_samples(400);
            assert!(coarse.cols * coarse.rows <= 400);
            assert!(coarse.cols <= fine.cols && coarse.rows <= fine.rows);
            let edges = |p: Projection| {
                [
                    p.x(0) - p.step[0] * 0.5,
                    p.x(p.cols - 1) + p.step[0] * 0.5,
                    p.y(0) - p.step[1] * 0.5,
                    p.y(p.rows - 1) + p.step[1] * 0.5,
                ]
            };
            for (a, b) in edges(fine).into_iter().zip(edges(coarse)) {
                assert!((a - b).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn offsets_translate_the_center_in_drawable_cells_without_changing_scale() {
        for (cols, rows, pitch) in [(200, 64, 0.5), (160, 48, 0.467), (140, 50, 0.7)] {
            for zoom in [0.50, 1.0, 2.0] {
                let make = |x: &str, y: &str| {
                    Projection::new(
                        cols,
                        rows,
                        (1000.0, 640.0),
                        pitch,
                        zoom,
                        [x.parse().unwrap(), y.parse().unwrap()],
                    )
                    .unwrap()
                };
                let centered = make("0", "0");
                let shifted = make("25", "-25");
                let (dx, dy) = centered.step();
                assert_eq!(shifted.step(), (dx, dy));
                assert!((dx / dy - pitch).abs() < 1e-12);
                assert!((centered.x(0) - shifted.x(0) - cols as f64 * dx / 4.0).abs() < 1e-12);
                assert!((shifted.y(0) - centered.y(0) - rows as f64 * dy / 4.0).abs() < 1e-12);
            }
        }
    }
}
