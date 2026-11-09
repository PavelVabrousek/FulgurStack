//! blindspike: feasibility spike for blind plate solving of fields 2 to 6
//! degrees wide. A quad index (astrometry.net style) is built from the
//! brightest, evenly spread Gaia DR3 stars; a frame's brightest stars form
//! quads that are looked up in it; a hypothesis is verified by counting star
//! matches, then handed to FulgurStack's own refinement as a hint.
//! Nothing here touches FulgurStack's crates; only their readers are used.
use fulgur_catalog::xpsd::XpsdCatalogue;
use fulgur_pipeline::sky::SkyFrame;
use fulgur_pipeline::sky::catalogue::GaiaStar;
use fulgur_pipeline::sky::detect::bright_stars;
use fulgur_pipeline::sky::solve::{self, BlindCache};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

const RAD: f64 = std::f64::consts::PI / 180.0;

// ------------------------------------------------------------------ sphere

type V3 = [f64; 3];

fn unit(ra: f64, dec: f64) -> V3 {
    let (r, d) = (ra * RAD, dec * RAD);
    [d.cos() * r.cos(), d.cos() * r.sin(), d.sin()]
}
fn dot(a: &V3, b: &V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: &V3, b: &V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: &V3) -> V3 {
    let n = dot(a, a).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}
fn sep_deg(a: &V3, b: &V3) -> f64 {
    let c = cross(a, b);
    dot(&c, &c).sqrt().atan2(dot(a, b)) / RAD
}
fn radec(v: &V3) -> (f64, f64) {
    (
        v[1].atan2(v[0]).to_degrees().rem_euclid(360.),
        v[2].clamp(-1., 1.).asin().to_degrees(),
    )
}

/// Tangent plane at `a`: east and north unit vectors.
struct Basis {
    a: V3,
    e: V3,
    n: V3,
}
impl Basis {
    fn at(a: V3) -> Self {
        let mut e = cross(&[0., 0., 1.], &a);
        if dot(&e, &e) < 1e-18 {
            e = [1., 0., 0.];
        }
        let e = norm(&e);
        let n = cross(&a, &e);
        Self { a, e, n }
    }
    fn plane(&self, p: &V3) -> [f64; 2] {
        let c = dot(&self.a, p);
        [dot(&self.e, p) / c, dot(&self.n, p) / c]
    }
    fn sky(&self, q: [f64; 2]) -> V3 {
        norm(&[
            self.a[0] + q[0] * self.e[0] + q[1] * self.n[0],
            self.a[1] + q[0] * self.e[1] + q[1] * self.n[1],
            self.a[2] + q[0] * self.e[2] + q[1] * self.n[2],
        ])
    }
}

// ------------------------------------------------------------------- stars

struct Star {
    u: V3,
    g: f32,
}

/// Every Gaia star brighter than `gmax`, read from the PixInsight files.
fn read_sky(dir: &Path, gmax: f64) -> Result<Vec<Star>, String> {
    let cat = XpsdCatalogue::open(dir)?;
    let mut out = vec![];
    for file in &cat.files {
        if file.magnitude.0 >= gmax {
            continue;
        }
        let part: Vec<GaiaStar> = file.search(0., 0., 180., -5., gmax)?;
        out.extend(part.iter().map(|s| Star {
            u: unit(s.ra, s.dec),
            g: s.g as f32,
        }));
    }
    Ok(out)
}

/// Equal-height rows, each cut into cells of about the same width.
struct Grid {
    cs: f64,
    rows: i32,
    cells: HashMap<u64, Vec<u32>>,
}
fn row_cells(row: i32, cs: f64) -> i32 {
    let mid = -90. + (row as f64 + 0.5) * cs;
    ((360. * (mid * RAD).cos() / cs).floor() as i32).max(1)
}
impl Grid {
    fn key(row: i32, j: i32) -> u64 {
        ((row as u64) << 32) | j as u64
    }
    fn cell(&self, ra: f64, dec: f64) -> (i32, i32) {
        let row = (((dec + 90.) / self.cs).floor() as i32).clamp(0, self.rows - 1);
        let n = row_cells(row, self.cs);
        let j = ((ra.rem_euclid(360.) / (360. / n as f64)).floor() as i32).rem_euclid(n);
        (row, j)
    }
    fn new(ids: &[u32], stars: &[Star], cs: f64) -> Self {
        let mut g = Self {
            cs,
            rows: (180. / cs).ceil() as i32,
            cells: HashMap::new(),
        };
        for &i in ids {
            let (ra, dec) = radec(&stars[i as usize].u);
            let (r, j) = g.cell(ra, dec);
            g.cells.entry(Self::key(r, j)).or_default().push(i);
        }
        g
    }
    /// Stars within `r` degrees of `c`.
    fn near(&self, stars: &[Star], c: &V3, r: f64, out: &mut Vec<u32>) {
        out.clear();
        let (ra, dec) = radec(c);
        let lo = (((dec - r + 90.) / self.cs).floor() as i32).clamp(0, self.rows - 1);
        let hi = (((dec + r + 90.) / self.cs).floor() as i32).clamp(0, self.rows - 1);
        let cos_r = (r * RAD).cos();
        for row in lo..=hi {
            let n = row_cells(row, self.cs);
            let (d0, d1) = (-90. + row as f64 * self.cs, -90. + (row + 1) as f64 * self.cs);
            let cosd = (d0.abs().max(d1.abs()) * RAD).cos().max(1e-6);
            let dra = r / cosd;
            let width = 360. / n as f64;
            let (j0, j1) = (
                ((ra - dra) / width).floor() as i64,
                ((ra + dra) / width).floor() as i64,
            );
            let all = n == 1 || dra >= 180. || j1 - j0 + 1 >= n as i64;
            let range: Vec<i32> = if all {
                (0..n).collect()
            } else {
                (j0..=j1).map(|j| j.rem_euclid(n as i64) as i32).collect()
            };
            for j in range {
                if let Some(v) = self.cells.get(&Self::key(row, j)) {
                    for &i in v {
                        if dot(c, &stars[i as usize].u) >= cos_r {
                            out.push(i);
                        }
                    }
                }
            }
        }
    }
}

