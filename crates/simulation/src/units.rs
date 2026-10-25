//! Shared physical constants and unit conversions.

use std::f64::consts::TAU;

pub const AU_METRES: f64 = 149_597_870_700.0;
pub const SUN_RADIUS_METRES: f64 = 695_700_000.0;
pub const LY_METRES: f64 = 9_460_730_472_580_800.0;
pub const YEAR_SECONDS: f64 = 365.256_898_3 * 86_400.0;
/// Speed of light in metres per second.
pub const LIGHT_SPEED_MS: f64 = 299_792_458.0;
/// Exact SI Boltzmann constant, J/K.
pub const BOLTZMANN_J_K: f64 = 1.380_649e-23;
/// Atomic mass constant, kg (CODATA 2022, rounded).
pub const ATOMIC_MASS_KG: f64 = 1.660_539_068_92e-27;

/// Gravitational parameter GM of one solar mass in m³/s², from the same year
/// and AU as `orbit::period_seconds`, so forces and analytic orbits agree.
pub const SUN_GM: f64 =
    TAU * TAU * AU_METRES * AU_METRES * AU_METRES / (YEAR_SECONDS * YEAR_SECONDS);
/// Earth's mass in solar masses.
pub const EARTH_SOLAR: f64 = 3.003_489_6e-6;
/// Newtonian gravitational constant, SI (CODATA 2022).
pub const GRAVITATIONAL_CONSTANT: f64 = 6.674_30e-11;
/// Mass unit consistent with the simulator's orbital gravitational parameter.
pub const EARTH_MASS_KG: f64 = SUN_GM * EARTH_SOLAR / GRAVITATIONAL_CONSTANT;
