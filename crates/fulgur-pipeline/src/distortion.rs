//! Optional cubic catalog geometry, accepted only by held-out spatial validation.
use crate::geometry::{self, Match, Transform};
use crate::stars::{Star, median};
use fulgur_core::{Result, check_cancelled};
use nalgebra::{SMatrix, SVector};
use serde::Serialize;
use std::{collections::BTreeSet, sync::atomic::AtomicBool};

pub(crate) mod validation;
pub use validation::FieldValidation;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Warp {
    pub base: Transform,
    pub center: [f64; 2],
    pub normalization: [f64; 2],
    /// 1, x, y, x*y, x², y², x³, x²*y, x*y², y³; reference-pixel residuals.
    pub coefficients: [[f64; 10]; 2],
}

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
fn derivatives(x: f64, y: f64) -> [[f64; 10]; 2] {
    [
        [0., 1., 0., y, 2. * x, 0., 3. * x * x, 2. * x * y, y * y, 0.],
        [0., 0., 1., x, 0., 2. * y, 0., x * x, 2. * x * y, 3. * y * y],
    ]
}
fn dot(a: &[f64; 10], b: &[f64; 10]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

impl Warp {
    /// Exact cubic composition with an affine source pose, solved on a fixed
    /// polynomial basis. This is only a correspondence seed, never validation.
    pub(crate) fn compose_source(&self, pose: Transform, sensor: [usize; 2]) -> Option<Self> {
        let center = [(sensor[0] as f64 - 1.) / 2., (sensor[1] as f64 - 1.) / 2.];
        let base = self.base.compose(pose);
        let mut matrix = SMatrix::<f64, 10, 10>::zeros();
        let mut rhs = [SVector::<f64, 10>::zeros(), SVector::<f64, 10>::zeros()];
        for y in 0..7 {
            for x in 0..7 {
                let nx = x as f64 / 3. - 1.;
                let ny = y as f64 / 3. - 1.;
                let p = [center[0] * (1. + nx), center[1] * (1. + ny)];
                let q = self.forward(pose.apply(p));
                let linear = base.apply(p);
                let b = SVector::<f64, 10>::from_row_slice(&basis(nx, ny));
                matrix += b * b.transpose();
                rhs[0] += b * (q[0] - linear[0]);
                rhs[1] += b * (q[1] - linear[1]);
            }
        }
        let solver = matrix.cholesky()?;
        let mut model = Self {
            base,
            center,
            normalization: center,
            coefficients: [[0.; 10]; 2],
        };
        for (c, rhs) in model.coefficients.iter_mut().zip(rhs) {
            c.copy_from_slice(solver.solve(&rhs).as_slice());
        }
        model.valid().then_some(model)
    }
    pub fn forward(&self, p: [f64; 2]) -> [f64; 2] {
        let b = basis(
            (p[0] - self.center[0]) / self.normalization[0],
            (p[1] - self.center[1]) / self.normalization[1],
        );
        let q = self.base.apply(p);
        [
            q[0] + dot(&self.coefficients[0], &b),
            q[1] + dot(&self.coefficients[1], &b),
        ]
    }

    fn source_coefficients(&self) -> [[f64; 10]; 2] {
        let inverse = self.base.inverse();
        let (s, c) = inverse.theta_rad.sin_cos();
        std::array::from_fn(|axis| {
            std::array::from_fn(|i| {
                inverse.scale_factor
                    * if axis == 0 {
                        c * self.coefficients[0][i] - s * self.coefficients[1][i]
                    } else {
                        s * self.coefficients[0][i] + c * self.coefficients[1][i]
                    }
            })
        })
    }

    /// Interval bound for inverse displacement in original source pixels.
    /// Every normalized source monomial has absolute value <=1 on the sensor.
    pub fn source_margin(&self) -> [f64; 2] {
        self.source_coefficients()
            .map(|c| c.iter().map(|v| v.abs()).sum())
    }

    /// Conservative interval fixed-point enclosure of all inverse points in a
    /// target tile. Tightening only intersects proven enclosures; no corner-only
    /// approximation of a curved footprint and no dense image grid are used.
    pub(crate) fn inverse_region(&self, origin: [i32; 2], size: [usize; 2]) -> Option<[f64; 4]> {
        let inverse = self.base.inverse();
        let x = origin[0] as f64;
        let y = origin[1] as f64;
        let points = [
            [x, y],
            [x + size[0] as f64 - 1., y],
            [x + size[0] as f64 - 1., y + size[1] as f64 - 1.],
            [x, y + size[1] as f64 - 1.],
        ]
        .map(|p| inverse.apply(p));
        let linear = [
            points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
            points
                .iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max),
            points
                .iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max),
        ];
        let margin = self.source_margin();
        let mut b = [
            (linear[0] - margin[0] - 1e-5).max(0.),
            (linear[1] - margin[1] - 1e-5).max(0.),
            (linear[2] + margin[0] + 1e-5).min(2. * self.center[0]),
            (linear[3] + margin[1] + 1e-5).min(2. * self.center[1]),
        ];
        let coefficients = self.source_coefficients();
        for _ in 0..4 {
            if b[0] > b[2] || b[1] > b[3] {
                return None;
            }
            let x = [
                (b[0] - self.center[0]) / self.normalization[0],
                (b[2] - self.center[0]) / self.normalization[0],
            ];
            let y = [
                (b[1] - self.center[1]) / self.normalization[1],
                (b[3] - self.center[1]) / self.normalization[1],
            ];
            let square = |r: [f64; 2]| {
                if r[0] <= 0. && r[1] >= 0. {
                    [0., r[0].abs().max(r[1].abs()).powi(2)]
                } else {
                    [
                        r[0].powi(2).min(r[1].powi(2)),
                        r[0].powi(2).max(r[1].powi(2)),
                    ]
                }
            };
            let xx = square(x);
            let yy = square(y);
            let monomials = [
                [1., 1.],
                x,
                y,
                interval_product(x, y),
                xx,
                yy,
                [x[0].powi(3), x[1].powi(3)],
                interval_product(xx, y),
                interval_product(x, yy),
                [y[0].powi(3), y[1].powi(3)],
            ];
            for axis in 0..2 {
                let mut delta = [0., 0.];
                for (coefficient, range) in coefficients[axis].iter().zip(monomials) {
                    let a = coefficient * range[0];
                    let z = coefficient * range[1];
                    delta[0] += a.min(z);
                    delta[1] += a.max(z);
                }
                b[axis] = b[axis].max(linear[axis] - delta[1] - 1e-5);
                b[axis + 2] = b[axis + 2].min(linear[axis + 2] - delta[0] + 1e-5);
            }
        }
        (b[0] <= b[2] && b[1] <= b[3]).then_some(b)
    }

    pub fn valid(&self) -> bool {
        if ![
            self.base.scale_factor,
            self.base.theta_rad,
            self.base.tx_px,
            self.base.ty_px,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return false;
        }
        if !self
            .center
            .iter()
            .chain(&self.normalization)
            .chain(self.coefficients.iter().flatten())
            .all(|v| v.is_finite())
            || self.normalization.iter().any(|v| *v <= 0.)
            || !fulgur_core::geometry_scale_supported(self.base.scale_factor)
        {
            return false;
        }
        // ||base^-1 * D(residual)||_infinity <1 proves a nonfolding
        // injective perturbation on the complete convex sensor domain.
        let dx = [0., 1., 0., 1., 2., 0., 3., 2., 1., 0.];
        let dy = [0., 0., 1., 1., 0., 2., 0., 1., 2., 3.];
        self.source_coefficients().iter().all(|c| {
            c.iter()
                .zip(dx)
                .map(|(v, d)| v.abs() * d / self.normalization[0])
                .sum::<f64>()
                + c.iter()
                    .zip(dy)
                    .map(|(v, d)| v.abs() * d / self.normalization[1])
                    .sum::<f64>()
                < 0.45
        })
    }

    pub fn inverse(&self, q: [f64; 2]) -> Option<[f64; 2]> {
        self.inverse_with_rotation(q, self.base.theta_rad.sin_cos())
    }

    #[inline]
    pub(crate) fn inverse_with_rotation(
        &self,
        q: [f64; 2],
        (s, c): (f64, f64),
    ) -> Option<[f64; 2]> {
        let scale = self.base.scale_factor;
        let inverse_scale = 1. / scale;
        let mut p = [
            inverse_scale * (c * q[0] + s * q[1])
                + (-c * self.base.tx_px - s * self.base.ty_px) / scale,
            inverse_scale * (-s * q[0] + c * q[1])
                + (s * self.base.tx_px - c * self.base.ty_px) / scale,
        ];
        for _ in 0..8 {
            let x = (p[0] - self.center[0]) / self.normalization[0];
            let y = (p[1] - self.center[1]) / self.normalization[1];
            if !x.is_finite() || !y.is_finite() || x.abs() > 2. || y.abs() > 2. {
                return None;
            }
            let b = basis(x, y);
            let f = [
                scale * (c * p[0] - s * p[1]) + self.base.tx_px + dot(&self.coefficients[0], &b),
                scale * (s * p[0] + c * p[1]) + self.base.ty_px + dot(&self.coefficients[1], &b),
            ];
            let error = [f[0] - q[0], f[1] - q[1]];
            if error[0].hypot(error[1]) < 1e-7 {
                return (x.abs() <= 1. + 1e-9 && y.abs() <= 1. + 1e-9).then_some(p);
            }
            let d = derivatives(x, y);
            let j = [
                [
                    scale * c + dot(&self.coefficients[0], &d[0]) / self.normalization[0],
                    -scale * s + dot(&self.coefficients[0], &d[1]) / self.normalization[1],
                ],
                [
                    scale * s + dot(&self.coefficients[1], &d[0]) / self.normalization[0],
                    scale * c + dot(&self.coefficients[1], &d[1]) / self.normalization[1],
                ],
            ];
            let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
            if !det.is_finite() || det <= 1e-8 {
                return None;
            }
            p[0] -= (j[1][1] * error[0] - j[0][1] * error[1]) / det;
            p[1] -= (-j[1][0] * error[0] + j[0][0] * error[1]) / det;
        }
        None
    }

    /// Conservative convex interior for optional crop planning; actual validity
    /// in reference mode is always tested by inverse mapping at every pixel.
    pub(crate) fn footprint(&self, sensor: [usize; 2]) -> Vec<[f64; 2]> {
        let m = self.source_margin();
        let lo = [1. + m[0], 1. + m[1]];
        let hi = [
            sensor[0] as f64 - 2. - 1e-6 - m[0],
            sensor[1] as f64 - 2. - 1e-6 - m[1],
        ];
        if lo[0] >= hi[0] || lo[1] >= hi[1] {
            return vec![];
        }
        [
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ]
        .map(|p| self.base.apply(p))
        .to_vec()
    }
}

