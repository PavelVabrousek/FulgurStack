//! Sky registration (plan phase 3, ADR-137): every frame is plate-solved on
//! its own against Gaia DR3, and a frame registers on the reference through
//! the sky (source pixel -> sky -> reference pixel), whatever camera, optics,
//! filter or orientation took it. The engine never touches the network; the
//! caller passes the field centre and a catalogue provider.
//!
//! The result is an ordinary [`RegistrationReport`]: each registered frame
//! carries the composed map as a validated cubic [`crate::distortion::Warp`],
//! photometry from stars matched through the sky, and weights computed as the
//! star engine computes them. Frames that do not solve, fail the end-to-end
//! check or cannot be carried by the cubic go to the star engine in the same
//! run, on the same grid and brightness.
use super::*;
use crate::sky::catalogue::{GAIA_EPOCH, GaiaStar};
use crate::sky::check::{self, Check, Target};
use crate::sky::detect::SkyStar;
use crate::sky::solve::{self, BlindCache, Grid, Solution};
use crate::sky::{SkyFrame, batch, point_sources, warp};
use rayon::prelude::*;
use std::sync::Arc;

/// Gaia stars of a cone: centre RA and Dec, radius (degrees) and faintest G,
/// brightest first or in any order.
pub type CatalogueProvider =
    dyn Fn(f64, f64, f64, f64) -> std::result::Result<Vec<GaiaStar>, String> + Send + Sync;

/// What the sky engine needs beyond the star engine's options.
#[derive(Clone)]
pub struct SkyOptions {
    /// Field centre (RA, Dec in degrees) the blind search starts from.
    pub centre: (f64, f64),
    pub catalogue: Arc<CatalogueProvider>,
    /// A plate solution is kept when its held-out stars fit within this.
    pub max_solve_rms_px: f64,
    /// Further centres the master's plate solve tries when `centre` finds
    /// no solution, such as the frames' own OBJECT when a different object
    /// was chosen (ADR-158). Sky registration itself searches `centre` only.
    pub fallback_centres: Vec<(f64, f64)>,
}

impl SkyOptions {
    pub fn new(centre: (f64, f64), catalogue: Arc<CatalogueProvider>) -> Self {
        Self {
            centre,
            catalogue,
            max_solve_rms_px: 1.,
            fallback_centres: vec![],
        }
    }
}

/// Deep cone around the centre: the blind search and refinement.
const DEEP_RADIUS_DEG: f64 = 1.7;
const DEEP_G: f64 = 17.5;
/// Bright stars out to this radius, for wide fields and offset framing.
const WIDE_RADIUS_DEG: f64 = 6.5;
const WIDE_G: f64 = 13.5;
/// The exact composition of two plate solutions stacks only when one cubic
/// carries it within this (risk R1, spike S1).
const MAX_CARRIER_ERROR_PX: f64 = 0.05;
/// Folds of the cross-validation of a cubic fitted on star pairs.
const FOLDS: usize = 5;
/// Pairing radius for photometry, in pixels of the coarser of the two frames.
const PHOTOMETRY_RADIUS_COARSE_PX: f64 = 1.5;

/// One frame's sky registration, in the report.
#[derive(Clone, Debug, Serialize)]
pub struct SkyFrameReport {
    /// `sky`, or `stars` for a frame the star engine registered instead.
    pub engine: &'static str,
    pub solved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solve_error: Option<String>,
    /// 1 blind, 2 seeded by its camera, 3 from another solved centre.
    pub solve_pass: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arcsec_per_px: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out_rms_px: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degree: Option<usize>,
    pub catalogue_matches: usize,
    /// The check through the plate solutions, which identifies the stars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solution_check: Option<Check>,
    /// The check of the map the frame stacks with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<Check>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_max_error_px: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_rms_error_px: Option<f64>,
    pub mirrored_rows: bool,
}

/// The run as a whole.
#[derive(Clone, Debug, Serialize)]
pub struct SkyReport {
    pub centre_ra_deg: f64,
    pub centre_dec_deg: f64,
    pub catalogue_stars: usize,
    pub catalogue_seconds: f64,
    pub solve_seconds: f64,
    pub solve_rounds: Vec<batch::Round>,
    pub frames_to_solve: usize,
    pub solved: usize,
    pub reference: String,
    pub reference_arcsec_per_px: f64,
    pub photometric_anchor: String,
    pub registered_by_sky: usize,
    pub registered_by_stars: usize,
    /// Why the star engine was not asked, or what it could not do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_note: Option<String>,
    pub max_solve_rms_px: f64,
    pub check_rule: &'static str,
    /// The plate solution of the grid's frame, for the master's WCS (ADR-155).
    #[serde(skip)]
    pub grid_solution: Option<crate::astrometry::GridSolution>,
}

pub(crate) fn decimal_year(frame: &QualityFrame) -> Option<f64> {
    timestamp(frame).map(|t| 1970. + t as f64 / (365.25 * 86400. * 1e6))
}

/// The geometric reference: a solved frame of the stack or an outside file.
struct Geometry<'a> {
    name: String,
    width: usize,
    height: usize,
    solution: &'a Solution,
    points: Vec<[f64; 2]>,
}

/// Solutions and star lists shared by every mapping of one run.
struct Solved {
    solutions: Vec<Option<Solution>>,
    outcomes: Vec<batch::Outcome>,
    points: Vec<Vec<[f64; 2]>>,
    scheduled: Vec<bool>,
}

/// One frame mapped onto a reference, before the run is assembled.
struct Mapped {
    row: RegisteredFrame,
    mirrored: bool,
}

fn sky_record(solved: &Solved, i: usize) -> SkyFrameReport {
    let solution = solved.solutions[i].as_ref();
    SkyFrameReport {
        engine: "sky",
        solved: solution.is_some(),
        solve_error: solved.outcomes[i].solution.as_ref().err().cloned(),
        solve_pass: solved.outcomes[i].pass,
        arcsec_per_px: solution.map(|s| s.arcsec_per_px),
        held_out_rms_px: solution.map(|s| s.held_out_rms_px),
        degree: solution.map(|s| s.degree),
        catalogue_matches: solution.map_or(0, |s| s.matched),
        solution_check: None,
        check: None,
        carrier_max_error_px: None,
        carrier_rms_error_px: None,
        mirrored_rows: false,
    }
}

