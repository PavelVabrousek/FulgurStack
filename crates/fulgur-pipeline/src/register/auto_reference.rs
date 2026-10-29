//! Count-based, bounded catalog-only framing selection; never a pixel Coverage map.
use super::*;

const THRESHOLD: f64 = 0.8;
const WORLD_SIDE: usize = 1024;
const FRAME_SIDE: usize = 128;

#[derive(Debug, Serialize)]
pub struct AutoReferenceReport {
    pub method: &'static str,
    pub nights: usize,
    pub selected_frames: usize,
    pub mapped_frames: usize,
    pub candidate_count: usize,
    pub preliminary_reference: String,
    pub selected_reference: String,
    pub minimum_selected_frame_fraction: f64,
    pub predicted_rectangle_width_px: f64,
    pub predicted_rectangle_height_px: f64,
    pub predicted_rectangle_area_px: f64,
    pub median_reference_rectangle_area_px: f64,
    pub predicted_mean_selected_coverage: f64,
    pub median_reference_mean_selected_coverage: f64,
    pub world_grid_side_limit: usize,
    pub candidate_grid_side_limit: usize,
    pub preliminary_seconds: f64,
    pub scoring_seconds: f64,
    pub timing_scope: &'static str,
    pub prediction_rule: &'static str,
    /// The chosen reference mapped fewer Use-on frames than the preliminary
    /// one in final registration, so the preliminary reference was kept (ADR-085).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub kept_preliminary_reference: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_reference_mapped_frames: Option<usize>,
}

/// Only Use-on acquisition times define observing groups. Midnight is irrelevant;
/// Use-off timestamps and diagnostics cannot change the selection.
pub(super) fn nights(frames: &[QualityFrame]) -> Result<Vec<Vec<usize>>> {
    let mut indices: Vec<_> = frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.accepted)
        .map(|(i, _)| i)
        .collect();
    if indices.iter().any(|&i| timestamp(&frames[i]).is_none()) {
        return Err(Error::Invalid(
            "Automatic reference selection requires valid DATE-OBS on every Use-on frame; select a reference manually.",
        ));
    }
    indices.sort_by(|&a, &b| {
        timestamp(&frames[a])
            .cmp(&timestamp(&frames[b]))
            .then_with(|| frames[a].file_name.cmp(&frames[b].file_name))
    });
    let mut result: Vec<Vec<usize>> = vec![];
    let mut previous = None;
    for i in indices {
        let t = timestamp(&frames[i]).unwrap();
        if previous.is_none_or(|p| t - p > 6 * 3600 * 1_000_000) {
            result.push(vec![]);
        }
        result.last_mut().unwrap().push(i);
        previous = Some(t);
    }
    Ok(result)
}

