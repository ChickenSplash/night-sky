//! Bounded Peng–Robinson pure vapour/supercritical densities. This is not a
//! planetary-pressure EOS, liquid/solid model, reaction model or mixture flash.
//! Equations, parameter sources and deliberate engineering masks are documented
//! in docs/simulation/planet-composition.md#hhe-rich-molecular-mixtures.
use super::Material;
use crate::{
    system::atmosphere::{Gas, condensation::vapour_pressure},
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};

pub const MODEL_ID: &str = "screened_peng_robinson_molecular_v1";
pub const MIN_TEMPERATURE_K: f64 = 80.0;
pub const MAX_TEMPERATURE_K: f64 = 625.0;
pub const MAX_PRESSURE_PA: f64 = 1e8;

#[cfg(test)]
thread_local! {
    pub(crate) static LAST_CONDENSATION: std::cell::Cell<Option<(Material,f64,f64)>> = const { std::cell::Cell::new(None) };
}

fn error_at(material: Material, pressure: f64, temperature: f64, reason: Reason) -> Error {
    #[cfg(test)]
    if reason == Reason::PureCondensationScreen {
        LAST_CONDENSATION.set(Some((material, pressure, temperature)));
    }
    let _ = (pressure, temperature);
    Error { material, reason }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    UnsupportedMaterial,
    StateDomain,
    PureCondensationScreen,
    CriticalRegion,
    UnstableRoot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub material: Material,
    pub reason: Reason,
}

impl Error {
    pub fn label(self) -> &'static str {
        match self.reason {
            Reason::UnsupportedMaterial => "molecular component unsupported",
            Reason::StateDomain => "molecular component pressure/temperature domain",
            Reason::PureCondensationScreen => "molecular conservative pure condensation screen",
            Reason::CriticalRegion => "molecular near-critical domain",
            Reason::UnstableRoot => "molecular root stability",
        }
    }
}

// Rounded physical parameters from CoolProp v7.2.0's cubic-fluid catalogue.
// These are pure-fluid constants, not fitted binary interaction parameters.
fn parameters(material: Material) -> Option<(Gas, f64, f64, f64)> {
    Some(match material {
        Material::CarbonDioxide => (Gas::CarbonDioxide, 304.1282, 7_377_300.0, 0.22394),
        Material::Nitrogen => (Gas::Nitrogen, 126.192, 3_395_800.0, 0.0372),
        Material::Methane => (Gas::Methane, 190.564, 4_599_200.0, 0.01142),
        Material::Oxygen => (Gas::Oxygen, 154.581, 5_043_000.0, 0.0222),
        Material::Argon => (Gas::Argon, 150.687, 4_863_000.0, -0.00219),
        _ => return None,
    })
}

