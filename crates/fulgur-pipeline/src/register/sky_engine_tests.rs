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
