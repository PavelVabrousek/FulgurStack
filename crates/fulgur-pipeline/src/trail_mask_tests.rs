use super::super::{
    IntegratedPixel, IntegrationConfig, ReductionContext, Rejection, TrailMask, TrailMode,
};
use super::reduce_tile;
use std::sync::atomic::AtomicBool;

const SIZE: [usize; 2] = [72, 48];
const FRAMES: usize = 80;
const SKY: f32 = 1000.;
const NOISE: f32 = 10.;
/// The moving object: start, motion per frame and peak in single-frame sigmas.
const TRAIL_START: [f32; 2] = [8., 30.];
const TRAIL_STEP: [f32; 2] = [0.6, 0.05];
const TRAIL_PEAK: f32 = 8.;
const STAR: [f32; 2] = [40., 12.];

/// Deterministic Gaussian noise (xorshift and Box-Muller).
struct Noise(u64);
impl Noise {
    fn uniform(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) as f32
    }
    fn gauss(&mut self) -> f32 {
        let (u, v) = (self.uniform().max(1e-12), self.uniform());
        (-2. * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

fn gaussian(dx: f32, dy: f32, fwhm: f32) -> f32 {
    let s = fwhm / 2.3548;
    (-(dx * dx + dy * dy) / (2. * s * s)).exp()
}

/// A mono stack with noise, a bright star whose seeing changes from frame to
/// frame and, optionally, a slowly moving faint object.
fn stack(trail: bool, star: bool) -> Vec<Vec<[f32; 3]>> {
    let mut noise = Noise(0x9e37_79b9_7f4a_7c15);
    (0..FRAMES)
        .map(|f| {
            let seeing = 2.4 + 1.6 * ((f * 7) % 11) as f32 / 10.;
            let position = [
                TRAIL_START[0] + TRAIL_STEP[0] * f as f32,
                TRAIL_START[1] + TRAIL_STEP[1] * f as f32,
            ];
            (0..SIZE[0] * SIZE[1])
                .map(|p| {
                    let (x, y) = ((p % SIZE[0]) as f32, (p / SIZE[0]) as f32);
                    let mut v = SKY + NOISE * noise.gauss();
                    if star {
                        // Same flux in every frame, spread by the seeing.
                        v += 4000.
                            * (3. / seeing).powi(2)
                            * gaussian(x - STAR[0], y - STAR[1], seeing);
                    }
                    if trail {
                        v += TRAIL_PEAK * NOISE * gaussian(x - position[0], y - position[1], 3.);
                    }
                    [v; 3]
                })
                .collect()
        })
        .collect()
}

fn config(mode: TrailMode, sigma: f64) -> IntegrationConfig {
    IntegrationConfig {
        rejection: Rejection::Esd,
        trail: TrailMask {
            mode,
            radius_px: 4.,
            sigma,
        },
        ..IntegrationConfig::default()
    }
}

fn reduce(frames: &[Vec<[f32; 3]>], config: &IntegrationConfig) -> (Vec<IntegratedPixel>, u64) {
    let weights = vec![1.; frames.len()];
    let cancel = AtomicBool::new(false);
    let context = ReductionContext {
        weights: &weights,
        original_input_count: frames.len(),
        total_weight: frames.len() as f64,
        config,
        cancel: &cancel,
    };
    let mut out = Vec::new();
    let (_, masked) =
        reduce_tile(frames, SIZE, [0, 0, SIZE[0], SIZE[1]], &mut out, &context).unwrap();
    (out, masked)
}

fn plain(frames: &[Vec<[f32; 3]>]) -> Vec<IntegratedPixel> {
    let weights = vec![1.; frames.len()];
    let cancel = AtomicBool::new(false);
    let config = config(TrailMode::Off, 2.);
    let context = ReductionContext {
        weights: &weights,
        original_input_count: frames.len(),
        total_weight: frames.len() as f64,
        config: &config,
        cancel: &cancel,
    };
    let mut out = Vec::new();
    super::super::reduce_tile(frames, &mut out, SIZE, 4, &context).unwrap();
    out
}

/// Mean excess over the sky, in master noise units, of pixels within 2 px of
/// the object's path.
fn trail_residual(master: &[IntegratedPixel]) -> f64 {
    let end = [
        TRAIL_START[0] + TRAIL_STEP[0] * (FRAMES - 1) as f32,
        TRAIL_START[1] + TRAIL_STEP[1] * (FRAMES - 1) as f32,
    ];
    let (mut sum, mut n) = (0., 0);
    for (p, pixel) in master.iter().enumerate() {
        let (x, y) = ((p % SIZE[0]) as f32, (p / SIZE[0]) as f32);
        if x < TRAIL_START[0] + 3. || x > end[0] - 3. {
            continue;
        }
        let t = (x - TRAIL_START[0]) / (end[0] - TRAIL_START[0]);
        let path_y = TRAIL_START[1] + t * (end[1] - TRAIL_START[1]);
        if (y - path_y).abs() <= 2. {
            sum += f64::from(pixel.0[0] - SKY);
            n += 1;
        }
    }
    sum / n as f64 / (f64::from(NOISE) / (FRAMES as f64).sqrt())
}

#[test]
fn without_a_trail_the_mask_matches_plain_esd() {
    let frames = stack(false, false);
    let reference = plain(&frames);
    let (masked, _) = reduce(&frames, &config(TrailMode::Hysteresis, 5.));
    for (a, b) in reference.iter().zip(&masked) {
        assert!((a.0[0] - b.0[0]).abs() < 1e-3, "{} {}", a.0[0], b.0[0]);
        assert_eq!(a.2[0], b.2[0]);
    }
}

#[test]
fn hysteresis_and_grow_remove_the_faint_edges_of_a_moving_object() {
    let frames = stack(true, false);
    let before = trail_residual(&plain(&frames));
    let (hysteresis, h_masked) = reduce(&frames, &config(TrailMode::Hysteresis, 2.));
    let (grow, g_masked) = reduce(&frames, &config(TrailMode::Grow, 2.));
    let (h, g) = (trail_residual(&hysteresis), trail_residual(&grow));
    // Plain ESD leaves a trail of several master sigmas; the mask removes
    // most of it, grow at least as much as hysteresis.
    assert!(before > 1., "plain ESD residual {before:.2}");
    assert!(h < before * 0.5, "hysteresis {h:.2} vs plain {before:.2}");
    assert!(g < before * 0.35, "grow {g:.2} vs plain {before:.2}");
    assert!(g_masked > h_masked && h_masked > 0);
    // The high-rejection map shows the wider trail.
    let wider = |m: &[IntegratedPixel]| m.iter().filter(|p| p.2[0] > 0.).count();
    assert!(wider(&grow) > wider(&plain(&frames)));
}

#[test]
fn stars_are_protected_from_the_mask() {
    let frames = stack(false, true);
    let reference = plain(&frames);
    let (grow, _) = reduce(&frames, &config(TrailMode::Grow, 2.));
    let flux = |m: &[IntegratedPixel]| -> f64 {
        m.iter()
            .enumerate()
            .filter(|(p, _)| {
                let (x, y) = ((p % SIZE[0]) as f32, (p / SIZE[0]) as f32);
                (x - STAR[0]).hypot(y - STAR[1]) <= 6.
            })
            .map(|(_, v)| f64::from(v.0[0] - SKY))
            .sum()
    };
    let (a, b) = (flux(&reference), flux(&grow));
    assert!((a - b).abs() / a < 0.005, "star flux {a:.0} -> {b:.0}");
}

#[test]
fn a_tile_with_a_halo_gives_the_pixels_of_the_whole_image() {
    let frames = stack(true, false);
    let config = config(TrailMode::Grow, 2.);
    let (whole, _) = reduce(&frames, &config);
    // The right half, read with the 5 px halo of a 4 px radius on its left.
    let left = SIZE[0] / 2 - 5;
    let crop: Vec<Vec<[f32; 3]>> = frames
        .iter()
        .map(|f| {
            (0..SIZE[1])
                .flat_map(|y| f[y * SIZE[0] + left..(y + 1) * SIZE[0]].to_vec())
                .collect()
        })
        .collect();
    let weights = vec![1.; FRAMES];
    let cancel = AtomicBool::new(false);
    let context = ReductionContext {
        weights: &weights,
        original_input_count: FRAMES,
        total_weight: FRAMES as f64,
        config: &config,
        cancel: &cancel,
    };
    let width = SIZE[0] - left;
    let mut half = Vec::new();
    reduce_tile(
        &crop,
        [width, SIZE[1]],
        [5, 0, width - 5, SIZE[1]],
        &mut half,
        &context,
    )
    .unwrap();
    for y in 0..SIZE[1] {
        for x in 0..width - 5 {
            let a = whole[y * SIZE[0] + left + 5 + x];
            let b = half[y * (width - 5) + x];
            assert_eq!(a.0[0].to_bits(), b.0[0].to_bits(), "pixel {x},{y}");
        }
    }
}