// -------------------------------------------------------------------- codes

type P2 = [f64; 2];

/// The code of four points: A to 0 and B to 1 in the complex plane, C and D
/// the other two. The symmetry of the labels is broken so that
/// Re(C) + Re(D) <= 1 and Re(C) <= Re(D); `order` says which input point
/// took each of the four places.
fn canonical(p: [P2; 4]) -> Option<([f64; 4], [usize; 4])> {
    let mut order = [0usize, 1, 2, 3];
    let z = |a: P2, b: P2, q: P2| -> P2 {
        let w = [b[0] - a[0], b[1] - a[1]];
        let d = w[0] * w[0] + w[1] * w[1];
        let v = [q[0] - a[0], q[1] - a[1]];
        [(v[0] * w[0] + v[1] * w[1]) / d, (v[1] * w[0] - v[0] * w[1]) / d]
    };
    let mut zc = z(p[0], p[1], p[2]);
    let mut zd = z(p[0], p[1], p[3]);
    if !zc[0].is_finite() || !zd[0].is_finite() {
        return None;
    }
    if zc[0] + zd[0] > 1. {
        order.swap(0, 1);
        zc = [1. - zc[0], -zc[1]];
        zd = [1. - zd[0], -zd[1]];
    }
    if zc[0] > zd[0] {
        order.swap(2, 3);
        std::mem::swap(&mut zc, &mut zd);
    }
    let code = [zc[0], zc[1] + 0.5, zd[0], zd[1] + 0.5];
    code.iter().all(|c| (0. ..=1.).contains(c)).then_some((code, order))
}

/// C and D inside the circle on the diameter AB (a lens of the index).
fn inside_circle(a: P2, b: P2, q: P2) -> bool {
    let m = [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.];
    let r2 = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)) / 4.;
    (q[0] - m[0]).powi(2) + (q[1] - m[1]).powi(2) <= r2
}

// -------------------------------------------------------------------- index

const BUCKETS: usize = 25;

struct Band {
    dmin: f64,
    dmax: f64,
    /// Width of the cells that thin the stars, and how many each keeps.
    cell: f64,
    per_cell: usize,
    /// Partner stars B tried for every A, and (C, D) pairs for every AB.
    partners: usize,
    pairs: usize,
}

struct Index {
    stars: Vec<Star>,
    all: Grid,
    quads: Vec<[u32; 4]>,
    offsets: Vec<u32>,
    entries: Vec<([u16; 4], u32)>,
    bands: Vec<(f64, usize, usize)>, // dmin, stars, quads
}

fn bucket(code: &[f64; 4]) -> usize {
    code.iter()
        .fold(0, |k, c| k * BUCKETS + ((c * BUCKETS as f64) as usize).min(BUCKETS - 1))
}

fn band_quads(stars: &[Star], band: &Band) -> (usize, Vec<[u32; 4]>) {
    // Evenly spread: the brightest few of every cell as wide as the band's
    // smallest quad.
    let cs = band.cell;
    let probe = Grid::new(&[], stars, cs);
    let mut keyed: Vec<(u64, f32, u32)> = stars
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (ra, dec) = radec(&s.u);
            let (r, j) = probe.cell(ra, dec);
            (Grid::key(r, j), s.g, i as u32)
        })
        .collect();
    keyed.par_sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut ids: Vec<u32> = vec![];
    let mut run = (u64::MAX, 0usize);
    for (k, _, i) in keyed {
        if k != run.0 {
            run = (k, 0);
        }
        if run.1 < band.per_cell {
            ids.push(i);
        }
        run.1 += 1;
    }
    let grid = Grid::new(&ids, stars, band.dmax);
    let quads: Vec<[u32; 4]> = ids
        .par_iter()
        .flat_map_iter(|&a| {
            let mut out: Vec<[u32; 4]> = vec![];
            let (ua, mut near, mut inner) = (stars[a as usize].u, vec![], vec![]);
            grid.near(stars, &ua, band.dmax, &mut near);
            let mut bs: Vec<(f32, u32)> = near
                .iter()
                .filter(|&&b| b != a)
                .filter(|&&b| {
                    let d = sep_deg(&ua, &stars[b as usize].u);
                    d >= band.dmin && d <= band.dmax
                })
                .map(|&b| (stars[b as usize].g, b))
                .collect();
            bs.sort_by(|x, y| x.0.total_cmp(&y.0));
            for &(_, b) in bs.iter().take(band.partners) {
                let ub = stars[b as usize].u;
                let m = norm(&[ua[0] + ub[0], ua[1] + ub[1], ua[2] + ub[2]]);
                let half = sep_deg(&ua, &ub) / 2.;
                grid.near(stars, &m, half, &mut inner);
                let mut cs: Vec<(f32, u32)> = inner
                    .iter()
                    .filter(|&&c| c != a && c != b)
                    .map(|&c| (stars[c as usize].g, c))
                    .collect();
                cs.sort_by(|x, y| x.0.total_cmp(&y.0));
                let cs: Vec<u32> = cs.iter().take(3).map(|x| x.1).collect();
                if cs.len() < 2 {
                    continue;
                }
                let basis = Basis::at(ua);
                for pair in [(0, 1), (0, 2)].into_iter().take(band.pairs) {
                    let (Some(&c), Some(&d)) = (cs.get(pair.0), cs.get(pair.1)) else {
                        continue;
                    };
                    let ids = [a, b, c, d];
                    let pts = ids.map(|i| basis.plane(&stars[i as usize].u));
                    if let Some((_, order)) = canonical(pts) {
                        out.push(order.map(|o| ids[o]));
                    }
                }
            }
            out
        })
        .collect();
    (ids.len(), quads)
}

