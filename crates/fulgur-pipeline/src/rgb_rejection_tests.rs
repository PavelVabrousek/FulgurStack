use super::super::*;
use super::*;

#[allow(clippy::too_many_arguments)]
fn prior_band(
    frames: &[Vec<[f32; 3]>],
    weights: &[f64],
    offset: usize,
    output: &mut [IntegratedPixel],
    original_input_count: usize,
    total_weight: f64,
    config: &IntegrationConfig,
    cancel: &AtomicBool,
) -> Result<RowStats> {
    let mut result = RowStats {
        min_n: frames.len(),
        min_fraction: 1.,
        min_surviving: 1.,
        ..Default::default()
    };
    let mut values = Vec::with_capacity(frames.len());
    let mut scratch = ClippingScratch::with_capacity(frames.len());
    let mut packed = vec![[0.; 3]; REJECTION_BLOCK_PIXELS.min(output.len()) * frames.len()];
    for (block_index, block) in output.chunks_mut(REJECTION_BLOCK_PIXELS).enumerate() {
        check_cancelled(cancel)?;
        let packed = &mut packed[..block.len() * frames.len()];
        pack_rejection_block(
            frames,
            offset + block_index * REJECTION_BLOCK_PIXELS,
            packed,
        );
        for (pixel, stack) in block.iter_mut().zip(packed.chunks_exact(frames.len())) {
            let mut covered = Sum::default();
            let mut n = 0;
            for (sample, w) in stack.iter().zip(weights) {
                if sample[0].is_finite() {
                    covered.add(*w);
                    n += 1;
                }
            }
            result.min_n = result.min_n.min(n);
            result.max_n = result.max_n.max(n);
            result.min_fraction = result.min_fraction.min(covered.value() / total_weight);
            if n == 0 {
                result.zero += 1;
            }
            for (c, out) in pixel.0.iter_mut().enumerate() {
                values.clear();
                values.extend(
                    stack
                        .iter()
                        .enumerate()
                        .filter_map(|(i, sample)| sample[c].is_finite().then_some((sample[c], i))),
                );
                result.valid += values.len() as u64;
                let (value, low, high, surviving) = combine(&values, weights, &mut scratch, config);
                *out = value;
                pixel.1[c] = ((values.len() as u64 - low - high) as f64
                    / original_input_count as f64) as f32;
                pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                result.low += low;
                result.high += high;
                result.min_surviving = result.min_surviving.min(surviving / total_weight);
            }
        }
    }
    Ok(result)
}

#[test]
fn shared_rgb_preparation_matches_independent_channel_filters_without_reallocation() {
    let mut prepared = PreparedRgb::with_capacity(1500);
    let addresses = (
        prepared.samples.each_ref().map(|v| v.as_ptr()),
        prepared.weights.each_ref().map(|v| v.as_ptr()),
    );
    let weights: Vec<_> = (0..1500)
        .map(|i| 0.001 + ((i * 37) % 100) as f64 / 101.)
        .collect();
    for n in [0, 1, 2, 4, 5, 6, 27, 32, 940, 1140, 1500, 5, 0] {
        for mode in 0..8 {
            let stack: Vec<[f32; 3]> = (0..n)
                .map(|i| {
                    std::array::from_fn(|c| {
                        if mode == 5
                            || (mode == 1 && i % 3 == 0)
                            || (mode == 2 && (i + c) % 7 == 0)
                            || (mode == 3 && i + 1 == n && c == 2)
                            || (mode == 4 && i == 0 && c == 0)
                            || (mode == 6 && (i & (1 << c)) != 0)
                        {
                            [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][(i + c) % 3]
                        } else if mode == 7 {
                            if (i + c) % 2 == 0 { -0. } else { 0. }
                        } else {
                            ((i * 7919 + c * 31) % 65536) as f32 * 0.03125 - 100.125
                        }
                    })
                })
                .collect();
            let before: Vec<_> = stack.iter().map(|v| v.map(f32::to_bits)).collect();
            let covered = prepared.prepare(&stack, &weights[..n]);
            let mut expected = Sum::default();
            for (s, &w) in stack.iter().zip(&weights) {
                if s[0].is_finite() {
                    expected.add(w);
                }
            }
            assert_eq!(covered.to_bits(), expected.value().to_bits());
            for c in 0..3 {
                let expected: Vec<_> = stack
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v[c].is_finite())
                    .map(|(i, v)| (v[c], weights[i]))
                    .collect();
                assert_eq!(
                    prepared.samples[c]
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    expected.iter().map(|v| v.0.to_bits()).collect::<Vec<_>>()
                );
                assert_eq!(
                    prepared
                        .channel_weights(c)
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    expected.iter().map(|v| v.1.to_bits()).collect::<Vec<_>>()
                );
            }
            assert_eq!(
                (
                    prepared.samples.each_ref().map(|v| v.as_ptr()),
                    prepared.weights.each_ref().map(|v| v.as_ptr())
                ),
                addresses
            );
            assert_eq!(
                stack
                    .iter()
                    .map(|v| v.map(f32::to_bits))
                    .collect::<Vec<_>>(),
                before
            );
            if mode == 0 || mode == 1 || mode == 5 || mode == 7 {
                assert!(prepared.shared);
            }
            if n > 0 && (mode == 3 || mode == 4) {
                assert!(!prepared.shared);
            }
        }
    }
}