fn interval_product(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let values = [a[0] * b[0], a[0] * b[1], a[1] * b[0], a[1] * b[1]];
    [
        values.iter().copied().fold(f64::INFINITY, f64::min),
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ]
}

#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_correction_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_validation: Option<FieldValidation>,
    pub status: &'static str,
    pub reason: &'static str,
    pub training_stars: usize,
    pub validation_stars: usize,
    pub baseline_validation_rmse_px: Option<f64>,
    pub corrected_validation_rmse_px: Option<f64>,
    pub baseline_outer_rmse_px: Option<f64>,
    pub corrected_outer_rmse_px: Option<f64>,
    pub model: Option<Warp>,
    /// Bounded, invertible cubic fitted on training stars whose held-out
    /// improvement over the linear fit was not confirmed. Never applied unless
    /// an absolute whole-field validation of exactly this model accepts it.
    #[serde(skip)]
    pub(crate) candidate: Option<Warp>,
}
impl Decision {
    pub(crate) fn skipped(reason: &'static str) -> Self {
        Self {
            initial_correction_reason: None,
            registration_validation: None,
            status: "skipped",
            reason,
            training_stars: 0,
            validation_stars: 0,
            baseline_validation_rmse_px: None,
            corrected_validation_rmse_px: None,
            baseline_outer_rmse_px: None,
            corrected_outer_rmse_px: None,
            model: None,
            candidate: None,
        }
    }
}

