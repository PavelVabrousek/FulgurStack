//! Which columns of each source ROI row a rotated tile can reach (ADR-113).
//! The ROI is the bounding box of the rotated tile, so its corners hold pixels
//! no output sample reads; cosmetics and demosaicing skip them. A frame with a
//! distortion model takes its spans from the solved source positions of its
//! tile's edge samples instead of a straight outline (ADR-144).

/// Rows of the 4x4 interpolation footprint around a position, and its columns.
const FOOTPRINT_REACH: f64 = 3.;
/// Cosmetics also serve the 5x5 demosaicing window around each footprint pixel.
const WINDOW_REACH: f64 = 2.;

/// Column range `[from, to)` per ROI row; `[0, 0]` for a row nothing reads.
#[derive(Default)]
pub struct RowSpans {
    pub cosmetic: Vec<[usize; 2]>,
    pub demosaic: Vec<[usize; 2]>,
}

impl RowSpans {
    /// `corners` is the convex tile outline in ROI coordinates, in order.
    pub(crate) fn fill(&mut self, corners: [[f64; 2]; 4], width: usize, height: usize) {
        spans(&mut self.demosaic, corners, width, height, FOOTPRINT_REACH);
        spans(
            &mut self.cosmetic,
            corners,
            width,
            height,
            FOOTPRINT_REACH + WINDOW_REACH,
        );
    }
}

/// Extra pixels around an outline's samples: the edge between two samples
/// one output pixel apart bends far less than a pixel in the source.
const OUTLINE_MARGIN: usize = 1;

impl RowSpans {
    /// Spans of a tile whose source outline is curved, from the solved
    /// source positions of its edge samples. The tile's source region is
    /// bounded by its edge, so in every row band its samples lie between the
    /// outermost edge samples of that band; one more pixel on every side
    /// covers the edge between samples.
    pub(crate) fn fill_outline(
        &mut self,
        edge: &[[f64; 2]],
        origin: [usize; 2],
        width: usize,
        height: usize,
    ) {
        self.fill_points(edge, origin, width, height);
        let m = OUTLINE_MARGIN;
        let rows = self.demosaic.clone();
        for (row, span) in self.demosaic.iter_mut().enumerate() {
            let (mut from, mut to) = (usize::MAX, 0);
            for near in &rows[row.saturating_sub(m)..(row + m + 1).min(height)] {
                if near[0] < near[1] {
                    from = from.min(near[0].saturating_sub(m));
                    to = to.max((near[1] + m).min(width));
                }
            }
            *span = if from < to { [from, to] } else { [0, 0] };
        }
        // Between the left and right edge every row of the band is covered.
        let first = self.demosaic.iter().position(|s| s[0] < s[1]);
        let last = self.demosaic.iter().rposition(|s| s[0] < s[1]);
        if let (Some(first), Some(last)) = (first, last) {
            for span in &mut self.demosaic[first..=last] {
                if span[0] >= span[1] {
                    *span = [0, width];
                }
            }
        }
        self.widen_cosmetic(width, height);
    }

    /// Spans of the 4x4 footprints around `points`, source positions in
    /// sensor pixels; `origin` is the ROI's top-left sensor pixel. Points that
    /// are not finite read nothing. Cosmetics get the 5x5 demosaicing window
    /// around every footprint pixel.
    pub(crate) fn fill_points(
        &mut self,
        points: &[[f64; 2]],
        origin: [usize; 2],
        width: usize,
        height: usize,
    ) {
        let empty = [usize::MAX, 0];
        self.demosaic.clear();
        self.demosaic.resize(height, empty);
        let (w, h) = (width as f64, height as f64);
        for p in points {
            let (x, y) = (p[0] - origin[0] as f64, p[1] - origin[1] as f64);
            if !(x.is_finite() && y.is_finite()) {
                continue;
            }
            // The sampler reads rows and columns floor - 1 ..= floor + 2.
            let (fx, fy) = (x.floor(), y.floor());
            if fx + 2. < 0. || fy + 2. < 0. || fx - 1. >= w || fy - 1. >= h {
                continue;
            }
            let from = (fx - 1.).max(0.) as usize;
            let to = ((fx + 3.).min(w)) as usize;
            let top = (fy - 1.).max(0.) as usize;
            let bottom = ((fy + 3.).min(h)) as usize;
            for span in &mut self.demosaic[top..bottom] {
                span[0] = span[0].min(from);
                span[1] = span[1].max(to);
            }
        }
        for span in &mut self.demosaic {
            if span[0] >= span[1] {
                *span = [0, 0];
            }
        }
        self.widen_cosmetic(width, height);
    }

    /// Cosmetic spans: the demosaicing spans with the 5x5 window around them.
    fn widen_cosmetic(&mut self, width: usize, height: usize) {
        let reach = WINDOW_REACH as usize;
        self.cosmetic.clear();
        self.cosmetic.extend((0..height).map(|row| {
            let (mut from, mut to) = (usize::MAX, 0);
            for span in &self.demosaic[row.saturating_sub(reach)..(row + reach + 1).min(height)] {
                if span[0] < span[1] {
                    from = from.min(span[0].saturating_sub(reach));
                    to = to.max((span[1] + reach).min(width));
                }
            }
            if from < to { [from, to] } else { [0, 0] }
        }));
    }
}