#[test]
fn dense_rgb_combiner_matches_prior_tuple_bits_and_rejection_counts() {
    let mut scratch = ClippingScratch::with_capacity(1500);
    let mut prior_scratch = ClippingScratch::with_capacity(1500);
    let mut prepared = PreparedRgb::with_capacity(1500);
    let weights: Vec<_> = (0..1500)
        .map(|i| {
            if i % 31 == 0 {
                0.
            } else {
                0.001 + ((i * 37) % 100) as f64 / 101.
            }
        })
        .collect();
    let mut seed = 0x123456789abcdef_u64;
    for n in [0, 1, 2, 4, 5, 6, 27, 32, 33, 940, 1140, 1500] {
        for mode in 0..16 {
            let stack: Vec<_> = (0..n)
                .map(|i| {
                    std::array::from_fn(|c| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let bits = (seed >> 32) as u32;
                        if mode < 8 && (i + c + mode) % 19 == 0 {
                            f32::NAN
                        } else {
                            match mode % 8 {
                                0 => f32::from_bits(bits & 0xfeffffff),
                                1 => f32::from_bits(bits & 0x807fffff),
                                2 => {
                                    if i % 2 == 0 {
                                        -0.
                                    } else {
                                        0.
                                    }
                                }
                                3 => 65535. + (bits % 500) as f32 / 8192.,
                                4 => {
                                    if i % 19 == 0 {
                                        60000.
                                    } else {
                                        2064.
                                    }
                                }
                                5 => (bits % 10000) as f32 / 8192. - 0.5,
                                6 => {
                                    if i % 7 == 0 {
                                        -f32::MAX
                                    } else {
                                        f32::MAX
                                    }
                                }
                                _ => (bits % 10000) as f32 - 2000.,
                            }
                        }
                    })
                })
                .collect();
            prepared.prepare(&stack, &weights[..n]);
            for (sigma_low, sigma_high) in [
                (3., 2.5),
                (1., 1.),
                (0.3, 0.6),
                (1e-10, 100.),
                (100., 1e-10),
            ] {
                let config = IntegrationConfig {
                    sigma_low,
                    sigma_high,
                    ..Default::default()
                };
                for c in 0..3 {
                    let tuples: Vec<_> = stack
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| v[c].is_finite().then_some((v[c], i)))
                        .collect();
                    let a = combine_dense(
                        &prepared.samples[c],
                        prepared.channel_weights(c),
                        &mut scratch,
                        &config,
                    );
                    let e = combine(&tuples, &weights, &mut prior_scratch, &config);
                    assert_eq!(
                        (a.0.to_bits(), a.1, a.2, a.3.to_bits()),
                        (e.0.to_bits(), e.1, e.2, e.3.to_bits()),
                        "n={n}, mode={mode}, channel={c}"
                    );
                }
            }
        }
    }
}

#[test]
fn shared_rgb_reducer_matches_frozen_band_all_maps_statistics_and_memory_bound() {
    for n in [1, 4, 5, 27, 32, 940, 1500] {
        let prepared = PreparedRgb::with_capacity(n);
        let scratch = ClippingScratch::with_capacity(n);
        let payload = prepared
            .samples
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<f32>())
            .sum::<usize>()
            + prepared
                .weights
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<f64>())
                .sum::<usize>()
            + (scratch.winsorized.capacity() + scratch.deviations.capacity())
                * std::mem::size_of::<f32>();
        assert_eq!(payload, n * 44);
        assert!(payload <= n * 64); // Existing conservative planner allowance.
        let weights: Vec<_> = (0..n)
            .map(|i| 0.001 + ((i * 37) % 100) as f64 / 101.)
            .collect();
        let mut total = Sum::default();
        for w in &weights {
            total.add(*w);
        }
        for mode in 0..4 {
            let frames: Vec<Vec<[f32; 3]>> = (0..n)
                .map(|i| {
                    (0..83)
                        .map(|p| {
                            std::array::from_fn(|c| {
                                if p % 19 == 0
                                    || (mode == 1 && i % 7 == 0)
                                    || (mode == 2 && (i + p + c) % 23 == 0)
                                    || (mode == 3 && i + 1 == n && c == 1)
                                {
                                    f32::NAN
                                } else if (i + p + c) % 29 == 0 {
                                    65535.
                                } else {
                                    ((i * 7919 + p * 59 + c * 31) % 65536) as f32 * 0.03125
                                        - 100.125
                                }
                            })
                        })
                        .collect()
                })
                .collect();
            for length in [0, 1, 31, 32, 33, 73] {
                let mut a = vec![([0.; 3], [0.; 3], [0.; 3]); length];
                let mut e = a.clone();
                let cancel = AtomicBool::new(false);
                let config = IntegrationConfig::default();
                let ar = reduce_band(
                    &frames,
                    &weights,
                    5,
                    &mut a,
                    n + 7,
                    total.value(),
                    &config,
                    &cancel,
                )
                .unwrap();
                let er = prior_band(
                    &frames,
                    &weights,
                    5,
                    &mut e,
                    n + 7,
                    total.value(),
                    &config,
                    &cancel,
                )
                .unwrap();
                assert_eq!(
                    a.iter()
                        .map(|v| (
                            v.0.map(f32::to_bits),
                            v.1.map(f32::to_bits),
                            v.2.map(f32::to_bits)
                        ))
                        .collect::<Vec<_>>(),
                    e.iter()
                        .map(|v| (
                            v.0.map(f32::to_bits),
                            v.1.map(f32::to_bits),
                            v.2.map(f32::to_bits)
                        ))
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    (
                        ar.valid,
                        ar.low,
                        ar.high,
                        ar.zero,
                        ar.min_n,
                        ar.max_n,
                        ar.min_fraction.to_bits(),
                        ar.min_surviving.to_bits()
                    ),
                    (
                        er.valid,
                        er.low,
                        er.high,
                        er.zero,
                        er.min_n,
                        er.max_n,
                        er.min_fraction.to_bits(),
                        er.min_surviving.to_bits()
                    )
                );
            }
        }
    }
}