fn matches(
    src: &[Star],
    dst: &[Star],
    map: impl Fn([f64; 2]) -> [f64; 2],
    radius: f64,
) -> Vec<Match> {
    let mut pairs = vec![];
    for (i, s) in src.iter().enumerate() {
        let p = map([s.x_px, s.y_px]);
        if let Some((j, d)) = dst
            .iter()
            .enumerate()
            .map(|(j, r)| (j, (p[0] - r.x_px).hypot(p[1] - r.y_px)))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            pairs.push(Match {
                source: i,
                reference: j,
                residual_px: d,
            });
        }
    }
    pairs.sort_by(|a, b| {
        a.residual_px
            .total_cmp(&b.residual_px)
            .then(a.source.cmp(&b.source))
    });
    let mut used = BTreeSet::new();
    pairs.retain(|m| used.insert(m.reference));
    pairs
}

fn fit(
    src: &[Star],
    dst: &[Star],
    pairs: &[Match],
    base: Transform,
    sensor: [usize; 2],
) -> Option<Warp> {
    let center = [(sensor[0] as f64 - 1.) / 2., (sensor[1] as f64 - 1.) / 2.];
    let mut model = Warp {
        base,
        center,
        normalization: center,
        coefficients: [[0.; 10]; 2],
    };
    let mut kept: Vec<_> = pairs.iter().collect();
    for iteration in 0..4 {
        if kept.len() < 40 {
            return None;
        }
        let mut matrix = SMatrix::<f64, 10, 10>::zeros();
        let mut rhs = [SVector::<f64, 10>::zeros(), SVector::<f64, 10>::zeros()];
        for m in &kept {
            let s = &src[m.source];
            let r = &dst[m.reference];
            let p = [s.x_px, s.y_px];
            let q = base.apply(p);
            let b = SVector::<f64, 10>::from_row_slice(&basis(
                (p[0] - center[0]) / center[0],
                (p[1] - center[1]) / center[1],
            ));
            let weight = s.snr.min(r.snr).clamp(1., 100.).sqrt();
            matrix += b * b.transpose() * weight;
            rhs[0] += b * ((r.x_px - q[0]) * weight);
            rhs[1] += b * ((r.y_px - q[1]) * weight);
        }
        let solver = matrix.cholesky()?;
        for (c, rhs) in model.coefficients.iter_mut().zip(rhs) {
            c.copy_from_slice(solver.solve(&rhs).as_slice());
        }
        if iteration == 3 {
            break;
        }
        let mut residuals: Vec<_> = kept
            .iter()
            .map(|m| {
                let s = &src[m.source];
                let r = &dst[m.reference];
                let p = model.forward([s.x_px, s.y_px]);
                (p[0] - r.x_px).hypot(p[1] - r.y_px)
            })
            .collect();
        let threshold = (median(&mut residuals) * 3.).max(1.5);
        kept.retain(|m| {
            let s = &src[m.source];
            let r = &dst[m.reference];
            let p = model.forward([s.x_px, s.y_px]);
            (p[0] - r.x_px).hypot(p[1] - r.y_px) <= threshold
        });
    }
    Some(model)
}

