//! Display thresholds, not measured constants. These never select structure,
//! appearance or physics.

/// Ordered display thresholds only, never structure or appearance inputs.
pub mod classification {
    pub const METAL_RICH_MASS_FRACTION: f64 = 0.5;
    pub const WATER_RICH_MASS_FRACTION: f64 = 0.25;
    pub const SIGNIFICANT_H_HE_MASS_FRACTION: f64 = 0.001;
    pub const H_HE_DOMINATED_MASS_FRACTION: f64 = 0.5;
    pub const ENVELOPE_WORLD_MASS_FRACTION: f64 = 0.1;
}