fn spans(
    out: &mut Vec<[usize; 2]>,
    corners: [[f64; 2]; 4],
    width: usize,
    height: usize,
    reach: f64,
) {
    out.clear();
    out.extend((0..height).map(|row| {
        let (above, below) = (row as f64 - reach, row as f64 + reach);
        let (mut left, mut right) = (f64::INFINITY, f64::NEG_INFINITY);
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let (top, bottom) = if a[1] <= b[1] { (a, b) } else { (b, a) };
            if bottom[1] < above || top[1] > below {
                continue;
            }
            let height = bottom[1] - top[1];
            let slope = if height > 0. {
                (bottom[0] - top[0]) / height
            } else {
                0.
            };
            for y in [top[1].max(above), bottom[1].min(below)] {
                let x = top[0] + slope * (y - top[1]);
                left = left.min(x);
                right = right.max(x);
            }
        }
        if left > right {
            return [0, 0];
        }
        let from = (left - reach).floor().max(0.) as usize;
        let to = ((right + reach).ceil().max(0.) as usize).saturating_add(1);
        [from.min(width), to.min(width)]
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Corners of a rotated square, with points inside it.
    fn rotated(theta: f64, side: f64, shift: [f64; 2]) -> ([[f64; 2]; 4], Vec<[f64; 2]>) {
        let (s, c) = theta.sin_cos();
        let place = |u: f64, v: f64| [c * u - s * v + shift[0], s * u + c * v + shift[1]];
        let corners = [
            place(0., 0.),
            place(side, 0.),
            place(side, side),
            place(0., side),
        ];
        let steps = side as usize * 4;
        let inside = (0..=steps)
            .flat_map(|i| (0..=steps).map(move |j| [i as f64 / 4., j as f64 / 4.]))
            .map(|[u, v]| place(u, v))
            .collect();
        (corners, inside)
    }

    fn reads(span: [usize; 2], x: usize) -> bool {
        (span[0]..span[1]).contains(&x)
    }

    #[test]
    fn every_footprint_and_window_pixel_of_a_rotated_tile_is_inside_its_spans() {
        let (width, height) = (160, 150);
        let mut spans = RowSpans::default();
        for degrees in [0., 1., 7.5, 30., 45., 60., 90., 123., 180., 271.] {
            let (corners, inside) = rotated(f64::to_radians(degrees), 40., [80., 70.]);
            spans.fill(corners, width, height);
            for p in inside {
                if p[0] < 1. || p[1] < 1. {
                    continue;
                }
                let (x, y) = (p[0] as usize, p[1] as usize);
                if x + 2 >= width || y + 2 >= height {
                    continue;
                }
                for fy in y - 1..=y + 2 {
                    for fx in x - 1..=x + 2 {
                        assert!(reads(spans.demosaic[fy], fx), "{degrees}: {p:?}");
                        for wy in fy.saturating_sub(2)..=(fy + 2).min(height - 1) {
                            for wx in fx.saturating_sub(2)..=(fx + 2).min(width - 1) {
                                assert!(reads(spans.cosmetic[wy], wx), "{degrees}: {p:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_rotated_tile_leaves_the_corners_of_its_box_out() {
        let side = 512.;
        let extent = side * 2_f64.sqrt();
        let (width, height) = (extent as usize + 20, extent as usize + 20);
        let (corners, _) = rotated(f64::to_radians(45.), side, [extent / 2. + 10., 10.]);
        let mut spans = RowSpans::default();
        spans.fill(corners, width, height);
        let read: usize = spans.demosaic.iter().map(|s| s[1] - s[0]).sum();
        let share = read as f64 / (width * height) as f64;
        assert!((0.45..0.6).contains(&share), "{share}");
    }

    #[test]
    fn rows_beyond_the_tile_are_empty() {
        let corners = [[10., 40.], [30., 40.], [30., 60.], [10., 60.]];
        let mut spans = RowSpans::default();
        spans.fill(corners, 100, 100);
        assert_eq!(spans.demosaic[30], [0, 0]);
        assert_eq!(spans.demosaic[37], [7, 34]);
        assert_eq!(spans.demosaic[90], [0, 0]);
        assert_eq!(spans.cosmetic[35], [5, 36]);
    }
}

#[cfg(test)]
mod point_tests {
    use super::*;

    #[test]
    fn every_footprint_and_window_pixel_of_curved_samples_is_inside_their_spans() {
        let (width, height) = (120, 110);
        let origin = [30, 40];
        // A curved, rotated band of sample positions in sensor pixels.
        let points: Vec<[f64; 2]> = (0..60)
            .flat_map(|j| (0..50).map(move |i| (i as f64, j as f64)))
            .map(|(u, v)| {
                let x = 50. + 0.9 * u - 0.3 * v + 0.004 * u * u;
                let y = 60. + 0.3 * u + 0.9 * v - 0.003 * v * v;
                [x + 0.37, y + 0.61]
            })
            .chain([[f64::NEG_INFINITY; 2], [f64::NAN, 3.]])
            .collect();
        let mut spans = RowSpans::default();
        spans.fill_points(&points, origin, width, height);
        let reads = |span: [usize; 2], x: usize| (span[0]..span[1]).contains(&x);
        for p in points
            .iter()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
        {
            let (x, y) = ((p[0] - 30.) as usize, (p[1] - 40.) as usize);
            for fy in y - 1..=y + 2 {
                for fx in x - 1..=x + 2 {
                    assert!(reads(spans.demosaic[fy], fx), "{p:?}");
                    for wy in fy - 2..=fy + 2 {
                        for wx in fx - 2..=fx + 2 {
                            assert!(reads(spans.cosmetic[wy], wx), "{p:?}");
                        }
                    }
                }
            }
        }
        let read: usize = spans.demosaic.iter().map(|s| s[1] - s[0]).sum();
        assert!(read < width * height * 2 / 3, "{read}");
        assert_eq!(spans.demosaic[0], [0, 0]);
    }
}
