//! Shared cold-trap prescription, independent of formation/accessibility priors.
use super::Gas;

/// Rough Clausius-Clapeyron caps, anchored near a measured phase point.
/// Only a cold-trap heuristic, not a phase diagram or hydrosphere model.
pub(crate) fn vapour_pressure(gas: Gas, temperature: f64) -> f64 {
    let (anchor_k, anchor_pa, latent_over_r, critical_k) = match gas {
        Gas::CarbonDioxide => (194.65, 101_325.0, 3030.0, 304.13),
        Gas::Nitrogen => (77.36, 101_325.0, 670.0, 126.2),
        Gas::Methane => (111.66, 101_325.0, 980.0, 190.56),
        Gas::Argon => (87.30, 101_325.0, 770.0, 150.69),
        Gas::Water if temperature < 273.16 => (273.16, 611.657, 6140.0, 647.1),
        Gas::Water => (273.16, 611.657, 5420.0, 647.1),
        _ => return f64::INFINITY,
    };
    if temperature >= critical_k {
        return f64::INFINITY;
    }
    anchor_pa * (-latent_over_r * (1.0 / temperature - 1.0 / anchor_k)).exp()
}