/// Inspection stars of `source` paired with those of `anchor` through both
/// plate solutions; each anchor star keeps its closest partner.
fn sky_matches(
    source: &QualityFrame,
    source_solution: &Solution,
    anchor_solution: &Solution,
    anchor_grid: &Grid,
) -> Vec<Match> {
    let coarse = source_solution
        .arcsec_per_px
        .max(anchor_solution.arcsec_per_px);
    let radius = PHOTOMETRY_RADIUS_COARSE_PX * coarse / anchor_solution.arcsec_per_px;
    let mut best: std::collections::BTreeMap<usize, Match> = Default::default();
    for (i, star) in source.stars.iter().enumerate() {
        let (ra, dec) = source_solution.wcs.pixel_to_sky([star.x_px, star.y_px]);
        let Some(q) = anchor_solution.wcs.sky_to_pixel(ra, dec) else {
            continue;
        };
        if let Some((n, d)) = anchor_grid.nearest(q, radius) {
            if best.get(&n).is_none_or(|m| d < m.residual_px) {
                best.insert(
                    n,
                    Match {
                        source: i,
                        reference: n,
                        residual_px: d,
                    },
                );
            }
        }
    }
    let mut matches: Vec<Match> = best.into_values().collect();
    matches.sort_by_key(|m| m.source);
    matches
}

/// Photometry and weight of a registered frame against the photometric
/// anchor, per anchor pixel: a star resampled onto the anchor's grid spreads
/// over the area its source pixels cover there.
struct Brightness {
    scale: f64,
    ratio: f64,
    transmission: f64,
    offset: f64,
    channels: Option<[f64; 3]>,
    noise: f64,
    weight: f64,
}

/// Median flux ratios anchor/source and source/anchor of bright-star pairs
/// below saturation in both frames, as `photometry` takes them from the
/// inspection catalogues.
fn flux_ratios(pairs: &[(SkyStar, SkyStar)], limits: [f64; 2]) -> Option<(f64, f64)> {
    let usable: Vec<(f64, f64)> = pairs
        .iter()
        .filter(|(s, a)| s[5] <= limits[0] && a[5] <= limits[1] && s[4] > 0. && a[4] > 0.)
        .map(|(s, a)| (s[4], a[4]))
        .collect();
    let mut forward: Vec<f64> = usable
        .iter()
        .map(|(s, a)| a / s)
        .filter(|v| v.is_finite())
        .collect();
    let mut backward: Vec<f64> = usable
        .iter()
        .map(|(s, a)| s / a)
        .filter(|v| v.is_finite())
        .collect();
    (forward.len() >= MIN_SKY_PHOTOMETRY_PAIRS)
        .then(|| (median(&mut forward), median(&mut backward)))
}

/// Bright stars of `source` paired with the anchor's through both plate
/// solutions; each anchor star keeps its closest partner.
fn sky_flux_pairs(
    source: &QualityFrame,
    source_solution: &Solution,
    anchor: &QualityFrame,
    anchor_solution: &Solution,
    anchor_grid: &Grid,
) -> Vec<(SkyStar, SkyStar)> {
    let coarse = source_solution
        .arcsec_per_px
        .max(anchor_solution.arcsec_per_px);
    let radius = PHOTOMETRY_RADIUS_COARSE_PX * coarse / anchor_solution.arcsec_per_px;
    let mut best: std::collections::BTreeMap<usize, (f64, usize)> = Default::default();
    for (i, star) in source.sky_stars.iter().enumerate() {
        let (ra, dec) = source_solution.wcs.pixel_to_sky([star[0], star[1]]);
        let Some(q) = anchor_solution.wcs.sky_to_pixel(ra, dec) else {
            continue;
        };
        if let Some((n, d)) = anchor_grid.nearest(q, radius) {
            if best.get(&n).is_none_or(|b| d < b.0) {
                best.insert(n, (d, i));
            }
        }
    }
    best.into_iter()
        .map(|(n, (_, i))| (source.sky_stars[i], anchor.sky_stars[n]))
        .collect()
}

/// Peaks above this share of the saturation level carry no brightness.
fn saturation_limit(frame: &QualityFrame) -> f64 {
    frame
        .frame
        .as_ref()
        .and_then(|i| i.saturation_adu)
        .map_or(62000., |s| 0.95 * f64::from(s))
}

/// Pairs of the bright-star lists one brightness scale needs.
const MIN_SKY_PHOTOMETRY_PAIRS: usize = 10;

fn brightness(
    frame: &QualityFrame,
    anchor: &QualityFrame,
    ratios: Option<(f64, f64)>,
    area: f64,
) -> std::result::Result<Brightness, &'static str> {
    let Some((per_star, ratio)) = ratios else {
        return Err("insufficient_photometric_matches");
    };
    let scale = per_star / area;
    let ratio = ratio * area;
    let exposure = frame.frame.as_ref().map_or(0., |f| f.exposure_seconds);
    let anchor_exposure = anchor.frame.as_ref().map_or(0., |f| f.exposure_seconds);
    let transmission = ratio * anchor_exposure / exposure;
    let offset =
        anchor.background_adu.unwrap_or(f64::NAN) - scale * frame.background_adu.unwrap_or(0.);
    let noise = scale * frame.noise_adu.unwrap_or(0.);
    // Star size in anchor pixels, so cameras of another plate scale compare.
    let hfr = frame.hfr_px.unwrap_or(0.) * area.sqrt();
    let weight = 1. / (noise * noise * hfr * hfr);
    if [scale, ratio, transmission, offset, noise, weight]
        .iter()
        .all(|v| v.is_finite())
        && noise > 0.
        && weight > 0.
    {
        Ok(Brightness {
            scale,
            ratio,
            transmission,
            offset,
            channels: channel_offsets(scale, frame, anchor),
            noise,
            weight,
        })
    } else {
        Err("invalid_photometric_normalization")
    }
}

fn validation(check: &Check) -> crate::distortion::FieldValidation {
    crate::distortion::FieldValidation {
        accepted: true,
        reason: "sky_end_to_end_check",
        fixed_stars: (check.matched as f64 * check.kept_fraction).round() as usize,
        expected_stars: check.inside,
        covered_stars: None,
        extended_witnesses: false,
        validation_stars: check.matched,
        rmse_px: Some(check.rms_target_px),
        validation_rmse_px: Some(check.mean_offset_target_px),
        maximum_error_px: Some(check.worst_cell_offset_target_px),
    }
}

