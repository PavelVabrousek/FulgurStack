//! Rigid/similarity catalog geometry, deterministic consensus and exact footprints.
use crate::stars::Star;
use fulgur_core::{MAX_GEOMETRY_SCALE, MIN_GEOMETRY_SCALE, Result, check_cancelled};
use nalgebra::{Matrix2, Vector2};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{OnceLock, atomic::AtomicBool};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Transform {
    pub scale_factor: f64,
    pub theta_rad: f64,
    pub tx_px: f64,
    pub ty_px: f64,
}
impl Transform {
    pub const IDENTITY: Self = Self {
        scale_factor: 1.,
        theta_rad: 0.,
        tx_px: 0.,
        ty_px: 0.,
    };
    pub fn apply(self, p: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.theta_rad.sin_cos();
        [
            self.scale_factor * (c * p[0] - s * p[1]) + self.tx_px,
            self.scale_factor * (s * p[0] + c * p[1]) + self.ty_px,
        ]
    }
    pub fn inverse(self) -> Self {
        let (s, c) = self.theta_rad.sin_cos();
        Self {
            scale_factor: 1. / self.scale_factor,
            theta_rad: -self.theta_rad,
            tx_px: (-c * self.tx_px - s * self.ty_px) / self.scale_factor,
            ty_px: (s * self.tx_px - c * self.ty_px) / self.scale_factor,
        }
    }
    pub fn compose(self, other: Self) -> Self {
        let t = self.apply([other.tx_px, other.ty_px]);
        Self {
            scale_factor: self.scale_factor * other.scale_factor,
            theta_rad: (self.theta_rad + other.theta_rad)
                .sin()
                .atan2((self.theta_rad + other.theta_rad).cos()),
            tx_px: t[0],
            ty_px: t[1],
        }
    }
}
fn point(s: &Star) -> [f64; 2] {
    [s.x_px, s.y_px]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
#[derive(Clone, Debug, Serialize)]
pub struct Match {
    pub source: usize,
    pub reference: usize,
    pub residual_px: f64,
}
#[derive(Clone, Debug)]
pub struct Solution {
    pub transform: Transform,
    pub pairs: Vec<Match>,
    pub rmse: f64,
    pub method: &'static str,
}

/// Every fifth source star is held out: it validates a fit and never helps to make it.
pub(crate) fn held_out(m: &Match) -> bool {
    m.source % 5 == 0
}

/// Pixel distance within which a predicted star counts as the same star.
pub(crate) const MATCH_TOLERANCE_PX: f64 = 1.5;

pub(crate) fn correspondences(src: &[Star], dst: &[Star], t: Transform, radius: f64) -> Vec<Match> {
    let mut candidates = Vec::new();
    for (i, s) in src.iter().enumerate() {
        let p = t.apply(point(s));
        if let Some((j, d)) = dst
            .iter()
            .enumerate()
            .map(|(j, r)| (j, distance(p, point(r))))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            candidates.push(Match {
                source: i,
                reference: j,
                residual_px: d,
            });
        }
    }
    candidates.sort_by(|a, b| {
        a.residual_px
            .total_cmp(&b.residual_px)
            .then(a.source.cmp(&b.source))
    });
    let mut used = BTreeSet::new();
    candidates.retain(|m| used.insert(m.reference));
    candidates
}
fn pair_transform(
    p: [f64; 2],
    q: [f64; 2],
    r: [f64; 2],
    s: [f64; 2],
    similarity: bool,
) -> Option<Transform> {
    let lp = distance(p, q);
    let lr = distance(r, s);
    // Seeds may sit 1% outside the supported range; fits are clamped to it.
    if lp < 10.
        || if similarity {
            !(MIN_GEOMETRY_SCALE / 1.01..=MAX_GEOMETRY_SCALE * 1.01).contains(&(lr / lp))
        } else {
            (lp - lr).abs() > 3.
        }
    {
        return None;
    }
    let angle = ((s[1] - r[1]).atan2(s[0] - r[0]) - (q[1] - p[1]).atan2(q[0] - p[0]))
        .sin()
        .atan2(((s[1] - r[1]).atan2(s[0] - r[0]) - (q[1] - p[1]).atan2(q[0] - p[0])).cos());
    let mut t = Transform {
        scale_factor: 1.,
        theta_rad: angle,
        tx_px: 0.,
        ty_px: 0.,
    };
    if similarity {
        t.scale_factor = (lr / lp).clamp(MIN_GEOMETRY_SCALE, MAX_GEOMETRY_SCALE);
    }
    let a = t.apply(p);
    t.tx_px = r[0] - a[0];
    t.ty_px = r[1] - a[1];
    Some(t)
}
/// Weighted proper-rotation Procrustes with optional bounded uniform scale, never shear.
pub(crate) fn procrustes(
    src: &[Star],
    dst: &[Star],
    pairs: &[Match],
    similarity: bool,
) -> Option<Transform> {
    if pairs.len() < 2 {
        return None;
    }
    let weights: Vec<_> = pairs
        .iter()
        .map(|m| {
            src[m.source]
                .snr
                .min(dst[m.reference].snr)
                .clamp(1., 100.)
                .sqrt()
        })
        .collect();
    let sum: f64 = weights.iter().sum();
    let mut p = Vector2::zeros();
    let mut q = Vector2::zeros();
    for (m, w) in pairs.iter().zip(&weights) {
        p += Vector2::from(point(&src[m.source])) * *w;
        q += Vector2::from(point(&dst[m.reference])) * *w;
    }
    p /= sum;
    q /= sum;
    let mut h = Matrix2::zeros();
    for (m, w) in pairs.iter().zip(&weights) {
        let a = Vector2::from(point(&src[m.source])) - p;
        let b = Vector2::from(point(&dst[m.reference])) - q;
        h += a * b.transpose() * *w;
    }
    let svd = h.svd(true, true);
    if svd.singular_values[1] <= 1e-9 {
        return None;
    }
    let u = svd.u?;
    let v = svd.v_t?.transpose();
    let mut d = Matrix2::identity();
    d[(1, 1)] = (v * u.transpose()).determinant().signum();
    let r = v * d * u.transpose();
    let scale = if similarity {
        let mut numerator = 0.;
        let mut denominator = 0.;
        for (m, w) in pairs.iter().zip(&weights) {
            let a = Vector2::from(point(&src[m.source])) - p;
            let b = Vector2::from(point(&dst[m.reference])) - q;
            numerator += w * b.dot(&(r * a));
            denominator += w * a.norm_squared();
        }
        if denominator <= 0. {
            return None;
        }
        (numerator / denominator).clamp(MIN_GEOMETRY_SCALE, MAX_GEOMETRY_SCALE)
    } else {
        1.
    };
    let shift = q - scale * r * p;
    Some(Transform {
        scale_factor: scale,
        theta_rad: r[(1, 0)].atan2(r[(0, 0)]),
        tx_px: shift[0],
        ty_px: shift[1],
    })
}
fn consensus(
    src: &[Star],
    dst: &[Star],
    initial: Transform,
    radius: f64,
    method: &'static str,
    similarity: bool,
) -> Option<Solution> {
    let candidates = correspondences(src, dst, initial, radius);
    if candidates.len() < 10 {
        return None;
    }
    let mut best = Vec::new();
    let mut inliers = Vec::with_capacity(candidates.len());
    let mut best_error = f64::INFINITY;
    // Deterministic RANSAC over well separated candidate pairs; bounded to 512 hypotheses.
    let n = candidates.len();
    let mut tested = 0;
    'outer: for gap in 1..n {
        for i in 0..n - gap {
            let a = &candidates[i];
            let b = &candidates[i + gap];
            if distance(point(&src[a.source]), point(&src[b.source])) < 30. {
                continue;
            }
            let Some(t) = pair_transform(
                point(&src[a.source]),
                point(&src[b.source]),
                point(&dst[a.reference]),
                point(&dst[b.reference]),
                similarity,
            ) else {
                continue;
            };
            inliers.clear();
            inliers.extend(
                candidates
                    .iter()
                    .filter(|m| {
                        distance(t.apply(point(&src[m.source])), point(&dst[m.reference]))
                            <= MATCH_TOLERANCE_PX
                    })
                    .cloned(),
            );
            let error: f64 = inliers
                .iter()
                .map(|m| distance(t.apply(point(&src[m.source])), point(&dst[m.reference])).powi(2))
                .sum();
            if inliers.len() > best.len() || (inliers.len() == best.len() && error < best_error) {
                std::mem::swap(&mut best, &mut inliers);
                best_error = error;
            }
            tested += 1;
            if tested >= 512 {
                break 'outer;
            }
        }
    }
    if best.len() < 10 {
        return None;
    }
    let mut t = procrustes(src, dst, &best, similarity)?;
    for _ in 0..3 {
        best = correspondences(src, dst, t, MATCH_TOLERANCE_PX);
        t = procrustes(src, dst, &best, similarity)?;
    }
    best = correspondences(src, dst, t, MATCH_TOLERANCE_PX);
    if best.len() < 10 {
        return None;
    }
    let rmse = (best.iter().map(|m| m.residual_px.powi(2)).sum::<f64>() / best.len() as f64).sqrt();
    (rmse <= 1.5
        && [t.theta_rad, t.tx_px, t.ty_px, rmse]
            .iter()
            .all(|x| x.is_finite()))
    .then_some(Solution {
        transform: t,
        pairs: best,
        rmse,
        method,
    })
}
fn coarse_consensus(
    src: &[Star],
    dst: &[Star],
    initial: Transform,
    radius: f64,
    method: &'static str,
    similarity: bool,
    tolerance: f64,
) -> Option<Solution> {
    let candidates = correspondences(src, dst, initial, radius);
    if candidates.len() < 10 {
        return None;
    }
    let mut best = Vec::new();
    let mut inliers = Vec::with_capacity(candidates.len());
    let mut best_error = f64::INFINITY;
    // Deterministic RANSAC over well separated candidate pairs; bounded to 512 hypotheses.
    let n = candidates.len();
    let mut tested = 0;
    'outer: for gap in 1..n {
        for i in 0..n - gap {
            let a = &candidates[i];
            let b = &candidates[i + gap];
            if distance(point(&src[a.source]), point(&src[b.source])) < 30. {
                continue;
            }
            let Some(t) = pair_transform(
                point(&src[a.source]),
                point(&src[b.source]),
                point(&dst[a.reference]),
                point(&dst[b.reference]),
                similarity,
            ) else {
                continue;
            };
            inliers.clear();
            inliers.extend(
                candidates
                    .iter()
                    .filter(|m| {
                        distance(t.apply(point(&src[m.source])), point(&dst[m.reference]))
                            <= tolerance
                    })
                    .cloned(),
            );
            let error: f64 = inliers
                .iter()
                .map(|m| distance(t.apply(point(&src[m.source])), point(&dst[m.reference])).powi(2))
                .sum();
            if inliers.len() > best.len() || (inliers.len() == best.len() && error < best_error) {
                std::mem::swap(&mut best, &mut inliers);
                best_error = error;
            }
            tested += 1;
            if tested >= 512 {
                break 'outer;
            }
        }
    }
    if best.len() < 10 {
        return None;
    }
    let mut t = procrustes(src, dst, &best, similarity)?;
    for _ in 0..3 {
        best = correspondences(src, dst, t, tolerance);
        t = procrustes(src, dst, &best, similarity)?;
    }
    best = correspondences(src, dst, t, tolerance);
    if best.len() < 10 {
        return None;
    }
    let rmse = (best.iter().map(|m| m.residual_px.powi(2)).sum::<f64>() / best.len() as f64).sqrt();
    (rmse <= tolerance
        && [t.theta_rad, t.tx_px, t.ty_px, rmse]
            .iter()
            .all(|x| x.is_finite()))
    .then_some(Solution {
        transform: t,
        pairs: best,
        rmse,
        method,
    })
}
#[derive(Clone)]
struct Triangle {
    ids: [usize; 3],
    features: [f64; 3],
}
fn triangles(stars: &[Star]) -> Vec<Triangle> {
    let mut out = Vec::new();
    let mut unique = BTreeSet::new();
    for (i, s) in stars.iter().enumerate() {
        let mut neighbors: Vec<_> = stars
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(j, r)| (j, distance(point(s), point(r))))
            .collect();
        neighbors.sort_by(|a, b| a.1.total_cmp(&b.1));
        neighbors.truncate(8);
        for a in 0..neighbors.len() {
            for b in a + 1..neighbors.len() {
                let mut ids = [i, neighbors[a].0, neighbors[b].0];
                ids.sort();
                if !unique.insert(ids) {
                    continue;
                }
                let mut sides = [
                    (
                        distance(point(&stars[ids[1]]), point(&stars[ids[2]])),
                        ids[0],
                    ),
                    (
                        distance(point(&stars[ids[0]]), point(&stars[ids[2]])),
                        ids[1],
                    ),
                    (
                        distance(point(&stars[ids[0]]), point(&stars[ids[1]])),
                        ids[2],
                    ),
                ];
                sides.sort_by(|a, b| a.0.total_cmp(&b.0));
                let l = sides[2].0;
                if l < 30.
                    || sides[0].0 / l < 0.15
                    || (sides[1].0 - sides[0].0) / l < 0.025
                    || (l - sides[1].0) / l < 0.025
                {
                    continue;
                }
                let p = point(&stars[sides[0].1]);
                let q = point(&stars[sides[1].1]);
                let r = point(&stars[sides[2].1]);
                if ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])).abs() / l.powi(2)
                    < 0.05
                {
                    continue;
                }
                out.push(Triangle {
                    ids: [sides[0].1, sides[1].1, sides[2].1],
                    features: [sides[0].0 / l, sides[1].0 / l, l.ln()],
                });
            }
        }
    }
    out
}
fn key(f: [f64; 3]) -> (i32, i32, i32) {
    (
        (f[0] / 0.01).floor() as i32,
        (f[1] / 0.01).floor() as i32,
        (f[2] / 0.02).floor() as i32,
    )
}
#[cfg(test)]
pub fn solve(
    src: &[Star],
    dst: &[Star],
    prediction: Option<Transform>,
    cancel: &AtomicBool,
) -> Result<Option<Solution>> {
    solve_model(src, dst, prediction, cancel, false)
}
pub fn solve_similarity(
    src: &[Star],
    dst: &[Star],
    prediction: Option<Transform>,
    cancel: &AtomicBool,
) -> Result<Option<Solution>> {
    solve_model(src, dst, prediction, cancel, true)
}
fn solve_model(
    src: &[Star],
    dst: &[Star],
    prediction: Option<Transform>,
    cancel: &AtomicBool,
    similarity: bool,
) -> Result<Option<Solution>> {
    Matcher::new(dst, similarity).solve(src, prediction, cancel)
}

