//! Frozen pre-optimization fallback and consensus oracle; test-only.
use super::*;
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
pub(super) fn solve_model(
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
