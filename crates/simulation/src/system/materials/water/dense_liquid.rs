//! Direct IAPWS R6-95(2018) compressed-liquid density, attributed to IAPWS.
//! Independent of the low-pressure model and not a high-pressure ice EOS.

mod helmholtz;

use super::{Phase, State, liquid, melting, on_curve};
use std::fmt;

pub const MODEL_ID: &str = "iapws95_compressed_liquid_v1";
pub const MIN_PRESSURE_PA: f64 = 100e6;
pub const MAX_PRESSURE_PA: f64 = 1e9;
pub const MIN_TEMPERATURE_K: f64 = 273.16;
pub const MAX_TEMPERATURE_K: f64 = 623.15;
const MAX_ITERATIONS: usize = 96;
/// One IF97 bracket seed and at most 98 Helmholtz pressure probes.
pub const MAX_EOS_EVALUATIONS: usize = MAX_ITERATIONS + 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvaluatedState {
    pub state: State,
    pub eos_evaluations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    PressureDomain,
    TemperatureDomain,
    /// The melting curve excludes liquid, but does not identify the deeper ice.
    SolidDomain,
    CoexistenceDensity,
    DensityBracket,
    DensityNonConvergence,
    EvaluationLimit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Error {
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub reason: Failure,
    /// Includes the IF97 seed and unsuccessful pressure probes.
    pub eos_evaluations: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "compressed-liquid EOS: {:?} at {} Pa, {} K",
            self.reason, self.pressure_pa, self.temperature_k
        )
    }
}
impl std::error::Error for Error {}

/// A pure compressed-liquid state under prescribed P–T. No blending with IF97,
/// ice-density fallback, thermal closure or extrapolation is supplied.
pub fn state(pressure_pa: f64, temperature_k: f64) -> Result<State, Error> {
    state_with_budget(pressure_pa, temperature_k, MAX_EOS_EVALUATIONS).map(|s| s.state)
}

/// Reduce, never raise, the per-call work ceiling. Both outcomes report work.
pub fn state_with_budget(
    pressure_pa: f64,
    temperature_k: f64,
    max_eos_evaluations: usize,
) -> Result<EvaluatedState, Error> {
    evaluate(pressure_pa, temperature_k, max_eos_evaluations, false)
}

/// Explicit liquid-side limit on the nominal melting curve. This does not
/// select a coexistence mixture or permit metastable solid-side extension.
pub fn liquid_limit_with_budget(
    pressure_pa: f64,
    temperature_k: f64,
    max_eos_evaluations: usize,
) -> Result<EvaluatedState, Error> {
    evaluate(pressure_pa, temperature_k, max_eos_evaluations, true)
}

fn evaluate(
    pressure_pa: f64,
    temperature_k: f64,
    max_eos_evaluations: usize,
    liquid_limit: bool,
) -> Result<EvaluatedState, Error> {
    let error = |reason| Error {
        pressure_pa,
        temperature_k,
        reason,
        eos_evaluations: 0,
    };
    if !pressure_pa.is_finite() || !(MIN_PRESSURE_PA..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(error(Failure::PressureDomain));
    }
    if !temperature_k.is_finite()
        || !(MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&temperature_k)
    {
        return Err(error(Failure::TemperatureDomain));
    }
    // At >=100 MPa and <=623.15 K there is no vapour equilibrium. Below
    // 355 K use the applicable high-pressure melting branch as a liquid mask.
    // Above 355 K the VII melting curve is already above our 1 GPa ceiling.
    let branch = if temperature_k <= 273.31 {
        Some(melting::Branch::V)
    } else if temperature_k <= 355.0 {
        Some(melting::Branch::VI)
    } else {
        None
    };
    if let Some(branch) = branch {
        let melting = melting::pressure_pa(branch, temperature_k).unwrap();
        if on_curve(pressure_pa, melting) && !liquid_limit {
            return Err(error(Failure::CoexistenceDensity));
        }
        if pressure_pa > melting && !on_curve(pressure_pa, melting) {
            return Err(error(Failure::SolidDomain));
        }
    }
    let limit = max_eos_evaluations.min(MAX_EOS_EVALUATIONS);
    let mut evaluations = 0;
    let density = invert(pressure_pa, temperature_k, &mut evaluations, limit);
    let density_kg_m3 = density.map_err(|reason| Error {
        eos_evaluations: evaluations,
        ..error(reason)
    })?;
    Ok(EvaluatedState {
        state: State {
            pressure_pa,
            temperature_k,
            phase: Phase::Liquid,
            density_kg_m3,
        },
        eos_evaluations: evaluations,
    })
}

fn invert(
    pressure: f64,
    temperature: f64,
    evaluations: &mut usize,
    limit: usize,
) -> Result<f64, Failure> {
    // IF97 at its 100 MPa ceiling is only a numerical bracket seed. Neither
    // this estimate nor an adjusted pressure can become a returned density.
    // This bracket stays on the dense branch for the narrow application mask;
    // it is not a general IAPWS95 vapour/liquid or critical-region inversion.
    // Endpoint pressures are numerical algebra probes, not certified physical
    // states: only a closed root at the supported requested P–T is returned.
    if *evaluations == limit {
        return Err(Failure::EvaluationLimit);
    }
    *evaluations += 1;
    let seed = liquid::density_kg_m3(MIN_PRESSURE_PA, temperature);
    let mut evaluate = |density| {
        if *evaluations == limit {
            return Err(Failure::EvaluationLimit);
        }
        *evaluations += 1;
        Ok(helmholtz::pressure_pa(density, temperature))
    };
    let mut low = 0.98 * seed;
    let mut high = 1.6 * seed;
    let a = evaluate(low)?;
    let b = evaluate(high)?;
    if !a.is_finite() || !b.is_finite() || a >= pressure || b <= pressure {
        return Err(Failure::DensityBracket);
    }
    for _ in 0..MAX_ITERATIONS {
        let density = low + (high - low) * 0.5;
        let evaluated = evaluate(density)?;
        if !evaluated.is_finite() {
            return Err(Failure::DensityNonConvergence);
        }
        // An actual forward pressure residual, not density-interval width alone.
        if (evaluated - pressure).abs() <= (pressure * 1e-11).max(0.01) {
            return Ok(density);
        }
        if evaluated < pressure {
            low = density;
        } else {
            high = density;
        }
    }
    Err(Failure::DensityNonConvergence)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn explicit_melting_limit_keeps_unconstrained_coexistence_refusal() {
    let t = 300.0;
    let p = melting::pressure_pa(melting::Branch::VI, t).unwrap();
    assert_eq!(state(p, t).unwrap_err().reason, Failure::CoexistenceDensity);
    let limit = liquid_limit_with_budget(p, t, MAX_EOS_EVALUATIONS).unwrap();
    let inside = state(p - 1.0, t).unwrap();
    assert!((limit.state.density_kg_m3 / inside.density_kg_m3 - 1.0).abs() < 1e-9);
    assert_eq!(
        liquid_limit_with_budget(p + 1.0, t, MAX_EOS_EVALUATIONS)
            .unwrap_err()
            .reason,
        Failure::SolidDomain
    );
    assert_eq!(
        liquid_limit_with_budget(p, t, limit.eos_evaluations - 1)
            .unwrap_err()
            .reason,
        Failure::EvaluationLimit
    );
}
