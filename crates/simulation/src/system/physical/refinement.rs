//! Complete worker-prepared replacement. No structure/partition solve runs at
//! publication, and caller-owned navigation/identity metadata is never replaced.
use super::*;
use crate::system::{
    atmosphere::{Composition, Gas, ReferenceLevel},
    materials::Material,
    structure::prototype::Outcome,
};
use space_shared::{
    PhysicalRevision, PlanetEnvironment, PlanetResult, RadiusReference, RefinementStatus,
};

#[derive(Clone, Debug)]
pub struct Refinement {
    pub base: PhysicalRevision,
    original_record: space_shared::LayeredPlanet,
    pub result: PlanetResult,
    pub environment: PlanetEnvironment,
}

impl Refinement {
    /// Stage every physical projection on a clone. Failure leaves the live
    /// planet untouched. Replay is allowed only onto the exact original base.
    pub fn project(&self, planet: &Planet) -> Result<Planet, &'static str> {
        validate(planet)?.ok_or("missing active physical facts")?;
        let active = planet
            .initial_result
            .as_ref()
            .ok_or("missing active result")?;
        if active.revision != self.base {
            return Err("stale refinement base");
        }
        if active.record != self.original_record {
            return Err("active facts changed without a new revision");
        }
        let env = planet
            .environment
            .as_ref()
            .ok_or("missing immutable forcing")?;
        if env.irradiation_w_m2 != self.environment.irradiation_w_m2
            || env.assumed_bond_albedo != self.environment.assumed_bond_albedo
            || env.equilibrium_temperature_k != self.environment.equilibrium_temperature_k
            || env.age_gyr != self.environment.age_gyr
        {
            return Err("refinement forcing differs from active inputs");
        }
        validate_refinement(active, &self.result)?;
        let mut next = planet.clone();
        let facts = &self.result.record.physical;
        next.mass_earth = facts.retained_mass_kg / EARTH_MASS_KG;
        next.radius_km = facts.reference_radius_m / 1000.0;
        next.inertia_factor = Some(facts.inertia_factor);
        next.physical = Some(facts.clone());
        next.initial_result = Some(self.result.clone());
        next.environment = Some(self.environment.clone());
        // Preserve rotational phase/period and orbital state. The shape consumer
        // recomputes bounded flattening from the new mass/radius/inertia.
        validate(&next)?.ok_or("missing refined physical facts")?;
        Ok(next)
    }
}

/// Called on the solver worker with the immutable starting result. Refusals
/// retain the initial partition/geometry and only complete its status. They do
/// not install a different unpartitioned fallback from the failed trial.
pub fn prepare_refinement(
    initial: &PlanetResult,
    forcing: &PlanetEnvironment,
    outcome: &Outcome,
) -> Result<Refinement, &'static str> {
    validate_result(initial)?;
    let mut revision = initial.revision.clone();
    revision.sequence = revision
        .sequence
        .checked_add(1)
        .ok_or("revision overflow")?;
    let result = match outcome {
        Outcome::Layered(_) => {
            // The layered solve keeps generation's decisions; carry their
            // audit facts forward instead of dropping them at adoption.
            let mut result = resolved_result(outcome, revision)?;
            result.record.blowoff_kg = initial.record.blowoff_kg;
            if result.record.surface != "envelope" {
                result.record.greenhouse_optical_depth = initial
                    .record
                    .greenhouse_optical_depth
                    .or(result.record.greenhouse_optical_depth);
            }
            result
        }
        Outcome::Fallback(f) => {
            let refused = fallback(f);
            let reason = refused.fallback_reason.ok_or("missing refusal reason")?;
            let mut result = initial.clone();
            result.revision = revision;
            result.record.fallback_reason = Some(reason.clone());
            result.refinement = RefinementStatus::Refused { reason };
            result
        }
    };
    validate_refinement(initial, &result)?;
    if matches!(outcome, Outcome::Fallback(_)) {
        return Ok(Refinement {
            base: initial.revision.clone(),
            original_record: initial.record.clone(),
            result,
            environment: forcing.clone(),
        });
    }
    let mut environment = forcing.clone();
    environment.atmosphere = None;
    if result.record.surface_pressure_pa >= MIN_PRESSURE_PA {
        let facts = &result.record.physical;
        let weights = Gas::ALL.map(|gas| {
            let mass = facts
                .materials
                .iter()
                .find(|m| m.material == Material::from_gas(gas).id())
                .map_or(0.0, |m| m.gas_kg);
            (gas, mass / gas.molecular_mass_u())
        });
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        if !total.is_finite() || total <= 0.0 {
            return Err("missing refined gas inventory");
        }
        let gas = GasAtmosphere::new(
            Composition::new(&weights.map(|(g, w)| (g, w / total)))
                .map_err(|_| "invalid refined mixture")?,
            if facts.radius_reference == RadiusReference::OneBar {
                ReferenceLevel::OneBar
            } else {
                ReferenceLevel::Surface
            },
            result.record.surface_pressure_pa,
            result.record.surface_temperature_k,
            None,
        )
        .map_err(|_| "invalid refined gas state")?;
        // Unsupported clouds/optical domains must not turn into invisible air.
        OpticalColumn::from_result(&result, &gas)
            .map_err(|_| "refined cloud/optical inputs unsupported")?;
        environment.atmosphere = Some(
            gas.to_data(
                facts.reference_gravity_m_s2 * facts.reference_radius_m.powi(2),
                facts.reference_radius_m,
            )
            .map_err(|_| "invalid refined atmospheric projection")?,
        );
    }
    // These are the same immutable forcing values, not a candidate equilibrium
    // temperature overwritten with its solved boundary temperature.
    Ok(Refinement {
        base: initial.revision.clone(),
        original_record: initial.record.clone(),
        result,
        environment,
    })
}

#[cfg(test)]
mod tests;
