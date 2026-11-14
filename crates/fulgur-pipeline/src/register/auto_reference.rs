//! Catalog-only reference selection: framing as a limit, then quality and plate
//! scale. Framing stays a count-based estimate, never a pixel Coverage map.
use super::*;

const THRESHOLD: f64 = 0.8;
const WORLD_SIDE: usize = 1024;
const FRAME_SIDE: usize = 128;
/// A reference may give up this share of the best shared rectangle (ADR-119).
const AREA_TOLERANCE: f64 = 0.97;
/// Frames beyond this quantile of a quality metric do not anchor the stack.
const QUALITY_QUANTILE: f64 = 0.75;
/// Scale offsets this close to the closest candidate's count as equal.
const SCALE_TOLERANCE: f64 = 50e-6;
const SCALE_MATCH_RADIUS_PX: f64 = 8.;
const SCALE_MIN_PAIRS: usize = 20;

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
    pub selection_rule: &'static str,
    /// Largest plate-scale difference among mapped frames.
    pub plate_scale_spread_ppm: f64,
    /// Distance of the chosen reference from the median plate scale.
    pub selected_scale_offset_ppm: Option<f64>,
    pub candidates_after_filters: usize,
    /// The chosen reference mapped fewer Use-on frames than the preliminary
    /// one in final registration, so the preliminary reference was kept (ADR-085).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub kept_preliminary_reference: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_reference_mapped_frames: Option<usize>,
    /// Use-on frames that final registration could not map (ADR-107).
    #[serde(skip_serializing_if = "is_zero")]
    pub unmapped_frames: usize,
    /// Observing nights without any mapped frame.
    #[serde(skip_serializing_if = "is_zero")]
    pub unmapped_nights: usize,
    /// No rectangle reached the threshold; the preliminary reference stayed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub no_common_rectangle: bool,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl AutoReferenceReport {
    /// Owner warning when the stack leaves registered-looking frames out.
    pub fn warning(&self) -> Option<String> {
        (self.unmapped_frames > 0).then(|| {
            let nights = match self.unmapped_nights {
                0 => String::new(),
                1 => ", including one whole night".to_owned(),
                n => format!(", including {n} whole nights"),
            };
            format!(
                "{} of {} frames could not be registered and stay out of the stack{nights}",
                self.unmapped_frames, self.selected_frames
            )
        })
    }
}