fn field_region(src: &[Star], pairs: &[Match]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for m in pairs {
        let s = &src[m.source];
        b[0] = b[0].min(s.x_px);
        b[1] = b[1].min(s.y_px);
        b[2] = b[2].max(s.x_px);
        b[3] = b[3].max(s.y_px);
    }
    b
}
fn coverage(src: &[Star], pairs: &[Match], b: [f64; 4]) -> usize {
    pairs
        .iter()
        .map(|m| {
            let s = &src[m.source];
            (((s.x_px - b[0]) / (b[2] - b[0]) * 4.).max(0.) as usize).min(3)
                + 4 * ((((s.y_px - b[1]) / (b[3] - b[1]) * 4.).max(0.) as usize).min(3))
        })
        .collect::<BTreeSet<_>>()
        .len()
}
fn outer(s: &Star, b: [f64; 4]) -> bool {
    let x = (s.x_px - (b[0] + b[2]) / 2.) / ((b[2] - b[0]) / 2.);
    let y = (s.y_px - (b[1] + b[3]) / 2.) / ((b[3] - b[1]) / 2.);
    x.abs().max(y.abs()) > 0.65
}

fn rmse(src: &[Star], dst: &[Star], pairs: &[Match], map: impl Fn([f64; 2]) -> [f64; 2]) -> f64 {
    (pairs
        .iter()
        .map(|m| {
            let s = &src[m.source];
            let r = &dst[m.reference];
            let p = map([s.x_px, s.y_px]);
            (p[0] - r.x_px).powi(2) + (p[1] - r.y_px).powi(2)
        })
        .sum::<f64>()
        / pairs.len() as f64)
        .sqrt()
}