struct CoverageGrid {
    origin: [f64; 2],
    step: f64,
    width: usize,
    height: usize,
    counts: Vec<u32>,
}
impl CoverageGrid {
    fn new(plan: &RegistrationReport, cancel: &AtomicBool) -> Result<Self> {
        let polygons: Vec<_> = plan
            .frames
            .iter()
            .filter(|f| f.quality_accepted && f.accepted)
            .map(|f| &f.footprint)
            .collect();
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in &polygons {
            for q in *p {
                bounds[0] = bounds[0].min(q[0]);
                bounds[1] = bounds[1].min(q[1]);
                bounds[2] = bounds[2].max(q[0]);
                bounds[3] = bounds[3].max(q[1]);
            }
        }
        let span = [bounds[2] - bounds[0], bounds[3] - bounds[1]];
        if !span.iter().all(|v| v.is_finite() && *v > 0.) {
            return Err(Error::Invalid(
                "Auto reference has no usable geometric footprints.",
            ));
        }
        let step = span[0].max(span[1]) / WORLD_SIDE as f64;
        let width = (span[0] / step).ceil().clamp(1., WORLD_SIDE as f64) as usize;
        let height = (span[1] / step).ceil().clamp(1., WORLD_SIDE as f64) as usize;
        let mut grid = Self {
            origin: [bounds[0], bounds[1]],
            step,
            width,
            height,
            counts: vec![0; width * height],
        };
        // Scan-convert convex conservative footprints, one frame at a time.
        // Every frame contributes one vote regardless of exposure or final weight.
        for polygon in polygons {
            check_cancelled(cancel)?;
            for y in 0..height {
                let yy = grid.origin[1] + (y as f64 + 0.5) * step;
                let mut left = f64::INFINITY;
                let mut right = f64::NEG_INFINITY;
                for i in 0..polygon.len() {
                    let a = polygon[i];
                    let b = polygon[(i + 1) % polygon.len()];
                    if (a[1] <= yy && yy < b[1]) || (b[1] <= yy && yy < a[1]) {
                        let x = a[0] + (yy - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                        left = left.min(x);
                        right = right.max(x);
                    }
                }
                if left < right {
                    let l = ((left - grid.origin[0]) / step - 0.5)
                        .ceil()
                        .clamp(0., width as f64) as usize;
                    let r = ((right - grid.origin[0]) / step - 0.5)
                        .ceil()
                        .clamp(0., width as f64) as usize;
                    for cell in &mut grid.counts[y * width + l..y * width + r] {
                        *cell += 1;
                    }
                }
            }
        }
        Ok(grid)
    }
    fn count(&self, p: [f64; 2]) -> u32 {
        let x = ((p[0] - self.origin[0]) / self.step).floor();
        let y = ((p[1] - self.origin[1]) / self.step).floor();
        if x < 0. || y < 0. || x >= self.width as f64 || y >= self.height as f64 {
            0
        } else {
            self.counts[y as usize * self.width + x as usize]
        }
    }
}

#[derive(Clone, Copy)]
struct Score {
    area: usize,
    mean: u64,
    width: usize,
    height: usize,
    representative: u64,
}

fn score(
    plan: &RegistrationReport,
    index: usize,
    grid: &CoverageGrid,
    cancel: &AtomicBool,
) -> Result<Score> {
    let row = &plan.frames[index];
    let info = plan.inspection.frames[index].frame.as_ref().unwrap();
    let w = (FRAME_SIDE as f64 * info.width as f64 / info.width.max(info.height) as f64)
        .round()
        .max(1.) as usize;
    let h = (FRAME_SIDE as f64 * info.height as f64 / info.width.max(info.height) as f64)
        .round()
        .max(1.) as usize;
    let model = row.geometric_correction.as_ref().and_then(|d| d.model);
    let t = row.transform.unwrap();
    let map = |p| model.map_or_else(|| t.apply(p), |m| m.forward(p));
    let mut heights = vec![0; w];
    let mut best = (0, 0, 0);
    let mut mean = 0;
    for y in 0..h {
        check_cancelled(cancel)?;
        for (x, height) in heights.iter_mut().enumerate() {
            let p = map([
                (x as f64 + 0.5) * info.width as f64 / w as f64 - 0.5,
                (y as f64 + 0.5) * info.height as f64 / h as f64 - 0.5,
            ]);
            let count = grid.count(p);
            mean += u64::from(count);
            if u64::from(count) * 5 >= plan.quality_accepted_count as u64 * 4 {
                *height += 1;
            } else {
                *height = 0;
            }
        }
        let mut stack: Vec<(usize, usize)> = vec![];
        for (x, height) in heights
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .enumerate()
        {
            let mut left = x;
            while stack.last().is_some_and(|(_, hh)| *hh > height) {
                let (l, hh) = stack.pop().unwrap();
                let area = (x - l) * hh;
                if area > best.0 {
                    best = (area, x - l, hh);
                }
                left = l;
            }
            if stack.last().is_none_or(|(_, hh)| *hh < height) {
                stack.push((left, height));
            }
        }
    }
    let center = map([
        (info.width as f64 - 1.) / 2.,
        (info.height as f64 - 1.) / 2.,
    ]);
    let diagonal = (info.width as f64).hypot(info.height as f64);
    let representative: f64 = plan
        .frames
        .iter()
        .filter(|r| r.quality_accepted && r.accepted)
        .map(|r| {
            let other = r.transform.unwrap();
            let other_model = r.geometric_correction.as_ref().and_then(|d| d.model);
            let p = other_model.map_or_else(
                || {
                    other.apply([
                        (info.width as f64 - 1.) / 2.,
                        (info.height as f64 - 1.) / 2.,
                    ])
                },
                |m| {
                    m.forward([
                        (info.width as f64 - 1.) / 2.,
                        (info.height as f64 - 1.) / 2.,
                    ])
                },
            );
            (other.theta_rad - t.theta_rad).sin().abs()
                + (p[0] - center[0]).hypot(p[1] - center[1]) / diagonal
        })
        .sum();
    Ok(Score {
        area: best.0,
        mean,
        width: best.1,
        height: best.2,
        // Suppress floating-roundoff differences between equivalent 180-degree
        // framings before the meaningful quality/filename tie-breakers.
        representative: (representative / plan.quality_accepted_count as f64 * 1e9).round() as u64,
    })
}

pub(super) fn choose(
    plan: &RegistrationReport,
    nights: &[Vec<usize>],
    cancel: &AtomicBool,
) -> Result<(usize, AutoReferenceReport)> {
    for group in nights {
        if !group.iter().any(|&i| plan.frames[i].accepted) {
            return Err(Error::Invalid(
                "Auto reference could not map every selected observing night; inspect registration or choose a reference manually.",
            ));
        }
    }
    let start = Instant::now();
    let grid = CoverageGrid::new(plan, cancel)?;
    let median = plan
        .frames
        .iter()
        .position(|f| f.file_name == plan.reference_anchor)
        .unwrap();
    let baseline = score(plan, median, &grid, cancel)?;
    let mut best = plan
        .frames
        .iter()
        .position(|f| f.quality_accepted && f.accepted)
        .ok_or(Error::Invalid("Auto reference has no accepted candidates."))?;
    let mut winning = score(plan, best, &grid, cancel)?;
    let mut candidates = 0;
    for (i, row) in plan.frames.iter().enumerate() {
        check_cancelled(cancel)?;
        if !row.quality_accepted || !row.accepted {
            continue;
        }
        candidates += 1;
        let s = score(plan, i, &grid, cancel)?;
        let f = &plan.inspection.frames[i];
        let b = &plan.inspection.frames[best];
        let better = s
            .area
            .cmp(&winning.area)
            .then(s.mean.cmp(&winning.mean))
            .then_with(|| winning.representative.cmp(&s.representative))
            .then_with(|| {
                b.hfr_px
                    .unwrap_or(f64::INFINITY)
                    .total_cmp(&f.hfr_px.unwrap_or(f64::INFINITY))
            })
            .then_with(|| f.stars.len().cmp(&b.stars.len()))
            .then_with(|| b.file_name.cmp(&f.file_name))
            .is_gt();
        if better {
            best = i;
            winning = s;
        }
    }
    if winning.area == 0 {
        return Err(Error::Invalid(
            "Auto reference found no rectangle covered by 80% of Use-on frames; inspect framing or select a reference manually.",
        ));
    }
    let info = plan.inspection.frames[best].frame.as_ref().unwrap();
    let w = (FRAME_SIDE as f64 * info.width as f64 / info.width.max(info.height) as f64)
        .round()
        .max(1.);
    let h = (FRAME_SIDE as f64 * info.height as f64 / info.width.max(info.height) as f64)
        .round()
        .max(1.);
    let px = info.width as f64 / w * info.height as f64 / h;
    Ok((
        best,
        AutoReferenceReport {
            method: "auto_multi_night_count_coverage",
            nights: nights.len(),
            selected_frames: plan.quality_accepted_count,
            mapped_frames: plan
                .frames
                .iter()
                .filter(|f| f.quality_accepted && f.accepted)
                .count(),
            candidate_count: candidates,
            preliminary_reference: plan.reference_anchor.clone(),
            selected_reference: plan.frames[best].file_name.clone(),
            minimum_selected_frame_fraction: THRESHOLD,
            predicted_rectangle_width_px: winning.width as f64 * info.width as f64 / w,
            predicted_rectangle_height_px: winning.height as f64 * info.height as f64 / h,
            predicted_rectangle_area_px: winning.area as f64 * px,
            median_reference_rectangle_area_px: baseline.area as f64 * px,
            predicted_mean_selected_coverage: winning.mean as f64
                / (w * h * plan.quality_accepted_count as f64),
            median_reference_mean_selected_coverage: baseline.mean as f64
                / (w * h * plan.quality_accepted_count as f64),
            world_grid_side_limit: WORLD_SIDE,
            candidate_grid_side_limit: FRAME_SIDE,
            preliminary_seconds: 0.,
            scoring_seconds: start.elapsed().as_secs_f64(),
            timing_scope: "preliminary catalog registration and scoring are within total registration; preliminary may also be the final reused native profile; do not sum overlapping scopes",
            prediction_rule: "approximate count-based geometric estimate on bounded cell-center grids, conservative nonlinear footprints; all Use-on frames in denominator, unmapped votes zero; not post-rejection Coverage and not an exact pixel optimum",
            kept_preliminary_reference: false,
            chosen_reference_mapped_frames: None,
        },
    ))
}
