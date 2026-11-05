//! Plate solving of every frame on its own against Gaia DR3.
//!
//! 1. Blind search near a hint: triangle asterisms of the 40 brightest
//!    detected and the 70 brightest catalogue stars, over a ladder of field
//!    sizes and both parities; the candidate that maps most catalogue stars
//!    onto distinct detected stars wins.
//! 2. Refinement: the frame's brighter catalogue stars are matched with
//!    shrinking tolerance; a polynomial of degree 1, 2 or 3 (pixel <-> sky,
//!    tangent point at the frame centre) is chosen by its held-out stars.
use super::SkyFrame;
use super::catalogue::{self, GaiaStar};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Half field heights tried by the blind search, degrees.
pub const LADDER: [f64; 11] = [
    0.06, 0.09, 0.13, 0.19, 0.28, 0.42, 0.63, 0.95, 1.4, 2.1, 3.2,
];
const DETECTED_FOR_TRIANGLES: usize = 40;
const CATALOGUE_FOR_TRIANGLES: usize = 70;
const CATALOGUE_FOR_VOTES: usize = 300;
const VOTE_PX: f64 = 3.;
const MIN_VOTES: usize = 12;

// ---------------------------------------------------------------- algebra

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
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (value, &q) in row.iter_mut().zip(&pivot).skip(c) {
                    *value -= f * q;
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

fn terms(degree: usize, u: f64, v: f64) -> Vec<f64> {
    let mut t = vec![1., u, v];
    if degree >= 2 {
        t.extend([u * u, u * v, v * v]);
    }
    if degree >= 3 {
        t.extend([u * u * u, u * u * v, u * v * v, v * v * v]);
    }
    t
}

fn term_count(degree: usize) -> usize {
    [0, 3, 6, 10][degree]
}

/// A 2-D polynomial map between normalised planes.
#[derive(Clone, Debug, Serialize)]
pub struct Poly {
    pub degree: usize,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl Poly {
    fn eval(&self, u: f64, v: f64) -> [f64; 2] {
        let t = terms(self.degree, u, v);
        [
            t.iter().zip(&self.x).map(|(a, b)| a * b).sum(),
            t.iter().zip(&self.y).map(|(a, b)| a * b).sum(),
        ]
    }

    fn fit(degree: usize, src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Self> {
        let rows: Vec<_> = src.iter().map(|p| terms(degree, p[0], p[1])).collect();
        Some(Self {
            degree,
            x: least_squares(&rows, &dst.iter().map(|d| d[0]).collect::<Vec<_>>())?,
            y: least_squares(&rows, &dst.iter().map(|d| d[1]).collect::<Vec<_>>())?,
        })
    }
}

// ---------------------------------------------------------------- WCS

/// Pixel <-> sky of one frame: standard coordinates about a tangent point,
/// polynomials in both directions on normalised coordinates.
#[derive(Clone, Debug, Serialize)]
pub struct Wcs {
    pub ra0: f64,
    pub dec0: f64,
    pub centre: [f64; 2],
    /// Pixel normalisation (half the longer side) and sky normalisation (arcsec).
    pub pixel_norm: f64,
    pub sky_norm: f64,
    pub forward: Poly,
    pub inverse: Poly,
    pub width: usize,
    pub height: usize,
}

impl Wcs {
    pub fn pixel_to_sky(&self, p: [f64; 2]) -> (f64, f64) {
        let s = self.forward.eval(
            (p[0] - self.centre[0]) / self.pixel_norm,
            (p[1] - self.centre[1]) / self.pixel_norm,
        );
        catalogue::deproject(
            [s[0] * self.sky_norm, s[1] * self.sky_norm],
            self.ra0,
            self.dec0,
        )
    }

    pub fn sky_to_pixel(&self, ra: f64, dec: f64) -> Option<[f64; 2]> {
        let s = catalogue::project(ra, dec, self.ra0, self.dec0)?;
        let p = self
            .inverse
            .eval(s[0] / self.sky_norm, s[1] / self.sky_norm);
        Some([
            p[0] * self.pixel_norm + self.centre[0],
            p[1] * self.pixel_norm + self.centre[1],
        ])
    }

    pub fn inside(&self, p: [f64; 2], margin: f64) -> bool {
        p[0] >= -margin
            && p[1] >= -margin
            && p[0] <= self.width as f64 - 1. + margin
            && p[1] <= self.height as f64 - 1. + margin
    }

    /// Local pixel scale at the frame centre, arcsec per pixel.
    pub fn scale(&self) -> f64 {
        let f = &self.forward;
        let det = (f.x[1] * f.y[2] - f.x[2] * f.y[1]).abs();
        det.sqrt() * self.sky_norm / self.pixel_norm
    }

    /// Mirrored with respect to the sky as seen from below (north up, east
    /// left has a positive determinant in top-down pixels).
    pub fn mirrored(&self) -> bool {
        let f = &self.forward;
        f.x[1] * f.y[2] - f.x[2] * f.y[1] < 0.
    }

    /// Direction of north, degrees clockwise from the image top.
    pub fn north_deg(&self) -> f64 {
        let i = &self.inverse;
        let (dx, dy) = (i.x[2], i.y[2]);
        dx.atan2(-dy).to_degrees()
    }

    pub fn centre_sky(&self) -> (f64, f64) {
        self.pixel_to_sky([
            (self.width as f64 - 1.) / 2.,
            (self.height as f64 - 1.) / 2.,
        ])
    }
}

// ---------------------------------------------------------------- blind

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

pub struct Grid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    pub points: Vec<[f64; 2]>,
}

impl Grid {
    pub fn new(points: Vec<[f64; 2]>, cell: f64) -> Self {
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

    pub fn nearest(&self, p: [f64; 2], radius: f64) -> Option<(usize, f64)> {
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
    vertices: [usize; 3],
}

fn triangles(points: &[[f64; 2]]) -> Vec<Triangle> {
    let d = |i: usize, j: usize| (points[i][0] - points[j][0]).hypot(points[i][1] - points[j][1]);
    let mut out = vec![];
    for i in 0..points.len() {
        for j in i + 1..points.len() {
            for k in j + 1..points.len() {
                let mut sides = [(d(j, k), i), (d(i, k), j), (d(i, j), k)];
                sides.sort_by(|a, b| a.0.total_cmp(&b.0));
                let [(a, va), (b, vb), (c, vc)] = sides;
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

/// Catalogue side of one blind configuration, shared by all frames.
pub struct BlindIndex {
    catalogue: Vec<[f64; 2]>,
    votes: Vec<[f64; 2]>,
    triangles: Vec<Triangle>,
    index: HashMap<(i64, i64), Vec<usize>>,
}

const BIN: f64 = 0.01;
const TOLERANCE: f64 = 0.006;

impl BlindIndex {
    fn new(cat: &[GaiaStar], hint: (f64, f64), half: f64, aspect: f64, parity: f64) -> Self {
        let box_arcsec = half * aspect * 3600. * 1.3;
        let plane: Vec<[f64; 2]> = cat
            .iter()
            .filter_map(|c| catalogue::project(c.ra, c.dec, hint.0, hint.1))
            .map(|p| [parity * p[0], p[1]])
            .filter(|p| p[0].abs().max(p[1].abs()) < box_arcsec)
            .take(CATALOGUE_FOR_VOTES)
            .collect();
        let catalogue: Vec<_> = plane
            .iter()
            .take(CATALOGUE_FOR_TRIANGLES)
            .copied()
            .collect();
        let triangles = triangles(&catalogue);
        let mut index: HashMap<_, Vec<_>> = HashMap::new();
        for (n, t) in triangles.iter().enumerate() {
            index
                .entry(((t.u / BIN) as i64, (t.v / BIN) as i64))
                .or_default()
                .push(n);
        }
        Self {
            catalogue,
            votes: plane,
            triangles,
            index,
        }
    }
}

/// Hint position (1e-4 degree steps), ladder rung, parity and aspect.
type Key = (i64, i64, usize, i8, u32);

/// Blind indexes per hint, ladder rung, parity and frame aspect, built once.
pub struct BlindCache {
    pub catalogue: Arc<Vec<GaiaStar>>,
    map: Mutex<HashMap<Key, Arc<BlindIndex>>>,
}

impl BlindCache {
    pub fn new(catalogue: Arc<Vec<GaiaStar>>) -> Self {
        Self {
            catalogue,
            map: Mutex::new(HashMap::new()),
        }
    }

    fn get(
        &self,
        hints: &[(f64, f64)],
        h: usize,
        rung: usize,
        parity: f64,
        aspect: f64,
    ) -> Arc<BlindIndex> {
        // Keyed by the hint's position: frames carry different hint lists.
        let key = (
            (hints[h].0 * 1e4).round() as i64,
            (hints[h].1 * 1e4).round() as i64,
            rung,
            parity as i8,
            (aspect * 100.).round() as u32,
        );
        if let Some(found) = self.map.lock().unwrap().get(&key) {
            return found.clone();
        }
        let built = Arc::new(BlindIndex::new(
            &self.catalogue,
            hints[h],
            LADDER[rung],
            aspect,
            parity,
        ));
        self.map.lock().unwrap().entry(key).or_insert(built).clone()
    }
}

/// What a solved frame of the same camera tells the next one: its pixel
/// scale, parity and field centre. The search then tries one field size, one
/// parity and a scale within 5 % (ADR-131 speed-up).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Prior {
    pub arcsec_per_px: f64,
    pub parity: f64,
    pub centre: (f64, f64),
}

impl Prior {
    pub fn of(solution: &Solution) -> Self {
        Self {
            arcsec_per_px: solution.arcsec_per_px,
            parity: if solution.blind.parity == "direct" {
                1.
            } else {
                -1.
            },
            centre: (solution.centre_ra, solution.centre_dec),
        }
    }
}

/// A pose with this many votes ends the search: wrong poses gather about ten.
const STRONG_VOTES: usize = 40;

#[derive(Clone, Debug, Serialize)]
pub struct BlindResult {
    pub votes: usize,
    pub runner_up: usize,
    pub half_field_deg: f64,
    pub parity: &'static str,
    pub hint: (f64, f64),
    pub arcsec_per_px: f64,
    /// Searched from a sibling's scale and parity.
    pub seeded: bool,
    /// Work of the search: configurations (hint x rung x parity) and poses
    /// whose votes were counted, and its time.
    pub configs: usize,
    pub evaluated: usize,
    pub seconds: f64,
}

/// What a blind search compares within one configuration.
struct Search<'a> {
    detected: &'a [[f64; 2]],
    triangles: &'a [Triangle],
    grid: &'a Grid,
    size: [f64; 2],
}

impl Search<'_> {
    /// The best pose of one configuration (hint, field size, parity) with its
    /// votes, and how many poses had their votes counted.
    fn config(&self, idx: &BlindIndex, (low, high): (f64, f64)) -> (Option<(usize, Sim)>, usize) {
        let (detected, size) = (self.detected, self.size);
        let mut evaluated = 0;
        let mut seen = HashSet::new();
        let mut local: Option<(usize, Sim)> = None;
        'config: for t in self.triangles {
            let (bu, bv) = ((t.u / BIN) as i64, (t.v / BIN) as i64);
            for gu in bu - 1..=bu + 1 {
                for gv in bv - 1..=bv + 1 {
                    for &n in idx.index.get(&(gu, gv)).into_iter().flatten() {
                        let c = &idx.triangles[n];
                        if (c.u - t.u).abs() > TOLERANCE || (c.v - t.v).abs() > TOLERANCE {
                            continue;
                        }
                        let src = c.vertices.map(|i| idx.catalogue[i]);
                        let dst = t.vertices.map(|i| detected[i]);
                        let Some(sim) = Sim::fit(&src, &dst) else {
                            continue;
                        };
                        let s = sim.a.hypot(sim.b);
                        if s < low || s > high {
                            continue;
                        }
                        if src.iter().zip(&dst).any(|(s, d)| {
                            let q = sim.apply(*s);
                            (q[0] - d[0]).hypot(q[1] - d[1]) > VOTE_PX
                        }) {
                            continue;
                        }
                        let key = (
                            (s.ln() / 0.01).round() as i64,
                            (sim.b.atan2(sim.a) / 0.005).round() as i64,
                            (sim.tx / 8.).round() as i64,
                            (sim.ty / 8.).round() as i64,
                        );
                        if !seen.insert(key) {
                            continue;
                        }
                        evaluated += 1;
                        let count = idx
                            .votes
                            .iter()
                            .map(|&p| sim.apply(p))
                            .filter(|q| {
                                q[0] >= 0. && q[1] >= 0. && q[0] < size[0] && q[1] < size[1]
                            })
                            .filter_map(|q| self.grid.nearest(q, VOTE_PX).map(|n| n.0))
                            .collect::<HashSet<_>>()
                            .len();
                        if local.is_none_or(|l| count > l.0) {
                            local = Some((count, sim));
                        }
                        // A strong pose ends this configuration.
                        if count >= STRONG_VOTES {
                            break 'config;
                        }
                    }
                }
            }
        }
        (local, evaluated)
    }
}

fn blind(
    stars: &[[f64; 2]],
    frame: &SkyFrame<'_>,
    hints: &[(f64, f64)],
    cache: &BlindCache,
    prior: Option<&Prior>,
) -> Option<(Sim, f64, (f64, f64), BlindResult)> {
    let size = [frame.width as f64, frame.height as f64];
    let points: Vec<[f64; 2]> = stars.to_vec();
    let detected: Vec<_> = points
        .iter()
        .take(DETECTED_FOR_TRIANGLES)
        .copied()
        .collect();
    if detected.len() < 6 {
        return None;
    }
    let started = std::time::Instant::now();
    let (mut configs, mut evaluated) = (0usize, 0usize);
    let grid = Grid::new(points.clone(), 8.);
    let aspect = (size[0].max(size[1]) / size[0].min(size[1]) * 4.).round() / 4.;
    let det_tri = triangles(&detected);
    let short = size[0].min(size[1]);
    // With a prior: the one rung whose catalogue box fits the known field.
    let rungs: Vec<usize> = match prior {
        Some(p) => {
            let half = short * p.arcsec_per_px / 2. / 3600.;
            let nearest = (0..LADDER.len())
                .min_by(|&a, &b| {
                    (LADDER[a] / half)
                        .ln()
                        .abs()
                        .total_cmp(&(LADDER[b] / half).ln().abs())
                })
                .unwrap();
            vec![nearest]
        }
        None => (0..LADDER.len()).collect(),
    };
    let parities: Vec<f64> = prior.map_or(vec![1., -1.], |p| vec![p.parity]);
    let mut best: Option<(usize, Sim, f64, usize, usize)> = None;
    // Every configuration's best candidate: the same pose found at another
    // field size or from another hint is no rival of itself.
    let mut found: Vec<(usize, f64, f64, f64)> = vec![];
    let scales = |rung: usize| match prior {
        Some(p) => (0.95 / p.arcsec_per_px, 1.05 / p.arcsec_per_px),
        None => {
            let expected = short / (2. * LADDER[rung] * 3600.);
            (expected / 2., expected * 2.)
        }
    };
    let search = Search {
        detected: &detected,
        triangles: &det_tri,
        grid: &grid,
        size,
    };
    let evaluate = |h: usize, rung: usize, parity: f64| {
        let idx = cache.get(hints, h, rung, parity, aspect);
        search.config(&idx, scales(rung))
    };
    'search: for h in 0..hints.len() {
        let order: Vec<(usize, f64)> = rungs
            .iter()
            .flat_map(|&rung| parities.iter().map(move |&parity| (rung, parity)))
            .collect();
        // A blind search tries every field size and parity of a hint at once
        // and then reads the results in the sequential order, stopping where
        // the sequential search stopped (ADR-149); a seeded one stays serial.
        let results: Vec<(Option<(usize, Sim)>, usize)> = if prior.is_none() {
            use rayon::prelude::*;
            order
                .par_iter()
                .map(|&(rung, parity)| evaluate(h, rung, parity))
                .collect()
        } else {
            vec![]
        };
        for (n, &(rung, parity)) in order.iter().enumerate() {
            configs += 1;
            let (local, counted) = match results.get(n) {
                Some(&result) => result,
                None => evaluate(h, rung, parity),
            };
            evaluated += counted;
            if let Some((count, sim)) = local {
                found.push((count, sim.a.hypot(sim.b).ln(), sim.b.atan2(sim.a), parity));
                if best.as_ref().is_none_or(|b| count > b.0) {
                    best = Some((count, sim, parity, h, rung));
                }
            }
            // ... and the whole search.
            if best.as_ref().is_some_and(|b| b.0 >= STRONG_VOTES) {
                break 'search;
            }
        }
    }
    let (votes, sim, parity, h, rung) = best?;
    let (scale, angle) = (sim.a.hypot(sim.b).ln(), sim.b.atan2(sim.a));
    let second = found
        .iter()
        .filter(|f| {
            let turn = (f.2 - angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            !(f.3 == parity && (f.1 - scale).abs() < 0.02 && turn.abs() < 0.02)
        })
        .map(|f| f.0)
        .max()
        .unwrap_or(0);
    (votes >= MIN_VOTES && votes >= 2 * second.max(3)).then(|| {
        (
            sim,
            parity,
            hints[h],
            BlindResult {
                votes,
                runner_up: second,
                half_field_deg: LADDER[rung],
                parity: if parity > 0. { "direct" } else { "mirrored" },
                hint: hints[h],
                arcsec_per_px: 1. / sim.a.hypot(sim.b),
                seeded: prior.is_some(),
                configs,
                evaluated,
                seconds: started.elapsed().as_secs_f64(),
            },
        )
    })
}

// ---------------------------------------------------------------- refine

#[derive(Clone, Debug, Serialize)]
pub struct Solution {
    /// Time of the refinement and model choice after the blind search.
    pub refine_s: f64,
    pub wcs: Wcs,
    pub blind: BlindResult,
    pub matched: usize,
    pub degree: usize,
    pub held_out: usize,
    pub held_out_rms_px: f64,
    pub held_out_rms_by_degree: Vec<Option<f64>>,
    pub fit_rms_px: f64,
    pub max_px: f64,
    pub quadrant_rms_px: [Option<f64>; 4],
    pub arcsec_per_px: f64,
    pub north_deg: f64,
    pub mirrored: bool,
    pub centre_ra: f64,
    pub centre_dec: f64,
}

fn rms(v: &[f64]) -> f64 {
    (v.iter().map(|r| r * r).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

/// Pixel and sky positions of matched stars -> both polynomials.
fn fit_wcs(
    frame: &SkyFrame<'_>,
    ra0: f64,
    dec0: f64,
    pix: &[[f64; 2]],
    sky: &[[f64; 2]],
    degree: usize,
) -> Option<Wcs> {
    let centre = [
        (frame.width as f64 - 1.) / 2.,
        (frame.height as f64 - 1.) / 2.,
    ];
    let pixel_norm = frame.width.max(frame.height) as f64 / 2.;
    let u: Vec<_> = pix
        .iter()
        .map(|p| {
            [
                (p[0] - centre[0]) / pixel_norm,
                (p[1] - centre[1]) / pixel_norm,
            ]
        })
        .collect();
    // Sky normalisation from a first linear fit.
    let lin = Poly::fit(1, &u, sky)?;
    let sky_norm = ((lin.x[1] * lin.y[2] - lin.x[2] * lin.y[1]).abs())
        .sqrt()
        .max(1e-6);
    let s: Vec<_> = sky
        .iter()
        .map(|q| [q[0] / sky_norm, q[1] / sky_norm])
        .collect();
    Some(Wcs {
        ra0,
        dec0,
        centre,
        pixel_norm,
        sky_norm,
        forward: Poly::fit(degree, &u, &s)?,
        inverse: Poly::fit(degree, &s, &u)?,
        width: frame.width,
        height: frame.height,
    })
}

pub fn solve(
    frame: &SkyFrame<'_>,
    hints: &[(f64, f64)],
    cache: &BlindCache,
    max_held_out_rms: f64,
    prior: Option<&Prior>,
) -> Result<Solution, String> {
    let (sim, parity, hint, blind_result) =
        blind(frame.stars, frame, hints, cache, prior).ok_or(if prior.is_some() {
            "seeded search found no unique match"
        } else {
            "blind search found no unique match"
        })?;
    let refine_start = std::time::Instant::now();
    let points: Vec<[f64; 2]> = frame.stars.to_vec();
    let grid = Grid::new(points.clone(), 8.);
    let cat = &cache.catalogue;
    // First pixel <-> sky estimate from the similarity about the hint.
    let det = sim.a * sim.a + sim.b * sim.b;
    let to_std = |p: [f64; 2]| {
        let (dx, dy) = (p[0] - sim.tx, p[1] - sim.ty);
        let u = (sim.a * dx + sim.b * dy) / det;
        let v = (-sim.b * dx + sim.a * dy) / det;
        [u * parity, v]
    };
    let centre_px = [frame.width as f64 / 2., frame.height as f64 / 2.];
    let (mut ra0, mut dec0) = catalogue::deproject(to_std(centre_px), hint.0, hint.1);
    let radius =
        blind_result.arcsec_per_px * (frame.width as f64).hypot(frame.height as f64) / 2. / 3600.
            * 1.15;
    // The frame's catalogue at its epoch, brightest first.
    let local: Vec<(usize, f64, f64)> = cat
        .iter()
        .enumerate()
        .filter(|(_, c)| catalogue::separation(c.ra, c.dec, ra0, dec0) < radius)
        .map(|(i, c)| {
            let (ra, dec) = c.at(frame.epoch);
            (i, ra, dec)
        })
        .collect();
    let limit = (frame.stars.len() * 3).max(300);
    let predict_sim = |ra: f64, dec: f64| {
        catalogue::project(ra, dec, hint.0, hint.1).map(|p| sim.apply([parity * p[0], p[1]]))
    };
    let mut wcs: Option<Wcs> = None;
    let mut pairs: Vec<(usize, usize)> = vec![];
    let tolerances = [6., 4., 3., 2.5, 2.];
    for (round, &tol) in tolerances.iter().enumerate() {
        let mut best: HashMap<usize, (usize, f64)> = HashMap::new();
        let mut used = 0;
        for &(i, ra, dec) in &local {
            let p = match &wcs {
                Some(w) => w.sky_to_pixel(ra, dec),
                None => predict_sim(ra, dec),
            };
            let Some(p) = p else { continue };
            if p[0] < 0. || p[1] < 0. || p[0] >= frame.width as f64 || p[1] >= frame.height as f64 {
                continue;
            }
            used += 1;
            if used > limit {
                break;
            }
            if let Some((d, dist)) = grid.nearest(p, tol) {
                if best.get(&d).is_none_or(|b| dist < b.1) {
                    best.insert(d, (i, dist));
                }
            }
        }
        pairs = best.iter().map(|(&d, &(i, _))| (i, d)).collect();
        pairs.sort_unstable();
        if pairs.len() < 8 {
            return Err(format!(
                "refinement round {} kept only {} stars",
                round + 1,
                pairs.len()
            ));
        }
        let (c_ra, c_dec) = match &wcs {
            Some(w) => w.centre_sky(),
            None => (ra0, dec0),
        };
        (ra0, dec0) = (c_ra, c_dec);
        let pix: Vec<_> = pairs.iter().map(|&(_, d)| points[d]).collect();
        let sky: Vec<_> = pairs
            .iter()
            .map(|&(i, _)| {
                let (ra, dec) = cat[i].at(frame.epoch);
                catalogue::project(ra, dec, ra0, dec0).unwrap_or([0., 0.])
            })
            .collect();
        let degree = if round < 2 {
            1
        } else if pairs.len() >= 60 {
            3
        } else if pairs.len() >= 30 {
            2
        } else {
            1
        };
        wcs = fit_wcs(frame, ra0, dec0, &pix, &sky, degree);
        if wcs.is_none() {
            return Err("degenerate fit".into());
        }
    }
    // Model choice by held-out stars: every fifth match is left out.
    let pix: Vec<_> = pairs.iter().map(|&(_, d)| points[d]).collect();
    let sky: Vec<_> = pairs
        .iter()
        .map(|&(i, _)| {
            let (ra, dec) = cat[i].at(frame.epoch);
            catalogue::project(ra, dec, ra0, dec0).unwrap_or([0., 0.])
        })
        .collect();
    let held: Vec<bool> = (0..pairs.len()).map(|k| k % 5 == 2).collect();
    let train_pix: Vec<_> = pix
        .iter()
        .zip(&held)
        .filter(|(_, h)| !**h)
        .map(|(p, _)| *p)
        .collect();
    let train_sky: Vec<_> = sky
        .iter()
        .zip(&held)
        .filter(|(_, h)| !**h)
        .map(|(p, _)| *p)
        .collect();
    let mut by_degree = vec![None; 3];
    let mut chosen: Option<(usize, f64)> = None;
    for degree in 1..=3 {
        if train_pix.len() < 3 * term_count(degree) + 6 {
            continue;
        }
        let Some(w) = fit_wcs(frame, ra0, dec0, &train_pix, &train_sky, degree) else {
            continue;
        };
        let res: Vec<f64> = pairs
            .iter()
            .zip(&held)
            .filter(|(_, h)| **h)
            .filter_map(|(&(i, d), _)| {
                let (ra, dec) = cat[i].at(frame.epoch);
                w.sky_to_pixel(ra, dec)
                    .map(|q| (q[0] - points[d][0]).hypot(q[1] - points[d][1]))
            })
            .collect();
        let r = rms(&res);
        by_degree[degree - 1] = Some(r);
        // A higher degree must earn its terms: 5 % better on held-out stars.
        if chosen.is_none_or(|c| r < c.1 * 0.95) {
            chosen = Some((degree, r));
        }
    }
    let (degree, held_out_rms_px) = chosen.ok_or("too few stars for a held-out check")?;
    let wcs = fit_wcs(frame, ra0, dec0, &pix, &sky, degree).ok_or("degenerate final fit")?;
    let res: Vec<f64> = pairs
        .iter()
        .filter_map(|&(i, d)| {
            let (ra, dec) = cat[i].at(frame.epoch);
            wcs.sky_to_pixel(ra, dec)
                .map(|q| (q[0] - points[d][0]).hypot(q[1] - points[d][1]))
        })
        .collect();
    let mut quads: [Vec<f64>; 4] = Default::default();
    for (&(_, d), r) in pairs.iter().zip(&res) {
        let q = usize::from(points[d][0] >= frame.width as f64 / 2.)
            + 2 * usize::from(points[d][1] >= frame.height as f64 / 2.);
        quads[q].push(*r);
    }
    let quadrant_rms_px = quads.each_ref().map(|q| (q.len() >= 3).then(|| rms(q)));
    let held_out = held.iter().filter(|h| **h).count();
    if pairs.len() < 12 {
        return Err(format!("only {} stars confirm the solution", pairs.len()));
    }
    if held_out_rms_px > max_held_out_rms {
        return Err(format!(
            "held-out RMS {held_out_rms_px:.2} px above {max_held_out_rms} px"
        ));
    }
    let (centre_ra, centre_dec) = wcs.centre_sky();
    Ok(Solution {
        refine_s: refine_start.elapsed().as_secs_f64(),
        arcsec_per_px: wcs.scale(),
        north_deg: wcs.north_deg(),
        mirrored: wcs.mirrored(),
        centre_ra,
        centre_dec,
        matched: pairs.len(),
        degree,
        held_out,
        held_out_rms_px,
        held_out_rms_by_degree: by_degree,
        fit_rms_px: rms(&res),
        max_px: res.iter().copied().fold(0., f64::max),
        quadrant_rms_px,
        blind: blind_result,
        wcs,
    })
}