/// Only Use-on acquisition times define observing groups. Midnight is irrelevant;
/// Use-off timestamps and diagnostics cannot change the selection.
/// More than this between two accepted frames starts a new observing session
/// (microseconds; six hours, independent of midnight).
pub(super) const SESSION_GAP_MICROS: i64 = 6 * 3600 * 1_000_000;

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
        if previous.is_none_or(|p| t - p > SESSION_GAP_MICROS) {
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
    /// Frames voting: only mapped frames can cover anything.
    frames: usize,
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
        let frames = polygons.len();
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
            frames,
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
            if u64::from(count) * 5 >= grid.frames as u64 * 4 {
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

/// Plate scale of a frame against the preliminary anchor, from its own stars.
/// Rigid rows report scale 1 whatever the night's true scale was (ADR-119).
fn plate_scale(
    source: &[crate::stars::Star],
    anchor: &[crate::stars::Star],
    t: Transform,
) -> Option<f64> {
    let mut pairs = geometry::correspondences(source, anchor, t, SCALE_MATCH_RADIUS_PX);
    if pairs.len() < SCALE_MIN_PAIRS {
        return None;
    }
    let first = geometry::procrustes(source, anchor, &pairs, true)?;
    let residual = |m: &Match| {
        let s = &source[m.source];
        let r = &anchor[m.reference];
        let q = first.apply([s.x_px, s.y_px]);
        (q[0] - r.x_px).hypot(q[1] - r.y_px)
    };
    let mut errors: Vec<_> = pairs.iter().map(residual).collect();
    let limit = (median(&mut errors) * 3.).max(1.5);
    pairs.retain(|m| residual(m) <= limit);
    if pairs.len() < SCALE_MIN_PAIRS {
        return None;
    }
    geometry::procrustes(source, anchor, &pairs, true).map(|fit| fit.scale_factor)
}

fn quantile(mut values: Vec<f64>, q: f64) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    (!values.is_empty()).then(|| values[((values.len() - 1) as f64 * q).round() as usize])
}

struct Candidate {
    index: usize,
    score: Score,
    theta: f64,
    scale: Option<f64>,
}

/// Narrows the pool, unless that would leave no candidate at all.
fn narrow(pool: &mut Vec<Candidate>, keep: impl Fn(&Candidate) -> bool) {
    if pool.iter().any(&keep) {
        pool.retain(|c| keep(c));
    }
}

pub(super) fn choose(
    plan: &RegistrationReport,
    nights: &[Vec<usize>],
    cancel: &AtomicBool,
) -> Result<(usize, AutoReferenceReport)> {
    use rayon::prelude::*;
    let start = Instant::now();
    let grid = CoverageGrid::new(plan, cancel)?;
    let frames = &plan.inspection.frames;
    let preliminary = plan
        .frames
        .iter()
        .position(|f| f.file_name == plan.reference_anchor)
        .unwrap();
    let baseline = score(plan, preliminary, &grid, cancel)?;
    let mut pool = vec![];
    for (index, row) in plan.frames.iter().enumerate() {
        check_cancelled(cancel)?;
        if row.quality_accepted && row.accepted {
            pool.push(Candidate {
                index,
                score: score(plan, index, &grid, cancel)?,
                theta: row.transform.unwrap().theta_rad,
                scale: None,
            });
        }
    }
    if pool.is_empty() {
        return Err(Error::Invalid("Auto reference has no accepted candidates."));
    }
    let candidates = pool.len();
    let anchor = &frames[preliminary].stars;
    pool.par_iter_mut().for_each(|c| {
        c.scale = plate_scale(
            &frames[c.index].stars,
            anchor,
            plan.frames[c.index].transform.unwrap(),
        );
    });
    // All mapped frames set the limits; the pool only narrows afterwards.
    let thetas: Vec<_> = pool.iter().map(|c| c.theta).collect();
    let metric = |value: fn(&QualityFrame) -> Option<f64>, q| {
        quantile(
            pool.iter()
                .filter_map(|c| value(&frames[c.index]))
                .collect(),
            q,
        )
    };
    let hfr_limit = metric(|f| f.hfr_px, QUALITY_QUANTILE);
    let eccentricity_limit = metric(|f| f.eccentricity, QUALITY_QUANTILE);
    let star_limit = metric(|f| Some(f.star_count as f64), 1. - QUALITY_QUANTILE);
    let mut scales: Vec<_> = pool.iter().filter_map(|c| c.scale).collect();
    let spread = scales.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - scales.iter().copied().fold(f64::INFINITY, f64::min);
    let scale_median = (!scales.is_empty()).then(|| median(&mut scales));
    let best_area = pool.iter().map(|c| c.score.area).max().unwrap_or(0);
    // Scattered framing shares no such rectangle: stack on the preliminary reference.
    let no_common_rectangle = best_area == 0;
    narrow(&mut pool, |c| {
        c.score.area as f64 >= best_area as f64 * AREA_TOLERANCE
    });
    // A frame on the minority side of a meridian flip faces most of the stack
    // with its distortion reversed.
    narrow(&mut pool, |c| {
        thetas.iter().filter(|t| (**t - c.theta).cos() > 0.).count() * 2 >= thetas.len()
    });
    let within = |value: Option<f64>, limit: Option<f64>| {
        limit.is_none_or(|l| value.is_some_and(|v| v <= l))
    };
    narrow(&mut pool, |c| {
        let f = &frames[c.index];
        within(f.hfr_px, hfr_limit)
            && within(f.eccentricity, eccentricity_limit)
            && star_limit.is_none_or(|l| f.star_count as f64 >= l)
    });
    if let Some(m) = scale_median {
        let closest = pool
            .iter()
            .filter_map(|c| c.scale)
            .map(|s| (s - m).abs())
            .fold(f64::INFINITY, f64::min);
        narrow(&mut pool, |c| {
            c.scale
                .is_some_and(|s| (s - m).abs() <= closest + SCALE_TOLERANCE)
        });
    }
    // Sum of ranks, so that no single metric's units decide.
    let worse = |value: Option<f64>| value.unwrap_or(f64::INFINITY);
    let rank = |c: &Candidate| -> usize {
        let f = &frames[c.index];
        pool.iter()
            .map(|o| {
                let g = &frames[o.index];
                usize::from(worse(g.hfr_px) < worse(f.hfr_px))
                    + usize::from(worse(g.eccentricity) < worse(f.eccentricity))
                    + usize::from(g.star_count > f.star_count)
            })
            .sum()
    };
    let chosen = pool
        .iter()
        .min_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then(a.score.representative.cmp(&b.score.representative))
                .then(b.score.mean.cmp(&a.score.mean))
                .then_with(|| frames[a.index].file_name.cmp(&frames[b.index].file_name))
        })
        .unwrap();
    let (best, winning, scale) = if no_common_rectangle {
        (preliminary, baseline, None)
    } else {
        (chosen.index, chosen.score, chosen.scale)
    };
    let info = frames[best].frame.as_ref().unwrap();
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
            method: "auto_multi_night_quality_scale_coverage",
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
            prediction_rule: "approximate count-based geometric estimate on bounded cell-center grids, conservative nonlinear footprints; rectangle threshold over mapped frames, mean coverage over all Use-on frames; not post-rejection Coverage and not an exact pixel optimum",
            selection_rule: "within 3% of the best rectangle, then the majority side of a meridian flip, then no worst quartile of HFR, eccentricity or star count, then closest to the median plate scale, then best rank sum of those three metrics",
            plate_scale_spread_ppm: if spread.is_finite() { spread * 1e6 } else { 0. },
            selected_scale_offset_ppm: scale.zip(scale_median).map(|(s, m)| (s - m).abs() * 1e6),
            candidates_after_filters: pool.len(),
            kept_preliminary_reference: false,
            chosen_reference_mapped_frames: None,
            unmapped_frames: 0,
            unmapped_nights: 0,
            no_common_rectangle,
        },
    ))
}
