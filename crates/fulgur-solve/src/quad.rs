//! Quads: four points reduced to a four-number code that does not change
//! with the position, rotation or scale of the picture they are in.

pub(crate) type P2 = [f64; 2];

/// Cells per code coordinate in the index's hash.
pub(crate) const BUCKETS: usize = 25;

/// The code of four points. A goes to 0 and B to 1 in the complex plane; C
/// and D are the other two. The symmetry of the labels is broken so that
/// `Re(C) + Re(D) <= 1` and `Re(C) <= Re(D)`. The code is
/// `(Re C, Im C + 0.5, Re D, Im D + 0.5)`; `order` says which input point
/// took each of the four places. None for a degenerate quad (A and B
/// together) or one whose C or D lies outside the circle on AB.
pub(crate) fn canonical(p: [P2; 4]) -> Option<([f64; 4], [usize; 4])> {
    let mut order = [0usize, 1, 2, 3];
    let z = |a: P2, b: P2, q: P2| -> P2 {
        let w = [b[0] - a[0], b[1] - a[1]];
        let d = w[0] * w[0] + w[1] * w[1];
        let v = [q[0] - a[0], q[1] - a[1]];
        [
            (v[0] * w[0] + v[1] * w[1]) / d,
            (v[1] * w[0] - v[0] * w[1]) / d,
        ]
    };
    let mut zc = z(p[0], p[1], p[2]);
    let mut zd = z(p[0], p[1], p[3]);
    if !zc[0].is_finite() || !zd[0].is_finite() {
        return None;
    }
    if zc[0] + zd[0] > 1.0 {
        order.swap(0, 1);
        zc = [1.0 - zc[0], -zc[1]];
        zd = [1.0 - zd[0], -zd[1]];
    }
    if zc[0] > zd[0] {
        order.swap(2, 3);
        std::mem::swap(&mut zc, &mut zd);
    }
    let code = [zc[0], zc[1] + 0.5, zd[0], zd[1] + 0.5];
    code.iter()
        .all(|c| (0.0..=1.0).contains(c))
        .then_some((code, order))
}

/// Whether `q` lies inside the circle on the diameter AB.
pub(crate) fn inside_circle(a: P2, b: P2, q: P2) -> bool {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let r2 = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)) / 4.0;
    (q[0] - m[0]).powi(2) + (q[1] - m[1]).powi(2) <= r2
}

/// The hash cell of one code coordinate.
pub(crate) fn cell(c: f64) -> usize {
    ((c * BUCKETS as f64) as usize).min(BUCKETS - 1)
}

/// The hash bucket of a code.
pub(crate) fn bucket(code: &[f64; 4]) -> usize {
    code.iter().fold(0, |k, &c| k * BUCKETS + cell(c))
}

pub(crate) fn quantize(code: &[f64; 4]) -> [u16; 4] {
    code.map(|x| (x * 65535.0).round() as u16)
}

pub(crate) fn dequantize(code: &[u16; 4]) -> [f64; 4] {
    code.map(|x| f64::from(x) / 65535.0)
}