pub(crate) fn solve(
    src: &[Star],
    dst: &[Star],
    seed: Option<Transform>,
    sensor: [usize; 2],
    cancel: &AtomicBool,
) -> Result<(Decision, Vec<Match>)> {
    solve_seeded(src, dst, seed, None, sensor, cancel)
}

pub(crate) fn solve_seeded(
    src: &[Star],
    dst: &[Star],
    seed: Option<Transform>,
    initial_warp: Option<Warp>,
    sensor: [usize; 2],
    cancel: &AtomicBool,
) -> Result<(Decision, Vec<Match>)> {
    check_cancelled(cancel)?;
    let skip = |why| Ok((Decision::skipped(why), vec![]));
    if src.len() < 60 || dst.len() < 60 {
        return skip("insufficient_stars_for_independent_validation");
    }
    let radius =
        (sensor[0] as f64).hypot(sensor[1] as f64) * seed.map_or(1., |t| t.scale_factor) * 0.04;
    let initial_base = if let Some(model) = initial_warp {
        model.base
    } else {
        let Some(coarse) = geometry::coarse_similarity(src, dst, seed, radius.max(8.), cancel)?
        else {
            return skip("no_reliable_coarse_correspondences");
        };
        coarse.transform
    };
    let mut model = initial_warp.unwrap_or(Warp {
        base: initial_base,
        center: [(sensor[0] as f64 - 1.) / 2., (sensor[1] as f64 - 1.) / 2.],
        normalization: [(sensor[0] as f64 - 1.) / 2., (sensor[1] as f64 - 1.) / 2.],
        coefficients: [[0.; 10]; 2],
    });
    let mut pairs = matches(src, dst, |p| model.forward(p), radius.max(8.));
    let initial_validation = pairs.iter().filter(|m| m.source % 5 == 0).count();
    for radius in [12., 4., 1.5] {
        check_cancelled(cancel)?;
        let train: Vec<_> = pairs
            .iter()
            .filter(|m| m.source % 5 != 0)
            .cloned()
            .collect();
        let Some(next) = fit(src, dst, &train, initial_base, sensor) else {
            return skip("insufficient_or_degenerate_training_field");
        };
        model = next;
        pairs = matches(src, dst, |p| model.forward(p), radius);
    }
    let train: Vec<_> = pairs
        .iter()
        .filter(|m| m.source % 5 != 0)
        .cloned()
        .collect();
    // Validate beyond the 1.5px fitting window so poor held-out stars cannot
    // silently disappear from the test set. The held-out labels never fit a model.
    let validation: Vec<_> = matches(src, dst, |p| model.forward(p), 12.)
        .into_iter()
        .filter(|m| m.source % 5 == 0)
        .collect();
    let region = field_region(src, &train);
    let outer_validation: Vec<_> = validation
        .iter()
        .filter(|m| outer(&src[m.source], region))
        .cloned()
        .collect();
    if region[2] - region[0] < sensor[0] as f64 * 0.35
        || region[3] - region[1] < sensor[1] as f64 * 0.35
        || train.len() < 40
        || validation.len() < 10
        || validation.len() * 5 < initial_validation * 4
        || outer_validation.len() < 6
        || coverage(src, &train, region) < 10
        || coverage(src, &validation, region) < 6
    {
        return skip("insufficient_independent_spatial_or_outer_coverage");
    }
    let Some(linear) = geometry::procrustes(src, dst, &train, true) else {
        return skip("degenerate_linear_comparison");
    };
    let Some(candidate) = fit(src, dst, &train, linear, sensor) else {
        return skip("degenerate_cubic_model");
    };
    let before = rmse(src, dst, &validation, |p| linear.apply(p));
    let after = rmse(src, dst, &validation, |p| candidate.forward(p));
    let outer_before = rmse(src, dst, &outer_validation, |p| linear.apply(p));
    let outer_after = rmse(src, dst, &outer_validation, |p| candidate.forward(p));
    let mut decision = Decision {
        initial_correction_reason: None,
        registration_validation: None,
        status: "skipped",
        reason: "no_confirmed_held_out_improvement",
        training_stars: train.len(),
        validation_stars: validation.len(),
        baseline_validation_rmse_px: Some(before),
        corrected_validation_rmse_px: Some(after),
        baseline_outer_rmse_px: Some(outer_before),
        corrected_outer_rmse_px: Some(outer_after),
        model: None,
        candidate: None,
    };
    let invertible = |model: &Warp| {
        // Same predicate as before the candidate was exposed (NaN-safe).
        model.valid()
            && !pairs.iter().any(|m| {
                let s = &src[m.source];
                let q = model.forward([s.x_px, s.y_px]);
                model
                    .inverse(q)
                    .is_none_or(|p| (p[0] - s.x_px).hypot(p[1] - s.y_px) > 1e-5)
            })
    };
    if invertible(&candidate) {
        decision.candidate = Some(candidate);
    }
    if ![before, after, outer_before, outer_after]
        .iter()
        .all(|v| v.is_finite())
        || after > 1.
    {
        decision.reason = "absolute_accuracy_failed";
        return Ok((decision, vec![]));
    }
    if before - after < 0.15 || after > before * 0.8 || outer_after > outer_before * 0.8 {
        return Ok((decision, vec![]));
    }
    // Every outer quadrant represented by validation stars must improve.
    for quadrant in 0..4 {
        let cell: Vec<_> = outer_validation
            .iter()
            .filter(|m| {
                let s = &src[m.source];
                usize::from(s.x_px > (region[0] + region[2]) / 2.)
                    + 2 * usize::from(s.y_px > (region[1] + region[3]) / 2.)
                    == quadrant
            })
            .cloned()
            .collect();
        if cell.is_empty()
            || rmse(src, dst, &cell, |p| candidate.forward(p))
                > rmse(src, dst, &cell, |p| linear.apply(p)) + 0.1
        {
            decision.reason = "outer_quadrant_not_validated";
            return Ok((decision, vec![]));
        }
    }
    // Apply exactly the model tested on held-out stars. Never refit using
    // validation labels after acceptance: reported metrics describe this model.
    let final_model = candidate;
    if decision.candidate.is_none() {
        decision.reason = "unbounded_noninvertible_or_folding_model";
        return Ok((decision, vec![]));
    }
    pairs = matches(src, dst, |p| final_model.forward(p), 1.5);
    decision.status = "applied";
    decision.reason = "held_out_and_outer_field_improvement_confirmed";
    decision.model = Some(final_model);
    Ok((decision, pairs))
}

#[cfg(test)]
mod tests;
