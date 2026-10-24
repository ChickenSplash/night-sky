//! Shared physical constants and unit conversions.

use std::f64::consts::TAU;

pub const AU_METRES: f64 = 149_597_870_700.0;
pub const SUN_RADIUS_METRES: f64 = 695_700_000.0;
pub const LY_METRES: f64 = 9_460_730_472_580_800.0;
pub const YEAR_SECONDS: f64 = 365.256_898_3 * 86_400.0;
/// Speed of light in metres per second.
pub const LIGHT_SPEED_MS: f64 = 299_792_458.0;

/// Gravitational parameter GM of one solar mass in m³/s², from the same year
/// and AU as `orbit::period_seconds`, so forces and analytic orbits agree.
pub const SUN_GM: f64 =
    TAU * TAU * AU_METRES * AU_METRES * AU_METRES / (YEAR_SECONDS * YEAR_SECONDS);
/// Earth's mass in solar masses.
pub const EARTH_SOLAR: f64 = 3.003_489_6e-6;