/// Immutable target descriptors, created only when a prediction needs fallback.
/// One master and at most one session matcher are retained during native registration.
pub(crate) struct Matcher<'a> {
    stars: &'a [Star],
    similarity: bool,
    fallback: OnceLock<FallbackIndex>,
}

struct FallbackIndex {
    triangles: Vec<Triangle>,
    buckets: BTreeMap<(i32, i32, i32), Vec<usize>>,
}

impl FallbackIndex {
    fn new(stars: &[Star], similarity: bool) -> Self {
        let triangles = triangles(stars);
        let mut buckets: BTreeMap<_, Vec<usize>> = BTreeMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let mut k = key(t.features);
            if similarity {
                k.2 = 0;
            }
            buckets.entry(k).or_default().push(i);
        }
        Self { triangles, buckets }
    }
}

impl<'a> Matcher<'a> {
    pub(crate) fn new(stars: &'a [Star], similarity: bool) -> Self {
        Self {
            stars,
            similarity,
            fallback: OnceLock::new(),
        }
    }

    pub(crate) fn solve(
        &self,
        src: &[Star],
        prediction: Option<Transform>,
        cancel: &AtomicBool,
    ) -> Result<Option<Solution>> {
        self.solve_at_tolerance(src, prediction, cancel, MATCH_TOLERANCE_PX)
    }

