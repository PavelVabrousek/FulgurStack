//! External geometry-only FITS references; scientific photometry stays in the native anchor.
use crate::{geometry, register::RegistrationReport, stars};
use fulgur_core::{Error, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::{path::Path, sync::atomic::AtomicBool, time::Instant};

#[derive(Debug, Serialize)]
pub struct ReferenceStar {
    pub x_px: f64,
    pub y_px: f64,
    pub snr: f64,
}
#[derive(Debug, Serialize)]
pub struct ExternalReferenceReport {
    pub path: String,
    pub width: usize,
    pub height: usize,
    pub photometric_anchor: String,
    pub geometry_only: bool,
    pub scale_range: [f64; 2],
    pub anchor_to_reference: geometry::Transform,
    pub anchor_matches: Vec<geometry::Match>,
    pub anchor_rmse_px: f64,
    pub stars: Vec<ReferenceStar>,
    pub pixel_rule: &'static str,
}

pub(crate) fn align(
    report: &mut RegistrationReport,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    align_with_progress(report, path, cancel, None)
}

pub(crate) fn align_with_progress(
    report: &mut RegistrationReport,
    path: &Path,
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
) -> Result<()> {
    let start = Instant::now();
    let mut profile = crate::registration_timing::Timer::new();
    let plane = fulgur_core::fits::read_reference_plane(path, cancel)?;
    profile.lap("reference_fits_read");
    if let Some(progress) = progress {
        progress.advance();
    }
    let anchor = report
        .inspection
        .frames
        .iter()
        .find(|f| f.file_name == report.reference_anchor)
        .unwrap();
    let info = anchor.frame.as_ref().unwrap();
    if plane
        .object
        .as_ref()
        .zip(info.object.as_ref())
        .is_some_and(|(a, b)| a.trim() != b.trim())
        || plane
            .filter
            .as_ref()
            .zip(info.filter.as_ref())
            .is_some_and(|(a, b)| a.trim() != b.trim())
    {
        return Err(Error::Invalid(
            "External reference target/filter differs from the input stack.",
        ));
    }
    profile.lap("reference_validation");
    let catalog = stars::reference_catalog(&plane, cancel)?;
    profile.lap("reference_star_catalog");
    if let Some(progress) = progress {
        progress.advance();
    }
    let bridge = geometry::solve_similarity(&anchor.stars, &catalog, None, cancel)?.ok_or(
        Error::Invalid(
            "External reference could not be matched with at least ten stars at 33..300% scale.",
        ),
    )?;
    profile.lap("native_anchor_to_external_bridge");
    if let Some(progress) = progress {
        progress.advance();
    }
    let name = path.to_string_lossy().into_owned();
    align_rows_observed(
        &mut report.frames,
        &report.inspection.frames,
        &catalog,
        bridge.transform,
        &name,
        [info.width, info.height],
        cancel,
        progress,
        report.diagnose_excluded,
    )?;
    profile.lap("parallel_source_refinements");
    let maximum = report
        .frames
        .iter()
        .map(|f| f.final_weight)
        .fold(0., f64::max);
    if maximum <= 0. {
        return Err(Error::Invalid(
            "No accepted sources registered to the external reference.",
        ));
    }
    for f in &mut report.frames {
        f.final_weight /= maximum;
    }
    let polygons: Vec<_> = report
        .frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| (f.footprint.clone(), f.final_weight))
        .collect();
    profile.lap("weight_normalization_and_footprints");
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    report.common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    report.common_intersection_polygon = common;
    profile.lap("common_intersection_canvas");
    report.coverage_optimized = geometry::canvas(&polygons, report.min_coverage, cancel)?;
    profile.lap("coverage_optimized_canvas");
    report.accepted_count = report.frames.iter().filter(|f| f.accepted).count();
    report.rejected_count = report.frame_count - report.accepted_count;
    report.registration_success_count = report
        .frames
        .iter()
        .filter(|f| f.registration_success)
        .count();
    report.accepted_exposure_seconds = report
        .frames
        .iter()
        .zip(&report.inspection.frames)
        .filter(|(f, _)| f.accepted)
        .map(|(_, f)| f.frame.as_ref().unwrap().exposure_seconds)
        .sum();
    report.reference_width = plane.width;
    report.reference_height = plane.height;
    report.reference_selection =
        "explicit external geometry-only FITS; native accepted-median photometric anchor retained";
    report.transform_convention = "source pixel centers -> external reference: q=scale*R(theta)*p+t; scale in [0.33,3]; zero-based top-down; session/local and photometric matches remain native anchor coordinates";
    report.external_reference = Some(ExternalReferenceReport {
        path: name,
        width: plane.width,
        height: plane.height,
        photometric_anchor: report.reference_anchor.clone(),
        geometry_only: true,
        scale_range: [0.33, 3.],
        anchor_to_reference: bridge.transform,
        anchor_matches: bridge.pairs,
        anchor_rmse_px: bridge.rmse,
        stars: catalog
            .iter()
            .map(|s| ReferenceStar {
                x_px: s.x_px,
                y_px: s.y_px,
                snr: s.snr,
            })
            .collect(),
        pixel_rule: "template defines geometry/crop only; intensities, exposure, GPS and weights come from native scientific inputs; no template pixels contribute; no area multiplier on resampled surface brightness",
    });
    report.phase_profile.external = Some(profile.finish());
    let elapsed = start.elapsed().as_secs_f64();
    report.registration_seconds += elapsed;
    report.wall_seconds += elapsed;
    report.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    if let Some(progress) = progress {
        progress.advance();
    }
    Ok(())
}