impl Index {
    fn build(stars: Vec<Star>, bands: &[Band]) -> Self {
        let all_ids: Vec<u32> = (0..stars.len() as u32).collect();
        let all = Grid::new(&all_ids, &stars, 1.0);
        let mut quads: Vec<[u32; 4]> = vec![];
        let mut summary = vec![];
        for band in bands {
            let t = Instant::now();
            let (n_stars, q) = band_quads(&stars, band);
            println!(
                "  band {:.1}-{:.1} deg: {} stars, {} quads, {:.1} s",
                band.dmin,
                band.dmax,
                n_stars,
                q.len(),
                t.elapsed().as_secs_f64()
            );
            summary.push((band.dmin, n_stars, q.len()));
            quads.extend(q);
        }
        // Codes in the tangent plane of each quad's own A, bucketed.
        let coded: Vec<([f64; 4], u32)> = quads
            .par_iter()
            .enumerate()
            .filter_map(|(n, q)| {
                let basis = Basis::at(stars[q[0] as usize].u);
                let pts = q.map(|i| basis.plane(&stars[i as usize].u));
                canonical(pts).map(|(code, _)| (code, n as u32))
            })
            .collect();
        let mut counts = vec![0u32; BUCKETS.pow(4) + 1];
        for (c, _) in &coded {
            counts[bucket(c) + 1] += 1;
        }
        for i in 1..counts.len() {
            counts[i] += counts[i - 1];
        }
        let mut fill = counts.clone();
        let mut entries = vec![([0u16; 4], 0u32); coded.len()];
        for (c, n) in &coded {
            let b = bucket(c);
            entries[fill[b] as usize] = (c.map(|x| (x * 65535.).round() as u16), *n);
            fill[b] += 1;
        }
        Self {
            stars,
            all,
            quads,
            offsets: counts,
            entries,
            bands: summary,
        }
    }

    fn bytes(&self) -> usize {
        self.stars.len() * std::mem::size_of::<Star>()
            + self.quads.len() * 16
            + self.offsets.len() * 4
            + self.entries.len() * 12
    }

    /// Quad ids whose code is within the tolerance of `code`.
    fn lookup(&self, code: &[f64; 4], tol: f64, out: &mut Vec<u32>) {
        out.clear();
        let mut options: [Vec<usize>; 4] = Default::default();
        for k in 0..4 {
            let x = code[k] * BUCKETS as f64;
            let b = (x as usize).min(BUCKETS - 1);
            options[k].push(b);
            let frac = x - b as f64;
            let t = tol * BUCKETS as f64;
            if frac < t && b > 0 {
                options[k].push(b - 1);
            }
            if frac > 1. - t && b + 1 < BUCKETS {
                options[k].push(b + 1);
            }
        }
        for &b0 in &options[0] {
            for &b1 in &options[1] {
                for &b2 in &options[2] {
                    for &b3 in &options[3] {
                        let b = ((b0 * BUCKETS + b1) * BUCKETS + b2) * BUCKETS + b3;
                        for e in &self.entries[self.offsets[b] as usize..self.offsets[b + 1] as usize] {
                            let ok = (0..4).all(|k| (e.0[k] as f64 / 65535. - code[k]).abs() <= tol);
                            if ok {
                                out.push(e.1);
                            }
                        }
                    }
                }
            }
        }
    }
}

// -------------------------------------------------------------------- solve

struct Frame {
    name: String,
    width: usize,
    height: usize,
    epoch: f64,
    /// x, y of the stars, brightest first.
    stars: Vec<[f64; 2]>,
}

struct Hypothesis {
    ra: f64,
    dec: f64,
    arcsec_per_px: f64,
    mirrored: bool,
    matched: usize,
    log10_p: f64,
}

/// Probability (log10) of m or more matches by chance when `n` catalogue
/// stars each have probability `p` of landing on a detection.
fn log10_tail(n: f64, p: f64, m: usize) -> f64 {
    let lambda = (n * p).max(1e-9);
    let ln_fact = |k: usize| (1..=k).map(|i| (i as f64).ln()).sum::<f64>();
    let mut terms = vec![];
    let mut lf = ln_fact(m);
    for k in m..m + 80 {
        terms.push(-lambda + k as f64 * lambda.ln() - lf);
        lf += ((k + 1) as f64).ln();
    }
    let top = terms.iter().cloned().fold(f64::MIN, f64::max);
    (top + terms.iter().map(|t| (t - top).exp()).sum::<f64>().ln()) / std::f64::consts::LN_10
}

const FIELD_MIN_DEG: f64 = 0.4;
const FIELD_MAX_DEG: f64 = 14.0;
/// The brightest catalogue stars of a hypothesis that are compared.
const VERIFY_STARS: usize = 300;
const MATCH_PX: f64 = 4.0;
const DETECTIONS_FOR_VERIFY: usize = 150;

struct Stats {
    quads_tried: usize,
    hits: usize,
    verified: usize,
}

