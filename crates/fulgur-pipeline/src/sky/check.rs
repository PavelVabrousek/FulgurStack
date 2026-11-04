//! End-to-end check of a registration source -> target, independent of the
//! catalogue: the source's point sources are carried through both solutions
//! (source pixel -> sky -> target pixel) and must land on point sources the
//! target detected itself.
//!
//! The registration error is the systematic displacement of those stars, not
//! the scatter of single centroids: faint narrowband stars scatter by more
//! than half a pixel even when the mapping is exact. The check therefore
//! measures the mean displacement vector over the whole frame and in each
//! cell of a 3 x 3 grid, and requires both to be small.
use super::solve::{Grid, Solution};
use serde::Serialize;

/// Matched stars below this leave a pair unverified (too little shared field).
pub const MIN_PAIRS: usize = 15;
/// Pairs within this distance (pixels of the coarser frame) are kept; farther
/// ones are blends, extended sources or chance pairs.
pub const CLIP_COARSE_PX: f64 = 1.0;
/// Systematic displacement limits in pixels of the coarser frame: the whole
/// frame and the worst cell of a 3 x 3 grid.
pub const MAX_MEAN_COARSE_PX: f64 = 0.15;
pub const MAX_CELL_COARSE_PX: f64 = 0.3;
/// A cell needs this many kept pairs to be judged.
pub const CELL_MIN_PAIRS: usize = 8;
/// Kept pairs must be at least this share of all pairs: a wrong mapping
/// leaves almost none within one pixel.
pub const MIN_KEPT: f64 = 0.5;
/// Match radius in pixels of the coarser frame.
const RADIUS_COARSE_PX: f64 = 2.5;

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub source: usize,
    pub status: &'static str,
    /// Source point sources that land inside the target.
    pub inside: usize,
    pub matched: usize,
    pub kept_fraction: f64,
    /// Scatter of single kept pairs (centroid noise of both frames).
    pub rms_target_px: f64,
    pub rms_arcsec: f64,
    pub raw_rms_target_px: f64,
    /// Systematic displacement: whole frame and worst judged cell.
    pub mean_offset_target_px: f64,
    pub worst_cell_offset_target_px: f64,
    pub judged_cells: usize,
    pub cell_offsets_target_px: Vec<Option<f64>>,
    /// Pixel of the coarser frame in target pixels.
    pub coarse_target_px: f64,
    pub coarse_arcsec: f64,
    /// Kept pairs: source star as given, target star.
    #[serde(skip)]
    pub pairs: Vec<([f64; 2], [f64; 2])>,
}

pub struct Target<'a> {
    pub solution: &'a Solution,
    pub grid: Grid,
}

impl<'a> Target<'a> {
    pub fn new(solution: &'a Solution, stars: &[[f64; 2]]) -> Self {
        Self {
            solution,
            grid: Grid::new(stars.to_vec(), 8.),
        }
    }
}

/// Distance, target star, displacement vector and source star of one pair.
type Pair = (f64, [f64; 2], [f64; 2], [f64; 2]);