    fn solve_at_tolerance(
        &self,
        src: &[Star],
        prediction: Option<Transform>,
        cancel: &AtomicBool,
        tolerance: f64,
    ) -> Result<Option<Solution>> {
        let dst = self.stars;
        let similarity = self.similarity;
        let fit = |t, radius, method| {
            if tolerance == MATCH_TOLERANCE_PX {
                consensus(src, dst, t, radius, method, similarity)
            } else {
                coarse_consensus(
                    src,
                    dst,
                    t,
                    radius.max(tolerance * 2.),
                    method,
                    similarity,
                    tolerance,
                )
            }
        };
        check_cancelled(cancel)?;
        if let Some(t) = prediction {
            if let Some(s) = fit(t, 12., "sequential_direct") {
                return Ok(Some(s));
            }
        }
        let target = self
            .fallback
            .get_or_init(|| FallbackIndex::new(dst, similarity));
        let source = triangles(src);
        let center = if src.is_empty() {
            [0., 0.]
        } else {
            [
                src.iter().map(|s| s.x_px).sum::<f64>() / src.len() as f64,
                src.iter().map(|s| s.y_px).sum::<f64>() / src.len() as f64,
            ]
        };
        // Votes are counted in parallel chunks of source triangles and merged
        // in chunk order, so each bin keeps the transform of its first vote in
        // triangle order, as one sequential pass would (ADR-148).
        use rayon::prelude::*;
        let chunks: Vec<Votes> = source
            .par_chunks(VOTE_CHUNK)
            .map(|chunk| -> Result<Votes> {
                let mut votes = Votes::new();
                for (n, t) in chunk.iter().enumerate() {
                    if n % 128 == 0 {
                        check_cancelled(cancel)?;
                    }
                    vote(target, dst, src, center, similarity, t, &mut votes);
                }
                Ok(votes)
            })
            .collect::<Result<_>>()?;
        let mut votes = Votes::new();
        for chunk in chunks {
            for (bin, (count, h)) in chunk {
                votes
                    .entry(bin)
                    .and_modify(|v| v.0 += count)
                    .or_insert((count, h));
            }
        }
        let mut ranked: Vec<_> = votes.into_values().collect();
        ranked.sort_by_key(|a| std::cmp::Reverse(a.0));
        check_cancelled(cancel)?;
        // The candidate fits run in parallel; the best is chosen in rank order.
        let candidates: Vec<Transform> = ranked.into_iter().take(24).map(|(_, t)| t).collect();
        let fits: Vec<Option<Solution>> = candidates
            .into_par_iter()
            .map(|t| fit(t, 5., "triangle_fallback"))
            .collect();
        let mut best: Option<Solution> = None;
        for s in fits.into_iter().flatten() {
            if best.as_ref().is_none_or(|b| {
                s.pairs.len() > b.pairs.len() || (s.pairs.len() == b.pairs.len() && s.rmse < b.rmse)
            }) {
                best = Some(s);
            }
        }
        Ok(best)
    }
}