fn decision(
    warp: crate::distortion::Warp,
    check: &Check,
    catalogue: usize,
) -> crate::distortion::Decision {
    crate::distortion::Decision {
        initial_correction_reason: None,
        fallback_failure: None,
        registration_validation: Some(validation(check)),
        status: "applied",
        reason: "sky_plate_solutions",
        training_stars: catalogue,
        validation_stars: check.matched,
        baseline_validation_rmse_px: None,
        corrected_validation_rmse_px: Some(check.mean_offset_target_px),
        baseline_outer_rmse_px: None,
        corrected_outer_rmse_px: Some(check.worst_cell_offset_target_px),
        model: Some(warp),
        candidate: None,
    }
}

/// A cubic on all star pairs (source in the carrier's input rows, reference),
/// and the check of cubics fitted without each fold on the fold they left out.
fn cross_validated(
    carrier: &warp::Carrier,
    pairs: &[([f64; 2], [f64; 2])],
    identified: &Check,
    size: [usize; 2],
) -> Option<(crate::distortion::Warp, Check)> {
    let inside = |p: [f64; 2]| {
        (0..2).all(|k| p[k] >= carrier.field_min[k] && p[k] <= carrier.field_max[k] - 1.)
    };
    let pairs: Vec<([f64; 2], [f64; 2])> =
        pairs.iter().copied().filter(|(p, _)| inside(*p)).collect();
    let whole = warp::fit_pairs(carrier, &pairs)?;
    let mut residuals = Vec::with_capacity(pairs.len());
    for fold in 0..FOLDS {
        let train: Vec<([f64; 2], [f64; 2])> = pairs
            .iter()
            .enumerate()
            .filter(|(k, _)| k % FOLDS != fold)
            .map(|(_, p)| *p)
            .collect();
        let w = warp::fit_pairs(carrier, &train)?;
        for (_, (p, q)) in pairs.iter().enumerate().filter(|(k, _)| k % FOLDS == fold) {
            let r = w.forward(*p);
            residuals.push((*q, [r[0] - q[0], r[1] - q[1]]));
        }
    }
    let checked = check::judge_held_out(identified, &residuals, size[0], size[1]);
    Some((whole, checked))
}

/// Every scheduled frame mapped onto `reference`, brightness against `anchor`.
fn map_rows(
    quality: &[QualityFrame],
    solved: &Solved,
    reference: &Geometry<'_>,
    anchor: usize,
    progress: Option<&RegistrationProgress<'_>>,
    cancel: &AtomicBool,
) -> Result<Vec<Mapped>> {
    let anchor_solution = solved.solutions[anchor].as_ref().ok_or(Error::Invalid(
        "The photometric anchor has no plate solution.",
    ))?;
    let anchor_grid = Grid::new(
        quality[anchor]
            .stars
            .iter()
            .map(|s| [s.x_px, s.y_px])
            .collect(),
        8.,
    );
    let anchor_sky_grid = Grid::new(
        quality[anchor]
            .sky_stars
            .iter()
            .map(|s| [s[0], s[1]])
            .collect(),
        8.,
    );
    let target = Target::new(reference.solution, &reference.points);
    let rectangle = [
        [0., 0.],
        [reference.width as f64 - 1., 0.],
        [reference.width as f64 - 1., reference.height as f64 - 1.],
        [0., reference.height as f64 - 1.],
    ];
    (0..quality.len())
        .into_par_iter()
        .map(|i| -> Result<Mapped> {
            check_cancelled(cancel)?;
            let f = &quality[i];
            let mut row = empty(f, 0, &reference.name);
            if !solved.scheduled[i] {
                return Ok(Mapped {
                    row,
                    mirrored: false,
                });
            }
            row.registration_attempted = true;
            let mut record = sky_record(solved, i);
            let mut mirrored = false;
            let refusal = 'map: {
                let Some(solution) = solved.solutions[i].as_ref() else {
                    break 'map Some("sky_plate_solve_failed");
                };
                // The plate solutions identify the stars both frames share.
                let identified = check::check(i, solution, &solved.points[i], &target);
                record.solution_check = Some(identified.clone());
                if identified.status == "UNVERIFIED" {
                    break 'map Some("sky_check_unverified");
                }
                if identified.kept_fraction < check::MIN_KEPT {
                    break 'map Some("sky_check_failed");
                }
                let Some(carrier) = warp::carry(solution, reference.solution) else {
                    break 'map Some("sky_map_not_representable");
                };
                record.carrier_max_error_px = Some(carrier.max_error_px);
                record.carrier_rms_error_px = Some(carrier.rms_error_px);
                // Carrier input rows: reversed for a mirror image.
                let height = f.frame.as_ref().map_or(0., |i| i.height as f64);
                let rows_of = |p: [f64; 2]| {
                    if carrier.mirrored_rows {
                        [p[0], height - 1. - p[1]]
                    } else {
                        p
                    }
                };
                let pairs: Vec<([f64; 2], [f64; 2])> = identified
                    .pairs
                    .iter()
                    .map(|(p, q)| (rows_of(*p), *q))
                    .collect();
                // The map integration resamples with: the exact composition of
                // the plate solutions where one cubic carries it (risk R1) and
                // the reference's stars confirm it; otherwise a cubic on the
                // star pairs, judged by cross-validation. The composition
                // comes first: on DS-A, M33 and M42 its masters were sharper
                // and rounder, though a cubic on the pairs often lies closer
                // to the reference's centroids (ADR-137).
                let inside = |p: [f64; 2]| {
                    (0..2)
                        .all(|k| p[k] >= carrier.field_min[k] && p[k] <= carrier.field_max[k] - 1.)
                };
                let composed = (carrier.max_error_px <= MAX_CARRIER_ERROR_PX).then(|| {
                    let w = carrier.warp;
                    let c = check::check_map(
                        i,
                        solution.arcsec_per_px,
                        &solved.points[i],
                        &target,
                        |p| {
                            let p = rows_of(p);
                            inside(p).then(|| w.forward(p))
                        },
                    );
                    ("sky_plate_solutions", w, c)
                });
                let chosen = match composed {
                    Some(c) if c.2.status == "PASS" => Some(c),
                    composed => cross_validated(
                        &carrier,
                        &pairs,
                        &identified,
                        [reference.width, reference.height],
                    )
                    .map(|(w, c)| ("sky_star_pairs", w, c))
                    .filter(|f| f.2.status == "PASS" || composed.is_none())
                    .or(composed),
                };
                let Some((method, chosen, checked)) = chosen else {
                    break 'map Some("sky_map_not_representable");
                };
                if checked.status != "PASS" {
                    record.check = Some(checked);
                    break 'map Some("sky_check_failed");
                }
                record.check = Some(checked.clone());
                let carrier = warp::Carrier {
                    warp: chosen,
                    ..carrier
                };
                let matches = sky_matches(f, solution, anchor_solution, &anchor_grid);
                let area = (solution.arcsec_per_px / anchor_solution.arcsec_per_px).powi(2);
                let t = carrier.warp.base;
                row.registration_success = true;
                row.method = Some(method);
                row.transform = Some(t);
                row.rotation_deg = Some(t.theta_rad.to_degrees());
                row.inlier_count = checked.matched;
                row.inlier_rmse_px = Some(checked.rms_target_px);
                row.geometric_correction = Some(decision(carrier.warp, &checked, solution.matched));
                mirrored = carrier.mirrored_rows;
                record.mirrored_rows = mirrored;
                // The inspection catalogues first, as the star engine measures
                // brightness; the longer bright-star lists when two cameras
                // share too few catalogue stars.
                let (ratios, provenance) = match photometry(f, &quality[anchor], &matches)
                    .zip(total_flux_ratio(f, &quality[anchor], &matches))
                {
                    Some(r) => (Some(r), "sky_matched_anchor_stars"),
                    None => {
                        let pairs = sky_flux_pairs(
                            f,
                            solution,
                            &quality[anchor],
                            anchor_solution,
                            &anchor_sky_grid,
                        );
                        let limits = [saturation_limit(f), saturation_limit(&quality[anchor])];
                        (flux_ratios(&pairs, limits), "sky_matched_bright_stars")
                    }
                };
                let found = brightness(f, &quality[anchor], ratios, area);
                if provenance == "sky_matched_anchor_stars" {
                    row.photometric_matches = matches;
                }
                match found {
                    Err(reason) => Some(reason),
                    Ok(b) => {
                        row.photometric_reference = Some(provenance);
                        row.photometric_scale = Some(b.scale);
                        row.matched_flux_ratio = Some(b.ratio);
                        row.exposure_corrected_transparency = Some(b.transmission);
                        row.background_offset_adu = Some(b.offset);
                        row.channel_offsets_adu = b.channels;
                        row.normalized_noise_adu = Some(b.noise);
                        let footprint =
                            geometry::intersection(carrier.warp.footprint(), &rectangle);
                        if footprint.len() < 3 {
                            row.registration_success = false;
                            break 'map Some("sky_map_outside_reference");
                        }
                        row.accepted = f.accepted;
                        if row.accepted {
                            row.final_weight = b.weight;
                            row.footprint = footprint;
                        }
                        None
                    }
                }
            };
            if let Some(reason) = refusal {
                row.rejection_reasons.push(reason.into());
            }
            row.sky = Some(record);
            if let Some(progress) = progress {
                progress.advance();
            }
            Ok(Mapped { row, mirrored })
        })
        .collect()
}

