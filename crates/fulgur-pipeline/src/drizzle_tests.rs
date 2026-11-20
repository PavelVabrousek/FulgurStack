use super::super::{Drizzle, IntegrationConfig, ReductionContext, Rejection};
use super::{Frame, clipped_area, drop_into, reduce_tile, reference_coordinate};
use crate::geometry::Transform;
use fulgur_core::{DemosaicMethod, RgbImage};
use std::sync::atomic::AtomicBool;

const SENSOR: [usize; 2] = [64, 48];

fn frame(tx: f64, ty: f64, theta: f64) -> Frame<'static> {
    Frame {
        base: Transform {
            scale_factor: 1.,
            theta_rad: theta,
            tx_px: tx,
            ty_px: ty,
        },
        distortion: None,
        sensor: SENSOR,
        scale: 1.,
        offset: [0.; 3],
    }
}

fn image(value: impl Fn(usize, usize) -> [f32; 3]) -> RgbImage {
    RgbImage {
        width: SENSOR[0],
        height: SENSOR[1],
        demosaic_method: DemosaicMethod::None,
        pixels: (0..SENSOR[0] * SENSOR[1])
            .map(|p| value(p % SENSOR[0], p / SENSOR[0]))
            .collect(),
    }
}

/// Values and weights of a drizzled tile.
fn drizzle(
    frame: &Frame<'_>,
    rgb: &RgbImage,
    origin: [i32; 2],
    size: [usize; 2],
    drizzle: Drizzle,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let n = size[0] * size[1];
    let mut out = vec![[f32::NAN; 3]; n];
    out.resize(2 * n, [0.; 3]);
    drop_into(
        frame,
        origin,
        size,
        [0, 0, SENSOR[0], SENSOR[1]],
        rgb,
        drizzle,
        &AtomicBool::new(false),
        &mut out,
    )
    .unwrap();
    let weights = out.split_off(n);
    (out, weights)
}

#[test]
fn clipped_area_is_exact() {
    let square = |h: f64, c: [f64; 2]| {
        [[-h, -h], [h, -h], [h, h], [-h, h]].map(|p| [p[0] + c[0], p[1] + c[1]])
    };
    assert!((clipped_area(&square(0.25, [0., 0.]), 0.43) - 0.25).abs() < 1e-12);
    assert!((clipped_area(&square(0.25, [0.43, 0.]), 0.43) - 0.125).abs() < 1e-12);
    assert_eq!(clipped_area(&square(0.25, [1., 0.]), 0.43), 0.);
    // A unit square turned 45 degrees around the centre of a unit drop.
    let r = 0.5_f64.sqrt();
    let diamond = [[0., -r], [r, 0.], [0., r], [-r, 0.]];
    let expected = 1. - 4. * 0.5 * (r - 0.5) * (r - 0.5) * 2.;
    assert!((clipped_area(&diamond, 0.5) - expected).abs() < 1e-12);
}

#[test]
fn a_flat_frame_drizzles_to_its_level_with_uniform_coverage() {
    let rgb = image(|_, _| [100., 200., 300.]);
    let (values, weights) = drizzle(
        &frame(0., 0., 0.),
        &rgb,
        [10, 10],
        [40, 30],
        Drizzle::default(),
    );
    for (v, w) in values.iter().zip(&weights) {
        assert_eq!(*v, [100., 200., 300.]);
        // A 0.5 px output pixel overlaps 0.43 x 0.43 of one 0.86 px drop.
        assert!((w[0] - 0.43 * 0.43 / 0.25).abs() < 1e-5, "{w:?}");
    }
    // A turned and shifted frame keeps the level everywhere it covers.
    let (values, _) = drizzle(
        &frame(0.37, -0.21, 0.3),
        &rgb,
        [20, 20],
        [30, 20],
        Drizzle::default(),
    );
    assert!(values.iter().all(|v| v.iter().all(|x| x.is_nan()
        || (x - 100.).abs() < 1e-3
        || (x - 200.).abs() < 1e-3
        || (x - 300.).abs() < 1e-3)));
}

