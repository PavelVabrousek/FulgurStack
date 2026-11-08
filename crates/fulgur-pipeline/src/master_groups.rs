//! One registration, several masters: the accepted frames of chosen light sets
//! are stacked apart on the common pixel grid (ADR-101). Each master keeps the
//! brightness of its own filter instead of being scaled onto the session anchor.

use crate::integrate::IntegrationSource;
use crate::register::RegistrationReport;
use fulgur_core::{Error, Mosaic, Result};
use serde::Serialize;

/// How the light sets of a run are divided into masters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MasterSplit {
    /// All sets into one master.
    #[default]
    Combined,
    /// One master per filter; sets without a filter card stay apart.
    PerFilter,
    /// One master per light set.
    PerSet,
}

impl MasterSplit {
    pub const ALL: [Self; 3] = [Self::Combined, Self::PerFilter, Self::PerSet];

    pub fn key(self) -> &'static str {
        match self {
            Self::Combined => "combined",
            Self::PerFilter => "per-filter",
            Self::PerSet => "per-set",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == text)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Combined => "One master",
            Self::PerFilter => "One master per filter",
            Self::PerSet => "One master per light set",
        }
    }
}

/// One master of a run: its frames and what its FITS header records.
pub struct MasterRun {
    /// File-name label; None for the one master of a combined run.
    pub label: Option<String>,
    /// Light sets stacked here, indexed like `InspectReport::light_sets`.
    pub sets: Vec<usize>,
    pub sources: Vec<IntegrationSource>,
    /// All input frames of these sets, the denominator of the diagnostic maps.
    pub input_frames: usize,
    /// Source frame whose brightness and header the master keeps.
    pub anchor: String,
    pub accepted_frames: usize,
    pub total_exposure_seconds: f64,
    pub monochrome_frames: usize,
    pub geometric_correction_frames: usize,
}

/// A label that is safe in a file name, on every supported system.
fn file_label(name: &str) -> String {
    let label: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let label = label.trim_matches('_');
    if label.is_empty() {
        "set".into()
    } else {
        label.to_owned()
    }
}

/// Group of each light set, in order of first appearance, with its name.
fn set_groups(report: &RegistrationReport, split: MasterSplit) -> Vec<(String, Vec<usize>)> {
    let inspection = &report.inspection;
    let mut groups: Vec<(String, Vec<usize>)> = vec![];
    for (index, set) in inspection.light_sets.iter().enumerate() {
        let filter = inspection
            .frames
            .iter()
            .filter(|f| f.light_set == index)
            .find_map(|f| f.frame.as_ref()?.filter.as_deref())
            .map(str::trim)
            .filter(|f| !f.is_empty());
        let name = match (split, filter) {
            (MasterSplit::PerFilter, Some(filter)) => filter.to_owned(),
            _ => set.label.clone(),
        };
        match groups.iter_mut().find(|(known, _)| *known == name) {
            Some((_, sets)) if split == MasterSplit::PerFilter && filter.is_some() => {
                sets.push(index);
            }
            _ => groups.push((name, vec![index])),
        }
    }
    groups
}

/// Index of the middle photometric scale: the frame the group is expressed in.
fn group_anchor(scales: &[f64]) -> usize {
    let mut order: Vec<usize> = (0..scales.len()).collect();
    order.sort_by(|&a, &b| scales[a].total_cmp(&scales[b]).then(a.cmp(&b)));
    order[(order.len() - 1) / 2]
}

/// Scale and offset of one frame relative to the group anchor, from its scale
/// and sky level. The anchor itself maps to exactly `(1, 0)`.
fn rebased(scale: f64, background: f64, anchor_scale: f64, anchor_background: f64) -> (f64, f64) {
    let scale = scale / anchor_scale;
    (scale, anchor_background - scale * background)
}