/// The chronological median of the frames, or the sharpest when some lack a
/// capture time.
fn median_or_sharpest(indices: &[usize], quality: &[QualityFrame]) -> Option<usize> {
    if indices.is_empty() {
        return None;
    }
    if indices.iter().all(|&i| timestamp(&quality[i]).is_some()) {
        return select_anchor(indices, quality).ok();
    }
    indices.iter().copied().min_by(|&a, &b| {
        let hfr = |i: usize| quality[i].hfr_px.unwrap_or(f64::INFINITY);
        hfr(a)
            .total_cmp(&hfr(b))
            .then_with(|| quality[a].file_name.cmp(&quality[b].file_name))
    })
}

/// Rows of frames the sky engine could not register, taken from a star-engine
/// run on the same grid and brought onto the sky run's photometric anchor.
#[allow(clippy::too_many_arguments)] // The run's context, as for the star engine.
fn star_fallback(
    rows: &mut [Mapped],
    inspection: &mut InspectReport,
    root: &Path,
    min_coverage: f64,
    reference: &Path,
    anchor: usize,
    options: RegistrationOptions,
    calibrations: &[Option<std::sync::Arc<crate::calibration::Calibration>>],
    cancel: &AtomicBool,
) -> Result<Option<String>> {
    let failed: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            inspection.frames[*i].accepted && m.row.registration_attempted && !m.row.accepted
        })
        .map(|(i, _)| i)
        .collect();
    if failed.is_empty() {
        return Ok(None);
    }
    // Only the failed frames, the reference and the anchor take part.
    let mut subset = inspection.clone();
    let reference_name = rows[anchor].row.match_reference.clone();
    for (i, frame) in subset.frames.iter_mut().enumerate() {
        let keep = failed.contains(&i) || i == anchor || frame.file_name == reference_name;
        if !keep && frame.accepted {
            frame.accepted = false;
            frame.rejection_reasons.push("registered_by_sky".into());
        }
    }
    subset.accepted_count = subset.frames.iter().filter(|f| f.accepted).count();
    let star_options = RegistrationOptions {
        geometric_correction: true,
        partial_frames: false,
        ..options
    };
    let stars = match register_inspected_with_calibration(
        subset,
        root.to_path_buf(),
        min_coverage,
        Some(reference),
        star_options,
        cancel,
        Instant::now(),
        None,
        calibrations,
    ) {
        Ok((report, _)) => report,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => return Ok(Some(format!("star engine found nothing: {error}"))),
    };
    let anchor_row = &stars.frames[anchor];
    let (Some(a), Some(base)) = (
        anchor_row.photometric_scale,
        anchor_row.background_offset_adu,
    ) else {
        return Ok(Some(
            "star engine did not register the photometric anchor; no frame taken".into(),
        ));
    };
    if !anchor_row.accepted || a <= 0. {
        return Ok(Some(
            "star engine did not register the photometric anchor; no frame taken".into(),
        ));
    }
    let base_channels = anchor_row.channel_offsets_adu.unwrap_or([base; 3]);
    let anchor_scale = anchor_row.transform.map_or(1., |t| t.scale_factor);
    let mut stars = stars;
    let mut taken = 0;
    for &i in &failed {
        let star_row = std::mem::replace(&mut stars.frames[i], empty(&inspection.frames[i], 0, ""));
        if !star_row.accepted {
            stars.frames[i] = star_row;
            continue;
        }
        let mut row = star_row;
        let (Some(scale), Some(offset), Some(noise), Some(t)) = (
            row.photometric_scale,
            row.background_offset_adu,
            row.normalized_noise_adu,
            row.transform,
        ) else {
            continue;
        };
        let channels = row.channel_offsets_adu.unwrap_or([offset; 3]);
        row.photometric_scale = Some(scale / a);
        row.background_offset_adu = Some((offset - base) / a);
        row.channel_offsets_adu = Some(std::array::from_fn(|c| {
            (channels[c] - base_channels[c]) / a
        }));
        let noise = noise / a;
        row.normalized_noise_adu = Some(noise);
        let hfr = inspection.frames[i].hfr_px.unwrap_or(0.) * t.scale_factor / anchor_scale;
        row.final_weight = 1. / (noise * noise * hfr * hfr);
        if !(row.final_weight.is_finite() && row.final_weight > 0.) {
            continue;
        }
        let mut record = rows[i].row.sky.take().unwrap_or(SkyFrameReport {
            engine: "stars",
            solved: false,
            solve_error: None,
            solve_pass: 0,
            arcsec_per_px: None,
            held_out_rms_px: None,
            degree: None,
            catalogue_matches: 0,
            solution_check: None,
            check: None,
            carrier_max_error_px: None,
            carrier_rms_error_px: None,
            mirrored_rows: false,
        });
        record.engine = "stars";
        row.rejection_reasons = rows[i].row.rejection_reasons.clone();
        row.rejection_reasons.retain(|r| !r.starts_with("sky_"));
        row.sky = Some(record);
        row.session_id = 0;
        // The star run may have reversed this frame's rows.
        inspection.frames[i] = stars.inspection.frames[i].clone();
        rows[i] = Mapped {
            row,
            mirrored: false,
        };
        taken += 1;
    }
    Ok(Some(format!(
        "star engine registered {taken} of {} frames the sky engine could not",
        failed.len()
    )))
}

