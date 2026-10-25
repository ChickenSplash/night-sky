//! Model coefficients and engineering bounds, not measured constants.
//! Physical constants belong in units.rs; these defaults do not select classes.

pub const EARTH_REFERENCE_RADIUS_M: f64 = 6_371_000.0;
pub const EARTH_REFERENCE_FLUX_W_M2: f64 = 1361.0;
pub const MIN_MASS_EARTH: f64 = 1e-4;
pub const MAX_MASS_EARTH: f64 = 4000.0;
pub const MIN_ENVELOPE_MASS_EARTH: f64 = 0.1;
pub const MAX_CONDENSED_MASS_EARTH: f64 = 1000.0;
pub const CONDENSED_FIT_LIMIT_EARTH: f64 = 20.0;
pub const CONDENSED_CONTINUATION_EXPONENT: f64 = 0.2;
pub const MAX_TRACE_VOLATILE_FRACTION: f64 = 0.01;
pub const MIN_HELIUM_SHARE: f64 = 0.15;
pub const MAX_HELIUM_SHARE: f64 = 0.35;
pub const HEAVY_INERTIA_BASE: f64 = 0.36;
pub const METAL_INERTIA_REDUCTION: f64 = 0.08;
pub const ENVELOPE_INERTIA: f64 = 0.25;
pub const MIN_INERTIA: f64 = 0.12;
/// Environmental boundary model: percent-level gas self-gravity and cold-water
/// radius contributions only. Neither limit is a classification/pressure cutoff.
pub const MAX_BOUNDARY_GAS_FRACTION: f64 = 0.01;
pub const MAX_BOUNDARY_WATER_FRACTION: f64 = 0.01;

// Lopez & Fortney (2014), equation 4. Applied to envelope thickness only.
pub const ENVELOPE_THICKNESS_EARTH: f64 = 2.06;
pub const ENVELOPE_MASS_EXPONENT: f64 = -0.21;
pub const ENVELOPE_FRACTION_EXPONENT: f64 = 0.59;
pub const ENVELOPE_FLUX_EXPONENT: f64 = 0.044;
pub const ENVELOPE_AGE_EXPONENT: f64 = -0.18;
pub const ENVELOPE_FIT_MASS_RANGE: [f64; 2] = [1.0, 20.0];
pub const ENVELOPE_FIT_FLUX_RANGE: [f64; 2] = [0.1, 1000.0];
pub const ENVELOPE_FIT_AGE_RANGE: [f64; 2] = [0.1, 10.0];
pub const ENVELOPE_FIT_MAX_FRACTION: f64 = 0.2;

// Our tunable self-compressed giant surrogate, not a published giant fit.
pub const GIANT_RADIUS_EARTH: f64 = 11.2;
pub const GIANT_REFERENCE_MASS_EARTH: f64 = 318.0;
pub const GIANT_TURNOVER_MASS_EARTH: f64 = 95.0;
pub const GIANT_HIGH_MASS_EXPONENT: f64 = -0.04;
pub const GIANT_BLEND_FRACTION_RANGE: [f64; 2] = [0.1, 0.5];

/// Ordered display thresholds only, never structure or appearance inputs.
pub mod classification {
    pub const METAL_RICH_MASS_FRACTION: f64 = 0.5;
    pub const WATER_RICH_MASS_FRACTION: f64 = 0.25;
    pub const SIGNIFICANT_H_HE_MASS_FRACTION: f64 = 0.001;
    pub const H_HE_DOMINATED_MASS_FRACTION: f64 = 0.5;
    pub const ENVELOPE_WORLD_MASS_FRACTION: f64 = 0.1;
}
