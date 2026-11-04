//! The composed registration source -> sky -> reference expressed as the
//! carrier integration already resamples with: [`Warp`], a similarity plus a
//! cubic residual in source pixels. The fit error is measured, never assumed.
use super::solve::Solution;
use crate::distortion::Warp;
use crate::geometry::Transform;
use nalgebra::{SMatrix, SVector};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Carrier {
    pub warp: Warp,
    /// The source parity differs from the reference: its rows are reversed
    /// first, as integration does for mirrored cameras (`rows_mirrored`).
    pub mirrored_rows: bool,
    /// Part of the source (after any row reversal) that lands on the
    /// reference, in pixel-edge coordinates.
    pub field_min: [f64; 2],
    pub field_max: [f64; 2],
    /// Carrier against the exact composition over that part, reference pixels.
    pub max_error_px: f64,
    pub rms_error_px: f64,
}

/// FulgurStack's cubic basis order (`distortion::basis`).
fn basis(x: f64, y: f64) -> [f64; 10] {
    [
        1.,
        x,
        y,
        x * y,
        x * x,
        y * y,
        x * x * x,
        x * x * y,
        x * y * y,
        y * y * y,
    ]
}

fn similarity(src: &[[f64; 2]], dst: &[[f64; 2]]) -> Option<Transform> {
    let n = src.len() as f64;
    let ms = src
        .iter()
        .fold([0., 0.], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
    let md = dst
        .iter()
        .fold([0., 0.], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
    let (mut sq, mut a, mut b) = (0., 0., 0.);
    for (s, d) in src.iter().zip(dst) {
        let q = [s[0] - ms[0], s[1] - ms[1]];
        let p = [d[0] - md[0], d[1] - md[1]];
        sq += q[0] * q[0] + q[1] * q[1];
        a += q[0] * p[0] + q[1] * p[1];
        b += q[0] * p[1] - q[1] * p[0];
    }
    if sq <= 0. {
        return None;
    }
    let (a, b) = (a / sq, b / sq);
    Some(Transform {
        scale_factor: a.hypot(b),
        theta_rad: b.atan2(a),
        tx_px: md[0] - (a * ms[0] - b * ms[1]),
        ty_px: md[1] - (b * ms[0] + a * ms[1]),
    })
}

/// Least-squares cubic on point pairs (source, reference): a similarity base
/// and the ten-term residual, normalized over the field's pixel centres.
fn fit_cubic(center: [f64; 2], half: [f64; 2], pairs: &[([f64; 2], [f64; 2])]) -> Option<Warp> {
    let base = similarity(
        &pairs.iter().map(|f| f.0).collect::<Vec<_>>(),
        &pairs.iter().map(|f| f.1).collect::<Vec<_>>(),
    )?;
    let mut matrix = SMatrix::<f64, 10, 10>::zeros();
    let mut rhs = [SVector::<f64, 10>::zeros(), SVector::<f64, 10>::zeros()];
    for (p, q) in pairs {
        let b = SVector::<f64, 10>::from_row_slice(&basis(
            (p[0] - center[0]) / half[0],
            (p[1] - center[1]) / half[1],
        ));
        let linear = base.apply(*p);
        matrix += b * b.transpose();
        rhs[0] += b * (q[0] - linear[0]);
        rhs[1] += b * (q[1] - linear[1]);
    }
    let solver = matrix.cholesky()?;
    let mut warp = Warp {
        base,
        center,
        normalization: half,
        coefficients: [[0.; 10]; 2],
    };
    for (c, rhs) in warp.coefficients.iter_mut().zip(rhs) {
        c.copy_from_slice(solver.solve(&rhs).as_slice());
    }
    warp.valid().then_some(warp)
}

/// Pairs a cubic needs before it is fitted on stars alone.
pub const MIN_FIT_PAIRS: usize = 40;

/// The cubic on star pairs (source pixel in the carrier's input rows,
/// reference pixel), on the carrier's field. The plate solutions identify the
/// stars; the map then follows the stars themselves, so plate-solution
/// polynomials that cannot be composed exactly into one cubic (a large sensor
/// with strong distortion) cost nothing (ADR-137).
pub fn fit_pairs(carrier: &Carrier, pairs: &[([f64; 2], [f64; 2])]) -> Option<Warp> {
    if pairs.len() < MIN_FIT_PAIRS {
        return None;
    }
    let inside: Vec<([f64; 2], [f64; 2])> = pairs
        .iter()
        .copied()
        .filter(|(p, _)| {
            (0..2).all(|k| p[k] >= carrier.field_min[k] && p[k] <= carrier.field_max[k] - 1.)
        })
        .collect();
    if inside.len() < MIN_FIT_PAIRS {
        return None;
    }
    fit_cubic(carrier.warp.center, carrier.warp.normalization, &inside)
}

/// The carrier of `source` registered onto `reference`, or None when the
/// frames share no field or the cubic is not a valid (non-folding) map.
pub fn carry(source: &Solution, reference: &Solution) -> Option<Carrier> {
    let (w, h) = (source.wcs.width as f64, source.wcs.height as f64);
    let exact = |p: [f64; 2]| {
        let (ra, dec) = source.wcs.pixel_to_sky(p);
        reference.wcs.sky_to_pixel(ra, dec)
    };
    // Parity from the Jacobian at the source centre.
    let c = [(w - 1.) / 2., (h - 1.) / 2.];
    let (p0, px, py) = (
        exact(c)?,
        exact([c[0] + 1., c[1]])?,
        exact([c[0], c[1] + 1.])?,
    );
    let det = (px[0] - p0[0]) * (py[1] - p0[1]) - (px[1] - p0[1]) * (py[0] - p0[0]);
    let mirrored_rows = det < 0.;
    let map = |p: [f64; 2]| {
        if mirrored_rows {
            exact([p[0], h - 1. - p[1]])
        } else {
            exact(p)
        }
    };
    // The part of the source that lands on the reference.
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    const N: usize = 64;
    for j in 0..=N {
        for i in 0..=N {
            let p = [
                i as f64 / N as f64 * (w - 1.),
                j as f64 / N as f64 * (h - 1.),
            ];
            if map(p).is_some_and(|q| reference.wcs.inside(q, 0.)) {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
    }
    if !(lo[0] < hi[0] && lo[1] < hi[1]) {
        return None;
    }
    let step = [(w - 1.) / N as f64, (h - 1.) / N as f64];
    let field_min = [
        (lo[0] - step[0]).max(0.).floor(),
        (lo[1] - step[1]).max(0.).floor(),
    ];
    let field_max = [
        (hi[0] + step[0] + 1.).min(w).ceil(),
        (hi[1] + step[1] + 1.).min(h).ceil(),
    ];
    let center = [
        (field_min[0] + field_max[0] - 1.) / 2.,
        (field_min[1] + field_max[1] - 1.) / 2.,
    ];
    let half = [
        (field_max[0] - field_min[0] - 1.) / 2.,
        (field_max[1] - field_min[1] - 1.) / 2.,
    ];
    let grid = |n: usize| -> Vec<([f64; 2], [f64; 2])> {
        let mut out = vec![];
        for j in 0..=n {
            for i in 0..=n {
                let nx = i as f64 / n as f64 * 2. - 1.;
                let ny = j as f64 / n as f64 * 2. - 1.;
                let p = [center[0] + half[0] * nx, center[1] + half[1] * ny];
                if let Some(q) = map(p) {
                    out.push((p, q));
                }
            }
        }
        out
    };
    let fit = grid(24);
    let warp = fit_cubic(center, half, &fit)?;
    let errors: Vec<f64> = grid(48)
        .iter()
        .map(|(p, q)| {
            let r = warp.forward(*p);
            (r[0] - q[0]).hypot(r[1] - q[1])
        })
        .collect();
    Some(Carrier {
        warp,
        mirrored_rows,
        field_min,
        field_max,
        max_error_px: errors.iter().copied().fold(0., f64::max),
        rms_error_px: (errors.iter().map(|e| e * e).sum::<f64>() / errors.len().max(1) as f64)
            .sqrt(),
    })
}
