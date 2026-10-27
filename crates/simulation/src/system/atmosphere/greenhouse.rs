//! Labelled grey greenhouse estimate, not a radiative-transfer or climate model.
//! T_s⁴ = T_eq⁴ (1 + 3τ/4), the Eddington grey radiative-equilibrium surface.
//! τ is a power-law fit in partial and total pressure, calibrated so Earth,
//! Mars and Venus land on their observed mean surface temperatures.
//! See docs/simulation/atmospheres.md#grey-greenhouse-estimate.
use super::*;

pub const MODEL_ID: &str = "grey_eddington_pressure_fit_greenhouse_v1";
/// Band saturation: optical depth grows sublinearly with absorber pressure.
pub const ABSORBER_EXPONENT: f64 = 0.4453;
/// Pressure broadening by the whole column, including transparent gases.
pub const BROADENING_EXPONENT: f64 = 0.2808;
/// Optical depth of one bar of absorber in one bar of air. Water's lumps in
/// clouds; CO₂'s also serves CH₄, which has no separate fit.
pub const CARBON_COEFFICIENT: f64 = 5.40;
pub const WATER_COEFFICIENT: f64 = 4.07;

/// Grey infrared optical depth of a surface-referenced column. N₂, O₂, Ar and
/// H/He are transparent, apart from broadening; no collision-induced absorption.
pub fn optical_depth(air: &GasAtmosphere) -> f64 {
    let bar = air.pressure_pa() / ONE_BAR_PA;
    let absorber = |gas: Gas| (bar * air.composition().fraction(gas)).powf(ABSORBER_EXPONENT);
    bar.powf(BROADENING_EXPONENT)
        * (WATER_COEFFICIENT * absorber(Gas::Water)
            + CARBON_COEFFICIENT * (absorber(Gas::CarbonDioxide) + absorber(Gas::Methane)))
}

pub fn surface_temperature_k(equilibrium_k: f64, optical_depth: f64) -> f64 {
    equilibrium_k * (1.0 + 0.75 * optical_depth).powf(0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn air(bar: f64, parts: &[(Gas, f64)]) -> GasAtmosphere {
        GasAtmosphere::new(
            Composition::new(parts).unwrap(),
            ReferenceLevel::Surface,
            bar * ONE_BAR_PA,
            250.0,
            None,
        )
        .unwrap()
    }

    #[test]
    fn calibration_planets_land_on_their_surface_temperatures() {
        // Mean surface conditions; Earth's water is saturated at 288 K, as the
        // coupling would hold it over an ocean. Clouds are inside the fit.
        let earth = air(
            1.013,
            &[
                (Gas::Nitrogen, 0.76978),
                (Gas::Oxygen, 0.2088),
                (Gas::Argon, 0.0042),
                (Gas::Water, 0.0168),
                (Gas::CarbonDioxide, 0.00042),
            ],
        );
        let mars = air(0.006, &[(Gas::CarbonDioxide, 0.95), (Gas::Nitrogen, 0.05)]);
        let venus = air(92.0, &[(Gas::CarbonDioxide, 0.965), (Gas::Nitrogen, 0.035)]);
        for (air, equilibrium, surface, tolerance) in [
            (earth, 255.0, 288.0, 2.0),
            (mars, 210.0, 215.0, 1.0),
            (venus, 229.0, 737.0, 5.0),
        ] {
            let t = surface_temperature_k(equilibrium, optical_depth(&air));
            assert!((t - surface).abs() < tolerance, "{t} != {surface}");
        }
        // Titan with CH₄ on the CO₂ fit: about its greenhouse-only 103 K,
        // before the haze anti-greenhouse that is not modelled.
        let titan = air(1.47, &[(Gas::Nitrogen, 0.943), (Gas::Methane, 0.057)]);
        let t = surface_temperature_k(82.0, optical_depth(&titan));
        assert!((95.0..110.0).contains(&t), "{t}");
    }

    #[test]
    fn transparent_and_absent_air_leave_the_equilibrium_temperature() {
        let nitrogen = air(5.0, &[(Gas::Nitrogen, 0.8), (Gas::Hydrogen, 0.2)]);
        assert_eq!(optical_depth(&nitrogen), 0.0);
        assert_eq!(surface_temperature_k(200.0, 0.0), 200.0);
    }
}
