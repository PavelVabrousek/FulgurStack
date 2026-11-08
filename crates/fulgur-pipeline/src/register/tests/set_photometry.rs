//! Frames of another filter that share no stars with the photometric anchor
//! are brought onto it through an anchor of their own light set (ADR-156).
use super::*;
use crate::register::set_photometry::{UNLINKED, rescue};

fn stranded(f: &QualityFrame) -> RegisteredFrame {
    let mut row = empty(f, 0, "frame_0.fit");
    row.registration_attempted = true;
    row.registration_success = true;
    row.transform = Some(Transform::IDENTITY);
    row.rejection_reasons = vec!["insufficient_photometric_matches".into()];
    row
}

#[test]
fn a_light_set_without_shared_stars_keeps_its_own_brightness() {
    let anchor = planned_frame(0, "2026-11-04T22:00:00", 300., Transform::IDENTITY, 1.);
    let mut quality = vec![anchor];
    for (index, (date, flux)) in [("2026-11-06T00:10:00", 1.), ("2026-11-06T00:20:00", 2.)]
        .into_iter()
        .enumerate()
    {
        let mut f = planned_frame(index + 1, date, 300., Transform::IDENTITY, flux);
        f.light_set = 1;
        quality.push(f);
    }
    let mut first = empty(&quality[0], 0, "frame_0.fit");
    first.registration_success = true;
    first.accepted = true;
    first.transform = Some(Transform::IDENTITY);
    let mut rows = [first, stranded(&quality[1]), stranded(&quality[2])];
    let cancel = AtomicBool::new(false);
    let sets = rescue(
        &mut rows.iter_mut().collect::<Vec<_>>(),
        &quality,
        0,
        None,
        &|row| geometry::footprint(row.transform.unwrap(), 2160, 3840),
        &|_| 1.,
        &["OIII".into(), "L".into()],
        &cancel,
    );
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].label, "L");
    assert_eq!(
        sets[0].anchor, "frame_1.fit",
        "the set's chronological median"
    );
    assert_eq!((sets[0].frames, sets[0].link_scale), (2, None));
    for row in &rows[1..] {
        assert!(row.accepted, "{:?}", row.rejection_reasons);
        assert!(row.rejection_reasons.is_empty());
        assert_eq!(row.photometric_reference, Some(UNLINKED));
        assert!(row.final_weight > 0. && row.footprint.len() >= 3);
    }
    // The twice brighter frame is scaled by one half onto its set's anchor.
    assert!((rows[1].photometric_scale.unwrap() - 1.).abs() < 1e-12);
    assert!((rows[2].photometric_scale.unwrap() - 0.5).abs() < 1e-12);
    // A frame of the anchor's own set is never rescued this way.
    let mut own = stranded(&quality[1]);
    let mut same = quality[1].clone();
    same.light_set = 0;
    let none = rescue(
        &mut [&mut rows[0], &mut own],
        &[quality[0].clone(), same],
        0,
        None,
        &|_| vec![],
        &|_| 1.,
        &["OIII".into()],
        &cancel,
    );
    assert!(none.is_empty());
    assert!(!own.accepted);
}
