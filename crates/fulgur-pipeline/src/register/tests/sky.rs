//! Sky registration on synthetic catalogues (ADR-137): frames of two cameras,
//! one of them mirrored and rotated, register through their plate solutions
//! within hundredths of a pixel, with brightness per reference pixel; a frame
//! of another sky field is refused.
use super::*;
use crate::sky::catalogue::{self, GaiaStar};

const CENTRE: (f64, f64) = (23.46, 30.66);

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
}

fn field(seed: u64, centre: (f64, f64), radius: f64, count: usize) -> Vec<GaiaStar> {
    let mut rng = Rng(seed);
    let mut stars: Vec<GaiaStar> = (0..count)
        .map(|k| {
            let r = radius * 3600. * rng.next().sqrt();
            let a = std::f64::consts::TAU * rng.next();
            let (ra, dec) = catalogue::deproject([r * a.cos(), r * a.sin()], centre.0, centre.1);
            GaiaStar {
                source_id: seed * 1_000_000 + k as u64,
                ra,
                dec,
                pmra: 0.,
                pmdec: 0.,
                g: 6. + 12. * rng.next(),
            }
        })
        .collect();
    stars.sort_by(|a, b| a.g.total_cmp(&b.g));
    stars
}

/// A camera pose: sky -> pixel through rotation, parity and scale.
#[derive(Clone, Copy)]
struct Pose {
    centre: (f64, f64),
    arcsec_per_px: f64,
    theta: f64,
    parity: f64,
    width: usize,
    height: usize,
}

impl Pose {
    fn pixel(&self, ra: f64, dec: f64) -> Option<[f64; 2]> {
        let p = catalogue::project(ra, dec, self.centre.0, self.centre.1)?;
        let (u, v) = (
            -self.parity * p[0] / self.arcsec_per_px,
            p[1] / self.arcsec_per_px,
        );
        let (sn, cs) = self.theta.sin_cos();
        let (x, y) = (cs * u - sn * v, sn * u + cs * v);
        let q = [
            x + (self.width as f64 - 1.) / 2.,
            -y + (self.height as f64 - 1.) / 2.,
        ];
        (q[0] >= 3.
            && q[1] >= 3.
            && q[0] < self.width as f64 - 4.
            && q[1] < self.height as f64 - 4.)
            .then_some(q)
    }
}

/// One frame: the sky list (brightest 600, 0.03 px noise) and an inspection
/// catalogue of 150 unsaturated stars whose flux follows G.
fn frame(index: usize, minute: usize, pose: Pose, sky: &[GaiaStar], flux: f64) -> QualityFrame {
    let mut f = planned_frame(
        index,
        &format!("2026-10-06T22:{minute:02}:00"),
        60.,
        Transform::IDENTITY,
        1.,
    );
    let info = f.frame.as_mut().unwrap();
    info.width = pose.width;
    info.height = pose.height;
    let mut rng = Rng(index as u64 + 7);
    let mut noise = || 0.03 * (rng.next() - 0.5) * 3.4;
    let visible: Vec<(&GaiaStar, [f64; 2])> = sky
        .iter()
        .filter_map(|s| pose.pixel(s.ra, s.dec).map(|p| (s, p)))
        .collect();
    f.sky_stars = visible
        .iter()
        .take(600)
        .map(|(s, p)| {
            let flux = flux * 10f64.powf(-0.4 * (s.g - 12.));
            [
                p[0] + noise(),
                p[1] + noise(),
                3.0,
                0.1,
                flux,
                1000. + flux / 20.,
            ]
        })
        .collect();
    f.stars = visible
        .iter()
        .skip(20)
        .take(150)
        .map(|(s, p)| crate::stars::Star {
            x_px: p[0],
            y_px: p[1],
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.1,
            flux_adu: flux * 10f64.powf(-0.4 * (s.g - 12.)),
            peak_adu: 20000.,
            snr: 100.,
        })
        .collect();
    f
}

fn provider(sky: Vec<GaiaStar>) -> SkyOptions {
    let sky = std::sync::Arc::new(sky);
    SkyOptions::new(
        CENTRE,
        std::sync::Arc::new(move |ra: f64, dec: f64, radius: f64, g: f64| {
            Ok(sky
                .iter()
                .filter(|s| s.g <= g && catalogue::separation(s.ra, s.dec, ra, dec) <= radius)
                .cloned()
                .collect())
        }),
    )
}

