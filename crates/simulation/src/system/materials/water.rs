//! Bounded pure-water P-T physics, not a planetary thermal prescription.
//! IAPWS R7-97(2012), R10-06(2009), R14-08(2011), R6-95(2018). Equations and coefficients
//! attributed to the International Association for the Properties of Water and
//! Steam, whose releases permit publication with attribution. No external code
//! or EOS tables are imported. See docs/simulation/water-physics.md.
//! The separate prescribed ice-VII compression branch is attributed to Lai
//! et al. (2023); it does not extend the original equilibrium-state API.

pub mod cold_ice;
pub mod deep_ice;
pub mod dense_liquid;
pub mod hot;
mod ice;
pub mod ice_vii;
mod liquid;
pub mod loaded;
pub mod melting;
pub mod subfreezing_liquid;
pub mod surface;

use std::fmt;

pub const MODEL_ID: &str = "iapws_low_pressure_water_v1";
pub const MIN_TEMPERATURE_K: f64 = 250.0;
pub const MAX_TEMPERATURE_K: f64 = 623.15;
pub const MIN_PRESSURE_PA: f64 = 1000.0;
pub const MAX_PRESSURE_PA: f64 = 100e6;
const TRIPLE_TEMPERATURE_K: f64 = 273.16;
const TRIPLE_PRESSURE_PA: f64 = 611.657;
pub const CRITICAL_TEMPERATURE_K: f64 = 647.096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    IceIh,
    Liquid,
    Vapour,
    IceIhLiquidCoexistence,
    LiquidVapourCoexistence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub phase: Phase,
    pub density_kg_m3: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    PressureDomain,
    TemperatureDomain,
    /// No phase fraction or energy constraint is supplied by a P-T request.
    CoexistenceDensity,
    VapourDensity,
    /// IF97 region 1 stops at 273.15 K, even where colder liquid is stable.
    SubfreezingLiquidDensity,
    /// The requested one-sided phase is not stable at this P-T point.
    PhaseMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterError {
    /// Absent for a temperature-only melting-curve request.
    pub pressure_pa: Option<f64>,
    pub temperature_k: f64,
    pub reason: Unsupported,
}

impl fmt::Display for WaterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "water EOS: {:?} at {} K",
            self.reason, self.temperature_k
        )?;
        if let Some(pressure) = self.pressure_pa {
            write!(f, ", {pressure} Pa")?;
        }
        Ok(())
    }
}
impl std::error::Error for WaterError {}

fn error(pressure_pa: f64, temperature_k: f64, reason: Unsupported) -> WaterError {
    WaterError {
        pressure_pa: Some(pressure_pa),
        temperature_k,
        reason,
    }
}

/// Stable equilibrium phase within the application domain, not a statement
/// about metastability, kinetics, phase fractions or the accuracy of the curves.
/// Phase identification does not promise a density EOS for that phase.
pub fn phase(pressure_pa: f64, temperature_k: f64) -> Result<Phase, WaterError> {
    if !pressure_pa.is_finite() || !(MIN_PRESSURE_PA..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PressureDomain,
        ));
    }
    if !temperature_k.is_finite()
        || !(MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&temperature_k)
    {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::TemperatureDomain,
        ));
    }
    if temperature_k < 251.165 {
        // At <=100 MPa this is ice Ih, below the Ih/III/liquid triple point.
        return Ok(Phase::IceIh);
    }
    if temperature_k <= TRIPLE_TEMPERATURE_K {
        let melting = melting_pressure_ih_pa(temperature_k).unwrap();
        return Ok(if on_curve(pressure_pa, melting) {
            Phase::IceIhLiquidCoexistence
        } else if pressure_pa < melting {
            Phase::IceIh
        } else {
            Phase::Liquid
        });
    }
    let saturation = liquid::saturation_pressure_pa(temperature_k);
    Ok(if on_curve(pressure_pa, saturation) {
        Phase::LiquidVapourCoexistence
    } else if pressure_pa < saturation {
        Phase::Vapour
    } else {
        Phase::Liquid
    })
}

/// Single-phase condensed density in SI. Unsupported/coexisting states are
/// errors, never ambient-density proxies or interpolated phase mixtures.
pub fn state(pressure_pa: f64, temperature_k: f64) -> Result<State, WaterError> {
    let phase = phase(pressure_pa, temperature_k)?;
    condensed_state(pressure_pa, temperature_k, phase)
}

/// A named condensed branch, including its one-sided limits on the Ih melting
/// curve and, for liquid, on the saturation curve. This selects no mixture
/// fraction and enables no metastable extension. `state` still refuses
/// ambiguous coexistence densities.
pub fn condensed_state(
    pressure_pa: f64,
    temperature_k: f64,
    requested: Phase,
) -> Result<State, WaterError> {
    let equilibrium = phase(pressure_pa, temperature_k)?;
    let phase = if (equilibrium == Phase::IceIhLiquidCoexistence
        && matches!(requested, Phase::IceIh | Phase::Liquid))
        || (equilibrium == Phase::LiquidVapourCoexistence && requested == Phase::Liquid)
    {
        requested
    } else if equilibrium == requested {
        equilibrium
    } else {
        return Err(error(
            pressure_pa,
            temperature_k,
            Unsupported::PhaseMismatch,
        ));
    };
    let density_kg_m3 = match phase {
        Phase::IceIh => ice::density_kg_m3(pressure_pa, temperature_k),
        Phase::Liquid if temperature_k >= 273.15 => {
            liquid::density_kg_m3(pressure_pa, temperature_k)
        }
        Phase::Liquid => {
            return Err(error(
                pressure_pa,
                temperature_k,
                Unsupported::SubfreezingLiquidDensity,
            ));
        }
        Phase::Vapour => {
            return Err(error(
                pressure_pa,
                temperature_k,
                Unsupported::VapourDensity,
            ));
        }
        Phase::IceIhLiquidCoexistence | Phase::LiquidVapourCoexistence => {
            return Err(error(
                pressure_pa,
                temperature_k,
                Unsupported::CoexistenceDensity,
            ));
        }
    };
    Ok(State {
        pressure_pa,
        temperature_k,
        phase,
        density_kg_m3,
    })
}

// Roundoff recognition only, not a physical uncertainty band. The published
// melting curve has much larger scientific uncertainty, documented separately.
fn on_curve(pressure: f64, boundary: f64) -> bool {
    (pressure - boundary).abs() <= 16.0 * f64::EPSILON * pressure.max(boundary)
}

/// IF97 region 4 saturation pressure, published for 273.15-647.096 K. Wider
/// than the density API: it bounds liquid at a surface, not its density.
pub fn saturation_pressure_pa(temperature_k: f64) -> Result<f64, WaterError> {
    if !temperature_k.is_finite() || !(273.15..=CRITICAL_TEMPERATURE_K).contains(&temperature_k) {
        return Err(WaterError {
            pressure_pa: None,
            temperature_k,
            reason: Unsupported::TemperatureDomain,
        });
    }
    Ok(liquid::saturation_pressure_pa(temperature_k))
}

/// IAPWS R14-08(2011), equation 1. This curve is valid beyond the density API's
/// pressure range, down to the Ih/III/liquid triple point. It is NOT a density
/// EOS or a complete high-pressure phase selector.
pub fn melting_pressure_ih_pa(temperature_k: f64) -> Result<f64, WaterError> {
    melting::pressure_pa(melting::Branch::Ih, temperature_k)
}

#[cfg(test)]
mod tests;