/// Registration of an inspected stack through plate solutions of every frame.
/// The inspection must have kept its sky star lists (`InspectConfig::sky_stars`).
#[allow(clippy::too_many_arguments)] // As `register_inspected_with_calibration`, plus the sky.
pub fn register_sky(
    mut inspection: InspectReport,
    root: std::path::PathBuf,
    min_coverage: f64,
    reference: Option<&Path>,
    options: RegistrationOptions,
    sky: &SkyOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    calibrations: &[Option<std::sync::Arc<crate::calibration::Calibration>>],
) -> Result<(RegistrationReport, std::path::PathBuf)> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    check_cancelled(cancel)?;
    let mut profile = crate::registration_timing::Timer::new();
    let (selected, external) = resolve_reference(&inspection.frames, &root, reference)?;
    let quality = &inspection.frames;
    let scheduled: Vec<bool> = quality
        .iter()
        .map(|f| (f.accepted || options.diagnose_excluded) && f.frame.is_some())
        .collect();
    if !quality
        .iter()
        .zip(&scheduled)
        .any(|(f, s)| *s && !f.sky_stars.is_empty())
    {
        return Err(Error::Invalid(
            "Sky registration needs the inspection's sky star lists.",
        ));
    }
    let total = scheduled.iter().filter(|s| **s).count() + 2;
    let progress = progress.map(|observer| {
        observer(0, total);
        RegistrationProgress {
            completed: Mutex::new(0),
            total: AtomicUsize::new(total),
            observer,
        }
    });

    // Catalogue: a deep cone for the search, bright stars far out for wide fields.
    let catalogue_start = Instant::now();
    let (ra, dec) = sky.centre;
    let fetch = |radius, g| {
        (sky.catalogue)(ra, dec, radius, g).map_err(|e| Error::Io(std::io::Error::other(e)))
    };
    let mut stars = fetch(WIDE_RADIUS_DEG, WIDE_G)?;
    stars.extend(fetch(DEEP_RADIUS_DEG, DEEP_G)?);
    // The cones overlap. Local catalogue files carry no source ids, so a
    // star is known by its position and brightness.
    let mut seen = std::collections::HashSet::new();
    stars.retain(|s| seen.insert((s.ra.to_bits(), s.dec.to_bits(), s.g.to_bits())));
    stars.sort_by(|a, b| a.g.total_cmp(&b.g).then(a.source_id.cmp(&b.source_id)));
    if stars.len() < 50 {
        return Err(Error::Invalid(
            "The Gaia catalogue holds too few stars around the field centre.",
        ));
    }
    let catalogue_stars = stars.len();
    let catalogue_seconds = catalogue_start.elapsed().as_secs_f64();
    let cache = BlindCache::new(Arc::new(stars));
    profile.lap("sky_catalogue");
    check_cancelled(cancel)?;

    // Plate solutions, one blind search per camera and the rest seeded.
    let solve_start = Instant::now();
    let dated: Vec<f64> = quality.iter().filter_map(decimal_year).collect();
    let typical_epoch = if dated.is_empty() {
        GAIA_EPOCH
    } else {
        let mut d = dated.clone();
        median(&mut d)
    };
    let lists: Vec<Vec<[f64; 2]>> = quality
        .iter()
        .map(|f| f.sky_stars.iter().map(|s| [s[0], s[1]]).collect())
        .collect();
    let points: Vec<Vec<[f64; 2]>> = quality
        .iter()
        .map(|f| point_sources(&f.sky_stars))
        .collect();
    let order: Vec<usize> = (0..quality.len())
        .filter(|&i| scheduled[i] && !lists[i].is_empty())
        .collect();
    let frames: Vec<SkyFrame<'_>> = order
        .iter()
        .map(|&i| {
            let info = quality[i].frame.as_ref().unwrap();
            SkyFrame {
                name: &quality[i].file_name,
                width: info.width,
                height: info.height,
                epoch: decimal_year(&quality[i]).unwrap_or(typical_epoch),
                stars: &lists[i],
            }
        })
        .collect();
    let groups: Vec<String> = order
        .iter()
        .map(|&i| {
            let info = quality[i].frame.as_ref().unwrap();
            format!(
                "{}x{} {}",
                info.width,
                info.height,
                if info.mosaic == fulgur_core::Mosaic::Bayer {
                    "colour"
                } else {
                    "mono"
                }
            )
        })
        .collect();
    let (results, rounds) = batch::solve_all(
        &frames,
        &groups,
        &vec![None; frames.len()],
        sky.centre,
        &cache,
        sky.max_solve_rms_px,
    );
    drop(frames);
    let mut outcomes: Vec<batch::Outcome> = (0..quality.len())
        .map(|i| batch::Outcome {
            solution: Err(if scheduled[i] {
                "no sky star list".into()
            } else {
                "not scheduled".into()
            }),
            pass: 0,
            seconds: 0.,
        })
        .collect();
    for (&i, outcome) in order.iter().zip(results) {
        outcomes[i] = outcome;
    }
    let solutions: Vec<Option<Solution>> = outcomes
        .iter()
        .map(|o| o.solution.as_ref().ok().cloned())
        .collect();
    let solve_seconds = solve_start.elapsed().as_secs_f64();
    let solved = Solved {
        solutions,
        outcomes,
        points,
        scheduled,
    };
    let solved_count = solved.solutions.iter().filter(|s| s.is_some()).count();
    if let Some(progress) = &progress {
        progress.advance();
    }
    profile.lap("sky_plate_solutions");
    check_cancelled(cancel)?;

    // Reference and photometric anchor.
    let usable: Vec<usize> = (0..quality.len())
        .filter(|&i| quality[i].accepted && solved.solutions[i].is_some())
        .collect();
    let camera_groups = cameras::groups(quality, &[]);
    let anchor = match selected {
        Some(i) => {
            if solved.solutions[i].is_none() {
                return Err(Error::Invalid(
                    "The selected reference frame could not be plate-solved; choose another frame or the star registration.",
                ));
            }
            i
        }
        None => {
            // The camera with the most solved frames, its median frame.
            let best = camera_groups
                .iter()
                .map(|g| {
                    g.iter()
                        .copied()
                        .filter(|i| usable.contains(i))
                        .collect::<Vec<_>>()
                })
                .max_by_key(|g| g.len())
                .unwrap_or_default();
            median_or_sharpest(&best, quality).ok_or(Error::Invalid(
                "No quality-accepted frame could be plate-solved.",
            ))?
        }
    };
    let external_solution = match &external {
        Some(path) => {
            let plane = fulgur_core::fits::read_reference_plane(path, cancel)?;
            let (list, _) =
                crate::sky::detect::bright_stars(&plane.pixels, plane.width, plane.height, false);
            let positions: Vec<[f64; 2]> = list.iter().map(|s| [s[0], s[1]]).collect();
            let frame = SkyFrame {
                name: "external reference",
                width: plane.width,
                height: plane.height,
                epoch: typical_epoch,
                stars: &positions,
            };
            let mut hints = vec![sky.centre];
            if let Some(s) = solved.solutions[anchor].as_ref() {
                hints.push((s.centre_ra, s.centre_dec));
            }
            let solution = solve::solve(&frame, &hints, &cache, sky.max_solve_rms_px, None)
                .map_err(|_| Error::Invalid("The external reference could not be plate-solved."))?;
            Some((
                path.clone(),
                plane.width,
                plane.height,
                solution,
                point_sources(&list),
            ))
        }
        None => None,
    };
    let member = |i: usize| -> Geometry<'_> {
        let info = quality[i].frame.as_ref().unwrap();
        Geometry {
            name: quality[i].file_name.clone(),
            width: info.width,
            height: info.height,
            solution: solved.solutions[i].as_ref().unwrap(),
            points: solved.points[i].clone(),
        }
    };
    let geometry = match &external_solution {
        Some((path, width, height, solution, points)) => Geometry {
            name: path.to_string_lossy().into_owned(),
            width: *width,
            height: *height,
            solution,
            points: points.clone(),
        },
        None => member(anchor),
    };
    profile.lap("sky_reference");

    let mut rows = map_rows(
        quality,
        &solved,
        &geometry,
        anchor,
        progress.as_ref(),
        cancel,
    )?;
    let mut anchor = anchor;
    let mut geometry = geometry;
    let mut auto_selection = None;
    // Several nights of one camera: the star engine's automatic choice, on
    // the sky run's own footprints (ADR-085).
    if selected.is_none()
        && external.is_none()
        && options.reference_strategy == ReferenceStrategy::Auto
        && camera_groups.len() == 1
    {
        if let Ok(nights) = auto_reference::nights(quality) {
            if nights.len() > 1 {
                let preliminary_start = Instant::now();
                let plan = assemble(
                    inspection.clone(),
                    rows.iter().map(|m| clone_row(&m.row)).collect(),
                    &[],
                    &geometry,
                    anchor,
                    min_coverage,
                    options,
                    None,
                    start,
                    profile_snapshot(),
                    cancel,
                )?;
                let preliminary_seconds = preliminary_start.elapsed().as_secs_f64();
                if let Ok((winner, mut selection)) = auto_reference::choose(&plan, &nights, cancel)
                {
                    selection.preliminary_seconds = preliminary_seconds;
                    if winner != anchor && solved.solutions[winner].is_some() {
                        let mapped =
                            |rows: &[Mapped]| rows.iter().filter(|m| m.row.accepted).count();
                        let chosen =
                            map_rows(quality, &solved, &member(winner), winner, None, cancel)?;
                        if mapped(&chosen) < mapped(&rows) {
                            selection.kept_preliminary_reference = true;
                            selection.chosen_reference_mapped_frames = Some(mapped(&chosen));
                        } else {
                            rows = chosen;
                            anchor = winner;
                            geometry = member(winner);
                        }
                    }
                    auto_selection = Some(selection);
                }
            }
        }
    }
    profile.lap("sky_mapping");

    // Frames the sky could not register go to the star engine on the same grid.
    let reference_path = match &external {
        Some(path) => path.clone(),
        None => root.join(&quality[anchor].file_name),
    };
    let geometry_name = geometry.name.clone();
    let reference_size = [geometry.width, geometry.height];
    let reference_scale = geometry.solution.arcsec_per_px;
    let grid_epoch = match &external {
        Some(_) => typical_epoch,
        None => decimal_year(&quality[anchor]).unwrap_or(typical_epoch),
    };
    let grid_solution = crate::astrometry::GridSolution::new(
        geometry_name.clone(),
        "sky registration",
        geometry.solution,
        grid_epoch,
    );
    drop(geometry);
    // Frames of another filter that share no stars with the photometric
    // anchor, through an anchor of their own light set (ADR-156), before the
    // star engine would try them again on the same anchor.
    let rectangle = [
        [0., 0.],
        [reference_size[0] as f64 - 1., 0.],
        [reference_size[0] as f64 - 1., reference_size[1] as f64 - 1.],
        [0., reference_size[1] as f64 - 1.],
    ];
    let labels: Vec<String> = inspection
        .light_sets
        .iter()
        .map(|s| s.label.clone())
        .collect();
    let witness = WitnessCatalog {
        root: &root,
        calibrations,
    };
    let anchor_scale = solved.solutions[anchor].as_ref().map(|s| s.arcsec_per_px);
    let set_brightness = set_photometry::rescue(
        &mut rows.iter_mut().map(|m| &mut m.row).collect::<Vec<_>>(),
        &inspection.frames,
        anchor,
        Some(&witness),
        &|row| {
            row.geometric_correction
                .as_ref()
                .and_then(|d| d.model)
                .map(|warp| geometry::intersection(warp.footprint(), &rectangle))
                .unwrap_or_default()
        },
        &|i| {
            solved.solutions[i]
                .as_ref()
                .zip(anchor_scale)
                .map_or(1., |(s, a)| (s.arcsec_per_px / a).powi(2))
        },
        &labels,
        cancel,
    );
    let fallback_note = star_fallback(
        &mut rows,
        &mut inspection,
        &root,
        min_coverage,
        &reference_path,
        anchor,
        options,
        calibrations,
        cancel,
    )?;
    profile.lap("star_engine_fallback");
    let registered_by_sky = rows
        .iter()
        .filter(|m| m.row.accepted && m.row.sky.as_ref().is_some_and(|s| s.engine == "sky"))
        .count();
    let registered_by_stars = rows
        .iter()
        .filter(|m| m.row.accepted && m.row.sky.as_ref().is_some_and(|s| s.engine == "stars"))
        .count();
    let summary = SkyReport {
        centre_ra_deg: ra,
        centre_dec_deg: dec,
        catalogue_stars,
        catalogue_seconds,
        solve_seconds,
        solve_rounds: rounds,
        frames_to_solve: order.len(),
        solved: solved_count,
        reference: geometry_name.clone(),
        reference_arcsec_per_px: reference_scale,
        photometric_anchor: inspection.frames[anchor].file_name.clone(),
        registered_by_sky,
        registered_by_stars,
        fallback_note,
        max_solve_rms_px: sky.max_solve_rms_px,
        check_rule: "the plate solutions pair the source's point sources with the reference's own (at least 15 pairs, half of them within 1 px of the coarser frame); a cubic fitted on those pairs, or the exact composition, is kept when the stars carried through it show a systematic displacement <= 0.15 px over the frame and <= 0.3 px in every judged 3x3 cell",
        grid_solution: Some(grid_solution),
    };
    let mirrored: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, m)| m.mirrored && m.row.registration_success)
        .map(|(i, _)| i)
        .collect();
    let rows: Vec<RegisteredFrame> = rows.into_iter().map(|m| m.row).collect();
    let mut report = assemble(
        inspection,
        rows,
        &mirrored,
        &Geometry {
            name: geometry_name,
            width: reference_size[0],
            height: reference_size[1],
            solution: solved.solutions[anchor].as_ref().unwrap(),
            points: vec![],
        },
        anchor,
        min_coverage,
        options,
        Some(summary),
        start,
        profile.finish(),
        cancel,
    )?;
    report.set_photometry = set_brightness;
    if let Some(selection) = auto_selection {
        report.reference_selection =
            "sky registration: automatic multi-night quality, plate scale and coverage";
        report.auto_reference = Some(selection);
    }
    if let Some(progress) = &progress {
        progress.advance();
    }
    Ok((report, root))
}

