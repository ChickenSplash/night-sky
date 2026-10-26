//! Prescribed H₂O ice-VII compression branch, not an equilibrium phase selector.
//!
//! Lai et al. (2023), American Mineralogist 108, 1530–1537,
//! doi:10.2138/am-2022-8554, fixed-volume third-order Birch–Murnaghan fit.
//! Only the 300 K isotherm is enabled, over a deliberately narrower pressure
//! interval than the experiments. No source code, spline or EOS table is used.

use std::fmt;

pub const MODEL_ID: &str = "lai_2023_ice_vii_300k_bm3_v1";
pub const TEMPERATURE_K: f64 = 300.0;
pub const MIN_PRESSURE_PA: f64 = 3.5e9;
pub const MAX_PRESSURE_PA: f64 = 20e9;
pub const MAX_PRESSURE_EVALUATIONS: usize = 98;

// Figure 5 caption gives the fixed unit-cell volume more precisely than the
// rounded molar volume (12.3 cm³/mol) in table 1. The cubic cell contains Z=2
// H₂O molecules. Do not mix in the fitted-volume or Vinet parameter rows.
const REFERENCE_CELL_VOLUME_M3: f64 = 40.86e-30;
const BULK_MODULUS_PA: f64 = 21.0e9;
const BULK_MODULUS_DERIVATIVE: f64 = 4.45;
const MOLAR_MASS_KG_MOL: f64 = 0.01801528;
const AVOGADRO_PER_MOL: f64 = 6.02214076e23;
const REFERENCE_DENSITY_KG_M3: f64 =
    2.0 * MOLAR_MASS_KG_MOL / (AVOGADRO_PER_MOL * REFERENCE_CELL_VOLUME_M3);

/// A density on an explicitly requested branch. Not a `water::State`: this
/// result neither establishes equilibrium nor enables VI/VII/VIII selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BranchDensity {
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub density_kg_m3: f64,
    /// Signed forward EOS pressure minus the requested pressure.
    pub pressure_residual_pa: f64,
    pub pressure_evaluations: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    PressureDomain,
    TemperatureDomain,
    EvaluationLimit,
    DensityBracket,
    DensityNonConvergence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Error {
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub reason: Failure,
    /// Includes endpoint probes and unsuccessful inversion work.
    pub pressure_evaluations: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "prescribed ice-VII density: {:?} at {} Pa, {} K ({} pressure evaluations)",
            self.reason, self.pressure_pa, self.temperature_k, self.pressure_evaluations
        )
    }
}
impl std::error::Error for Error {}

/// Evaluate only the prescribed 300 K branch. Other temperatures, including
/// 250 K, are errors rather than an isothermal approximation or an ice-VIII proxy.
pub fn density(pressure_pa: f64, temperature_k: f64) -> Result<BranchDensity, Error> {
    density_with_budget(pressure_pa, temperature_k, MAX_PRESSURE_EVALUATIONS)
}

/// Limit nested inversion work, including both bracket endpoint probes.
/// A larger requested budget cannot raise the fixed per-call ceiling. Failures
/// retain consumed work so a future radial caller can debit unsuccessful trials.
pub fn density_with_budget(
    pressure_pa: f64,
    temperature_k: f64,
    max_pressure_evaluations: usize,
) -> Result<BranchDensity, Error> {
    let error = |reason, pressure_evaluations| Error {
        pressure_pa,
        temperature_k,
        reason,
        pressure_evaluations,
    };
    if !pressure_pa.is_finite() || !(MIN_PRESSURE_PA..=MAX_PRESSURE_PA).contains(&pressure_pa) {
        return Err(error(Failure::PressureDomain, 0));
    }
    if temperature_k != TEMPERATURE_K {
        return Err(error(Failure::TemperatureDomain, 0));
    }
    let limit = max_pressure_evaluations.min(MAX_PRESSURE_EVALUATIONS);
    let mut evaluations = 0;
    let mut evaluate = |compression| {
        if evaluations == limit {
            return Err(error(Failure::EvaluationLimit, evaluations));
        }
        evaluations += 1;
        let pressure = pressure_at_compression(compression);
        if !pressure.is_finite() {
            return Err(error(Failure::DensityNonConvergence, evaluations));
        }
        Ok(pressure)
    };
    // x=(V0/V)^(1/3); endpoints are algebraic probes, not physical states
    // exposed outside the public mask. For K'=4.45 and x>=1, every factor is
    // nonnegative and increasing, so the compressive branch has a unique root.
    let mut low = 1.0;
    let mut high = 1.2;
    if evaluate(low)? >= pressure_pa || evaluate(high)? <= pressure_pa {
        return Err(error(Failure::DensityBracket, evaluations));
    }
    loop {
        let compression = low + (high - low) * 0.5;
        if compression == low || compression == high {
            return Err(error(Failure::DensityNonConvergence, evaluations));
        }
        let evaluated = evaluate(compression)?;
        let residual = evaluated - pressure_pa;
        if residual.abs() <= pressure_pa * 1e-11 {
            return Ok(BranchDensity {
                pressure_pa,
                temperature_k,
                density_kg_m3: REFERENCE_DENSITY_KG_M3 * compression.powi(3),
                pressure_residual_pa: residual,
                pressure_evaluations: evaluations,
            });
        }
        if residual < 0.0 {
            low = compression;
        } else {
            high = compression;
        }
    }
}

fn pressure_at_compression(x: f64) -> f64 {
    1.5 * BULK_MODULUS_PA
        * (x.powi(7) - x.powi(5))
        * (1.0 + 0.75 * (BULK_MODULUS_DERIVATIVE - 4.0) * (x * x - 1.0))
}

#[cfg(test)]
mod tests;