fn rms(v: &[f64]) -> f64 {
    (v.iter().map(|r| r * r).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

pub fn check(source: usize, solution: &Solution, stars: &[[f64; 2]], target: &Target) -> Check {
    let t = &target.solution.wcs;
    check_map(source, solution.arcsec_per_px, stars, target, |p| {
        let (ra, dec) = solution.wcs.pixel_to_sky(p);
        t.sky_to_pixel(ra, dec)
    })
}

/// As [`check`], with any map of source pixels onto the target: the cubic a
/// stack resamples with is judged by the same rule as the plate solutions.
pub fn check_map(
    source: usize,
    source_arcsec_per_px: f64,
    stars: &[[f64; 2]],
    target: &Target,
    map: impl Fn([f64; 2]) -> Option<[f64; 2]>,
) -> Check {
    let t = &target.solution.wcs;
    let t_scale = target.solution.arcsec_per_px;
    let coarse = source_arcsec_per_px.max(t_scale);
    let coarse_px = coarse / t_scale;
    let radius = RADIUS_COARSE_PX * coarse_px;
    let mut inside = 0;
    // Nearest target star per carried star; each target star keeps its closest.
    // Ordered by target star, so sums run in the same order every time.
    let mut best: std::collections::BTreeMap<usize, Pair> = Default::default();
    for &p in stars {
        let Some(q) = map(p) else {
            continue;
        };
        if !t.inside(q, 0.) {
            continue;
        }
        inside += 1;
        if let Some((n, d)) = target.grid.nearest(q, radius) {
            if best.get(&n).is_none_or(|b| d < b.0) {
                let r = target.grid.points[n];
                best.insert(n, (d, r, [q[0] - r[0], q[1] - r[1]], p));
            }
        }
    }
    let clip = CLIP_COARSE_PX * coarse_px;
    let all: Vec<f64> = best.values().map(|b| b.0).collect();
    let kept: Vec<&Pair> = best.values().filter(|b| b.0 <= clip).collect();
    let kept_fraction = kept.len() as f64 / all.len().max(1) as f64;
    let mean = |v: &[&Pair]| {
        let n = v.len().max(1) as f64;
        let (sx, sy) = v.iter().fold((0., 0.), |s, b| (s.0 + b.2[0], s.1 + b.2[1]));
        (sx / n).hypot(sy / n)
    };
    let mean_offset = mean(&kept);
    let mut cells: Vec<Vec<&Pair>> = vec![vec![]; 9];
    for b in &kept {
        let cx = ((b.1[0] / t.width as f64 * 3.) as usize).min(2);
        let cy = ((b.1[1] / t.height as f64 * 3.) as usize).min(2);
        cells[cy * 3 + cx].push(b);
    }
    let cell_offsets: Vec<Option<f64>> = cells
        .iter()
        .map(|c| (c.len() >= CELL_MIN_PAIRS).then(|| mean(c)))
        .collect();
    let worst_cell = cell_offsets.iter().flatten().copied().fold(0., f64::max);
    let judged = cell_offsets.iter().flatten().count();
    let distances: Vec<f64> = kept.iter().map(|b| b.0).collect();
    let r = rms(&distances);
    let status = if all.len() < MIN_PAIRS || judged == 0 {
        "UNVERIFIED"
    } else if kept_fraction >= MIN_KEPT
        && mean_offset <= MAX_MEAN_COARSE_PX * coarse_px
        && worst_cell <= MAX_CELL_COARSE_PX * coarse_px
    {
        "PASS"
    } else {
        "FAIL"
    };
    Check {
        source,
        status,
        inside,
        matched: all.len(),
        kept_fraction,
        rms_target_px: r,
        rms_arcsec: r * t_scale,
        raw_rms_target_px: rms(&all),
        mean_offset_target_px: mean_offset,
        worst_cell_offset_target_px: worst_cell,
        judged_cells: judged,
        cell_offsets_target_px: cell_offsets,
        coarse_target_px: coarse_px,
        coarse_arcsec: coarse,
        pairs: kept.iter().map(|b| (b.3, b.1)).collect(),
    }
}

/// The verdict on held-out displacements (target star, carried position minus
/// target star) of a map fitted on the other pairs, by the same limits; the
/// pairing statistics stay those of the check that identified the pairs.
pub fn judge_held_out(
    identified: &Check,
    residuals: &[([f64; 2], [f64; 2])],
    width: usize,
    height: usize,
) -> Check {
    let mean = |v: &[&([f64; 2], [f64; 2])]| {
        let n = v.len().max(1) as f64;
        let (sx, sy) = v.iter().fold((0., 0.), |s, b| (s.0 + b.1[0], s.1 + b.1[1]));
        (sx / n).hypot(sy / n)
    };
    let all: Vec<&([f64; 2], [f64; 2])> = residuals.iter().collect();
    let mean_offset = mean(&all);
    let mut cells: Vec<Vec<&([f64; 2], [f64; 2])>> = vec![vec![]; 9];
    for b in &all {
        let cx = ((b.0[0] / width as f64 * 3.) as usize).min(2);
        let cy = ((b.0[1] / height as f64 * 3.) as usize).min(2);
        cells[cy * 3 + cx].push(b);
    }
    let cell_offsets: Vec<Option<f64>> = cells
        .iter()
        .map(|c| (c.len() >= CELL_MIN_PAIRS).then(|| mean(c)))
        .collect();
    let worst_cell = cell_offsets.iter().flatten().copied().fold(0., f64::max);
    let judged = cell_offsets.iter().flatten().count();
    let distances: Vec<f64> = all.iter().map(|b| b.1[0].hypot(b.1[1])).collect();
    let coarse_px = identified.coarse_target_px;
    let status = if all.len() < MIN_PAIRS || judged == 0 {
        "UNVERIFIED"
    } else if identified.kept_fraction >= MIN_KEPT
        && mean_offset <= MAX_MEAN_COARSE_PX * coarse_px
        && worst_cell <= MAX_CELL_COARSE_PX * coarse_px
    {
        "PASS"
    } else {
        "FAIL"
    };
    let r = rms(&distances);
    Check {
        status,
        rms_target_px: r,
        rms_arcsec: r * identified.rms_arcsec / identified.rms_target_px.max(1e-12),
        mean_offset_target_px: mean_offset,
        worst_cell_offset_target_px: worst_cell,
        judged_cells: judged,
        cell_offsets_target_px: cell_offsets,
        pairs: vec![],
        ..identified.clone()
    }
}
