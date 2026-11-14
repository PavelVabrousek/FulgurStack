use super::super::rgb_rejection::combine_dense;
use super::super::{ClippingScratch, IntegrationConfig, Rejection};
use super::*;
use fulgur_core::Error;
use std::sync::atomic::AtomicBool;

fn kernels() -> Vec<Kernel> {
    let mut all = vec![Kernel::Scalar];
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            all.push(Kernel::Avx2);
        }
    }
    if std::env::var("FULGURSTACK_TEST_SIMD").as_deref() == Ok("1") {
        assert!(
            all.len() > 1,
            "Explicitly required local vector column sorting is unavailable."
        );
    }
    all
}

struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.unit(), self.unit());
        (-2. * u.ln()).sqrt() * (2. * std::f64::consts::PI * v).cos()
    }
}

#[test]
fn the_network_sorts_every_zero_one_column_and_random_columns() {
    for kernel in kernels() {
        let lanes = kernel.lanes();
        // By the zero-one principle this proves the network for each length.
        for n in 1..=16 {
            let patterns: Vec<usize> = (0..1 << n).collect();
            for group in patterns.chunks(lanes) {
                let mut rows = vec![1.; n * lanes];
                for (lane, &pattern) in group.iter().enumerate() {
                    for row in 0..n {
                        rows[row * lanes + lane] = (pattern >> row & 1) as f32;
                    }
                }
                let mut expected = rows.clone();
                kernel.sort(&mut rows, n);
                for lane in 0..lanes {
                    let mut column: Vec<f32> =
                        expected[lane..].iter().step_by(lanes).copied().collect();
                    column.sort_by(f32::total_cmp);
                    for (row, value) in column.into_iter().enumerate() {
                        expected[row * lanes + lane] = value;
                    }
                }
                assert_eq!(rows, expected, "{kernel:?}, n={n}, {group:?}");
            }
        }
        let mut random = Lcg(5);
        for n in [17, 31, 33, 100, 941, 1024, 1025, 1083, 2048, 2100] {
            let mut rows: Vec<f32> = (0..n * lanes)
                .map(|_| {
                    let draw = random.unit();
                    if draw < 0.05 {
                        f32::INFINITY
                    } else if draw < 0.1 {
                        -7.5
                    } else {
                        (random.normal() * 100.) as f32
                    }
                })
                .collect();
            let before = rows.clone();
            kernel.sort(&mut rows, n);
            for lane in 0..lanes {
                let mut expected: Vec<f32> =
                    before[lane..].iter().step_by(lanes).copied().collect();
                expected.sort_by(f32::total_cmp);
                let actual: Vec<f32> = rows[lane..].iter().step_by(lanes).copied().collect();
                assert_eq!(actual, expected, "{kernel:?}, n={n}, lane {lane}");
            }
        }
    }
}

/// Stacks with sky noise, outliers, equal saturated samples, unequal coverage
/// and channel-dependent masks. `mono` repeats one plane in all channels.
fn stack(count: usize, pixels: usize, seed: u64, mono: bool) -> (Vec<Vec<[f32; 3]>>, Vec<f64>) {
    let mut random = Lcg(seed);
    let mut frames = vec![vec![[0.; 3]; pixels]; count];
    for p in 0..pixels {
        let level = 2000. + 3000. * random.unit();
        let spread = if p % 7 == 0 { 300. } else { 25. };
        let covered = if p % 5 == 0 {
            count
        } else {
            (count as f64 * random.unit()).ceil() as usize
        };
        for (i, frame) in frames.iter_mut().enumerate() {
            let mut sample = [0.; 3];
            for (c, value) in sample.iter_mut().enumerate() {
                let draw = random.unit();
                let clean = level + c as f64 * 10. + spread * random.normal();
                *value = if draw < 0.004 {
                    (clean + 5000. + 40000. * random.unit()) as f32
                } else if draw < 0.006 {
                    (clean * 0.25) as f32
                } else if p % 11 == 0 && draw < 0.08 {
                    65535.
                } else if p % 13 == 0 && draw < 0.05 {
                    f32::NAN
                } else {
                    clean as f32
                };
            }
            if p % 17 == 0 {
                sample[1] = f32::INFINITY;
            }
            if p % 19 == 0 {
                sample = [f32::NEG_INFINITY; 3];
            }
            if i >= covered {
                sample = [f32::NAN; 3];
            }
            if mono {
                sample = [sample[0]; 3];
            }
            frame[p] = sample;
        }
    }
    let weights = (0..count).map(|_| 0.2 + random.unit()).collect();
    (frames, weights)
}

/// The per-pixel scalar path this band replaces.
fn oracle(
    frames: &[Vec<[f32; 3]>],
    weights: &[f64],
    offset: usize,
    output: &mut [IntegratedPixel],
    input: usize,
    total: f64,
) -> RowStats {
    let config = IntegrationConfig {
        rejection: Rejection::Esd,
        ..Default::default()
    };
    let mut result = RowStats {
        min_n: frames.len(),
        min_fraction: 1.,
        min_surviving: 1.,
        ..Default::default()
    };
    let mut rgb = PreparedRgb::with_capacity(frames.len());
    let mut scratch = ClippingScratch::with_capacity(frames.len());
    let mut stack = vec![[0.; 3]; frames.len()];
    for (x, pixel) in output.iter_mut().enumerate() {
        for (sample, frame) in stack.iter_mut().zip(frames) {
            *sample = frame[offset + x];
        }
        let covered = rgb.prepare(&stack, weights);
        let n = rgb.samples[0].len();
        result.min_n = result.min_n.min(n);
        result.max_n = result.max_n.max(n);
        result.min_fraction = result.min_fraction.min(covered / total);
        result.zero += u64::from(n == 0);
        for c in 0..3 {
            let values = &rgb.samples[c];
            let (value, low, high, surviving) =
                combine_dense(values, rgb.channel_weights(c), &mut scratch, &config);
            pixel.0[c] = value;
            pixel.1[c] = ((values.len() as u64 - low - high) as f64 / input as f64) as f32;
            pixel.2[c] = (high as f64 / input as f64) as f32;
            result.valid += values.len() as u64;
            result.low += low;
            result.high += high;
            result.min_surviving = result.min_surviving.min(surviving / total);
        }
    }
    result
}