fn profile_snapshot() -> crate::registration_timing::PhaseProfile {
    crate::registration_timing::Timer::new().finish()
}

fn clone_row(row: &RegisteredFrame) -> RegisteredFrame {
    RegisteredFrame {
        geometric_correction: row.geometric_correction.clone(),
        source_to_photometric_anchor_correction: row.source_to_photometric_anchor_correction,
        file_name: row.file_name.clone(),
        session_id: row.session_id,
        quality_accepted: row.quality_accepted,
        registration_attempted: row.registration_attempted,
        registration_success: row.registration_success,
        accepted: row.accepted,
        rejection_reasons: row.rejection_reasons.clone(),
        method: row.method,
        transform: row.transform,
        rotation_deg: row.rotation_deg,
        source_to_photometric_anchor: row.source_to_photometric_anchor,
        inlier_count: row.inlier_count,
        inlier_rmse_px: row.inlier_rmse_px,
        matches: row.matches.clone(),
        photometric_matches: row.photometric_matches.clone(),
        match_reference: row.match_reference.clone(),
        photometric_reference: row.photometric_reference,
        matched_flux_ratio: row.matched_flux_ratio,
        exposure_corrected_transparency: row.exposure_corrected_transparency,
        photometric_scale: row.photometric_scale,
        background_offset_adu: row.background_offset_adu,
        channel_offsets_adu: row.channel_offsets_adu,
        partial_field: None,
        normalized_noise_adu: row.normalized_noise_adu,
        final_weight: row.final_weight,
        footprint: row.footprint.clone(),
        sky: row.sky.clone(),
    }
}