#[test]
fn frames_of_two_cameras_one_mirrored_register_through_the_sky() {
    let sky = field(3, CENTRE, 1.6, 9000);
    let main = Pose {
        centre: CENTRE,
        arcsec_per_px: 2.4,
        theta: 0.,
        parity: 1.,
        width: 2160,
        height: 3840,
    };
    let shifted = Pose {
        centre: (CENTRE.0 + 0.02, CENTRE.1 - 0.015),
        theta: 0.4_f64.to_radians(),
        ..main
    };
    // Another camera: finer pixels, turned by 90 degrees and mirrored.
    let other = Pose {
        centre: (CENTRE.0 - 0.01, CENTRE.1 + 0.01),
        arcsec_per_px: 1.5,
        theta: 90.5_f64.to_radians(),
        parity: -1.,
        width: 3000,
        height: 2000,
    };
    // A frame of another sky field, far outside the catalogue.
    let far = (CENTRE.0 + 40., CENTRE.1 - 20.);
    let elsewhere = field(9, far, 1.6, 9000);
    let frames = vec![
        frame(0, 0, main, &sky, 1000.),
        frame(1, 1, main, &sky, 900.),
        frame(2, 2, shifted, &sky, 1100.),
        frame(3, 3, other, &sky, 1000.),
        frame(4, 4, other, &sky, 500.),
        frame(
            5,
            5,
            Pose {
                centre: far,
                ..main
            },
            &elsewhere,
            1000.,
        ),
    ];
    let poses = [main, main, shifted, other, other];
    let sky_options = provider(sky.clone());
    let (report, _) = register_inspected(
        report(frames),
        std::path::PathBuf::from("."),
        Engine::Sky(&sky_options),
        &RunContext::new(
            0.8,
            RegistrationOptions {
                geometric_correction: true,
                ..Default::default()
            },
            &AtomicBool::new(false),
        ),
    )
    .unwrap();
    let summary = report.sky.as_ref().unwrap();
    assert_eq!(
        (summary.frames_to_solve, summary.solved),
        (6, 5),
        "{summary:?}"
    );
    assert_eq!(summary.registered_by_sky, 5);
    // The chronological median of the camera with the most solved frames.
    assert_eq!(report.reference_anchor, "frame_1.fit");
    assert!(report.geometric_correction_enabled);
    assert_eq!(report.camera_groups.len(), 2);
    assert!(report.camera_groups.iter().any(|c| c.mirrored));
    let refused = &report.frames[5];
    assert!(!refused.accepted);
    assert!(
        refused
            .rejection_reasons
            .iter()
            .any(|r| r == "sky_plate_solve_failed"),
        "{:?}",
        refused.rejection_reasons
    );
    let reference = main;
    for (i, pose) in poses.iter().enumerate() {
        let row = &report.frames[i];
        assert!(row.accepted, "{i}: {:?}", row.rejection_reasons);
        let decision = row.geometric_correction.as_ref().unwrap();
        assert!(decision.registration_validation.as_ref().unwrap().accepted);
        let warp = decision.model.unwrap();
        let info = report.inspection.frames[i].frame.as_ref().unwrap();
        assert_eq!(info.rows_mirrored, i >= 3, "{i}");
        // True positions through the map, against the reference's own.
        let mut worst = 0_f64;
        for s in sky.iter().take(3000) {
            let (Some(p), Some(q)) = (pose.pixel(s.ra, s.dec), reference.pixel(s.ra, s.dec)) else {
                continue;
            };
            let p = if info.rows_mirrored {
                [p[0], info.height as f64 - 1. - p[1]]
            } else {
                p
            };
            let r = warp.forward(p);
            worst = worst.max((r[0] - q[0]).hypot(r[1] - q[1]));
        }
        assert!(worst < 0.05, "{i}: worst {worst} px");
        assert!(row.footprint.len() >= 3 && row.final_weight > 0. && row.final_weight <= 1.);
    }
    // Brightness per reference pixel against the anchor's flux of 900 (the
    // plate scales are measured, so equal cameras agree to 1e-4),
    // and the finer camera's pixels cover (1.5/2.4)^2 of a reference pixel.
    let scale = |i: usize| report.frames[i].photometric_scale.unwrap();
    assert!((scale(1) - 1.).abs() < 1e-9);
    assert!((scale(0) - 0.9).abs() < 1e-4, "{}", scale(0));
    assert!((scale(2) - 900. / 1100.).abs() < 1e-4, "{}", scale(2));
    let area = (1.5_f64 / 2.4).powi(2);
    assert!(
        (scale(3) - 0.9 / area).abs() < 0.02 * 0.9 / area,
        "{}",
        scale(3)
    );
    assert!(
        (scale(4) - 1.8 / area).abs() < 0.02 * 1.8 / area,
        "{}",
        scale(4)
    );
}