fn ulps(a: f32, b: f32) -> u32 {
    let key = |v: f32| {
        let bits = v.to_bits() as i32;
        bits ^ (((bits >> 31) as u32) >> 1) as i32
    };
    key(a).abs_diff(key(b))
}

#[test]
fn bands_match_the_scalar_test_on_every_kernel() {
    let cancel = AtomicBool::new(false);
    let all = kernels();
    for count in [1, 2, 4, 5, 6, 7, 27, 32, 33, 100, 300, 941] {
        for (seed, mono) in [(3, false), (9, true)] {
            for length in [1, 15, 31, 32, 33, 97] {
                let (frames, weights) = stack(count, length + 7, seed + count as u64, mono);
                let total: f64 = weights.iter().sum();
                let label = format!("count={count}, mono={mono}, length={length}");
                let mut expected = vec![([0.; 3], [0.; 3], [0.; 3]); length];
                let e = oracle(&frames, &weights, 7, &mut expected, count + 3, total);
                let mut first: Option<Vec<IntegratedPixel>> = None;
                for &kernel in &all {
                    let mut actual = vec![([0.; 3], [0.; 3], [0.; 3]); length];
                    let a = reduce_band_with(
                        kernel,
                        &frames,
                        7,
                        &mut actual,
                        &ReductionContext {
                            weights: &weights,
                            original_input_count: count + 3,
                            total_weight: total,
                            config: &IntegrationConfig::default(),
                            cancel: &cancel,
                        },
                    )
                    .unwrap();
                    assert_eq!(
                        (a.valid, a.low, a.high, a.zero, a.min_n, a.max_n),
                        (e.valid, e.low, e.high, e.zero, e.min_n, e.max_n),
                        "{kernel:?}, {label}"
                    );
                    assert_eq!(a.min_fraction.to_bits(), e.min_fraction.to_bits());
                    assert!((a.min_surviving - e.min_surviving).abs() < 1e-12);
                    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
                        for c in 0..3 {
                            assert!(
                                ulps(a.0[c], e.0[c]) <= 1,
                                "{kernel:?}, {label}, pixel {i}: {} vs {}",
                                a.0[c],
                                e.0[c]
                            );
                        }
                        assert_eq!(a.1.map(f32::to_bits), e.1.map(f32::to_bits), "{label}");
                        assert_eq!(a.2.map(f32::to_bits), e.2.map(f32::to_bits), "{label}");
                    }
                    let bits = |pixels: &[IntegratedPixel]| {
                        pixels
                            .iter()
                            .map(|p| p.0.map(f32::to_bits))
                            .collect::<Vec<_>>()
                    };
                    match &first {
                        Some(other) => assert_eq!(bits(&actual), bits(other), "{label}"),
                        None => first = Some(actual),
                    }
                }
            }
        }
    }
}

#[test]
fn a_ten_sample_tie_at_the_rejection_limit_goes_through_the_scalar_choice() {
    // Equal saturated samples above a tight sky: ESD removes only some of
    // them, so the survivors depend on which equal sample stays.
    let mut random = Lcg(21);
    let mut values: Vec<f32> = (0..60)
        .map(|_| (1000. + 5. * random.normal()) as f32)
        .collect();
    values.extend([1400.; 10]);
    let weights: Vec<f64> = (0..70).map(|i| 0.5 + i as f64 / 70.).collect();
    let stacked: Vec<Vec<[f32; 3]>> = values.iter().map(|&v| vec![[v; 3]]).collect();
    let total: f64 = weights.iter().sum();
    let mut expected = vec![([0.; 3], [0.; 3], [0.; 3]); 1];
    let e = oracle(&stacked, &weights, 0, &mut expected, 70, total);
    for kernel in kernels() {
        let mut actual = vec![([0.; 3], [0.; 3], [0.; 3]); 1];
        let a = reduce_band_with(
            kernel,
            &stacked,
            0,
            &mut actual,
            &ReductionContext {
                weights: &weights,
                original_input_count: 70,
                total_weight: total,
                config: &IntegrationConfig::default(),
                cancel: &AtomicBool::new(false),
            },
        )
        .unwrap();
        assert_eq!((a.low, a.high), (e.low, e.high));
        assert_eq!(
            actual[0].0.map(f32::to_bits),
            expected[0].0.map(f32::to_bits)
        );
    }
}

#[test]
fn a_cancelled_band_stops() {
    let frames = vec![vec![[1.; 3]; 4]; 6];
    let mut output = vec![([0.; 3], [0.; 3], [0.; 3]); 4];
    assert!(matches!(
        reduce_band(
            &frames,
            0,
            &mut output,
            &ReductionContext {
                weights: &[1.; 6],
                original_input_count: 6,
                total_weight: 6.,
                config: &IntegrationConfig::default(),
                cancel: &AtomicBool::new(true)
            }
        ),
        Err(Error::Cancelled)
    ));
}
