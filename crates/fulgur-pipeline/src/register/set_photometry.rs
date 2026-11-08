//! Brightness through an anchor of a frame's own light set (ADR-156).
//!
//! A frame is brought onto the run's photometric anchor by the median flux
//! ratio of stars that both measured unsaturated. Frames of another filter
//! may share no such star with the anchor: a dense broadband L frame keeps
//! its brightest stars, saturated, for position only and measures brightness
//! on fainter ones, which a narrowband anchor's list does not hold. Such a
//! frame was refused although it registered.
//!
//! Here those frames are measured against an anchor of their own light set,
//! whose frames share plenty of stars, and the set anchor is linked to the run
//! anchor through longer star lists of both, read once more from their files.
//! Without a link the set keeps its own brightness: a master of that set alone
//! is unaffected (its group is rebased onto its own anchor, ADR-101), and a
//! master that mixes it with other sets is refused (`master_groups::runs`).

use super::*;
use crate::stars::Star;

/// The provenance of a frame measured through its light set's anchor.
pub const LINKED: &str = "light_set_anchor_linked";
pub const UNLINKED: &str = "light_set_anchor_unlinked";

/// Stars of both anchors measured for the link, the brightest of each.
const LINK_STARS: usize = 5000;
/// Pair distance between frames on the grid, as the native direct matches.
const PAIR_PX: f64 = 1.5;

/// One light set brought onto its own anchor.
#[derive(Clone, Debug, Serialize)]
pub struct SetBrightness {
    pub light_set: usize,
    pub label: String,
    pub anchor: String,
    pub frames: usize,
    /// Scale of the set anchor onto the run's photometric anchor, when the
    /// longer lists linked them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_scale: Option<f64>,
    pub link_pairs: usize,
}

/// Scale (reference flux / source flux) and its inverse ratio of stars both
/// lists measured unsaturated: `photometry` and `total_flux_ratio` on plain
/// star lists.
fn scales(src: &[Star], dst: &[Star], matches: &[Match]) -> Option<(f64, f64, usize)> {
    let pairs: Vec<(f64, f64)> = matches
        .iter()
        .filter_map(|m| {
            let (a, b) = (&src[m.source], &dst[m.reference]);
            (a.peak_adu > 0.
                && b.peak_adu > 0.
                && a.peak_adu <= 62000.
                && b.peak_adu <= 62000.
                && a.flux_adu > 0.
                && b.flux_adu > 0.)
                .then_some((a.flux_adu, b.flux_adu))
        })
        .collect();
    if pairs.len() < 10 {
        return None;
    }
    let mut forward: Vec<f64> = pairs.iter().map(|(a, b)| b / a).collect();
    let mut backward: Vec<f64> = pairs.iter().map(|(a, b)| a / b).collect();
    Some((median(&mut forward), median(&mut backward), pairs.len()))
}

/// Pairs of `src` and `dst`: their grid transforms predict the pose, and
/// the star matcher fits it again on these two lists, so a frame whose own
/// registration is only approximate still pairs its stars within 1.5 px.
fn pairs(
    src: &[Star],
    src_to_grid: Transform,
    dst: &[Star],
    dst_to_grid: Transform,
    cancel: &AtomicBool,
) -> Vec<Match> {
    let prediction = dst_to_grid.inverse().compose(src_to_grid);
    geometry::Matcher::new(dst, true)
        .solve(src, Some(prediction), cancel)
        .ok()
        .flatten()
        .map(|solution| solution.pairs)
        .unwrap_or_else(|| geometry::correspondences(src, dst, prediction, PAIR_PX))
}

/// The set anchor's scale onto the run anchor from both frames' longer lists.
fn link(
    witness: Option<&WitnessCatalog<'_>>,
    set_anchor: (&QualityFrame, Transform),
    run_anchor: (&QualityFrame, Transform),
    cancel: &AtomicBool,
) -> Option<(f64, f64, usize)> {
    let witness = witness?;
    let deep = |frame: &QualityFrame| -> Option<Vec<Star>> {
        let raw = witness.raw(frame)?;
        crate::stars::analyze_with_limit(&raw, cancel, LINK_STARS)
            .ok()
            .map(|m| m.stars)
    };
    let (src, dst) = (deep(set_anchor.0)?, deep(run_anchor.0)?);
    scales(
        &src,
        &dst,
        &pairs(&src, set_anchor.1, &dst, run_anchor.1, cancel),
    )
}