pub fn density_kg_m3(
    material: Material,
    pressure_pa: f64,
    temperature_k: f64,
) -> Result<f64, Error> {
    let error = |reason| error_at(material, pressure_pa, temperature_k, reason);
    let (gas, tc, pc, omega) =
        parameters(material).ok_or_else(|| error(Reason::UnsupportedMaterial))?;
    if !pressure_pa.is_finite()
        || !(1.0..=MAX_PRESSURE_PA).contains(&pressure_pa)
        || !temperature_k.is_finite()
        || !(MIN_TEMPERATURE_K..=MAX_TEMPERATURE_K).contains(&temperature_k)
    {
        return Err(error(Reason::StateDomain));
    }
    screen_pure_endpoint(material, pressure_pa, temperature_k)?;
    let kappa = 0.37464 + 1.54226 * omega - 0.26992 * omega * omega;
    let alpha = (1.0 + kappa * (1.0 - (temperature_k / tc).sqrt())).powi(2);
    let a = 0.45724 * alpha * (tc / temperature_k).powi(2) * pressure_pa / pc;
    let b = 0.07780 * tc / temperature_k * pressure_pa / pc;
    let roots = cubic_roots(
        b - 1.0,
        a - 3.0 * b * b - 2.0 * b,
        -a * b + b * b + b.powi(3),
    );
    let physical = |z: f64| z.is_finite() && z > b;
    let z = roots
        .into_iter()
        .filter(|z| physical(*z))
        .max_by(f64::total_cmp)
        .ok_or_else(|| error(Reason::UnstableRoot))?;
    // A subcritical one-root result can be compressed liquid, not vapour.
    // PR's critical v/b = Zc/Bc supplies a conservative branch discriminator;
    // the shared cold-trap cap alone is not a liquid-vapour phase calculation.
    if temperature_k < tc && z < (0.3074 / 0.07780) * b {
        return Err(error(Reason::PureCondensationScreen));
    }
    let attraction_denominator = z * z + 2.0 * b * z - b * b;
    let derivative =
        -1.0 / (z - b).powi(2) + a * (2.0 * z + 2.0 * b) / attraction_denominator.powi(2);
    let polynomial =
        ((z + b - 1.0) * z + a - 3.0 * b * b - 2.0 * b) * z - a * b + b * b + b.powi(3);
    if !derivative.is_finite() || derivative >= 0.0 || polynomial.abs() > 1e-12 * (1.0 + z.powi(3))
    {
        return Err(error(Reason::UnstableRoot));
    }
    // Do not return a metastable vapour root when the cubic prefers its dense
    // root. Pure-fluid stability still does not establish mixture equilibrium.
    let ln_phi = |z: f64| {
        let s = std::f64::consts::SQRT_2;
        z - 1.0
            - (z - b).ln()
            - a / (2.0 * s * b) * ((z + (1.0 + s) * b) / (z + (1.0 - s) * b)).ln()
    };
    for other in roots.into_iter().filter(|r| physical(*r) && *r < z - 1e-8) {
        if ln_phi(other) < ln_phi(z) + 1e-10 {
            return Err(error(Reason::PureCondensationScreen));
        }
    }
    let density =
        pressure_pa * gas.molecular_mass_u() * ATOMIC_MASS_KG / (z * BOLTZMANN_J_K * temperature_k);
    if !density.is_finite() || density <= 0.0 {
        return Err(error(Reason::UnstableRoot));
    }
    Ok(density)
}

/// Common conservative pure-endpoint screen, not a mixture phase calculation.
/// Callers own their EOS application domains. It never evaluates PR out of mask.
pub(super) fn screen_pure_endpoint(
    material: Material,
    pressure_pa: f64,
    temperature_k: f64,
) -> Result<(), Error> {
    let error = |reason| error_at(material, pressure_pa, temperature_k, reason);
    let (gas, tc, pc, _) =
        parameters(material).ok_or_else(|| error(Reason::UnsupportedMaterial))?;
    // Conservative pure-endpoint restriction at TOTAL pressure. Dilution can
    // prevent condensation in a real mixture; no mixed dew/frost point is claimed.
    // O2 has no shared cap, so its subcritical range is deliberately unsupported.
    if temperature_k < tc
        && (gas == Gas::Oxygen || pressure_pa >= vapour_pressure(gas, temperature_k))
    {
        return Err(error(Reason::PureCondensationScreen));
    }
    // Cubic-EOS near-critical densities are not precise enough to grant new
    // planetary coverage. This is an engineering exclusion, not a phase curve.
    if (temperature_k / tc - 1.0).abs() < 0.1 && pressure_pa > 0.5 * pc {
        return Err(error(Reason::CriticalRegion));
    }
    Ok(())
}

/// Real roots of z³ + c2 z² + c1 z + c0, with no unbounded inner iteration.
fn cubic_roots(c2: f64, c1: f64, c0: f64) -> [f64; 3] {
    let p = c1 - c2 * c2 / 3.0;
    let q = 2.0 * c2.powi(3) / 27.0 - c2 * c1 / 3.0 + c0;
    let discriminant = q * q / 4.0 + p.powi(3) / 27.0;
    if discriminant >= 0.0 {
        let d = discriminant.sqrt();
        let z = (-q / 2.0 + d).cbrt() + (-q / 2.0 - d).cbrt() - c2 / 3.0;
        [z, f64::NAN, f64::NAN]
    } else {
        let r = (-p / 3.0).sqrt();
        let theta = (-q / (2.0 * r.powi(3))).clamp(-1.0, 1.0).acos() / 3.0;
        [
            0.0,
            2.0 * std::f64::consts::PI / 3.0,
            4.0 * std::f64::consts::PI / 3.0,
        ]
        .map(|angle| 2.0 * r * (theta - angle).cos() - c2 / 3.0)
    }
}

#[cfg(test)]
mod tests;
