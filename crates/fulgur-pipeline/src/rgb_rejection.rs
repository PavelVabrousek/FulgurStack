//! Shared finite RGB preparation with source-order dense channel samples/weights.
use super::{
    ClippingScratch, IntegrationConfig, Rejection, Sum, clipping_fixed_point, median_bounds,
    rejection_methods,
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
    pub fn channel_weights(&self, channel: usize) -> &[f64] {
        &self.weights[if self.shared { 0 } else { channel }]
    }
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
            winsorized,
            deviations,
            ..
        } = scratch;
        winsorized.clear();
        winsorized.extend_from_slice(values);
        for _ in 0..3 {
            let (center, middle) = median_bounds(winsorized);
            deviations.clear();
            deviations.extend(winsorized.iter().map(|v| (*v as f64 - center).abs() as f32));
            let (mad, mad_middle) = median_bounds(deviations);
            let sigma = (1.4826 * mad).max(1e-6);
            low = center - config.sigma_low * sigma;
            high = center + config.sigma_high * sigma;
            if clipping_fixed_point(center, middle, mad_middle[1], low, high) {
                break;
            }
            let mut changed = false;
            for x in winsorized.iter_mut() {
                let next = (*x as f64).clamp(low, high) as f32;
                changed |= next != *x;
                *x = next;
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