/// Source triangles per parallel voting chunk.
const VOTE_CHUNK: usize = 256;

type Votes = BTreeMap<(i32, i32, i32, i32), (usize, Transform)>;

/// Adds the votes of one source triangle against the target index.
fn vote(
    target: &FallbackIndex,
    dst: &[Star],
    src: &[Star],
    center: [f64; 2],
    similarity: bool,
    t: &Triangle,
    votes: &mut Votes,
) {
    let mut k = key(t.features);
    if similarity {
        k.2 = 0;
    }
    for x in -1..=1 {
        for y in -1..=1 {
            for z in if similarity { 0..=0 } else { -1..=1 } {
                let Some(list) = target.buckets.get(&(k.0 + x, k.1 + y, k.2 + z)) else {
                    continue;
                };
                for &r in list {
                    let r = &target.triangles[r];
                    if (t.features[0] - r.features[0]).abs() > 0.01
                        || (t.features[1] - r.features[1]).abs() > 0.01
                        || (!similarity && (t.features[2] - r.features[2]).abs() > 0.015)
                    {
                        continue;
                    }
                    let Some(h) = pair_transform(
                        point(&src[t.ids[0]]),
                        point(&src[t.ids[2]]),
                        point(&dst[r.ids[0]]),
                        point(&dst[r.ids[2]]),
                        similarity,
                    ) else {
                        continue;
                    };
                    if distance(h.apply(point(&src[t.ids[1]])), point(&dst[r.ids[1]])) > 3. {
                        continue;
                    }
                    let p = h.apply(center);
                    let bin = (
                        (h.theta_rad / 0.004).round() as i32,
                        (p[0] / 8.).round() as i32,
                        (p[1] / 8.).round() as i32,
                        if similarity {
                            (h.scale_factor.ln() / 0.01).round() as i32
                        } else {
                            0
                        },
                    );
                    votes.entry(bin).and_modify(|v| v.0 += 1).or_insert((1, h));
                }
            }
        }
    }
}

