//! Blind plate solving: placing a frame on the sky with no position, no
//! target name and no header help (ADR-168).
//!
//! An [`Index`] holds the brightest, evenly spread catalogue stars grouped
//! into quads of several size bands, each hashed by a four-number code that
//! does not change with position, rotation, scale or (with both mirror
//! states tried) mirroring. [`solve`] makes quads of a frame's brightest
//! stars, looks them up, and checks every hit by counting the catalogue
//! stars it explains against the chance of that many by luck. The answer is
//! a [`Hypothesis`] (centre, scale, mirroring) for a refining solver, such as
//! the sky registration's, to take over.
//!
//! The crate uses no other FulgurStack crate. Reading the Gaia files and
//! detecting stars are the caller's business; see `fulgur-solve-cli`.
mod index;
mod quad;
mod solve;
mod sphere;

pub use index::{Band, BandSummary, FIELD_LIMITS_DEG, Index, STANDARD_FIELDS_DEG, SizeEstimate};
pub use solve::{Hypothesis, SolveOptions, clean_detections, solve};
pub use sphere::IndexStar;

#[cfg(test)]
mod tests;
