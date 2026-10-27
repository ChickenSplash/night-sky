//! Cold and low-pressure water outside the IAPWS API mask, for structure only.
//! Ice uses the IAPWS R10-06 density, published for 0-273.16 K up to 210 MPa;
//! liquid uses IF97 region 1, published down to its saturation pressure. What
//! the mask avoided is phase selection: sublimation is not modelled here.

use super::{
    MAX_PRESSURE_PA, MAX_TEMPERATURE_K, MIN_PRESSURE_PA, Phase, TRIPLE_TEMPERATURE_K, Unsupported,
    WaterError, error, ice, liquid,
};

pub const MODEL_ID: &str = "iapws_cold_or_low_pressure_surface_water_v1";
/// Generation's column temperature floor.
pub const MIN_TEMPERATURE_K: f64 = 20.0;

pub const APPROXIMATION_NOTES: &[&str] = &[
    "Ice is kept below its sublimation pressure; sublimation loss is not modelled",
    "Below 1 kPa the phase is ice under 273.16 K and liquid above it",
    "Proton-ordered ice XI below 72 K is treated as ice Ih",
];

/// Ice below 273.16 K, otherwise liquid. Vapour is refused by the density.
pub fn phase(temperature_k: f64) -> Phase {
    if temperature_k < TRIPLE_TEMPERATURE_K {
        Phase::IceIh
    } else {
        Phase::Liquid
    }
}

/// Below 250 K: up to 100 MPa, where the extended cold Ih branch takes over.
/// At 250 K and above it is the API's own state from 1 kPa, so one region can
/// run from a thin surface into the API domain without a seam.
pub fn density_kg_m3(pressure_pa: f64, temperature_k: f64) -> Result<f64, WaterError> {
    if !temperature_k.is_finite()
        || !(MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&temperature_k)
    {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::TemperatureDomain,
        ));
    }
    if !pressure_pa.is_finite() || !(0.0..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PressureDomain,
        ));
    }
    if temperature_k >= super::MIN_TEMPERATURE_K && pressure_pa >= MIN_PRESSURE_PA {
        return super::condensed_state(pressure_pa, temperature_k, phase(temperature_k))
            .map(|s| s.density_kg_m3);
    }
    if phase(temperature_k) == Phase::IceIh {
        return Ok(ice::density_kg_m3(pressure_pa, temperature_k));
    }
    let saturation = liquid::saturation_pressure_pa(temperature_k);
    // On the curve the liquid-side limit is taken, as in the API.
    if pressure_pa < saturation && !super::on_curve(pressure_pa, saturation) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::VapourDensity,
        ));
    }
    Ok(liquid::density_kg_m3(pressure_pa, temperature_k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_and_airless_ice_and_thin_liquid_match_the_api_at_its_edges() {
        // Ice Ih is about 933 kg/m³ when cold, a little lighter near melting.
        let cold = density_kg_m3(0.0, MIN_TEMPERATURE_K).unwrap();
        assert!((930.0..936.0).contains(&cold), "{cold}");
        let mut previous = cold;
        for t in [50.0, 100.0, 150.0, 200.0, 249.9] {
            let rho = density_kg_m3(0.0, t).unwrap();
            assert!(rho < previous);
            previous = rho;
        }
        assert!(density_kg_m3(100e6, 200.0).unwrap() > density_kg_m3(0.0, 200.0).unwrap());
        for (p, t) in [
            (MIN_PRESSURE_PA, 250.0),
            (1e6, 260.0),
            (MIN_PRESSURE_PA, 273.15),
            (MIN_PRESSURE_PA, 275.0),
            (MAX_PRESSURE_PA, 300.0),
        ] {
            assert_eq!(
                density_kg_m3(p, t).unwrap(),
                super::super::state(p, t).unwrap().density_kg_m3
            );
        }
        // Continuous across the API floor, without a seam.
        let below = density_kg_m3(MIN_PRESSURE_PA * (1.0 - 1e-12), 260.0).unwrap();
        assert!((below / density_kg_m3(MIN_PRESSURE_PA, 260.0).unwrap() - 1.0).abs() < 1e-15);
        assert!(density_kg_m3(800.0, 275.0).is_ok());
        assert_eq!(phase(275.0), Phase::Liquid);
        for (p, t, reason) in [
            (600.0, 275.0, Unsupported::VapourDensity),
            (0.0, 300.0, Unsupported::VapourDensity),
            (100e6 + 1.0, 200.0, Unsupported::PressureDomain),
            (MAX_PRESSURE_PA + 1.0, 250.0, Unsupported::PressureDomain),
            (50e6, 273.0, Unsupported::PhaseMismatch),
            (-1.0, 200.0, Unsupported::PressureDomain),
            (f64::NAN, 200.0, Unsupported::PressureDomain),
            (0.0, 19.9, Unsupported::TemperatureDomain),
            (0.0, 700.0, Unsupported::TemperatureDomain),
        ] {
            assert_eq!(density_kg_m3(p, t).unwrap_err().reason, reason, "{p} {t}");
        }
    }
}