/// Detections fit for quads, brightest first. A saturated star breaks into
/// several detections a few pixels apart, and a bright nebula into dozens of
/// knots: the nearer ones are dropped (non-maximum suppression within 0.2 %
/// of the long side), then every detection among the 300 brightest that has
/// four or more others within 2 % of the long side, which is a crowd and not
/// a star.
fn clean(stars: &[[f64; 2]], width: usize, height: usize) -> Vec<[f64; 2]> {
    let long = width.max(height) as f64;
    let near = (0.002 * long).max(4.);
    let mut kept: Vec<[f64; 2]> = vec![];
    for s in stars {
        if kept.iter().all(|k| (k[0] - s[0]).hypot(k[1] - s[1]) > near) {
            kept.push(*s);
        }
    }
    let crowd = 0.02 * long;
    let head: Vec<[f64; 2]> = kept.iter().take(300).copied().collect();
    let isolated = |s: &[f64; 2]| {
        head.iter()
            .filter(|o| (o[0] - s[0]).hypot(o[1] - s[1]) <= crowd)
            .count()
            <= 4
    };
    kept.into_iter().enumerate().filter(|(i, s)| *i >= 300 || isolated(s)).map(|(_, s)| s).collect()
}

/// The first hypothesis of a frame that survives the match count.
fn blind(
    index: &Index,
    frame: &Frame,
    top: usize,
    min_matches: usize,
    max_log10_p: f64,
    stats: &mut Stats,
) -> Option<Hypothesis> {
    let (w, h) = (frame.width as f64, frame.height as f64);
    let cleaned = clean(&frame.stars, frame.width, frame.height);
    let n = cleaned.len().min(top);
    let pts: Vec<P2> = cleaned.iter().take(n).copied().collect();
    let verify_pts: Vec<P2> = cleaned.iter().take(DETECTIONS_FOR_VERIFY).copied().collect();
    // Detections in 16 px cells.
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, p) in verify_pts.iter().enumerate() {
        cells
            .entry(((p[0] / 16.) as i32, (p[1] / 16.) as i32))
            .or_default()
            .push(i);
    }
    let mut seen: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut hits: Vec<u32> = vec![];
    let mut near: Vec<u32> = vec![];
    for newest in 3..n {
        // Every 4-set whose newest star is `newest`.
        for i in 0..newest {
            for j in i + 1..newest {
                for k in j + 1..newest {
                    let set = [i, j, k, newest];
                    for pair in 0..6 {
                        let (ia, ib) = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)][pair];
                        let rest: Vec<usize> = (0..4).filter(|&x| x != ia && x != ib).collect();
                        let (a, b) = (pts[set[ia]], pts[set[ib]]);
                        let (c, d) = (pts[set[rest[0]]], pts[set[rest[1]]]);
                        // A short pair gives a vague code; the code's
                        // tolerance follows the centroid error over its length.
                        let ab_px = (a[0] - b[0]).hypot(a[1] - b[1]);
                        if ab_px < 0.15 * w.max(h) {
                            continue;
                        }
                        let tol = 0.004 + 1.2 / ab_px;
                        if !inside_circle(a, b, c) || !inside_circle(a, b, d) {
                            continue;
                        }
                        for mirrored in [false, true] {
                            let flip = |p: P2| if mirrored { [p[0], -p[1]] } else { p };
                            let quad = [flip(a), flip(b), flip(c), flip(d)];
                            let Some((code, order)) = canonical(quad) else {
                                continue;
                            };
                            stats.quads_tried += 1;
                            index.lookup(&code, tol, &mut hits);
                            for &q in hits.iter().take(20) {
                                stats.hits += 1;
                                // Similarity from the frame's four points (in
                                // the order of the code) to the index quad.
                                let used = [quad[order[0]], quad[order[1]], quad[order[2]], quad[order[3]]];
                                let ids = index.quads[q as usize];
                                let basis = Basis::at(index.stars[ids[0] as usize].u);
                                let planes = ids.map(|s| basis.plane(&index.stars[s as usize].u));
                                let Some((sr, si, tr, ti)) = fit_similarity(&used, &planes) else {
                                    continue;
                                };
                                let scale = (sr * sr + si * si).sqrt(); // radians per pixel
                                let field = scale * w.max(h) / RAD;
                                if !(FIELD_MIN_DEG..=FIELD_MAX_DEG).contains(&field) {
                                    continue;
                                }
                                let map = |p: P2| -> P2 {
                                    let p = flip(p);
                                    [sr * p[0] - si * p[1] + tr, sr * p[1] + si * p[0] + ti]
                                };
                                let centre = basis.sky(map([w / 2., h / 2.]));
                                let (ra, dec) = radec(&centre);
                                let key = (
                                    (ra / 0.25) as i32,
                                    ((dec + 90.) / 0.25) as i32,
                                    (scale.ln() / 0.03) as i32,
                                );
                                if !seen.insert(key) {
                                    continue;
                                }
                                stats.verified += 1;
                                // Catalogue stars inside the frame, through
                                // the hypothesis, against the detections.
                                let radius = (w.hypot(h) / 2.) * scale / RAD * 1.1;
                                index.all.near(&index.stars, &centre, radius, &mut near);
                                if near.len() > VERIFY_STARS * 3 {
                                    near.select_nth_unstable_by(VERIFY_STARS * 3, |a, b| {
                                        index.stars[*a as usize].g.total_cmp(&index.stars[*b as usize].g)
                                    });
                                    near.truncate(VERIFY_STARS * 3);
                                }
                                let (mut inside, mut matched) = (0usize, HashSet::new());
                                let denom = sr * sr + si * si;
                                for &s in &near {
                                    let q = basis.plane(&index.stars[s as usize].u);
                                    let (dx, dy) = (q[0] - tr, q[1] - ti);
                                    let (px, py) = ((dx * sr + dy * si) / denom, (dy * sr - dx * si) / denom);
                                    let (x, y) = (px, if mirrored { -py } else { py });
                                    if x < 0. || y < 0. || x >= w || y >= h {
                                        continue;
                                    }
                                    inside += 1;
                                    let (cx, cy) = ((x / 16.) as i32, (y / 16.) as i32);
                                    'cells: for gx in cx - 1..=cx + 1 {
                                        for gy in cy - 1..=cy + 1 {
                                            for &di in cells.get(&(gx, gy)).into_iter().flatten() {
                                                let v = verify_pts[di];
                                                if (v[0] - x).hypot(v[1] - y) <= MATCH_PX {
                                                    matched.insert(di);
                                                    break 'cells;
                                                }
                                            }
                                        }
                                    }
                                }
                                let p = verify_pts.len() as f64 * std::f64::consts::PI * MATCH_PX * MATCH_PX / (w * h);
                                let m = matched.len();
                                if m >= min_matches {
                                    let lp = log10_tail(inside as f64, p.min(1.), m);
                                    if lp <= max_log10_p {
                                        return Some(Hypothesis {
                                            ra,
                                            dec,
                                            arcsec_per_px: scale / RAD * 3600.,
                                            mirrored,
                                            matched: m,
                                            log10_p: lp,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Least-squares similarity Q = s P + t (complex s and t) of four pairs.
fn fit_similarity(p: &[P2; 4], q: &[P2; 4]) -> Option<(f64, f64, f64, f64)> {
    let mean = |v: &[P2; 4]| [v.iter().map(|x| x[0]).sum::<f64>() / 4., v.iter().map(|x| x[1]).sum::<f64>() / 4.];
    let (mp, mq) = (mean(p), mean(q));
    let (mut num_r, mut num_i, mut den) = (0., 0., 0.);
    for k in 0..4 {
        let (a, b) = (p[k][0] - mp[0], p[k][1] - mp[1]);
        let (c, d) = (q[k][0] - mq[0], q[k][1] - mq[1]);
        num_r += a * c + b * d;
        num_i += a * d - b * c;
        den += a * a + b * b;
    }
    if den < 1e-12 {
        return None;
    }
    let (sr, si) = (num_r / den, num_i / den);
    let tr = mq[0] - (sr * mp[0] - si * mp[1]);
    let ti = mq[1] - (sr * mp[1] + si * mp[0]);
    Some((sr, si, tr, ti))
}

// ----------------------------------------------------------------- frames

fn decimal_year(date: &str) -> f64 {
    let n = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<f64>().ok());
    match (n(0, 4), n(5, 7), n(8, 10)) {
        (Some(y), Some(m), Some(d)) => y + ((m - 1.) * 30.44 + d) / 365.25,
        _ => 2026.,
    }
}

fn load_frame(path: &Path) -> Result<(Frame, Option<(f64, f64)>), String> {
    let raw = fulgur_core::fits::read_raw(path).map_err(|e| e.to_string())?;
    let info = &raw.header.info;
    let mosaic = info.mosaic == fulgur_core::Mosaic::Bayer;
    let (mut stars, _) = bright_stars(&raw.pixels, info.width, info.height, mosaic);
    stars.sort_by(|a, b| b[4].total_cmp(&a[4]));
    Ok((
        Frame {
            name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            width: info.width,
            height: info.height,
            epoch: decimal_year(info.date_obs.as_deref().unwrap_or("")),
            stars: stars.iter().map(|s| [s[0], s[1]]).collect(),
        },
        fulgur_core::fits::read_sky_position(path),
    ))
}

/// FulgurStack's own solver around a hint: the refinement and held-out check.
fn refine(cat: &XpsdCatalogue, frame: &Frame, hint: (f64, f64)) -> Result<solve::Solution, String> {
    let mut stars = cat.cone(hint.0, hint.1, 6.5, 13.5)?;
    stars.extend(cat.cone(hint.0, hint.1, 1.7, 17.5)?);
    let mut seen = HashSet::new();
    stars.retain(|s| seen.insert((s.ra.to_bits(), s.dec.to_bits(), s.g.to_bits())));
    stars.sort_by(|a, b| a.g.total_cmp(&b.g));
    let cache = BlindCache::new(Arc::new(stars));
    let sky = SkyFrame {
        name: &frame.name,
        width: frame.width,
        height: frame.height,
        epoch: frame.epoch,
        stars: &frame.stars,
    };
    solve::solve(&sky, &[hint], &cache, 1.0, None)
}

fn sep_radec(a: (f64, f64), b: (f64, f64)) -> f64 {
    sep_deg(&unit(a.0, a.1), &unit(b.0, b.1))
}

// -------------------------------------------------------------------- main

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    }
    fn normal(&mut self) -> f64 {
        let (a, b) = (self.next().max(1e-12), self.next());
        (-2. * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

/// A frame of the real sky at a random place, rotation and mirroring, as a
/// detector would see it: the brightest `n_det` stars with a noisy order, a
/// fifth missed, centroids off by a few tenths of a pixel, a few spurious
/// detections. Returns the frame and the truth (centre, arcsec per pixel).
fn synthetic(index: &Index, rng: &mut Rng, n_det: usize, lo: f64, hi: f64) -> (Frame, (f64, f64, f64)) {
    let (w, h) = (3840usize, 2160usize);
    let ra = rng.next() * 360.;
    let dec = (2. * rng.next() - 1.).asin().to_degrees();
    let long_side = lo + (hi - lo) * rng.next();
    let scale = long_side * RAD / w as f64;
    let (theta, mirrored) = (rng.next() * std::f64::consts::TAU, rng.next() < 0.5);
    let centre = unit(ra, dec);
    let basis = Basis::at(centre);
    let mut near = vec![];
    let radius = (w as f64).hypot(h as f64) / 2. * scale / RAD * 1.05;
    index.all.near(&index.stars, &centre, radius, &mut near);
    let mut seen: Vec<(f64, [f64; 2])> = vec![];
    for &i in &near {
        let q = basis.plane(&index.stars[i as usize].u);
        let (c, s) = (theta.cos(), theta.sin());
        let (mut x, y) = ((c * q[0] - s * q[1]) / scale, (s * q[0] + c * q[1]) / scale);
        if mirrored {
            x = -x;
        }
        let (x, y) = (x + w as f64 / 2., y + h as f64 / 2.);
        if x < 0. || y < 0. || x >= w as f64 || y >= h as f64 || rng.next() < 0.2 {
            continue;
        }
        let key = index.stars[i as usize].g as f64 + 0.6 * rng.normal();
        seen.push((key, [x + 0.4 * rng.normal(), y + 0.4 * rng.normal()]));
    }
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut stars: Vec<[f64; 2]> = seen.iter().take(n_det).map(|s| s.1).collect();
    for _ in 0..5 {
        let at = (rng.next() * stars.len().max(1) as f64) as usize;
        stars.insert(at.min(stars.len()), [rng.next() * w as f64, rng.next() * h as f64]);
    }
    (
        Frame { name: "synthetic".into(), width: w, height: h, epoch: 2026., stars },
        (ra, dec, scale / RAD * 3600.),
    )
}

/// Why a frame does not solve blind: the brightest detections, which of them
/// are catalogue stars once the frame is solved around a known hint, and the
/// blind search on filtered star lists.
fn diag(index: &Index, cat: &XpsdCatalogue, path: &Path, hint: Option<(f64, f64)>) {
    let raw = fulgur_core::fits::read_raw(path).expect("frame");
    let info = &raw.header.info;
    let mosaic = info.mosaic == fulgur_core::Mosaic::Bayer;
    let (mut det, _) = bright_stars(&raw.pixels, info.width, info.height, mosaic);
    det.sort_by(|a, b| b[4].total_cmp(&a[4]));
    let (w, h) = (info.width, info.height);
    let epoch = decimal_year(info.date_obs.as_deref().unwrap_or(""));
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v.get(v.len() / 2).copied().unwrap_or(0.)
    };
    let fwhm = median(det.iter().take(300).map(|s| s[2]).collect());
    let ecc = median(det.iter().take(300).map(|s| s[3]).collect());
    println!("{}: {w}x{h}, {} detections, median FWHM {fwhm:.2} px, ecc {ecc:.2}", path.display(), det.len());
    let frame_of = |list: &[[f64; 6]]| Frame {
        name: "diag".into(),
        width: w,
        height: h,
        epoch,
        stars: list.iter().map(|s| [s[0], s[1]]).collect(),
    };
    let full = frame_of(&det);
    // Which detections are catalogue stars, once the frame is solved.
    let mut catalogue_g: Vec<Option<f32>> = vec![None; det.len().min(60)];
    if let Some(hint) = hint {
        match refine(cat, &full, hint) {
            Err(e) => println!("  solving around the hint failed: {e}"),
            Ok(sol) => {
                println!(
                    "  around the hint: RA {:.4} Dec {:+.4}, {:.2} arcsec/px, held-out {:.2} px",
                    sol.centre_ra, sol.centre_dec, sol.arcsec_per_px, sol.held_out_rms_px
                );
                let centre = unit(sol.centre_ra, sol.centre_dec);
                let mut near = vec![];
                index.all.near(&index.stars, &centre, (w as f64).hypot(h as f64) / 2. * sol.arcsec_per_px / 3600. * 1.1, &mut near);
                let mapped: Vec<(P2, f32)> = near
                    .iter()
                    .filter_map(|&i| {
                        let (ra, dec) = radec(&index.stars[i as usize].u);
                        sol.wcs.sky_to_pixel(ra, dec).map(|p| (p, index.stars[i as usize].g))
                    })
                    .collect();
                for (rank, slot) in catalogue_g.iter_mut().enumerate() {
                    let d = det[rank];
                    *slot = mapped
                        .iter()
                        .filter(|(p, _)| (p[0] - d[0]).hypot(p[1] - d[1]) <= 3.)
                        .map(|(_, g)| *g)
                        .min_by(|a, b| a.total_cmp(b));
                }
            }
        }
    }
    println!("  rank     x      y   fwhm   ecc      flux     peak   G<13 star");
    for (rank, d) in det.iter().take(40).enumerate() {
        println!(
            "  {:>4} {:>6.0} {:>6.0} {:>6.2} {:>5.2} {:>9.0} {:>8.0}   {}",
            rank + 1, d[0], d[1], d[2], d[3], d[4], d[5],
            catalogue_g.get(rank).copied().flatten().map_or("-".into(), |g| format!("G {g:.1}"))
        );
    }
    let in_cat = catalogue_g.iter().take(30).filter(|g| g.is_some()).count();
    println!("  of the 30 brightest detections {in_cat} are catalogue stars (G<{})", "index limit");
    let round: Vec<[f64; 6]> = det.iter().filter(|s| s[2] <= 1.6 * fwhm && s[3] <= 0.5).copied().collect();
    let sharp: Vec<[f64; 6]> = det.iter().filter(|s| s[2] <= 1.3 * fwhm && s[2] >= 0.6 * fwhm && s[3] <= 0.4).copied().collect();
    let unsaturated: Vec<[f64; 6]> = {
        let top_peak = det.iter().map(|s| s[5]).fold(0., f64::max);
        det.iter().filter(|s| s[5] < 0.9 * top_peak && s[2] <= 1.6 * fwhm && s[3] <= 0.5).copied().collect()
    };
    for (label, list, top) in [
        ("all detections, top 30", &det, 30usize),
        ("all detections, top 60", &det, 60),
        ("round stars, top 30", &round, 30),
        ("round stars, top 60", &round, 60),
        ("sharp stars, top 40", &sharp, 40),
        ("round and unsaturated, top 40", &unsaturated, 40),
    ] {
        let frame = frame_of(list);
        let (mut stats, start) = (Stats { quads_tried: 0, hits: 0, verified: 0 }, Instant::now());
        let found = blind(index, &frame, top, 12, -12., &mut stats);
        println!(
            "  {label}: {} of {} kept; {} ({:.2} s, {} quads, {} verified)",
            list.len().min(top), list.len(),
            found.map_or("no hypothesis".into(), |h| format!("hypothesis RA {:.3} Dec {:+.3}, {} matches", h.ra, h.dec, h.matched)),
            start.elapsed().as_secs_f64(), stats.quads_tried, stats.verified
        );
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let gaia = PathBuf::from(arg(&args, "--gaia").unwrap_or_else(|| r"C:\PixInsightData\Gaia DR3".into()));
    let gmax: f64 = arg(&args, "--gmax").and_then(|v| v.parse().ok()).unwrap_or(13.0);
    let per_cell: usize = arg(&args, "--per-cell").and_then(|v| v.parse().ok()).unwrap_or(10);
    let per_set: usize = arg(&args, "--frames-per-set").and_then(|v| v.parse().ok()).unwrap_or(6);
    let negatives: usize = arg(&args, "--negatives").and_then(|v| v.parse().ok()).unwrap_or(200);
    let top: usize = arg(&args, "--top").and_then(|v| v.parse().ok()).unwrap_or(30);
    let only = arg(&args, "--only");
    let root = PathBuf::from(arg(&args, "--root").unwrap_or_else(|| r"C:\Astrofoto".into()));

    let t = Instant::now();
    println!("reading Gaia stars brighter than G {gmax} ...");
    let stars = read_sky(&gaia, gmax).expect("Gaia files");
    println!("  {} stars in {:.1} s", stars.len(), t.elapsed().as_secs_f64());
    let band = |dmin: f64, dmax: f64, cell: f64, per_cell: usize, partners: usize, pairs: usize| Band {
        dmin,
        dmax,
        cell,
        per_cell,
        partners,
        pairs,
    };
    let bands = [
        // Fields of 0.5 to 2 degrees.
        band(0.1, 0.25, 0.15, 6, 2, 1),
        band(0.25, 0.5, 0.25, 8, 2, 2),
        // Fields of 2 to 6 degrees (the first spike).
        band(0.5, 1.0, 0.5, per_cell, 4, 2),
        band(1.0, 2.0, 1.0, per_cell, 4, 2),
        band(2.0, 4.0, 2.0, per_cell, 4, 2),
        band(4.0, 8.0, 4.0, per_cell, 4, 2),
        // Fields of 6 to 12 degrees.
        band(8.0, 16.0, 8.0, per_cell, 4, 2),
    ];
    let t = Instant::now();
    println!("building the index ...");
    let index = Index::build(stars, &bands);
    println!(
        "index: {} quads, about {:.0} MB in RAM, built in {:.1} s",
        index.quads.len(),
        index.bytes() as f64 / 1e6,
        t.elapsed().as_secs_f64()
    );
    let cat = XpsdCatalogue::open(&gaia).expect("Gaia files");
    if let Some(path) = arg(&args, "--diag") {
        let hint = arg(&args, "--hint").and_then(|h| {
            let mut it = h.split(',').filter_map(|v| v.trim().parse::<f64>().ok());
            Some((it.next()?, it.next()?))
        });
        diag(&index, &cat, Path::new(&path), hint);
        return;
    }

    // Set, and the known target when the headers carry no position.
    let sets: [(&str, Option<(f64, f64)>); 12] = [
        ("SeeStarTesty S50P", None),
        ("Siril Test", None),
        ("M 45_sub", None),
        ("M 45_sub Seestart30p EQ 30s", None),
        ("SeestarTesty S30P", None),
        ("Dwarf3mini Haspl", None),
        ("Test SQA60P Ha FullMoon", None),
        ("ZdenekVojcDataForTestFulgurStack", None),
        ("M27_Stellina_PsionDataForTestFUlgurStack", Some((299.9017, 22.7211))),
        ("20201121 m42 asi071", Some((83.8221, -5.3911))),
        ("20201121 asi1600", Some((83.8221, -5.3911))),
        ("M33 all time data", Some((23.4621, 30.6599))),
    ];
    let (mut tried, mut solved, mut right, mut wrong) = (0, 0, 0, 0);
    let mut seconds = vec![];
    for (set, known) in sets {
        if only.as_ref().is_some_and(|o| !set.contains(o.as_str())) {
            continue;
        }
        let mut files: Vec<PathBuf> = vec![];
        let mut stack = vec![root.join(set)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if fulgur_core::fits::is_input(&p) {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    // Lights only, not stacked results or masters.
                    if !["stack", "master", "result", "mosaic"].iter().any(|w| name.contains(w)) {
                        files.push(p);
                    }
                }
            }
        }
        files.sort();
        let step = (files.len() / per_set.max(1)).max(1);
        println!("\n== {set} ({} files)", files.len());
        for path in files.iter().step_by(step).take(per_set) {
            let Ok((frame, header_pos)) = load_frame(path) else {
                println!("  {}: unreadable", path.display());
                continue;
            };
            // Truth: FulgurStack's solver around the header position.
            let truth = header_pos.or(known).and_then(|p| refine(&cat, &frame, p).ok());
            tried += 1;
            let start = Instant::now();
            let mut stats = Stats { quads_tried: 0, hits: 0, verified: 0 };
            let found = blind(&index, &frame, top, 12, -12., &mut stats);
            let blind_s = start.elapsed().as_secs_f64();
            let line = match found {
                None => format!("no hypothesis ({} stars, {:.2} s, {} quads, {} hits)", frame.stars.len(), blind_s, stats.quads_tried, stats.hits),
                Some(h) => {
                    let refined = refine(&cat, &frame, (h.ra, h.dec));
                    let total = start.elapsed().as_secs_f64();
                    match refined {
                        Err(e) => format!("hypothesis RA {:.3} Dec {:+.3} ({} matches, p 1e{:.0}) but refinement failed: {e}", h.ra, h.dec, h.matched, h.log10_p),
                        Ok(sol) => {
                            solved += 1;
                            seconds.push(total);
                            let verdict = match &truth {
                                Some(t) => {
                                    let err = sep_radec((sol.centre_ra, sol.centre_dec), (t.centre_ra, t.centre_dec)) * 60.;
                                    let scale = sol.arcsec_per_px / t.arcsec_per_px;
                                    if err < 3. && (scale - 1.).abs() < 0.02 {
                                        right += 1;
                                        format!("matches the truth ({err:.2} arcmin, scale {scale:.4})")
                                    } else {
                                        wrong += 1;
                                        format!("DIFFERS from the truth ({err:.1} arcmin, scale {scale:.3})")
                                    }
                                }
                                None => "no truth (header has no position)".into(),
                            };
                            format!(
                                "SOLVED RA {:.4} Dec {:+.4}, {:.2} arcsec/px, field {:.2} deg, held-out {:.2} px, blind {:.2} s, total {:.1} s ({} quads, {} hits, {} verified); {verdict}",
                                sol.centre_ra, sol.centre_dec, sol.arcsec_per_px,
                                sol.arcsec_per_px * frame.width.max(frame.height) as f64 / 3600.,
                                sol.held_out_rms_px, blind_s, total, stats.quads_tried, stats.hits, stats.verified
                            )
                        }
                    }
                }
            };
            println!("  {}: truth {}; {line}", frame.name, if truth.is_some() { "yes" } else { "none" });
        }
    }
    println!("\nframes {tried}, blind solved {solved}, agree with truth {right}, disagree {wrong}");
    if !seconds.is_empty() {
        seconds.sort_by(|a, b| a.total_cmp(b));
        println!("time of a solved frame (blind + refinement): median {:.1} s, max {:.1} s", seconds[seconds.len() / 2], seconds[seconds.len() - 1]);
    }

    let synthetic_n: usize = arg(&args, "--synthetic").and_then(|v| v.parse().ok()).unwrap_or(0);
    if synthetic_n > 0 {
        println!("
synthetic frames at random places of the sky (3840 x 2160, field 2-6 deg, random rotation and mirroring)");
        let mut rng = Rng(12345);
        let levels: Vec<(f64, f64, usize)> = [10usize, 15, 20, 30, 50, 100]
            .iter()
            .map(|&n| (0.5, 2.0, n))
            .chain([20usize, 30, 50].iter().map(|&n| (2.0, 6.0, n)))
            .chain([30usize, 50, 100, 200].iter().map(|&n| (6.0, 12.0, n)))
            .collect();
        for (lo, hi, n_det) in levels {
            let (mut ok, mut wrong, mut none) = (0, 0, 0);
            let mut times = vec![];
            let mut failures: Vec<String> = vec![];
            for _ in 0..synthetic_n {
                let (frame, truth) = synthetic(&index, &mut rng, n_det, lo, hi);
                let start = Instant::now();
                let mut stats = Stats { quads_tried: 0, hits: 0, verified: 0 };
                let found = blind(&index, &frame, top, 12, -12., &mut stats);
                times.push(start.elapsed().as_secs_f64());
                match found {
                    Some(hyp) => {
                        let err = sep_radec((hyp.ra, hyp.dec), (truth.0, truth.1));
                        if err < 0.25 && (hyp.arcsec_per_px / truth.2 - 1.).abs() < 0.03 {
                            ok += 1;
                        } else {
                            wrong += 1;
                            failures.push(format!("WRONG at RA {:.2} Dec {:+.2}", truth.0, truth.1));
                        }
                    }
                    None => {
                        none += 1;
                        failures.push(format!("none at RA {:.2} Dec {:+.2} ({} detections)", truth.0, truth.1, frame.stars.len()));
                    }
                }
            }
            times.sort_by(|a, b| a.total_cmp(b));
            println!(
                "  field {lo}-{hi} deg, {n_det:>3} detections: {ok} correct, {wrong} wrong, {none} none of {synthetic_n}; blind time median {:.3} s, max {:.2} s",
                times[times.len() / 2],
                times[times.len() - 1]
            );
            for f in failures.iter().take(4) {
                println!("      {f}");
            }
        }
    }

    // Negative control: random star lists must not give a hypothesis.
    println!("\nnegative control: {negatives} random star lists of 150 stars (3840 x 2160)");
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    };
    let (mut false_hits, start) = (0, Instant::now());
    for _ in 0..negatives {
        let frame = Frame {
            name: "random".into(),
            width: 3840,
            height: 2160,
            epoch: 2026.,
            stars: (0..150).map(|_| [next() * 3840., next() * 2160.]).collect(),
        };
        let mut stats = Stats { quads_tried: 0, hits: 0, verified: 0 };
        if blind(&index, &frame, top, 12, -12., &mut stats).is_some() {
            false_hits += 1;
        }
    }
    println!(
        "  false hypotheses: {false_hits} of {negatives}; {:.2} s per list",
        start.elapsed().as_secs_f64() / negatives.max(1) as f64
    );
}