pub fn footprint(t: Transform, width: usize, height: usize) -> Vec<[f64; 2]> {
    // Strict 4x4 Keys footprint: floor(source)-1 >=0, floor(source)+2 < size.
    [
        [1., 1.],
        [(width - 2) as f64 - 1e-6, 1.],
        [(width - 2) as f64 - 1e-6, (height - 2) as f64 - 1e-6],
        [1., (height - 2) as f64 - 1e-6],
    ]
    .map(|p| t.apply(p))
    .to_vec()
}
fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
pub fn intersection(mut subject: Vec<[f64; 2]>, clip: &[[f64; 2]]) -> Vec<[f64; 2]> {
    for i in 0..clip.len() {
        if subject.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let old = std::mem::take(&mut subject);
        let mut p = *old.last().unwrap();
        let mut dp = cross(a, b, p);
        for q in old {
            let dq = cross(a, b, q);
            if (dp >= 0.) != (dq >= 0.) {
                let f = dp / (dp - dq);
                let mut vertex = [p[0] + f * (q[0] - p[0]), p[1] + f * (q[1] - p[1])];
                // Retain exact axis-aligned clipping boundaries; ceil must not
                // drop a whole row because a computed 3.0 became 3.0 + one ULP.
                if a[0] == b[0] {
                    vertex[0] = a[0];
                }
                if a[1] == b[1] {
                    vertex[1] = a[1];
                }
                subject.push(vertex);
            }
            if dq >= 0. {
                subject.push(q);
            }
            p = q;
            dp = dq;
        }
    }
    subject
}
#[derive(Debug, Serialize)]
pub struct Canvas {
    pub origin_x_px: i32,
    pub origin_y_px: i32,
    pub width: usize,
    pub height: usize,
    pub minimum_effective_weight_fraction: f64,
    pub coordinate_rule: &'static str,
}
/// Largest axis-aligned integer-center rectangle inside the exact weighted footprint mask.
/// Scanline edge events avoid N_frames x N_pixels memory/work. O(width) working memory.
pub fn canvas(
    polygons: &[(Vec<[f64; 2]>, f64)],
    threshold: f64,
    cancel: &AtomicBool,
) -> Result<Option<Canvas>> {
    if polygons.is_empty()
        || polygons.iter().any(|(p, w)| {
            p.len() < 3 || !w.is_finite() || *w <= 0. || p.iter().flatten().any(|v| !v.is_finite())
        })
    {
        return Ok(None);
    }
    let xmin = polygons
        .iter()
        .flat_map(|(p, _)| p.iter())
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .ceil() as i32;
    let xmax = polygons
        .iter()
        .flat_map(|(p, _)| p.iter())
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .floor() as i32;
    let ymin = polygons
        .iter()
        .flat_map(|(p, _)| p.iter())
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .ceil() as i32;
    let ymax = polygons
        .iter()
        .flat_map(|(p, _)| p.iter())
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .floor() as i32;
    if xmax < xmin || ymax < ymin || xmax - xmin > 100000 || ymax - ymin > 100000 {
        return Ok(None);
    }
    let width = (xmax - xmin + 1) as usize;
    let mut heights = vec![0usize; width];
    let mut best = (0usize, 0usize, 0i32, 0usize, 0usize);
    let total: f64 = polygons.iter().map(|(_, w)| w).sum();
    for y in ymin..=ymax {
        check_cancelled(cancel)?;
        let mut events = Vec::with_capacity(polygons.len() * 2);
        for (p, w) in polygons {
            let mut xs = Vec::new();
            for i in 0..p.len() {
                let a = p[i];
                let b = p[(i + 1) % p.len()];
                let yy = y as f64;
                if (a[1] <= yy && yy < b[1]) || (b[1] <= yy && yy < a[1]) {
                    xs.push(a[0] + (yy - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
            if xs.len() >= 2 {
                xs.sort_by(f64::total_cmp);
                let l = (xs[0].ceil() as i32 - xmin).max(0) as usize;
                let r = (xs[xs.len() - 1].ceil() as i32 - xmin).max(0) as usize;
                if r > l {
                    events.push((l, *w));
                    events.push((r, -*w));
                }
            }
        }
        events.sort_by_key(|e| e.0);
        let mut at = 0;
        let mut weight = 0.;
        for (x, h) in heights.iter_mut().enumerate() {
            while at < events.len() && events[at].0 <= x {
                weight += events[at].1;
                at += 1;
            }
            if weight + 1e-10 * total >= threshold * total {
                *h += 1;
            } else {
                *h = 0;
            }
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for (x, h) in heights
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .enumerate()
        {
            let mut left = x;
            while stack.last().is_some_and(|(_, hh)| *hh > h) {
                let (l, hh) = stack.pop().unwrap();
                let area = (x - l) * hh;
                if area > best.0 {
                    best = (area, l, y - hh as i32 + 1, x - l, hh);
                }
                left = l;
            }
            if stack.last().is_none_or(|(_, hh)| *hh < h) {
                stack.push((left, h));
            }
        }
    }
    Ok((best.0>0).then_some(Canvas{origin_x_px:xmin+best.1 as i32,origin_y_px:best.2,width:best.3,height:best.4,minimum_effective_weight_fraction:threshold,coordinate_rule:"integer pixel centers in reference-anchor coordinates; upper polygon edges excluded"}))
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "geometry_fallback_oracle.rs"]
mod fallback_oracle;

#[cfg(test)]
#[path = "geometry_fallback_tests.rs"]
mod fallback_tests;

/// Loose correspondences are a distortion bootstrap only, never an accepted fit.
pub(crate) fn coarse_similarity(
    src: &[Star],
    dst: &[Star],
    prediction: Option<Transform>,
    tolerance: f64,
    cancel: &AtomicBool,
) -> Result<Option<Solution>> {
    Matcher::new(dst, true).solve_at_tolerance(src, prediction, cancel, tolerance)
}
