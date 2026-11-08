//! A plate solution as FITS WCS cards (ADR-155): the gnomonic projection
//! (TAN) about the solution's own tangent point, its linear part as the CD
//! matrix, and its higher terms as SIP distortion. The conversion is exact:
//! the polynomial is re-expanded about the pixel of the tangent point, and
//! the forward SIP terms are its non-linear part in pixel space. The inverse
//! SIP terms (AP, BP) are fitted to the forward model over the frame.
//!
//! Pixels follow the master as stored: FITS pixel (i, j) is master pixel
//! (i - 1, j - 1), rows top-down, the first stored row being j = 1.

use super::solve::Wcs;

/// One FITS card: keyword, value as it is written, comment.
pub type Card = (String, String, &'static str);

/// Highest SIP order the solver produces.
const MAX_ORDER: usize = 3;

/// `wcs` describes the grid; `offset` is the grid pixel of master pixel (0, 0)
/// (CROPX, CROPY). None when the solution has no usable linear part.
pub fn cards(wcs: &Wcs, offset: [f64; 2]) -> Option<Vec<Card>> {
    let n = wcs.pixel_norm;
    let degree = wcs.forward.degree.min(MAX_ORDER);
    let expanded = |p: [f64; 2]| {
        expand(
            wcs,
            [(p[0] - wcs.centre[0]) / n, (p[1] - wcs.centre[1]) / n],
        )
    };
    // The pixel whose standard coordinates are zero: the tangent point.
    let mut p = wcs.sky_to_pixel(wcs.ra0, wcs.dec0).unwrap_or(wcs.centre);
    for _ in 0..50 {
        let g = expanded(p);
        let (f, l) = ([g[0][0][0], g[1][0][0]], linear(&g));
        let det = l[0][0] * l[1][1] - l[0][1] * l[1][0];
        if det == 0. || !det.is_finite() {
            return None;
        }
        // Newton step in normalised pixels.
        let step = [
            (l[1][1] * f[0] - l[0][1] * f[1]) / det,
            (-l[1][0] * f[0] + l[0][0] * f[1]) / det,
        ];
        p = [p[0] - step[0] * n, p[1] - step[1] * n];
        if step[0].hypot(step[1]) * n < 1e-12 {
            break;
        }
    }
    let g = expanded(p);
    let l = linear(&g);
    let det = l[0][0] * l[1][1] - l[0][1] * l[1][0];
    if det == 0. || !det.is_finite() || !p.iter().all(|v| v.is_finite()) {
        return None;
    }
    let degrees = wcs.sky_norm / 3600.;
    let cd = [
        [degrees * l[0][0] / n, degrees * l[0][1] / n],
        [degrees * l[1][0] / n, degrees * l[1][1] / n],
    ];
    let inverse = [
        [l[1][1] / det, -l[0][1] / det],
        [-l[1][0] / det, l[0][0] / det],
    ];
    // f(d) = n L^-1 N(d / n): the forward SIP terms in pixels.
    let mut a = [[0.; MAX_ORDER + 1]; MAX_ORDER + 1];
    let mut b = [[0.; MAX_ORDER + 1]; MAX_ORDER + 1];
    for i in 0..=degree {
        for j in 0..=degree - i {
            if i + j < 2 {
                continue;
            }
            let scale = n / n.powi((i + j) as i32);
            a[i][j] = scale * (inverse[0][0] * g[0][i][j] + inverse[0][1] * g[1][i][j]);
            b[i][j] = scale * (inverse[1][0] * g[0][i][j] + inverse[1][1] * g[1][i][j]);
        }
    }
    let sip = degree >= 2;
    let reference = [p[0] - offset[0] + 1., p[1] - offset[1] + 1.];
    let number = |v: f64| format!("{v:.15E}");
    let text = |v: &str| format!("'{v}'");
    let mut out: Vec<Card> = vec![
        ("WCSAXES".into(), "2".into(), "two celestial axes"),
        (
            "CTYPE1".into(),
            text(if sip { "RA---TAN-SIP" } else { "RA---TAN" }),
            "gnomonic projection",
        ),
        (
            "CTYPE2".into(),
            text(if sip { "DEC--TAN-SIP" } else { "DEC--TAN" }),
            "gnomonic projection",
        ),
        ("CUNIT1".into(), text("deg"), ""),
        ("CUNIT2".into(), text("deg"), ""),
        (
            "RADESYS".into(),
            text("ICRS"),
            "Gaia DR3 positions at the frame epoch",
        ),
        (
            "CRVAL1".into(),
            number(wcs.ra0),
            "[deg] RA of the tangent point",
        ),
        (
            "CRVAL2".into(),
            number(wcs.dec0),
            "[deg] Dec of the tangent point",
        ),
        (
            "CRPIX1".into(),
            number(reference[0]),
            "tangent point, FITS pixel",
        ),
        (
            "CRPIX2".into(),
            number(reference[1]),
            "tangent point, FITS pixel",
        ),
        ("CD1_1".into(), number(cd[0][0]), "[deg/px]"),
        ("CD1_2".into(), number(cd[0][1]), "[deg/px]"),
        ("CD2_1".into(), number(cd[1][0]), "[deg/px]"),
        ("CD2_2".into(), number(cd[1][1]), "[deg/px]"),
    ];
    if sip {
        out.push(("A_ORDER".into(), degree.to_string(), "SIP forward order"));
        for (i, row) in a.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                if i + j >= 2 && i + j <= degree {
                    out.push((format!("A_{i}_{j}"), number(v), ""));
                }
            }
        }
        out.push(("B_ORDER".into(), degree.to_string(), "SIP forward order"));
        for (i, row) in b.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                if i + j >= 2 && i + j <= degree {
                    out.push((format!("B_{i}_{j}"), number(v), ""));
                }
            }
        }
        let (ap, bp) = inverse_sip(wcs, p, &a, &b, degree)?;
        out.push(("AP_ORDER".into(), degree.to_string(), "SIP inverse order"));
        for (k, (i, j)) in powers(degree).into_iter().enumerate() {
            out.push((format!("AP_{i}_{j}"), number(ap[k]), ""));
        }
        out.push(("BP_ORDER".into(), degree.to_string(), "SIP inverse order"));
        for (k, (i, j)) in powers(degree).into_iter().enumerate() {
            out.push((format!("BP_{i}_{j}"), number(bp[k]), ""));
        }
    }
    Some(out)
}

