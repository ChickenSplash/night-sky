//! Planet structure: generation's labelled fallback and the headless
//! dry, water, coupled and envelope solvers.
//! Prescriptions, provenance and domains: docs/simulation/planet-composition.md.

pub mod coupled;
#[cfg(test)]
mod coverage;
pub mod dry;
pub mod envelope;
pub mod fallback;
pub mod parameters;
pub mod prototype;
mod radial;
pub mod water;

/// Allow only floating-point summation noise at declared inclusive boundaries.
fn in_domain(value: f64, min: f64, max: f64) -> bool {
    value.is_finite()
        && value >= min * (1.0 - 16.0 * f64::EPSILON)
        && value <= max * (1.0 + 16.0 * f64::EPSILON)
}
