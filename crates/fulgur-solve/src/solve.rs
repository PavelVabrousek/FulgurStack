//! The blind search: quads of a frame's brightest stars are looked up in the
//! index; each hit is a hypothesis (field centre, scale, mirroring) that is
//! checked by counting the star matches it explains.
use crate::index::Index;
use crate::quad::{P2, canonical, fit_similarity, inside_circle};
use crate::sphere::{Basis, RAD, radec};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

/// How the search is run. The defaults are those of the feasibility study.
#[derive(Clone, Debug, PartialEq)]
pub struct SolveOptions {
    /// How many of the brightest detections make quads.
    pub top: usize,
    /// The field's long side must be within these degrees.
    pub field_deg: (f64, f64),
    /// Fewest star matches of an accepted hypothesis.
    pub min_matches: usize,
    /// Largest log10 of the chance probability of those matches.
    pub max_log10_p: f64,
    /// A catalogue star matches a detection within this many pixels.
    pub match_px: f64,
    /// How many of the brightest detections the matches are counted on.
    pub verify_detections: usize,
    /// How many of the brightest catalogue stars of the field are compared.
    pub verify_stars: usize,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            top: 30,
            field_deg: (0.4, 14.0),
            min_matches: 12,
            max_log10_p: -12.0,
            match_px: 4.0,
            verify_detections: 150,
            verify_stars: 900,
        }
    }
}

/// Where a frame is, as far as the quads say. The caller refines it.
#[derive(Clone, Debug, PartialEq)]
pub struct Hypothesis {
    /// Centre of the frame, degrees.
    pub ra: f64,
    pub dec: f64,
    pub arcsec_per_px: f64,
    /// Length of the long side of the frame, degrees.
    pub field_deg: f64,
    /// The picture is the sky seen from the other side (pixel y runs the
    /// way a mirror would turn it).
    pub mirrored: bool,
    /// Detections that landed on catalogue stars, of those compared.
    pub matched: usize,
    /// log10 of the probability that so many landed by chance.
    pub log10_p: f64,
    pub quads_tried: usize,
    pub hits: usize,
    pub verified: usize,
}

/// Detections fit for quads, brightest first. A saturated star breaks into
/// several detections a few pixels apart, and a bright nebula into dozens of
/// knots: any detection within 0.2 % of the long side of a brighter one is
/// dropped, then every one of the 300 brightest that has more than four
/// others within 2 % of the long side, which is a crowd and not a star.
pub fn clean_detections(stars: &[[f64; 2]], width: usize, height: usize) -> Vec<[f64; 2]> {
    let long = width.max(height) as f64;
    let near = (0.002 * long).max(4.0);
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
    kept.into_iter()
        .enumerate()
        .filter(|(i, s)| *i >= 300 || isolated(s))
        .map(|(_, s)| s)
        .collect()
}

/// log10 of the probability of `m` or more matches by chance when `n`
/// catalogue stars each have probability `p` of landing on a detection.
fn log10_tail(n: f64, p: f64, m: usize) -> f64 {
    let lambda = (n * p).max(1e-9);
    let mut terms = Vec::with_capacity(80);
    let mut ln_factorial: f64 = (1..=m).map(|i| (i as f64).ln()).sum();
    for k in m..m + 80 {
        terms.push(-lambda + k as f64 * lambda.ln() - ln_factorial);
        ln_factorial += ((k + 1) as f64).ln();
    }
    let top = terms.iter().copied().fold(f64::MIN, f64::max);
    (top + terms.iter().map(|t| (t - top).exp()).sum::<f64>().ln()) / std::f64::consts::LN_10
}

/// The pairs of a 4-set tried as A and B.
const PAIRS: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

struct Search<'a> {
    index: &'a Index,
    options: &'a SolveOptions,
    w: f64,
    h: f64,
    verify: Vec<P2>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    seen: HashSet<(i32, i32, i32)>,
    hits: Vec<[u32; 4]>,
    near: Vec<u32>,
    quads_tried: usize,
    hit_count: usize,
    verified: usize,
}

