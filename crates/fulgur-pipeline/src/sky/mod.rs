//! Sky-based registration core (plan phase 1, ADR-131/ADR-132): every frame
//! is plate-solved on its own against Gaia DR3, and a registration source ->
//! reference is source pixel -> sky -> reference pixel. Nothing in the stacking
//! pipeline calls this module yet; it is a library with its own tests.
//!
//! - [`catalogue`]: Gaia stars with proper motion and the gnomonic projection.
//! - [`solve`]: blind and prior-seeded search, refinement with a polynomial
//!   whose degree held-out stars choose.
//! - [`batch`]: one blind search per camera group, the rest seeded.
//! - [`check`]: end-to-end check of a registration with the stars both frames
//!   detected.
//! - [`warp`]: the composed map as FulgurStack's cubic [`crate::distortion::Warp`].
pub mod batch;
pub mod catalogue;
pub mod check;
pub mod detect;
pub mod solve;
pub mod warp;

/// The stars of one frame as the solver needs them: zero-based pixel centres
/// in the frame's reader order, brightest first.
#[derive(Clone, Copy, Debug)]
pub struct SkyFrame<'a> {
    pub name: &'a str,
    pub width: usize,
    pub height: usize,
    /// Decimal year of the exposure, for proper motion.
    pub epoch: f64,
    pub stars: &'a [[f64; 2]],
}

/// Point sources for the end-to-end check: width near the frame's typical
/// star and no more elongated than its stars usually are. Galaxy knots, HII
/// regions and blends shift with filter and seeing and are left out. `stars`
/// begin with (x, y, fwhm, eccentricity), brightest first.
pub fn point_sources<const N: usize>(stars: &[[f64; N]]) -> Vec<[f64; 2]> {
    // Typical width and elongation from stars of middle brightness: the
    // brightest are often saturated and wider than the rest.
    let middle: Vec<&[f64; N]> = stars.iter().skip(50).take(1000).collect();
    if middle.len() < 20 {
        return stars.iter().map(|s| [s[0], s[1]]).collect();
    }
    let median = |mut v: Vec<f64>| {
        let m = v.len() / 2;
        v.select_nth_unstable_by(m, f64::total_cmp);
        v[m]
    };
    let width = median(middle.iter().map(|s| s[2]).collect());
    let typical = median(middle.iter().map(|s| s[3]).collect());
    stars
        .iter()
        .filter(|s| (0.6 * width..=1.6 * width).contains(&s[2]) && s[3] <= typical + 0.2)
        .map(|s| [s[0], s[1]])
        .collect()
}

#[cfg(test)]
mod tests;