/// A master may mix light sets only when each is on the run's brightness: a
/// set whose frames share no stars with the photometric anchor and that no
/// longer star list linked to it keeps its own brightness (ADR-156).
fn unlinked_mix(report: &RegistrationReport, rows: &[usize]) -> Result<()> {
    if mixes_unlinked(
        rows,
        |i| report.inspection.frames[i].light_set,
        |i| {
            report.frames[i].photometric_reference
                == Some(crate::register::set_photometry::UNLINKED)
        },
    ) {
        return Err(Error::Invalid(
            "A light set shares no stars with the reference filter, so its brightness cannot be matched to the other sets. Stack one master per light set or per filter (Masters).",
        ));
    }
    Ok(())
}

/// Rows of more than one light set where one row keeps its set's own brightness.
fn mixes_unlinked(
    rows: &[usize],
    set: impl Fn(usize) -> usize,
    unlinked: impl Fn(usize) -> bool,
) -> bool {
    rows.iter().any(|&i| unlinked(i)) && rows.iter().any(|&i| set(i) != set(rows[0]))
}

/// The masters of a run. `sources` must be the accepted frames of `report` in
/// report order. A split that yields one group is the plain combined run, so
/// its master stays byte-identical.
pub fn runs(
    report: &RegistrationReport,
    sources: Vec<IntegrationSource>,
    split: MasterSplit,
) -> Result<Vec<MasterRun>> {
    let accepted: Vec<usize> = (0..report.frames.len())
        .filter(|&i| report.frames[i].accepted)
        .collect();
    if accepted.len() != sources.len() {
        return Err(Error::Invalid(
            "Integration sources and accepted frames differ.",
        ));
    }
    let inspection = &report.inspection;
    let has_frames = |sets: &[usize]| {
        accepted
            .iter()
            .any(|&i| sets.contains(&inspection.frames[i].light_set))
    };
    let groups: Vec<_> = if split == MasterSplit::Combined {
        vec![]
    } else {
        set_groups(report, split)
            .into_iter()
            .filter(|(_, sets)| has_frames(sets))
            .collect()
    };
    let summary = |rows: &[usize]| {
        let count = |test: &dyn Fn(usize) -> bool| rows.iter().filter(|&&i| test(i)).count();
        (
            rows.iter()
                .filter_map(|&i| inspection.frames[i].frame.as_ref())
                .map(|f| f.exposure_seconds)
                .sum::<f64>(),
            count(&|i| {
                inspection.frames[i]
                    .frame
                    .as_ref()
                    .is_some_and(|f| f.mosaic == Mosaic::Mono)
            }),
            count(&|i| {
                report.frames[i]
                    .geometric_correction
                    .as_ref()
                    .is_some_and(|d| d.model.is_some())
            }),
        )
    };
    if groups.len() < 2 {
        unlinked_mix(report, &accepted)?;
        let (_, monochrome_frames, geometric_correction_frames) = summary(&accepted);
        return Ok(vec![MasterRun {
            label: None,
            sets: (0..inspection.light_sets.len()).collect(),
            sources,
            input_frames: report.frame_count,
            anchor: report.reference_anchor.clone(),
            accepted_frames: report.accepted_count,
            total_exposure_seconds: report.accepted_exposure_seconds,
            monochrome_frames,
            geometric_correction_frames,
        }]);
    }
    let mut sources: Vec<Option<IntegrationSource>> = sources.into_iter().map(Some).collect();
    let mut labels: Vec<String> = vec![];
    let mut runs = vec![];
    for (name, sets) in groups {
        let picks: Vec<usize> = (0..accepted.len())
            .filter(|&p| sets.contains(&inspection.frames[accepted[p]].light_set))
            .collect();
        let mut group: Vec<IntegrationSource> =
            picks.iter().filter_map(|&p| sources[p].take()).collect();
        let rows: Vec<usize> = picks.iter().map(|&p| accepted[p]).collect();
        unlinked_mix(report, &rows)?;
        let scales: Vec<f64> = group.iter().map(|s| s.scale).collect();
        let anchor = group_anchor(&scales);
        let sky = |row: usize| {
            let frame = &inspection.frames[row];
            frame
                .channel_background_adu
                .unwrap_or([frame.background_adu.unwrap_or(0.); 3])
        };
        let (anchor_scale, anchor_sky) = (scales[anchor], sky(rows[anchor]));
        let heaviest = group.iter().map(|s| s.weight).fold(0., f64::max);
        for (source, &row) in group.iter_mut().zip(&rows) {
            let frame_sky = sky(row);
            let channels: [(f64, f64); 3] = std::array::from_fn(|c| {
                rebased(source.scale, frame_sky[c], anchor_scale, anchor_sky[c])
            });
            source.scale = channels[0].0;
            source.offset = channels.map(|c| c.1);
            source.weight /= heaviest;
        }
        let base = file_label(&name);
        let mut label = base.clone();
        let mut copy = 1;
        while labels.iter().any(|l| l.eq_ignore_ascii_case(&label)) {
            copy += 1;
            label = format!("{base}_{copy}");
        }
        labels.push(label.clone());
        let (total_exposure_seconds, monochrome_frames, geometric_correction_frames) =
            summary(&rows);
        runs.push(MasterRun {
            label: Some(label),
            input_frames: inspection
                .frames
                .iter()
                .filter(|f| sets.contains(&f.light_set))
                .count(),
            sets,
            anchor: report.frames[rows[anchor]].file_name.clone(),
            accepted_frames: group.len(),
            sources: group,
            total_exposure_seconds,
            monochrome_frames,
            geometric_correction_frames,
        });
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_have_stable_keys() {
        for split in MasterSplit::ALL {
            assert_eq!(MasterSplit::parse(split.key()), Some(split));
        }
        assert_eq!(MasterSplit::parse("other"), None);
        assert_eq!(MasterSplit::default(), MasterSplit::Combined);
    }

    #[test]
    fn labels_are_safe_in_file_names() {
        assert_eq!(file_label("Ha"), "Ha");
        assert_eq!(file_label(" Duo-Band "), "Duo-Band");
        assert_eq!(file_label("M 33/b: L*"), "M_33_b__L");
        assert_eq!(file_label("***"), "set");
    }

    #[test]
    fn anchor_is_the_middle_frame_and_ties_are_stable() {
        assert_eq!(group_anchor(&[3., 1., 2.]), 2);
        assert_eq!(group_anchor(&[1., 2., 3., 4.]), 1);
        assert_eq!(group_anchor(&[2., 2., 2.]), 1);
        assert_eq!(group_anchor(&[5.]), 0);
    }

    #[test]
    fn a_set_without_a_brightness_link_stacks_only_in_its_own_master() {
        let set = |i: usize| [0, 0, 1, 1][i];
        let unlinked = |i: usize| i >= 2;
        // Combined: OIII and the unlinked L together.
        assert!(mixes_unlinked(&[0, 1, 2, 3], set, unlinked));
        // One master per set.
        assert!(!mixes_unlinked(&[2, 3], set, unlinked));
        assert!(!mixes_unlinked(&[0, 1], set, unlinked));
        // Linked sets may share a master.
        assert!(!mixes_unlinked(&[0, 1, 2, 3], set, |_| false));
    }

    #[test]
    fn rebasing_keeps_the_group_brightness_and_removes_the_session_scale() {
        // Ha frames scaled 4x onto a broadband session anchor at sky 1000.
        let (session_scale, sky) = (4., 200.);
        let (scale, offset) = rebased(session_scale, sky, 4., 200.);
        assert_eq!((scale, offset), (1., 0.), "the anchor stays as recorded");
        // A frame through thinner cloud: half the flux, same sky after scaling.
        let (scale, offset) = rebased(8., 110., 4., 200.);
        assert_eq!(scale, 2.);
        assert_eq!(offset, 200. - 2. * 110.);
        let star = 500.;
        let frame_star = 110. + star / 2.;
        assert_eq!(scale * frame_star + offset, 200. + star);
    }
}