/// Least-squares similarity `Q = s P + t` (complex `s` and `t`) of four
/// point pairs, as `(Re s, Im s, Re t, Im t)`.
pub(crate) fn fit_similarity(p: &[P2; 4], q: &[P2; 4]) -> Option<(f64, f64, f64, f64)> {
    let mean = |v: &[P2; 4]| {
        [
            v.iter().map(|x| x[0]).sum::<f64>() / 4.0,
            v.iter().map(|x| x[1]).sum::<f64>() / 4.0,
        ]
    };
    let (mp, mq) = (mean(p), mean(q));
    let (mut num_r, mut num_i, mut den) = (0.0, 0.0, 0.0);
    for (a, b) in p.iter().zip(q) {
        let (x, y) = (a[0] - mp[0], a[1] - mp[1]);
        let (u, v) = (b[0] - mq[0], b[1] - mq[1]);
        num_r += x * u + y * v;
        num_i += x * v - y * u;
        den += x * x + y * y;
    }
    if den < 1e-12 {
        return None;
    }
    let (sr, si) = (num_r / den, num_i / den);
    let tr = mq[0] - (sr * mp[0] - si * mp[1]);
    let ti = mq[1] - (sr * mp[1] + si * mp[0]);
    Some((sr, si, tr, ti))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn similar(p: [P2; 4], scale: f64, angle: f64, mirror: bool, shift: P2) -> [P2; 4] {
        let (c, s) = (angle.cos(), angle.sin());
        p.map(|q| {
            let y = if mirror { -q[1] } else { q[1] };
            [
                scale * (c * q[0] - s * y) + shift[0],
                scale * (s * q[0] + c * y) + shift[1],
            ]
        })
    }

    const QUAD: [P2; 4] = [[10.0, 20.0], [110.0, 40.0], [60.0, 50.0], [80.0, 25.0]];

    #[test]
    fn the_code_ignores_position_rotation_and_scale() {
        let (code, order) = canonical(QUAD).unwrap();
        for (scale, angle) in [(0.01, 0.7), (3.0, -2.0), (1.0, 3.0)] {
            let moved = similar(QUAD, scale, angle, false, [1234.0, -567.0]);
            let (other, other_order) = canonical(moved).unwrap();
            assert_eq!(order, other_order);
            for k in 0..4 {
                assert!((code[k] - other[k]).abs() < 1e-9, "{code:?} {other:?}");
            }
        }
    }

    #[test]
    fn a_mirror_image_has_the_mirrored_code() {
        // Mirroring turns every imaginary part around 0.5; the labels may
        // be given again, so compare the sets of (re, im) of C and D.
        let (code, _) = canonical(QUAD).unwrap();
        let (mirrored, _) = canonical(similar(QUAD, 1.0, 0.0, true, [0.0, 0.0])).unwrap();
        let mut a = [(code[0], code[1]), (code[2], code[3])];
        let mut b = [
            (mirrored[0], 1.0 - mirrored[1]),
            (mirrored[2], 1.0 - mirrored[3]),
        ];
        a.sort_by(|x, y| x.0.total_cmp(&y.0));
        b.sort_by(|x, y| x.0.total_cmp(&y.0));
        for k in 0..2 {
            assert!((a[k].0 - b[k].0).abs() < 1e-9 && (a[k].1 - b[k].1).abs() < 1e-9);
        }
    }

    #[test]
    fn degenerate_and_outside_quads_have_no_code() {
        assert!(canonical([[0.0, 0.0], [0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]).is_none());
        // C far outside the circle on AB.
        assert!(canonical([[0.0, 0.0], [1.0, 0.0], [50.0, 50.0], [0.5, 0.1]]).is_none());
        assert!(!inside_circle([0.0, 0.0], [2.0, 0.0], [5.0, 0.0]));
        assert!(inside_circle([0.0, 0.0], [2.0, 0.0], [1.0, 0.5]));
    }

    #[test]
    fn the_similarity_fit_recovers_the_map() {
        let moved = similar(QUAD, 0.002, 1.1, false, [0.3, -0.1]);
        let (sr, si, tr, ti) = fit_similarity(&QUAD, &moved).unwrap();
        assert!((sr.hypot(si) - 0.002).abs() < 1e-12);
        assert!((si.atan2(sr) - 1.1).abs() < 1e-9);
        let back = [
            sr * QUAD[0][0] - si * QUAD[0][1] + tr,
            sr * QUAD[0][1] + si * QUAD[0][0] + ti,
        ];
        assert!((back[0] - moved[0][0]).abs() < 1e-9 && (back[1] - moved[0][1]).abs() < 1e-9);
    }

    #[test]
    fn quantizing_keeps_the_code_within_a_cell_of_resolution() {
        let (code, _) = canonical(QUAD).unwrap();
        let back = dequantize(&quantize(&code));
        for k in 0..4 {
            assert!((code[k] - back[k]).abs() < 1.0 / 65535.0);
        }
        assert!(bucket(&code) < BUCKETS.pow(4));
    }
}