/// Brings registered frames that lack photometric matches with the run anchor
/// `anchor` onto it through their light set's anchor. `rows` and `quality`
/// share their order; `footprint` gives an accepted row's grid footprint;
/// `area` the ratio of a frame's pixel area to the anchor's.
#[allow(clippy::too_many_arguments)] // Rows, frames, anchor, files and the run's geometry.
pub(crate) fn rescue(
    rows: &mut [&mut RegisteredFrame],
    quality: &[QualityFrame],
    anchor: usize,
    witness: Option<&WitnessCatalog<'_>>,
    footprint: &dyn Fn(&RegisteredFrame) -> Vec<[f64; 2]>,
    area: &dyn Fn(usize) -> f64,
    labels: &[String],
    cancel: &AtomicBool,
) -> Vec<SetBrightness> {
    let stranded = |i: usize| {
        let row = &rows[i];
        row.registration_success
            && row.transform.is_some()
            && row.rejection_reasons == ["insufficient_photometric_matches"]
            && quality[i].accepted
            && quality[i].light_set != quality[anchor].light_set
    };
    let mut sets: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in (0..rows.len()).filter(|&i| stranded(i)) {
        sets.entry(quality[i].light_set).or_default().push(i);
    }
    let Some(anchor_transform) = rows[anchor].transform else {
        return vec![];
    };
    let master = &quality[anchor];
    let mut out = vec![];
    for (set, members) in sets {
        if fulgur_core::check_cancelled(cancel).is_err() {
            break;
        }
        // The set's chronological median, or its sharpest without dates.
        let set_anchor = select_anchor(&members, quality).unwrap_or_else(|_| {
            *members
                .iter()
                .min_by(|&&a, &&b| {
                    let hfr = |i: usize| quality[i].hfr_px.unwrap_or(f64::INFINITY);
                    hfr(a).total_cmp(&hfr(b))
                })
                .unwrap()
        });
        let set_transform = rows[set_anchor].transform.unwrap();
        let linked = link(
            witness,
            (&quality[set_anchor], set_transform),
            (master, anchor_transform),
            cancel,
        );
        let (link_scale, link_ratio, link_pairs) = linked.unwrap_or((1., 1., 0));
        let mut rescued = 0;
        for &i in &members {
            let f = &quality[i];
            let t = rows[i].transform.unwrap();
            let own = if i == set_anchor {
                Some((1., 1., 0))
            } else {
                let matches = pairs(
                    &f.stars,
                    t,
                    &quality[set_anchor].stars,
                    set_transform,
                    cancel,
                );
                scales(&f.stars, &quality[set_anchor].stars, &matches)
            };
            let Some((a, s, _)) = own else {
                continue;
            };
            let pixel_area = area(i);
            let scale = a * link_scale / pixel_area;
            let ratio = s * link_ratio * pixel_area;
            let (Some(info), Some(anchor_info)) = (f.frame.as_ref(), master.frame.as_ref()) else {
                continue;
            };
            let transmission = ratio * anchor_info.exposure_seconds / info.exposure_seconds;
            let offset =
                master.background_adu.unwrap_or(f64::NAN) - scale * f.background_adu.unwrap_or(0.);
            let noise = scale * f.noise_adu.unwrap_or(0.);
            let hfr = f.hfr_px.unwrap_or(0.) * pixel_area.sqrt();
            let weight = 1. / (noise * noise * hfr * hfr);
            if ![scale, ratio, transmission, offset, noise, weight]
                .iter()
                .all(|v| v.is_finite())
                || noise <= 0.
                || weight <= 0.
            {
                continue;
            }
            let row = &mut rows[i];
            row.rejection_reasons.clear();
            row.photometric_reference = Some(if linked.is_some() { LINKED } else { UNLINKED });
            row.photometric_scale = Some(scale);
            row.matched_flux_ratio = Some(ratio);
            row.exposure_corrected_transparency = Some(transmission);
            row.background_offset_adu = Some(offset);
            row.channel_offsets_adu = channel_offsets(scale, f, master);
            row.normalized_noise_adu = Some(noise);
            row.accepted = true;
            row.final_weight = weight;
            row.footprint = footprint(row);
            rescued += 1;
        }
        if rescued > 0 {
            out.push(SetBrightness {
                light_set: set,
                label: labels.get(set).cloned().unwrap_or_default(),
                anchor: quality[set_anchor].file_name.clone(),
                frames: rescued,
                link_scale: linked.map(|l| l.0),
                link_pairs,
            });
        }
    }
    out
}