/// The forward polynomial re-expanded about normalised pixel `q`:
/// `g[axis][i][j]` multiplies du^i dv^j.
fn expand(wcs: &Wcs, q: [f64; 2]) -> [[[f64; MAX_ORDER + 1]; MAX_ORDER + 1]; 2] {
    let mut g = [[[0.; MAX_ORDER + 1]; MAX_ORDER + 1]; 2];
    let binomial =
        |n: usize, k: usize| -> f64 { (0..k).fold(1., |c, m| c * (n - m) as f64 / (m + 1) as f64) };
    let monomials = monomials(wcs.forward.degree.min(MAX_ORDER));
    for (out, coefficients) in g.iter_mut().zip([&wcs.forward.x, &wcs.forward.y]) {
        for (&(a, b), &c) in monomials.iter().zip(coefficients.iter()) {
            for (i, row) in out.iter_mut().enumerate().take(a + 1) {
                for (j, value) in row.iter_mut().enumerate().take(b + 1) {
                    *value += c
                        * binomial(a, i)
                        * binomial(b, j)
                        * q[0].powi((a - i) as i32)
                        * q[1].powi((b - j) as i32);
                }
            }
        }
    }
    g
}

/// Exponents (of u, of v) of the solver's terms, in its order.
fn monomials(degree: usize) -> Vec<(usize, usize)> {
    let mut t = vec![(0, 0), (1, 0), (0, 1)];
    if degree >= 2 {
        t.extend([(2, 0), (1, 1), (0, 2)]);
    }
    if degree >= 3 {
        t.extend([(3, 0), (2, 1), (1, 2), (0, 3)]);
    }
    t
}

