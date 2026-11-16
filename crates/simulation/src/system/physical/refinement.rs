//! Complete worker-prepared replacement. No structure/partition solve runs at
//! publication, and caller-owned navigation/identity metadata is never replaced.
use super::*;
use crate::system::structure::prototype::Outcome;
use space_shared::{PhysicalRevision, PlanetEnvironment, PlanetResult, RefinementStatus};

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
        if env != &self.environment {
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
            if result.record.surface_pressure_pa != initial.record.surface_pressure_pa
                || result.record.surface_temperature_k != initial.record.surface_temperature_k
            {
                return Err("layered solve did not receive the decided pressure and temperature");
            }
            for decided in &initial.record.physical.materials {
                let solved = result
                    .record
                    .physical
                    .materials
                    .iter()
                    .find(|m| m.material == decided.material)
                    .ok_or("missing solved material")?;
                if solved.gas_kg != decided.gas_kg {
                    return Err("layered solve did not receive the decided gas inventory");
                }
                if solved.source_kg != decided.source_kg || solved.escaped_kg != decided.escaped_kg
                {
                    return Err("layered solve changed source or escape");
                }
            }
            layered::receive_boundary(&mut result.record, &initial.record);
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
    // These are the same immutable forcing values, not a candidate equilibrium
    // temperature overwritten with its solved boundary temperature.
    Ok(Refinement {
        base: initial.revision.clone(),
        original_record: initial.record.clone(),
        result,
        environment: forcing.clone(),
    })
}

#[cfg(test)]
mod tests;
