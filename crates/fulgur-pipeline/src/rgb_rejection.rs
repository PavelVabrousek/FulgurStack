//! Shared finite RGB preparation with source-order dense channel samples/weights.
use super::rejection_methods::{key_sample, sample_key};
use super::{
    ClippingScratch, IntegrationConfig, Rejection, Sum, clipping_fixed_point, rejection_methods,
};

pub(super) struct PreparedRgb {
    pub samples: [Vec<f32>; 3],
    weights: [Vec<f64>; 3],
    shared: bool,
}
impl PreparedRgb {
    pub fn with_capacity(sources: usize) -> Self {
        Self {
            samples: std::array::from_fn(|_| Vec::with_capacity(sources)),
            weights: std::array::from_fn(|_| Vec::with_capacity(sources)),
            shared: true,
        }
    }
    /// One traversal, preserving channel/source bits and compensated red coverage.
    pub fn prepare(&mut self, stack: &[[f32; 3]], weights: &[f64]) -> f64 {
        debug_assert_eq!(stack.len(), weights.len());
        for values in &mut self.samples {
            values.clear();
        }
        for weights in &mut self.weights {
            weights.clear();
        }
        self.shared = true;
        let mut covered = Sum::default();
        for (sample, &weight) in stack.iter().zip(weights) {
            let valid = sample.map(f32::is_finite);
            if valid == [true; 3] {
                self.samples[0].push(sample[0]);
                self.samples[1].push(sample[1]);
                self.samples[2].push(sample[2]);
                self.weights[0].push(weight);
                if !self.shared {
                    self.weights[1].push(weight);
                    self.weights[2].push(weight);
                }
                covered.add(weight);
            } else if valid != [false; 3] {
                if self.shared {
                    // Existing finite prefix had identical masks. Reuse reserved
                    // storage so a late mismatch cannot allocate inside a pixel.
                    let (red, others) = self.weights.split_at_mut(1);
                    others[0].extend_from_slice(&red[0]);
                    others[1].extend_from_slice(&red[0]);
                    self.shared = false;
                }
                for c in 0..3 {
                    if valid[c] {
                        self.samples[c].push(sample[c]);
                        self.weights[c].push(weight);
                    }
                }
                if valid[0] {
                    covered.add(weight);
                }
            }
        }
        covered.value()
    }
    /// All three channels hold the same samples with the same weights.
    pub fn identical_channels(&self) -> bool {
        self.shared && self.samples[0] == self.samples[1] && self.samples[1] == self.samples[2]
    }
    /// Total weight of a channel, given what `prepare` returned.
    pub fn channel_total(&self, channel: usize, covered: f64) -> f64 {
        if self.shared {
            return covered;
        }
        let mut total = Sum::default();
        for &weight in self.channel_weights(channel) {
            total.add(weight);
        }
        total.value()
    }
    pub fn channel_weights(&self, channel: usize) -> &[f64] {
        &self.weights[if self.shared { 0 } else { channel }]
    }
}

/// `median_bounds` on sample keys, which keeps the same order statistics.
fn key_bounds(keys: &mut [i32]) -> (f64, [f32; 2]) {
    let n = keys.len();
    let (below, middle, _) = keys.select_nth_unstable(n / 2);
    let upper = key_sample(*middle);
    let lower = match below.iter().max() {
        Some(&lower) if n % 2 == 0 => key_sample(lower),
        _ => upper,
    };
    let center = if n % 2 == 0 {
        (f64::from(upper) + f64::from(lower)) * 0.5
    } else {
        f64::from(upper)
    };
    (center, [lower, upper])
}

pub(super) fn combine_dense(
    values: &[f32],
    weights: &[f64],
    scratch: &mut ClippingScratch,
    config: &IntegrationConfig,
) -> (f32, u64, u64, f64) {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    match config.rejection {
        Rejection::Median => {
            return rejection_methods::median(values, weights, &mut scratch.methods);
        }
        Rejection::LinearFit => {
            return rejection_methods::linear_fit(values, weights, &mut scratch.methods);
        }
        Rejection::Esd => return rejection_methods::esd(values, weights, &mut scratch.methods),
        // `Auto` is resolved per stack before reduction starts.
        Rejection::Winsorized | Rejection::Auto => {}
    }
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    if values.len() >= 5 {
        let ClippingScratch {
            sample_keys,
            deviation_keys,
            ..
        } = scratch;
        sample_keys.clear();
        sample_keys.extend(values.iter().map(|&v| sample_key(v)));
        for _ in 0..3 {
            let (center, middle) = key_bounds(sample_keys);
            deviation_keys.clear();
            deviation_keys.extend(
                sample_keys
                    .iter()
                    .map(|&k| sample_key((key_sample(k) as f64 - center).abs() as f32)),
            );
            let (mad, mad_middle) = key_bounds(deviation_keys);
            let sigma = (1.4826 * mad).max(1e-6);
            low = center - config.sigma_low * sigma;
            high = center + config.sigma_high * sigma;
            if clipping_fixed_point(center, middle, mad_middle[1], low, high) {
                break;
            }
            let mut changed = false;
            for key in sample_keys.iter_mut() {
                let x = key_sample(*key);
                let next = (x as f64).clamp(low, high) as f32;
                changed |= next != x;
                *key = sample_key(next);
            }
            if !changed {
                break;
            }
        }
    }
    let mut numerator = Sum::default();
    let mut denominator = Sum::default();
    let mut rejected_low = 0;
    let mut rejected_high = 0;
    debug_assert_eq!(values.len(), weights.len());
    for (&v, &weight) in values.iter().zip(weights) {
        if (v as f64) < low {
            rejected_low += 1;
        } else if (v as f64) > high {
            rejected_high += 1;
        } else {
            numerator.add(v as f64 * weight);
            denominator.add(weight);
        }
    }
    let weight = denominator.value();
    if weight == 0. {
        (0., rejected_low, rejected_high, 0.)
    } else {
        (
            (numerator.value() / weight) as f32,
            rejected_low,
            rejected_high,
            weight,
        )
    }
}

#[cfg(test)]
#[path = "rgb_rejection_tests.rs"]
mod tests;