fn linear(g: &[[[f64; MAX_ORDER + 1]; MAX_ORDER + 1]; 2]) -> [[f64; 2]; 2] {
    [[g[0][1][0], g[0][0][1]], [g[1][1][0], g[1][0][1]]]
}

/// Exponents of the inverse SIP terms, all orders up to `degree`.
fn powers(degree: usize) -> Vec<(usize, usize)> {
    let mut out = vec![];
    for i in 0..=degree {
        for j in 0..=degree - i {
            out.push((i, j));
        }
    }
    out
}

fn sip(c: &[[f64; MAX_ORDER + 1]; MAX_ORDER + 1], d: [f64; 2]) -> f64 {
    let mut sum = 0.;
    for (i, row) in c.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            if v != 0. {
                sum += v * d[0].powi(i as i32) * d[1].powi(j as i32);
            }
        }
    }
    sum
}

/// AP and BP fitted by least squares on a 40 x 40 grid over the frame and a
/// margin around it: d = U + AP(U), with U = d + A(d) the intermediate pixel.
fn inverse_sip(
    wcs: &Wcs,
    p: [f64; 2],
    a: &[[f64; MAX_ORDER + 1]; MAX_ORDER + 1],
    b: &[[f64; MAX_ORDER + 1]; MAX_ORDER + 1],
    degree: usize,
) -> Option<(Vec<f64>, Vec<f64>)> {
    let n = wcs.pixel_norm;
    let terms = powers(degree);
    let (mut rows, mut dx, mut dy) = (vec![], vec![], vec![]);
    let steps = 40;
    for sy in 0..=steps {
        for sx in 0..=steps {
            let x = -0.05 * wcs.width as f64 + 1.1 * wcs.width as f64 * sx as f64 / steps as f64;
            let y = -0.05 * wcs.height as f64 + 1.1 * wcs.height as f64 * sy as f64 / steps as f64;
            let d = [x - p[0], y - p[1]];
            let u = [d[0] + sip(a, d), d[1] + sip(b, d)];
            rows.push(
                terms
                    .iter()
                    .map(|&(i, j)| (u[0] / n).powi(i as i32) * (u[1] / n).powi(j as i32))
                    .collect::<Vec<_>>(),
            );
            dx.push((d[0] - u[0]) / n);
            dy.push((d[1] - u[1]) / n);
        }
    }
    let scale = |c: Vec<f64>| -> Vec<f64> {
        c.into_iter()
            .zip(&terms)
            .map(|(v, &(i, j))| v * n / n.powi((i + j) as i32))
            .collect()
    };
    Some((
        scale(super::solve::least_squares(&rows, &dx)?),
        scale(super::solve::least_squares(&rows, &dy)?),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::catalogue::{deproject, project, separation};
    use super::super::solve::{Poly, Wcs};
    use super::*;

    fn value(cards: &[Card], key: &str) -> f64 {
        cards
            .iter()
            .find(|c| c.0 == key)
            .unwrap_or_else(|| panic!("{key}"))
            .1
            .parse()
            .unwrap()
    }

    /// A distorted, rotated, slightly mirrored-free 2160 x 3840 solution whose
    /// tangent point is off the frame centre.
    fn solution(degree: usize) -> Wcs {
        let mut x = vec![0.013, 0.98, -0.21];
        let mut y = vec![-0.007, 0.205, 0.975];
        if degree >= 2 {
            x.extend([0.0021, -0.0013, 0.0008]);
            y.extend([-0.0009, 0.0017, -0.0011]);
        }
        if degree >= 3 {
            x.extend([0.0004, -0.0002, 0.0003, -0.0001]);
            y.extend([-0.0003, 0.0001, 0.0002, -0.0004]);
        }
        Wcs {
            ra0: 23.4621,
            dec0: 30.6603,
            centre: [1079.5, 1919.5],
            pixel_norm: 1920.,
            sky_norm: 1920. * 2.25,
            forward: Poly { degree, x, y },
            // Only the starting guess of the tangent point uses it.
            inverse: Poly {
                degree: 1,
                x: vec![-0.013, 1.0, 0.21],
                y: vec![0.007, -0.21, 1.0],
            },
            width: 2160,
            height: 3840,
        }
    }

    /// Sky position of a FITS pixel by the FITS TAN-SIP rules.
    fn fits_sky(cards: &[Card], i: f64, j: f64) -> (f64, f64) {
        let d = [i - value(cards, "CRPIX1"), j - value(cards, "CRPIX2")];
        let mut u = d;
        if cards.iter().any(|c| c.0 == "A_ORDER") {
            let order = value(cards, "A_ORDER") as usize;
            for p in 0..=order {
                for q in 0..=order - p {
                    if p + q >= 2 {
                        let m = d[0].powi(p as i32) * d[1].powi(q as i32);
                        u[0] += value(cards, &format!("A_{p}_{q}")) * m;
                        u[1] += value(cards, &format!("B_{p}_{q}")) * m;
                    }
                }
            }
        }
        let x = value(cards, "CD1_1") * u[0] + value(cards, "CD1_2") * u[1];
        let y = value(cards, "CD2_1") * u[0] + value(cards, "CD2_2") * u[1];
        deproject(
            [x * 3600., y * 3600.],
            value(cards, "CRVAL1"),
            value(cards, "CRVAL2"),
        )
    }

    #[test]
    fn tan_sip_cards_reproduce_the_solution_and_invert_it() {
        for degree in [1, 2, 3] {
            let wcs = solution(degree);
            let offset = [12., -8.];
            let cards = cards(&wcs, offset).unwrap();
            let sip = cards.iter().any(|c| c.0 == "CTYPE1" && c.1.contains("SIP"));
            assert_eq!(sip, degree >= 2);
            let mut worst = 0_f64;
            for sy in 0..=12 {
                for sx in 0..=12 {
                    let grid = [2159. * sx as f64 / 12., 3839. * sy as f64 / 12.];
                    let (ra, dec) = wcs.pixel_to_sky(grid);
                    // Master pixel = grid - offset; FITS pixel = master + 1.
                    let (fra, fdec) =
                        fits_sky(&cards, grid[0] - offset[0] + 1., grid[1] - offset[1] + 1.);
                    worst = worst.max(separation(ra, dec, fra, fdec) * 3600.);
                    if degree >= 2 {
                        // The inverse terms take the sky back to the pixel.
                        let s =
                            project(fra, fdec, value(&cards, "CRVAL1"), value(&cards, "CRVAL2"))
                                .unwrap();
                        let (x, y) = (s[0] / 3600., s[1] / 3600.);
                        let det = value(&cards, "CD1_1") * value(&cards, "CD2_2")
                            - value(&cards, "CD1_2") * value(&cards, "CD2_1");
                        let u = [
                            (value(&cards, "CD2_2") * x - value(&cards, "CD1_2") * y) / det,
                            (-value(&cards, "CD2_1") * x + value(&cards, "CD1_1") * y) / det,
                        ];
                        let order = value(&cards, "AP_ORDER") as usize;
                        let mut d = u;
                        for p in 0..=order {
                            for q in 0..=order - p {
                                let m = u[0].powi(p as i32) * u[1].powi(q as i32);
                                d[0] += value(&cards, &format!("AP_{p}_{q}")) * m;
                                d[1] += value(&cards, &format!("BP_{p}_{q}")) * m;
                            }
                        }
                        let back = [
                            d[0] + value(&cards, "CRPIX1") - 1. + offset[0],
                            d[1] + value(&cards, "CRPIX2") - 1. + offset[1],
                        ];
                        let error = (back[0] - grid[0]).hypot(back[1] - grid[1]);
                        assert!(error < 0.02, "degree {degree}: inverse off by {error} px");
                    }
                }
            }
            // A millionth of an arcsecond: the conversion adds nothing.
            assert!(worst < 1e-6, "degree {degree}: {worst} arcsec");
        }
    }

    #[test]
    fn a_solution_without_a_linear_part_gives_no_cards() {
        let mut wcs = solution(1);
        wcs.forward.x = vec![0., 0., 0.];
        assert!(cards(&wcs, [0., 0.]).is_none());
    }
}
