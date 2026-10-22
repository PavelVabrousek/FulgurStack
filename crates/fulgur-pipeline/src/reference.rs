//! External geometry-only FITS references; scientific photometry stays in the native anchor.
use crate::{geometry, register::RegistrationReport, stars};
use fulgur_core::{Error, Result, check_cancelled};
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
    let start = Instant::now();
    let plane = fulgur_core::fits::read_reference_plane(path, cancel)?;
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
    let catalog = stars::reference_catalog(&plane, cancel)?;
    let bridge = geometry::solve_similarity(&anchor.stars, &catalog, None, cancel)?.ok_or(
        Error::Invalid(
            "External reference could not be matched with at least ten stars at 33..300% scale.",
        ),
    )?;
    let name = path.to_string_lossy().into_owned();
    for (row, source) in report.frames.iter_mut().zip(&report.inspection.frames) {
        check_cancelled(cancel)?;
        let Some(native) = row.transform else {
            continue;
        };
        row.source_to_photometric_anchor = Some(native);
        row.match_reference = name.clone();
        let fit = geometry::solve_similarity(
            &source.stars,
            &catalog,
            Some(bridge.transform.compose(native)),
            cancel,
        )?;
        if let Some(fit) = fit {
            row.transform = Some(fit.transform);
            row.rotation_deg = Some(fit.transform.theta_rad.to_degrees());
            row.matches = fit.pairs;
            row.match_reference = name.clone();
            row.method = Some("external_similarity_direct");
            row.inlier_count = row.matches.len();
            row.inlier_rmse_px = Some(fit.rmse);
            if row.accepted {
                row.footprint = geometry::footprint(fit.transform, info.width, info.height);
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
    }
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
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    report.common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    report.common_intersection_polygon = common;
    report.coverage_optimized = geometry::canvas(&polygons, report.min_coverage, cancel)?;
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
    let elapsed = start.elapsed().as_secs_f64();
    report.registration_seconds += elapsed;
    report.wall_seconds += elapsed;
    report.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    Ok(())
}
