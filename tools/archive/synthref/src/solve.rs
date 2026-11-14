//! Blind plate solving near a known target: triangle asterisms of the
//! brightest detected and catalogue stars give similarity candidates over a
//! ladder of field sizes and both parities; the candidate that maps the most
//! catalogue stars onto detected stars is refined to a linear TAN solution.
use crate::catalog::{self, CatStar};
use crate::detect::Star;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Half field heights tried, in degrees.
const LADDER: [f64; 9] = [0.06, 0.09, 0.13, 0.19, 0.28, 0.42, 0.63, 0.95, 1.4];
const DETECTED_FOR_TRIANGLES: usize = 40;
const CATALOGUE_FOR_TRIANGLES: usize = 70;
const CATALOGUE_FOR_VOTES: usize = 300;
const INLIER_PX: f64 = 3.;

#[derive(Clone, Copy, Debug)]
struct Sim {
    a: f64,
    b: f64,
    tx: f64,
    ty: f64,
}

impl Sim {
    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.a * p[0] - self.b * p[1] + self.tx,
            self.b * p[0] + self.a * p[1] + self.ty,
        ]
    }

    fn fit(src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Self> {
        let n = src.len() as f64;
        let ms = src
            .iter()
            .fold([0., 0.], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
        let md = dst
            .iter()
            .fold([0., 0.], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
        let (mut sq, mut a, mut b) = (0., 0., 0.);
        for (s, d) in src.iter().zip(dst) {
            let q = [s[0] - ms[0], s[1] - ms[1]];
            let p = [d[0] - md[0], d[1] - md[1]];
            sq += q[0] * q[0] + q[1] * q[1];
            a += q[0] * p[0] + q[1] * p[1];
            b += q[0] * p[1] - q[1] * p[0];
        }
        (sq > 0.).then(|| {
            let (a, b) = (a / sq, b / sq);
            Sim {
                a,
                b,
                tx: md[0] - (a * ms[0] - b * ms[1]),
                ty: md[1] - (b * ms[0] + a * ms[1]),
            }
        })
    }
}

/// Standard coordinates (arcsec) to pixels: x = m0 + m1 xi + m2 eta, y = m3 + m4 xi + m5 eta.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub m: [f64; 6],
}

impl Affine {
    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        let m = &self.m;
        [
            m[0] + m[1] * p[0] + m[2] * p[1],
            m[3] + m[4] * p[0] + m[5] * p[1],
        ]
    }

    pub fn invert(&self, q: [f64; 2]) -> [f64; 2] {
        let m = &self.m;
        let det = m[1] * m[5] - m[2] * m[4];
        let (dx, dy) = (q[0] - m[0], q[1] - m[3]);
        [
            (m[5] * dx - m[2] * dy) / det,
            (-m[4] * dx + m[1] * dy) / det,
        ]
    }

    /// Mean pixel scale in arcsec per pixel.
    pub fn arcsec_per_px(&self) -> f64 {
        1. / (self.m[1] * self.m[5] - self.m[2] * self.m[4]).abs().sqrt()
    }

    /// True when the sky appears mirrored (east and north not anticlockwise).
    pub fn mirrored(&self) -> bool {
        // Top-down pixels flip y; an unmirrored sky then has a positive determinant.
        self.m[1] * self.m[5] - self.m[2] * self.m[4] < 0.
    }

    /// Position angle of north, degrees east of up (image top).
    pub fn north_angle_deg(&self) -> f64 {
        // Pixel direction of +eta; "up" is -y.
        let (dx, dy) = (self.m[2], self.m[5]);
        dx.atan2(-dy).to_degrees()
    }

    fn from_sim(s: Sim, parity: f64) -> Self {
        Self {
            m: [s.tx, s.a * parity, -s.b, s.ty, s.b * parity, s.a],
        }
    }

    fn fit(src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Self> {
        let rows: Vec<Vec<f64>> = src.iter().map(|p| vec![1., p[0], p[1]]).collect();
        let x = least_squares(&rows, &dst.iter().map(|d| d[0]).collect::<Vec<_>>())?;
        let y = least_squares(&rows, &dst.iter().map(|d| d[1]).collect::<Vec<_>>())?;
        Some(Self {
            m: [x[0], x[1], x[2], y[0], y[1], y[2]],
        })
    }
}

/// Least squares through the normal equations with partial pivoting.
fn least_squares(rows: &[Vec<f64>], b: &[f64]) -> Option<Vec<f64>> {
    let n = rows.first()?.len();
    let mut a = vec![vec![0.; n + 1]; n];
    for (row, &v) in rows.iter().zip(b) {
        for i in 0..n {
            for j in 0..n {
                a[i][j] += row[i] * row[j];
            }
            a[i][n] += row[i] * v;
        }
    }
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-300 {
            return None;
        }
        a.swap(c, p);
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (value, &p) in row.iter_mut().zip(&pivot).skip(c) {
                    *value -= f * p;
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

struct Grid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    points: Vec<[f64; 2]>,
}

impl Grid {
    fn new(points: Vec<[f64; 2]>, cell: f64) -> Self {
        let mut cells: HashMap<_, Vec<_>> = HashMap::new();
        for (i, p) in points.iter().enumerate() {
            cells
                .entry(((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64))
                .or_default()
                .push(i);
        }
        Self {
            cell,
            cells,
            points,
        }
    }

    fn nearest(&self, p: [f64; 2], radius: f64) -> Option<(usize, f64)> {
        let (cx, cy) = (
            (p[0] / self.cell).floor() as i64,
            (p[1] / self.cell).floor() as i64,
        );
        let reach = (radius / self.cell).ceil() as i64;
        let mut best: Option<(usize, f64)> = None;
        for gy in cy - reach..=cy + reach {
            for gx in cx - reach..=cx + reach {
                for &i in self.cells.get(&(gx, gy)).into_iter().flatten() {
                    let q = self.points[i];
                    let d = (q[0] - p[0]).hypot(q[1] - p[1]);
                    if d <= radius && best.is_none_or(|b| d < b.1) {
                        best = Some((i, d));
                    }
                }
            }
        }
        best
    }
}

struct Triangle {
    u: f64,
    v: f64,
    /// Vertices opposite the shortest, middle and longest side.
    vertices: [usize; 3],
}

fn triangles(points: &[[f64; 2]]) -> Vec<Triangle> {
    let d = |i: usize, j: usize| (points[i][0] - points[j][0]).hypot(points[i][1] - points[j][1]);
    let mut out = Vec::new();
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            for k in j + 1..points.len() {
                let mut sides = [(d(j, k), i), (d(i, k), j), (d(i, j), k)];
                sides.sort_by(|a, b| a.0.total_cmp(&b.0));
                let [(a, va), (b, vb), (c, vc)] = sides;
                // Near-equal sides make the vertex labels ambiguous.
                if c <= 0. || a / c < 0.1 || (b - a) / c < 0.02 || (c - b) / c < 0.02 {
                    continue;
                }
                out.push(Triangle {
                    u: a / c,
                    v: b / c,
                    vertices: [va, vb, vc],
                });
            }
        }
    }
    out
}

/// The similarity (catalogue plane -> pixels) that maps the most vote stars
/// onto detected stars, with that count.
fn find_similarity(
    detected: &[[f64; 2]],
    catalogue: &[[f64; 2]],
    votes: &[[f64; 2]],
    grid: &Grid,
    size: [f64; 2],
    scale: [f64; 2],
) -> Option<(usize, Sim)> {
    const BIN: f64 = 0.01;
    const TOLERANCE: f64 = 0.006;
    let cat_tri = triangles(catalogue);
    let mut index: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (n, t) in cat_tri.iter().enumerate() {
        index
            .entry(((t.u / BIN) as i64, (t.v / BIN) as i64))
            .or_default()
            .push(n);
    }
    let mut seen = HashSet::new();
    let mut best: Option<(usize, Sim)> = None;
    for t in triangles(detected) {
        let (bu, bv) = ((t.u / BIN) as i64, (t.v / BIN) as i64);
        for gu in bu - 1..=bu + 1 {
            for gv in bv - 1..=bv + 1 {
                for &n in index.get(&(gu, gv)).into_iter().flatten() {
                    let c = &cat_tri[n];
                    if (c.u - t.u).abs() > TOLERANCE || (c.v - t.v).abs() > TOLERANCE {
                        continue;
                    }
                    let src = c.vertices.map(|i| catalogue[i]);
                    let dst = t.vertices.map(|i| detected[i]);
                    let Some(sim) = Sim::fit(&src, &dst) else {
                        continue;
                    };
                    if src.iter().zip(&dst).any(|(s, d)| {
                        let q = sim.apply(*s);
                        (q[0] - d[0]).hypot(q[1] - d[1]) > INLIER_PX
                    }) {
                        continue;
                    }
                    // Pixels per arcsec must fit the field size tried.
                    let s = sim.a.hypot(sim.b);
                    if s < scale[0] || s > scale[1] {
                        continue;
                    }
                    let key = (
                        (sim.a.hypot(sim.b).ln() / 0.01).round() as i64,
                        (sim.b.atan2(sim.a) / 0.005).round() as i64,
                        (sim.tx / 8.).round() as i64,
                        (sim.ty / 8.).round() as i64,
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    // Each detected star agrees with one catalogue star at most.
                    let count = votes
                        .iter()
                        .map(|&p| sim.apply(p))
                        .filter(|q| q[0] >= 0. && q[1] >= 0. && q[0] < size[0] && q[1] < size[1])
                        .filter_map(|q| grid.nearest(q, INLIER_PX).map(|n| n.0))
                        .collect::<HashSet<_>>()
                        .len();
                    if best.is_none_or(|b| count > b.0) {
                        best = Some((count, sim));
                    }
                }
            }
        }
    }
    best
}

pub struct Plate {
    /// Tangent point (degrees).
    pub ra0: f64,
    pub dec0: f64,
    pub affine: Affine,
    pub matched: usize,
    pub rms_px: f64,
    pub max_px: f64,
    pub cubic_rms_px: f64,
    pub candidate_inliers: usize,
    pub candidate_half_field_deg: f64,
    /// The deep catalogue around the solved field, brightest first.
    pub catalogue: Vec<CatStar>,
}

impl Plate {
    pub fn pixel(&self, s: &CatStar) -> Option<[f64; 2]> {
        catalog::project(s.ra_deg, s.dec_deg, self.ra0, self.dec0).map(|p| self.affine.apply(p))
    }
}

fn cubic_rms(src: &[[f64; 2]], dst: &[[f64; 2]]) -> f64 {
    let s = src
        .iter()
        .fold(1f64, |m, p| m.max(p[0].abs()).max(p[1].abs()));
    let terms = |p: &[f64; 2]| {
        let (u, v) = (p[0] / s, p[1] / s);
        vec![
            1.,
            u,
            v,
            u * u,
            u * v,
            v * v,
            u * u * u,
            u * u * v,
            u * v * v,
            v * v * v,
        ]
    };
    let rows: Vec<_> = src.iter().map(terms).collect();
    let fit = |axis: usize| least_squares(&rows, &dst.iter().map(|d| d[axis]).collect::<Vec<_>>());
    let (Some(x), Some(y)) = (fit(0), fit(1)) else {
        return f64::NAN;
    };
    let sum: f64 = rows
        .iter()
        .zip(dst)
        .map(|(r, d)| {
            let px: f64 = r.iter().zip(&x).map(|(a, b)| a * b).sum();
            let py: f64 = r.iter().zip(&y).map(|(a, b)| a * b).sum();
            (px - d[0]).powi(2) + (py - d[1]).powi(2)
        })
        .sum();
    (sum / dst.len() as f64).sqrt()
}

pub fn solve(
    stars: &[Star],
    width: usize,
    height: usize,
    hint: (f64, f64),
    epoch: f64,
    deep: usize,
    cache: &Path,
) -> Result<Plate, String> {
    let size = [width as f64, height as f64];
    let points: Vec<[f64; 2]> = stars.iter().map(|s| [s.x, s.y]).collect();
    let detected: Vec<_> = points
        .iter()
        .take(DETECTED_FOR_TRIANGLES)
        .copied()
        .collect();
    let grid = Grid::new(points.clone(), 8.);
    let aspect = size[0].max(size[1]) / size[0].min(size[1]);
    let mut best: Option<(usize, Affine, f64)> = None;
    for half in LADDER {
        let cone = catalog::cone(hint.0, hint.1, half * aspect * 1.6, 600, epoch, cache)?;
        for parity in [1., -1.] {
            let plane: Vec<[f64; 2]> = cone
                .iter()
                .filter_map(|c| catalog::project(c.ra_deg, c.dec_deg, hint.0, hint.1))
                .map(|p| [parity * p[0], p[1]])
                .filter(|p| p[0].abs().max(p[1].abs()) < half * aspect * 3600. * 1.3)
                .collect();
            let catalogue: Vec<_> = plane
                .iter()
                .take(CATALOGUE_FOR_TRIANGLES)
                .copied()
                .collect();
            let votes: Vec<_> = plane.iter().take(CATALOGUE_FOR_VOTES).copied().collect();
            // The tried half field spans half the frame's shorter side.
            let expected = size[0].min(size[1]) / (2. * half * 3600.);
            let range = [expected / 2., expected * 2.];
            if let Some((count, sim)) =
                find_similarity(&detected, &catalogue, &votes, &grid, size, range)
            {
                let affine = Affine::from_sim(sim, parity);
                eprintln!(
                    "  half field {half:.2} deg, {}: {count} stars agree, {:.3} arcsec/px",
                    if parity > 0. { "direct" } else { "mirrored" },
                    affine.arcsec_per_px()
                );
                if best.is_none_or(|b| count > b.0) {
                    best = Some((count, affine, half));
                }
            }
        }
        if best.is_some_and(|b| b.0 >= 80) {
            break;
        }
    }
    let (candidate_inliers, mut affine, candidate_half_field_deg) =
        best.ok_or("no plate solution found")?;
    if candidate_inliers < 15 {
        return Err(format!(
            "plate solution too weak: only {candidate_inliers} catalogue stars agree"
        ));
    }
    // Refine: match the deep catalogue, re-centre the tangent point on the
    // frame centre and fit a linear solution with 3-sigma clipping.
    let (mut ra0, mut dec0) = hint;
    let centre = catalog::deproject(affine.invert([size[0] / 2., size[1] / 2.]), ra0, dec0);
    let radius = affine.arcsec_per_px() * size[0].hypot(size[1]) / 2. / 3600. * 1.1;
    let catalogue = catalog::cone(
        (centre.0 * 1e4).round() / 1e4,
        (centre.1 * 1e4).round() / 1e4,
        (radius * 1e3).ceil() / 1e3,
        deep,
        epoch,
        cache,
    )?;
    let mut result = None;
    for (round, tolerance) in [6., 4., 3., 2.5, 2.5].into_iter().enumerate() {
        let mut pairs: HashMap<usize, (usize, f64)> = HashMap::new();
        for (c, s) in catalogue.iter().enumerate() {
            let Some(q) = catalog::project(s.ra_deg, s.dec_deg, ra0, dec0).map(|p| affine.apply(p))
            else {
                continue;
            };
            if q[0] < 0. || q[1] < 0. || q[0] >= size[0] || q[1] >= size[1] {
                continue;
            }
            if let Some((d, dist)) = grid.nearest(q, tolerance) {
                if pairs.get(&d).is_none_or(|p| dist < p.1) {
                    pairs.insert(d, (c, dist));
                }
            }
        }
        let (new_ra, new_dec) =
            catalog::deproject(affine.invert([size[0] / 2., size[1] / 2.]), ra0, dec0);
        let mut matched: Vec<(usize, usize)> = pairs.iter().map(|(&d, &(c, _))| (c, d)).collect();
        // Hash order varies between runs; a fixed order keeps the output reproducible.
        matched.sort_unstable();
        let mut fitted = affine;
        let mut stats = (0., 0.);
        for _ in 0..3 {
            let src: Vec<_> = matched
                .iter()
                .map(|&(c, _)| {
                    catalog::project(catalogue[c].ra_deg, catalogue[c].dec_deg, new_ra, new_dec)
                        .unwrap()
                })
                .collect();
            let dst: Vec<_> = matched.iter().map(|&(_, d)| points[d]).collect();
            fitted = Affine::fit(&src, &dst).ok_or("degenerate plate fit")?;
            let res: Vec<f64> = src
                .iter()
                .zip(&dst)
                .map(|(s, d)| {
                    let q = fitted.apply(*s);
                    (q[0] - d[0]).hypot(q[1] - d[1])
                })
                .collect();
            let rms = (res.iter().map(|r| r * r).sum::<f64>() / res.len() as f64).sqrt();
            stats = (rms, res.iter().copied().fold(0., f64::max));
            let keep: Vec<_> = matched
                .iter()
                .zip(&res)
                .filter(|(_, r)| **r <= 3. * rms.max(0.3))
                .map(|(m, _)| *m)
                .collect();
            if keep.len() == matched.len() {
                break;
            }
            matched = keep;
        }
        if matched.len() < 10 {
            return Err(format!(
                "only {} stars confirm the plate solution",
                matched.len()
            ));
        }
        affine = fitted;
        (ra0, dec0) = (new_ra, new_dec);
        eprintln!(
            "  refine {}: {} stars, linear RMS {:.3} px, max {:.2} px",
            round + 1,
            matched.len(),
            stats.0,
            stats.1
        );
        let src: Vec<_> = matched
            .iter()
            .map(|&(c, _)| {
                catalog::project(catalogue[c].ra_deg, catalogue[c].dec_deg, ra0, dec0).unwrap()
            })
            .collect();
        let dst: Vec<_> = matched.iter().map(|&(_, d)| points[d]).collect();
        result = Some((matched.len(), stats, cubic_rms(&src, &dst)));
    }
    let (matched, (rms_px, max_px), cubic_rms_px) = result.unwrap();
    Ok(Plate {
        ra0,
        dec0,
        affine,
        matched,
        rms_px,
        max_px,
        cubic_rms_px,
        candidate_inliers,
        candidate_half_field_deg,
        catalogue,
    })
}
