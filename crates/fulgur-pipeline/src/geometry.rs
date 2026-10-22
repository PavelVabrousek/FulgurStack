//! Rigid/similarity catalog geometry, deterministic consensus and exact footprints.
use crate::stars::Star;
use fulgur_core::{Result, check_cancelled};
use nalgebra::{Matrix2, Vector2};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

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
    if lp < 10.
        || if similarity {
            !(0.3267..=3.03).contains(&(lr / lp))
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
        t.scale_factor = (lr / lp).clamp(0.33, 3.);
    }
    let a = t.apply(p);
    t.tx_px = r[0] - a[0];
    t.ty_px = r[1] - a[1];
    Some(t)
}
/// Weighted proper-rotation Procrustes with optional bounded uniform scale, never shear.
fn procrustes(src: &[Star], dst: &[Star], pairs: &[Match], similarity: bool) -> Option<Transform> {
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
        (numerator / denominator).clamp(0.33, 3.)
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
            let inliers: Vec<_> = candidates
                .iter()
                .filter(|m| {
                    distance(t.apply(point(&src[m.source])), point(&dst[m.reference])) <= 1.5
                })
                .cloned()
                .collect();
            let error: f64 = inliers
                .iter()
                .map(|m| distance(t.apply(point(&src[m.source])), point(&dst[m.reference])).powi(2))
                .sum();
            if inliers.len() > best.len() || (inliers.len() == best.len() && error < best_error) {
                best = inliers;
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
        best = correspondences(src, dst, t, 1.5);
        t = procrustes(src, dst, &best, similarity)?;
    }
    best = correspondences(src, dst, t, 1.5);
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
    check_cancelled(cancel)?;
    if let Some(t) = prediction {
        if let Some(s) = consensus(src, dst, t, 12., "sequential_direct", similarity) {
            return Ok(Some(s));
        }
    }
    let target = triangles(dst);
    let source = triangles(src);
    let mut index: BTreeMap<_, Vec<&Triangle>> = BTreeMap::new();
    for t in &target {
        let mut k = key(t.features);
        if similarity {
            k.2 = 0;
        }
        index.entry(k).or_default().push(t);
    }
    let mut votes: BTreeMap<(i32, i32, i32, i32), (usize, Transform)> = BTreeMap::new();
    let center = if src.is_empty() {
        [0., 0.]
    } else {
        [
            src.iter().map(|s| s.x_px).sum::<f64>() / src.len() as f64,
            src.iter().map(|s| s.y_px).sum::<f64>() / src.len() as f64,
        ]
    };
    for (n, t) in source.iter().enumerate() {
        if n % 128 == 0 {
            check_cancelled(cancel)?;
        }
        let mut k = key(t.features);
        if similarity {
            k.2 = 0;
        }
        for x in -1..=1 {
            for y in -1..=1 {
                for z in if similarity { 0..=0 } else { -1..=1 } {
                    if let Some(list) = index.get(&(k.0 + x, k.1 + y, k.2 + z)) {
                        for r in list {
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
                            if distance(h.apply(point(&src[t.ids[1]])), point(&dst[r.ids[1]])) > 3.
                            {
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
    }
    let mut ranked: Vec<_> = votes.into_values().collect();
    ranked.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut best: Option<Solution> = None;
    for (_, t) in ranked.into_iter().take(24) {
        check_cancelled(cancel)?;
        if let Some(s) = consensus(src, dst, t, 5., "triangle_fallback", similarity) {
            if best.as_ref().is_none_or(|b| {
                s.pairs.len() > b.pairs.len() || (s.pairs.len() == b.pairs.len() && s.rmse < b.rmse)
            }) {
                best = Some(s);
            }
        }
    }
    Ok(best)
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
mod tests {
    use super::*;
    fn catalog() -> Vec<Star> {
        (0..90)
            .map(|i| {
                let x = 40. + ((i * 137 + i * i * 11) % 1900) as f64;
                let y = 40. + ((i * 241 + i * i * 7) % 3400) as f64;
                Star {
                    x_px: x,
                    y_px: y,
                    hfr_px: 1.7,
                    moment_fwhm_px: 3.4,
                    eccentricity: 0.2,
                    flux_adu: 10000. + i as f64 * 71.,
                    peak_adu: 15000.,
                    snr: 100.,
                }
            })
            .collect()
    }
    #[test]
    fn similarity_scale_rotation_sweep_and_composition() {
        let src = catalog();
        let cancel = AtomicBool::new(false);
        for scale in [0.33, 0.5, 1., 2., 3.] {
            for degrees in [0_f64, 15., 90., 179.5] {
                let expected = Transform {
                    scale_factor: scale,
                    theta_rad: degrees.to_radians(),
                    tx_px: 177.3,
                    ty_px: -64.9,
                };
                let mut dst: Vec<_> = src
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % 6 != 0)
                    .map(|(i, s)| {
                        let p = expected.apply(point(s));
                        let mut s = s.clone();
                        s.x_px = p[0] + (i as f64).sin() * 0.015;
                        s.y_px = p[1] + (i as f64).cos() * 0.015;
                        s
                    })
                    .collect();
                let mut outliers = catalog();
                for s in &mut outliers {
                    s.x_px += 52.31;
                    s.y_px -= 173.94;
                }
                dst.extend(outliers.into_iter().take(12));
                dst.reverse();
                let got = solve_similarity(&src, &dst, None, &cancel)
                    .unwrap()
                    .expect("similarity match");
                let error = (src
                    .iter()
                    .map(|s| {
                        distance(expected.apply(point(s)), got.transform.apply(point(s))).powi(2)
                    })
                    .sum::<f64>()
                    / src.len() as f64)
                    .sqrt();
                assert!(error < 0.15, "scale={scale} angle={degrees}: {error}");
                assert!((got.transform.scale_factor - scale).abs() < 0.0001);
                assert!(got.pairs.len() >= 70);
                let p = [101.7, 277.3];
                assert!(distance(got.transform.inverse().apply(got.transform.apply(p)), p) < 1e-9);
                let next = Transform {
                    scale_factor: 0.7,
                    theta_rad: 0.12,
                    tx_px: -13.,
                    ty_px: 9.,
                };
                assert!(
                    distance(
                        got.transform.compose(next).apply(p),
                        got.transform.apply(next.apply(p))
                    ) < 1e-9
                );
            }
        }
    }
    #[test]
    fn similarity_rejects_out_of_range_reflection_and_insufficient_matches() {
        let src = catalog();
        let cancel = AtomicBool::new(false);
        for scale in [0.30, 3.2, -1.] {
            let dst: Vec<_> = src
                .iter()
                .map(|s| {
                    let mut s = s.clone();
                    s.x_px *= scale;
                    s.y_px *= scale.abs();
                    s
                })
                .collect();
            assert!(
                solve_similarity(&src, &dst, None, &cancel)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            solve_similarity(&src[..9], &src, None, &cancel)
                .unwrap()
                .is_none()
        );
        assert!(solve_similarity(&src, &src, None, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn rotation_sweep_with_missing_stars_false_detections_and_centroid_noise() {
        let dst = catalog();
        let cancel = AtomicBool::new(false);
        for angle in [0_f64, 15., 90., 179.5, -179.5] {
            let expected = Transform {
                scale_factor: 1.,
                theta_rad: angle.to_radians(),
                tx_px: 132.7,
                ty_px: -81.3,
            };
            let mut src: Vec<_> = dst
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 5 != 0)
                .map(|(i, s)| {
                    let p = expected.inverse().apply(point(s));
                    let mut s = s.clone();
                    s.x_px = p[0] + (i as f64 * 1.7).sin() * 0.02;
                    s.y_px = p[1] + (i as f64 * 0.9).cos() * 0.02;
                    s
                })
                .collect();
            let mut extras = catalog();
            for s in extras.iter_mut().take(15) {
                s.x_px += 45.3;
                s.y_px -= 31.7;
            }
            src.extend(extras.into_iter().take(15));
            src.reverse();
            let got = solve(&src, &dst, None, &cancel)
                .unwrap()
                .expect("rotation fallback");
            let error = (src
                .iter()
                .take(72)
                .map(|s| distance(got.transform.apply(point(s)), expected.apply(point(s))).powi(2))
                .sum::<f64>()
                / 72.)
                .sqrt();
            println!(
                "Rotation gate {angle} deg: transform recovery {error:.9}px, inlier residual {:.9}px",
                got.rmse
            );
            assert!(error < 0.15, "angle={angle}, recovered RMSE={error}");
            assert!(got.pairs.len() >= 65);
            assert!(got.rmse < 0.04);
        }
    }
    #[test]
    fn prediction_fallback_and_composition_do_not_accumulate_drift() {
        let dst = catalog();
        let t = Transform {
            scale_factor: 1.,
            theta_rad: 0.002,
            tx_px: 2.3,
            ty_px: -1.7,
        };
        let src: Vec<_> = dst
            .iter()
            .map(|s| {
                let p = t.inverse().apply(point(s));
                let mut s = s.clone();
                s.x_px = p[0];
                s.y_px = p[1];
                s
            })
            .collect();
        let s = solve(
            &src,
            &dst,
            Some(Transform::IDENTITY),
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        assert_eq!(s.method, "sequential_direct");
        assert!(s.rmse < 1e-9);
        let bridge = Transform {
            scale_factor: 1.,
            theta_rad: 3.13,
            tx_px: 1800.,
            ty_px: 3500.,
        };
        let p = [120., 140.];
        assert!(distance(bridge.compose(t).apply(p), bridge.apply(t.apply(p))) < 1e-9);
        assert!(distance(t.inverse().apply(t.apply(p)), p) < 1e-9);
        assert!(
            solve(&src[..9], &dst, None, &AtomicBool::new(false))
                .unwrap()
                .is_none()
        );
        assert!(solve(&src, &dst, None, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn canvas_matches_brute_force_weighted_coverage_and_polygon_intersection() {
        let a = footprint(Transform::IDENTITY, 24, 20);
        let b = footprint(
            Transform {
                scale_factor: 1.,
                theta_rad: 0.,
                tx_px: 4.,
                ty_px: 2.,
            },
            24,
            20,
        );
        let common = intersection(a.clone(), &b);
        let cancel = AtomicBool::new(false);
        let c = canvas(&[(common, 1.)], 1., &cancel).unwrap().unwrap();
        assert_eq!(
            (c.origin_x_px, c.origin_y_px, c.width, c.height),
            (5, 3, 17, 15)
        );
        let polygons = vec![(a, 0.8), (b, 0.2)];
        let c = canvas(&polygons, 0.8, &cancel).unwrap().unwrap();
        assert_eq!(
            (c.origin_x_px, c.origin_y_px, c.width, c.height),
            (1, 1, 21, 17)
        );
        let c = canvas(&polygons, 1., &cancel).unwrap().unwrap();
        assert_eq!(
            (c.origin_x_px, c.origin_y_px, c.width, c.height),
            (5, 3, 17, 15)
        );
        let rotated = footprint(
            Transform {
                scale_factor: 1.,
                theta_rad: 0.4,
                tx_px: 10.,
                ty_px: -2.,
            },
            24,
            20,
        );
        let ps = vec![(polygons[0].0.clone(), 0.6), (rotated, 0.4)];
        for threshold in [0.6, 0.8, 1.] {
            if let Some(c) = canvas(&ps, threshold, &cancel).unwrap() {
                for y in c.origin_y_px..c.origin_y_px + c.height as i32 {
                    for x in c.origin_x_px..c.origin_x_px + c.width as i32 {
                        let weight: f64 = ps
                            .iter()
                            .filter(|(p, _)| {
                                (0..p.len()).all(|i| {
                                    cross(p[i], p[(i + 1) % p.len()], [x as f64, y as f64]) >= -1e-8
                                })
                            })
                            .map(|(_, w)| w)
                            .sum();
                        assert!(weight + 1e-9 >= threshold);
                    }
                }
            }
        }
        assert!(canvas(&[], 0.8, &cancel).unwrap().is_none());
        assert!(canvas(&ps, 0.8, &AtomicBool::new(true)).is_err());
    }
}