/// The report of a sky run: weights normalized, canvases from the footprints.
#[allow(clippy::too_many_arguments)] // The run's parts, as the star engine's report.
fn assemble(
    mut inspection: InspectReport,
    mut frames: Vec<RegisteredFrame>,
    mirrored: &[usize],
    reference: &Geometry<'_>,
    anchor: usize,
    min_coverage: f64,
    options: RegistrationOptions,
    sky: Option<SkyReport>,
    start: Instant,
    native: crate::registration_timing::PhaseProfile,
    cancel: &AtomicBool,
) -> Result<RegistrationReport> {
    for &i in mirrored {
        cameras::mirror(&mut inspection.frames[i]);
    }
    let quality = &inspection.frames;
    let max = frames.iter().map(|r| r.final_weight).fold(0., f64::max);
    if max <= 0. {
        return Err(Error::Invalid(
            "No registered frames with usable final weights.",
        ));
    }
    for r in &mut frames {
        r.final_weight /= max;
    }
    let polygons: Vec<_> = frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| (f.footprint.clone(), f.final_weight))
        .collect();
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    let common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    let coverage_optimized = geometry::canvas(&polygons, min_coverage, cancel)?;
    let accepted_count = frames.iter().filter(|r| r.accepted).count();
    let accepted_exposure_seconds = frames
        .iter()
        .zip(quality)
        .filter(|(r, _)| r.accepted)
        .map(|(_, f)| f.frame.as_ref().map_or(0., |i| i.exposure_seconds))
        .sum();
    let groups = cameras::groups(quality, &[]);
    let camera_groups = if groups.len() > 1 {
        groups
            .iter()
            .map(|members| {
                let info = quality[members[0]].frame.as_ref();
                cameras::CameraGroup {
                    width: info.map_or(0, |i| i.width),
                    height: info.map_or(0, |i| i.height),
                    frames: members.len(),
                    stacked: members.iter().filter(|&&i| frames[i].accepted).count(),
                    anchor: None,
                    brightness_scale: None,
                    shared_stars: 0,
                    refused: None,
                    mirrored: members.iter().any(|i| mirrored.contains(i)),
                    bridge_frame: None,
                }
            })
            .collect()
    } else {
        vec![]
    };
    let wall_seconds = start.elapsed().as_secs_f64();
    Ok(RegistrationReport {
        auto_reference: None,
        geometric_correction_enabled: true,
        accuracy: options.accuracy,
        schema_version: 2,
        command: "register",
        reference_anchor: quality[anchor].file_name.clone(),
        reference_selection: "sky registration: chronological median of the camera with the most plate-solved frames, or the selected frame",
        reference_date_obs: quality[anchor]
            .frame
            .as_ref()
            .and_then(|f| f.date_obs.clone()),
        reference_width: reference.width,
        reference_height: reference.height,
        external_reference: None,
        camera_groups,
        frame_count: frames.len(),
        quality_accepted_count: inspection.accepted_count,
        diagnose_excluded: options.diagnose_excluded,
        registration_attempted_count: frames.iter().filter(|r| r.registration_attempted).count(),
        registration_skipped_count: frames.iter().filter(|r| !r.registration_attempted).count(),
        registration_success_count: frames.iter().filter(|r| r.registration_success).count(),
        accepted_count,
        rejected_count: frames.len() - accepted_count,
        accepted_exposure_seconds,
        wall_seconds,
        inspection_seconds: inspection.wall_seconds,
        registration_seconds: wall_seconds - inspection.wall_seconds,
        phase_profile: RegistrationProfile {
            native,
            native_fit_details: Default::default(),
            external: None,
        },
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        intermediate_images_written: 0,
        transform_convention: "source integer pixel centers -> reference: validated cubic on star pairs the plate solutions identify, or the cubic carrier of the composed plate solutions (source pixel -> sky -> reference pixel); its similarity base as the transform; zero-based top-down",
        session_rule: "sky registration: one session; every frame solved on its own",
        photometry_rule: "y=a*x+b[c] per colour channel c in source ADU; a=median(anchor_flux/source_flux)/area, area=(source arcsec per px / anchor arcsec per px)^2; stars paired through both plate solutions; b[c]=anchor_sky[c]-a*source_sky[c]",
        weights_status: "final: inverse(normalized_noise^2 * HFR^2), HFR in anchor pixels, maximum accepted weight=1",
        sessions: vec![Session {
            id: 0,
            anchor: quality[anchor].file_name.clone(),
            frame_count: frames.len(),
            to_master: Some(Transform::IDENTITY),
            anchor_inliers: 0,
            anchor_rmse_px: None,
            matches: vec![],
        }],
        common_intersection_polygon: common,
        common_intersection,
        coverage_optimized,
        min_coverage,
        frames,
        sky,
        set_photometry: vec![],
        inspection,
    })
}