impl Search<'_> {
    /// A quad of the frame (A, B, C, D, with C and D inside the circle on AB).
    fn try_quad(&mut self, quad: [P2; 4]) -> Option<Hypothesis> {
        let ab_px = (quad[0][0] - quad[1][0]).hypot(quad[0][1] - quad[1][1]);
        // A short pair gives a vague code; the tolerance follows the
        // centroid error over the pair's length.
        if ab_px < 0.15 * self.w.max(self.h) {
            return None;
        }
        let tol = 0.004 + 1.2 / ab_px;
        for mirrored in [false, true] {
            let flip = |p: P2| if mirrored { [p[0], -p[1]] } else { p };
            let flipped = quad.map(flip);
            let Some((code, order)) = canonical(flipped) else {
                continue;
            };
            self.quads_tried += 1;
            self.index.lookup(&code, tol, &mut self.hits);
            let hits: Vec<[u32; 4]> = self.hits.iter().take(20).copied().collect();
            let used = order.map(|o| flipped[o]);
            for ids in hits {
                self.hit_count += 1;
                if let Some(found) = self.check(used, ids, mirrored) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// The hypothesis of one index quad for the frame's four points (`used`,
    /// already flipped when `mirrored`, in the order of the code).
    fn check(&mut self, used: [P2; 4], ids: [u32; 4], mirrored: bool) -> Option<Hypothesis> {
        let basis = Basis::at(self.index.star(ids[0]));
        let planes = ids.map(|s| basis.plane(&self.index.star(s)));
        let (sr, si, tr, ti) = fit_similarity(&used, &planes)?;
        let scale = sr.hypot(si); // radians per pixel
        let field = scale * self.w.max(self.h) / RAD;
        if !(self.options.field_deg.0..=self.options.field_deg.1).contains(&field) {
            return None;
        }
        let map = |p: P2| -> P2 {
            let p = if mirrored { [p[0], -p[1]] } else { p };
            [sr * p[0] - si * p[1] + tr, sr * p[1] + si * p[0] + ti]
        };
        let centre = basis.sky(map([self.w / 2.0, self.h / 2.0]));
        let (ra, dec) = radec(&centre);
        let key = (
            (ra / 0.25) as i32,
            ((dec + 90.0) / 0.25) as i32,
            (scale.ln() / 0.03) as i32,
        );
        if !self.seen.insert(key) {
            return None;
        }
        self.verified += 1;
        // The catalogue stars inside the frame, through the hypothesis,
        // against the detections.
        let radius = (self.w.hypot(self.h) / 2.0) * scale / RAD * 1.1;
        self.index
            .all
            .near(&self.index.stars, &centre, radius, &mut self.near);
        let keep = self.options.verify_stars;
        if self.near.len() > keep {
            let stars = &self.index.stars;
            self.near.select_nth_unstable_by(keep, |a, b| {
                stars[*a as usize].g.total_cmp(&stars[*b as usize].g)
            });
            self.near.truncate(keep);
        }
        let denom = sr * sr + si * si;
        let (mut inside, mut matched) = (0usize, HashSet::new());
        for &s in &self.near {
            let q = basis.plane(&self.index.star(s));
            let (dx, dy) = (q[0] - tr, q[1] - ti);
            let (px, py) = ((dx * sr + dy * si) / denom, (dy * sr - dx * si) / denom);
            let (x, y) = (px, if mirrored { -py } else { py });
            if x < 0.0 || y < 0.0 || x >= self.w || y >= self.h {
                continue;
            }
            inside += 1;
            if let Some(d) = self.detection_near(x, y) {
                matched.insert(d);
            }
        }
        let m = matched.len();
        if m < self.options.min_matches {
            return None;
        }
        let p = (self.verify.len() as f64 * std::f64::consts::PI * self.options.match_px.powi(2)
            / (self.w * self.h))
            .min(1.0);
        let log10_p = log10_tail(inside as f64, p, m);
        (log10_p <= self.options.max_log10_p).then(|| Hypothesis {
            ra,
            dec,
            arcsec_per_px: scale / RAD * 3600.0,
            field_deg: field,
            mirrored,
            matched: m,
            log10_p,
            quads_tried: self.quads_tried,
            hits: self.hit_count,
            verified: self.verified,
        })
    }

    /// The detection within the matching radius of (x, y), if any.
    fn detection_near(&self, x: f64, y: f64) -> Option<usize> {
        let (cx, cy) = ((x / 16.0) as i32, (y / 16.0) as i32);
        for gx in cx - 1..=cx + 1 {
            for gy in cy - 1..=cy + 1 {
                for &i in self.cells.get(&(gx, gy)).into_iter().flatten() {
                    let v = self.verify[i];
                    if (v[0] - x).hypot(v[1] - y) <= self.options.match_px {
                        return Some(i);
                    }
                }
            }
        }
        None
    }
}

/// Places a frame of `width` x `height` pixels on the sky from the pixel
/// positions of its stars (`stars`, brightest first), with no position given.
/// None when no hypothesis is convincing, and when `cancel` is set. The
/// result is a hypothesis for a refining solver to start from.
pub fn solve(
    index: &Index,
    width: usize,
    height: usize,
    stars: &[[f64; 2]],
    options: &SolveOptions,
    cancel: &AtomicBool,
) -> Option<Hypothesis> {
    let cleaned = clean_detections(stars, width, height);
    let n = cleaned.len().min(options.top);
    if n < 4 {
        return None;
    }
    let verify: Vec<P2> = cleaned
        .iter()
        .take(options.verify_detections)
        .copied()
        .collect();
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, p) in verify.iter().enumerate() {
        cells
            .entry(((p[0] / 16.0) as i32, (p[1] / 16.0) as i32))
            .or_default()
            .push(i);
    }
    let mut search = Search {
        index,
        options,
        w: width as f64,
        h: height as f64,
        verify,
        cells,
        seen: HashSet::new(),
        hits: vec![],
        near: vec![],
        quads_tried: 0,
        hit_count: 0,
        verified: 0,
    };
    // Brightest stars first: every 4-set whose newest star is `newest`.
    for newest in 3..n {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        for i in 0..newest {
            for j in i + 1..newest {
                for k in j + 1..newest {
                    let set = [cleaned[i], cleaned[j], cleaned[k], cleaned[newest]];
                    for (ia, ib) in PAIRS {
                        let rest: Vec<usize> = (0..4).filter(|&x| x != ia && x != ib).collect();
                        let (a, b) = (set[ia], set[ib]);
                        let (c, d) = (set[rest[0]], set[rest[1]]);
                        if !inside_circle(a, b, c) || !inside_circle(a, b, d) {
                            continue;
                        }
                        if let Some(found) = search.try_quad([a, b, c, d]) {
                            return Some(found);
                        }
                    }
                }
            }
        }
    }
    None
}