#[test]
fn bayer_samples_drop_only_into_their_own_channel() {
    // RGGB: red at even/even, blue at odd/odd, green elsewhere.
    let rgb = image(|x, y| match (x % 2, y % 2) {
        (0, 0) => [10., f32::NAN, f32::NAN],
        (1, 1) => [f32::NAN, f32::NAN, 30.],
        _ => [f32::NAN, 20., f32::NAN],
    });
    // Output pixels 8..48 and 8..40 lie over input columns 4..24, rows 4..20.
    let (values, weights) = drizzle(
        &frame(0., 0., 0.),
        &rgb,
        [8, 8],
        [40, 32],
        Drizzle::default(),
    );
    let (mut red, mut blue, mut green) = (0, 0, 0);
    for (v, w) in values.iter().zip(&weights) {
        for (c, expected) in [10., 20., 30.].into_iter().enumerate() {
            if w[c] > 0. {
                assert_eq!(v[c], expected);
            } else {
                assert!(v[c].is_nan());
            }
        }
        red += usize::from(w[0] > 0.);
        green += usize::from(w[1] > 0.);
        blue += usize::from(w[2] > 0.);
    }
    // Each output pixel lies under one input pixel: a quarter red, a quarter
    // blue, half green.
    assert_eq!(red * 4, values.len());
    assert_eq!(blue * 4, values.len());
    assert_eq!(green * 2, values.len());
}

/// An undersampled star seen by many dithered frames: drizzle keeps its
/// flux and position and recovers a width close to the true one.
#[test]
fn dithered_frames_drizzle_an_undersampled_star_sharply() {
    let sigma = 0.55;
    let star = [32.3, 24.7];
    let star_value = |x: f64, y: f64| {
        // Pixel-integrated Gaussian, by 8 x 8 sub-samples.
        let mut v = 0.;
        for j in 0..8 {
            for i in 0..8 {
                let (sx, sy) = (
                    x - 0.5 + (i as f64 + 0.5) / 8.,
                    y - 0.5 + (j as f64 + 0.5) / 8.,
                );
                let r2 = (sx - star[0]).powi(2) + (sy - star[1]).powi(2);
                v += (-r2 / (2. * sigma * sigma)).exp() / 64.;
            }
        }
        1000. * v / (2. * std::f64::consts::PI * sigma * sigma)
    };
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut uniform = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let size = [40, 40];
    let origin = [44, 30];
    let mut frames = Vec::new();
    for _ in 0..64 {
        // Frame shifted by (dx, dy): reference = source + (dx, dy).
        let (dx, dy) = (uniform() * 4. - 2., uniform() * 4. - 2.);
        let rgb = image(|x, y| [star_value(x as f64 + dx, y as f64 + dy) as f32; 3]);
        let (values, weights) = drizzle(&frame(dx, dy, 0.), &rgb, origin, size, Drizzle::default());
        let mut both = values;
        both.extend(weights);
        frames.push(both);
    }
    let config = IntegrationConfig {
        rejection: Rejection::Winsorized,
        sigma_low: 5.,
        sigma_high: 5.,
        ..IntegrationConfig::default()
    };
    let weights = vec![1.; frames.len()];
    let cancel = AtomicBool::new(false);
    let context = ReductionContext {
        weights: &weights,
        original_input_count: frames.len(),
        total_weight: frames.len() as f64,
        config: &config,
        cancel: &cancel,
    };
    let mut tile = Vec::new();
    reduce_tile(&frames, &mut tile, size, &context).unwrap();
    // Moments in reference pixels: an output pixel is a quarter of the area.
    let (mut flux, mut mx, mut my) = (0., 0., 0.);
    for (p, pixel) in tile.iter().enumerate() {
        let x = reference_coordinate((origin[0] + (p % size[0]) as i32) as f64, 2.);
        let y = reference_coordinate((origin[1] + (p / size[0]) as i32) as f64, 2.);
        let v = f64::from(pixel.0[0]) * 0.25;
        flux += v;
        mx += v * x;
        my += v * y;
    }
    let (cx, cy) = (mx / flux, my / flux);
    let mut var = 0.;
    for (p, pixel) in tile.iter().enumerate() {
        let x = reference_coordinate((origin[0] + (p % size[0]) as i32) as f64, 2.);
        let y = reference_coordinate((origin[1] + (p / size[0]) as i32) as f64, 2.);
        var += f64::from(pixel.0[0]) * 0.25 * ((x - cx).powi(2) + (y - cy).powi(2)) / 2.;
    }
    let measured = (var / flux).sqrt();
    // True star, blurred by the input pixel, the drop and the output pixel.
    let expected = (sigma * sigma + (1. + 0.86 * 0.86 + 0.25) / 12.).sqrt();
    assert!((flux - 1000.).abs() < 10., "flux {flux}");
    assert!(
        (cx - star[0]).abs() < 0.03 && (cy - star[1]).abs() < 0.03,
        "{cx} {cy}"
    );
    assert!(
        (measured - expected).abs() < 0.05,
        "sigma {measured} vs {expected}"
    );
}