/// Light sets inspected with their sky star lists, then registered through
/// the sky. `sky_for` turns the inspection into the field centre and the
/// catalogue (the object name may come from the headers).
pub fn register_sets_sky(
    sets: &[crate::inspect::InputSet],
    min_coverage: f64,
    reference: Option<&Path>,
    options: RegistrationOptions,
    sky_for: &dyn Fn(&InspectReport) -> Result<SkyOptions>,
    cancel: &AtomicBool,
) -> Result<(RegistrationReport, std::path::PathBuf)> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    let start = Instant::now();
    let config = InspectConfig {
        sky_stars: true,
        ..InspectConfig::default()
    };
    let (inspection, root) = crate::inspect::inspect_sets(sets, config, cancel, None)?;
    let sky = sky_for(&inspection)?;
    let calibrations: Vec<_> = sets.iter().map(|s| s.calibration.clone()).collect();
    register_sky(
        inspection,
        root,
        min_coverage,
        reference,
        options,
        &sky,
        cancel,
        start,
        None,
        &calibrations,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_carrier(size: [f64; 2]) -> warp::Carrier {
        warp::Carrier {
            warp: crate::distortion::Warp {
                base: Transform::IDENTITY,
                center: [(size[0] - 1.) / 2., (size[1] - 1.) / 2.],
                normalization: [(size[0] - 1.) / 2., (size[1] - 1.) / 2.],
                coefficients: [[0.; 10]; 2],
            },
            mirrored_rows: false,
            field_min: [0., 0.],
            field_max: size,
            max_error_px: 0.,
            rms_error_px: 0.,
        }
    }

    fn identified() -> Check {
        Check {
            source: 0,
            status: "PASS",
            inside: 900,
            matched: 900,
            kept_fraction: 0.9,
            rms_target_px: 0.3,
            rms_arcsec: 0.3,
            raw_rms_target_px: 0.3,
            mean_offset_target_px: 0.,
            worst_cell_offset_target_px: 0.,
            judged_cells: 9,
            cell_offsets_target_px: vec![],
            coarse_target_px: 1.,
            coarse_arcsec: 1.,
            pairs: vec![],
        }
    }

    /// Pairs on a grid with centroid scatter; `step` shifts the stars of the
    /// central cell by 1.5 px, which no cubic over the field can follow.
    fn pairs(step: bool) -> Vec<([f64; 2], [f64; 2])> {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.4
        };
        let mut out = vec![];
        for j in 0..30 {
            for i in 0..30 {
                let p = [20. + i as f64 * 100., 20. + j as f64 * 66.];
                let central = (1000. ..2000.).contains(&p[0]) && (667. ..1333.).contains(&p[1]);
                let shift = if step && central { 1.5 } else { 0. };
                out.push((p, [p[0] + 3. + shift + noise(), p[1] - 2. + noise()]));
            }
        }
        out
    }

    #[test]
    fn a_cubic_on_star_pairs_is_judged_on_the_pairs_it_left_out() {
        let carrier = identity_carrier([3000., 2000.]);
        let (warp, good) =
            cross_validated(&carrier, &pairs(false), &identified(), [3000, 2000]).unwrap();
        assert_eq!(good.status, "PASS", "{good:?}");
        let r = warp.forward([1500., 1000.]);
        assert!(
            (r[0] - 1503.).abs() < 0.05 && (r[1] - 998.).abs() < 0.05,
            "{r:?}"
        );
        // Held out, the cell the cubic smooths over shows.
        let (_, stepped) =
            cross_validated(&carrier, &pairs(true), &identified(), [3000, 2000]).unwrap();
        assert_eq!(stepped.status, "FAIL", "{stepped:?}");
        assert!(stepped.worst_cell_offset_target_px > 0.3);
    }
}
