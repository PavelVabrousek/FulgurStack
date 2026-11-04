//! Synthetic recovery tests: star fields with a known pixel <-> sky relation
//! (rotation, mirroring, scale, cubic distortion, noise, false and missing
//! stars) must come back from the solver within hundredths of a pixel.
use super::catalogue::{self, GaiaStar};
use super::solve::{self, BlindCache};
use super::*;
use std::sync::Arc;

const CENTRE: (f64, f64) = (23.46, 30.66);

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn gauss(&mut self) -> f64 {
        let (u, v) = (self.next(), self.next());
        (-2. * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

/// Random stars within `radius` degrees of the centre, brightest first.
fn field(seed: u64, radius: f64, count: usize) -> Vec<GaiaStar> {
    let mut rng = Rng(seed);
    let mut stars: Vec<GaiaStar> = (0..count)
        .map(|k| {
            let r = radius * 3600. * rng.next().sqrt();
            let a = std::f64::consts::TAU * rng.next();
            let (ra, dec) = catalogue::deproject([r * a.cos(), r * a.sin()], CENTRE.0, CENTRE.1);
            GaiaStar {
                source_id: k as u64,
                ra,
                dec,
                pmra: 20. * rng.gauss(),
                pmdec: 20. * rng.gauss(),
                g: 6. + 12. * rng.next(),
            }
        })
        .collect();
    stars.sort_by(|a, b| a.g.total_cmp(&b.g));
    stars
}

/// A camera's true relation: sky -> pixel through rotation, parity, scale
/// and a radial distortion of `bend` pixels at the field corner.
struct Truth {
    centre: (f64, f64),
    arcsec_per_px: f64,
    theta: f64,
    parity: f64,
    width: usize,
    height: usize,
    bend: f64,
    epoch: f64,
}

impl Truth {
    fn pixel(&self, s: &GaiaStar) -> Option<[f64; 2]> {
        let (ra, dec) = s.at(self.epoch);
        let p = catalogue::project(ra, dec, self.centre.0, self.centre.1)?;
        // Parity +1 is the sky as seen from below: north up, east left.
        let (u, v) = (
            -self.parity * p[0] / self.arcsec_per_px,
            p[1] / self.arcsec_per_px,
        );
        let (sn, cs) = self.theta.sin_cos();
        let (x, y) = (cs * u - sn * v, sn * u + cs * v);
        let corner = (self.width as f64).hypot(self.height as f64) / 2.;
        let k = 1. + self.bend / corner * (x * x + y * y) / (corner * corner);
        let q = [
            x * k + (self.width as f64 - 1.) / 2.,
            -y * k + (self.height as f64 - 1.) / 2.,
        ];
        (q[0] >= 0. && q[1] >= 0. && q[0] < self.width as f64 && q[1] < self.height as f64)
            .then_some(q)
    }

    /// Detected stars: brightest 400 with 0.1 px noise, 10 % missing and
    /// 20 % false detections among them.
    fn detect(&self, cat: &[GaiaStar], seed: u64) -> Vec<[f64; 2]> {
        let mut rng = Rng(seed);
        let mut out = vec![];
        for s in cat {
            let Some(p) = self.pixel(s) else { continue };
            if rng.next() < 0.1 {
                continue;
            }
            if rng.next() < 0.2 {
                out.push([
                    rng.next() * self.width as f64,
                    rng.next() * self.height as f64,
                ]);
            }
            out.push([p[0] + 0.1 * rng.gauss(), p[1] + 0.1 * rng.gauss()]);
            if out.len() >= 400 {
                break;
            }
        }
        out
    }
}

/// RMS and maximum pixel error of a solution over the true star positions.
fn error(sol: &solve::Solution, truth: &Truth, cat: &[GaiaStar]) -> (f64, f64, usize) {
    let errors: Vec<f64> = cat
        .iter()
        .filter_map(|s| {
            let t = truth.pixel(s)?;
            let (ra, dec) = s.at(truth.epoch);
            let p = sol.wcs.sky_to_pixel(ra, dec)?;
            Some((p[0] - t[0]).hypot(p[1] - t[1]))
        })
        .collect();
    let rms = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();
    (rms, errors.iter().copied().fold(0., f64::max), errors.len())
}

fn camera(arcsec_per_px: f64, theta_deg: f64, parity: f64, bend: f64) -> Truth {
    Truth {
        centre: (CENTRE.0 + 0.05, CENTRE.1 - 0.04),
        arcsec_per_px,
        theta: theta_deg.to_radians(),
        parity,
        width: 3000,
        height: 2000,
        bend,
        epoch: 2024.5,
    }
}

fn solve_one(truth: &Truth, cat: &Arc<Vec<GaiaStar>>, seed: u64) -> solve::Solution {
    let stars = truth.detect(cat, seed);
    let frame = SkyFrame {
        name: "synthetic",
        width: truth.width,
        height: truth.height,
        epoch: truth.epoch,
        stars: &stars,
    };
    let cache = BlindCache::new(cat.clone());
    solve::solve(&frame, &[CENTRE], &cache, 1.0, None).expect("solved")
}

#[test]
fn rotation_mirroring_scale_and_distortion_are_recovered() {
    let cat = Arc::new(field(7, 4.5, 60_000));
    for (arcsec_per_px, theta, parity, bend) in [
        (1.5, 37., 1., 0.),
        (1.0, -120., -1., 3.),
        (0.5, 10., 1., 1.),
        (5.0, 200., -1., 4.),
    ] {
        let truth = camera(arcsec_per_px, theta, parity, bend);
        let sol = solve_one(&truth, &cat, 11);
        let (rms, max, n) = error(&sol, &truth, &cat);
        assert!(
            (sol.arcsec_per_px / arcsec_per_px - 1.).abs() < 0.01,
            "scale {} for {arcsec_per_px}",
            sol.arcsec_per_px
        );
        assert_eq!(sol.mirrored, parity < 0., "parity for {theta} deg");
        assert!(
            rms < 0.05 && max < 0.25,
            "{arcsec_per_px}\"/px {theta} deg: RMS {rms:.3} px, max {max:.3} px over {n} stars"
        );
    }
}

#[test]
fn a_camera_group_needs_one_blind_search_and_seeds_the_rest() {
    let cat = Arc::new(field(9, 3.0, 40_000));
    let mut truths = vec![];
    let mut groups = vec![];
    for (g, (scale, theta, parity)) in [(1.2, 15., 1.), (2.4, 95., -1.)].into_iter().enumerate() {
        for k in 0..4 {
            let mut t = camera(scale, theta + k as f64 * 0.3, parity, 1.);
            t.centre = (t.centre.0 + 0.01 * k as f64, t.centre.1 + 0.008 * k as f64);
            truths.push(t);
            groups.push(format!("camera {g}"));
        }
    }
    let stars: Vec<Vec<[f64; 2]>> = truths
        .iter()
        .enumerate()
        .map(|(i, t)| t.detect(&cat, 100 + i as u64))
        .collect();
    let frames: Vec<SkyFrame> = truths
        .iter()
        .zip(&stars)
        .map(|(t, s)| SkyFrame {
            name: "synthetic",
            width: t.width,
            height: t.height,
            epoch: t.epoch,
            stars: s,
        })
        .collect();
    let cache = BlindCache::new(cat.clone());
    let hints = vec![None; frames.len()];
    let (out, _) = batch::solve_all(&frames, &groups, &hints, CENTRE, &cache, 1.0);
    let blind = out.iter().filter(|o| o.pass == 1).count();
    let seeded = out.iter().filter(|o| o.pass == 2).count();
    assert_eq!((blind, seeded), (2, 6));
    for (o, t) in out.iter().zip(&truths) {
        let (rms, _, _) = error(o.solution.as_ref().unwrap(), t, &cat);
        assert!(rms < 0.05, "RMS {rms:.3} px");
    }
}

#[test]
fn the_check_passes_a_true_registration_and_fails_a_displaced_one() {
    let cat = Arc::new(field(13, 3.0, 40_000));
    let a = camera(1.0, 20., 1., 2.);
    let mut b = camera(1.6, 140., -1., 1.);
    b.width = 2400;
    let (sa, sb) = (solve_one(&a, &cat, 21), solve_one(&b, &cat, 22));
    let stars_a = a.detect(&cat, 31);
    let stars_b = b.detect(&cat, 32);
    let target = check::Target::new(&sb, &stars_b);
    assert_eq!(check::check(0, &sa, &stars_a, &target).status, "PASS");
    let mut moved = sb.clone();
    moved.wcs.centre[0] += 0.5;
    let displaced = check::Target::new(&moved, &stars_b);
    assert_eq!(check::check(0, &sa, &stars_a, &displaced).status, "FAIL");
}

#[test]
fn the_composed_map_rides_on_the_cubic_carrier() {
    let cat = Arc::new(field(17, 3.0, 40_000));
    let a = camera(1.0, 20., 1., 2.);
    let b = camera(1.3, -60., 1., 1.);
    let c = camera(1.3, -60., -1., 1.);
    let (sa, sb, sc) = (
        solve_one(&a, &cat, 41),
        solve_one(&b, &cat, 42),
        solve_one(&c, &cat, 43),
    );
    let same = warp::carry(&sa, &sb).expect("carrier");
    assert!(!same.mirrored_rows);
    assert!(same.max_error_px < 0.02, "max {:.4} px", same.max_error_px);
    // A source of the other parity is carried with its rows reversed.
    let other = warp::carry(&sa, &sc).expect("carrier");
    assert!(other.mirrored_rows);
    assert!(
        other.max_error_px < 0.02,
        "max {:.4} px",
        other.max_error_px
    );
}

#[test]
fn point_sources_leave_out_wide_and_elongated_detections() {
    let mut stars: Vec<[f64; 4]> = (0..200)
        .map(|i| [i as f64, 0., 3. + 0.01 * (i % 7) as f64, 0.2])
        .collect();
    stars.push([500., 0., 9., 0.2]);
    stars.push([501., 0., 3., 0.8]);
    let kept = point_sources(&stars);
    assert_eq!(kept.len(), 200);
}