/// Independent source fits mutate disjoint rows, capped at sixteen chunks.
/// Every later weight/footprint/report reduction remains in original source order.
#[cfg(test)]
fn align_rows(
    rows: &mut [crate::register::RegisteredFrame],
    sources: &[crate::inspect::QualityFrame],
    catalog: &[stars::Star],
    bridge: geometry::Transform,
    name: &str,
    sensor: [usize; 2],
    cancel: &AtomicBool,
) -> Result<()> {
    align_rows_observed(
        rows, sources, catalog, bridge, name, sensor, cancel, None, true,
    )
}

#[allow(clippy::too_many_arguments)] // Unchanged fit context plus an optional UI observer.
fn align_rows_observed(
    rows: &mut [crate::register::RegisteredFrame],
    sources: &[crate::inspect::QualityFrame],
    catalog: &[stars::Star],
    bridge: geometry::Transform,
    name: &str,
    sensor: [usize; 2],
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
    diagnose_excluded: bool,
) -> Result<()> {
    check_cancelled(cancel)?;
    if rows.len() != sources.len() {
        return Err(Error::Invalid(
            "External registration row/catalog counts differ.",
        ));
    }
    let workers = rayon::current_num_threads().min(16);
    let chunk = rows.len().div_ceil(workers).max(1);
    rows.par_chunks_mut(chunk)
        .zip(sources.par_chunks(chunk))
        .try_for_each(|(rows, sources)| -> Result<()> {
            for (row, source) in rows.iter_mut().zip(sources) {
                check_cancelled(cancel)?;
                if !row.quality_accepted && !row.registration_attempted {
                    // A no-anchor diagnostic row still consumed a scheduled native work unit.
                    if diagnose_excluded {
                        if let Some(progress) = progress {
                            progress.advance();
                        }
                    }
                    continue;
                }
                let Some(native) = row.transform else {
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                    continue;
                };
                row.source_to_photometric_anchor = Some(native);
                row.match_reference = name.to_owned();
                let fit = geometry::solve_similarity(
                    &source.stars,
                    catalog,
                    Some(bridge.compose(native)),
                    cancel,
                )?;
                if let Some(fit) = fit {
                    row.transform = Some(fit.transform);
                    row.rotation_deg = Some(fit.transform.theta_rad.to_degrees());
                    row.matches = fit.pairs;
                    row.match_reference = name.to_owned();
                    row.method = Some("external_similarity_direct");
                    row.inlier_count = row.matches.len();
                    row.inlier_rmse_px = Some(fit.rmse);
                    if row.accepted {
                        row.footprint = geometry::footprint(fit.transform, sensor[0], sensor[1]);
                    }
                } else {
                    row.registration_success = false;
                    row.accepted = false;
                    row.final_weight = 0.;
                    row.transform = None;
                    row.rotation_deg = None;
                    row.method = None;
                    row.footprint.clear();
                    row.matches.clear();
                    row.inlier_count = 0;
                    row.inlier_rmse_px = None;
                    row.rejection_reasons
                        .push("external_reference_registration_failed".into());
                }
                if let Some(progress) = progress {
                    progress.advance();
                }
            }
            Ok(())
        })
}

#[cfg(test)]
#[path = "reference_parallel_tests.rs"]
mod parallel_tests;
