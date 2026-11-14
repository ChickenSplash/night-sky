use super::*;
use crate::system::generated::{Mode, generated_system_with_mode};

fn fixture() -> (Planet, Refinement) {
    let prepared =
        generated_system_with_mode("g3c0200000.2.128", [0.0; 3], Mode::PrepareRefinement).unwrap();
    prepared
        .jobs
        .iter()
        .find_map(|job| {
            let outcome = job.resolve();
            if !matches!(outcome, Outcome::Layered(_)) {
                return None;
            }
            let refinement =
                prepare_refinement(&job.initial_result, &job.environment, &outcome).ok()?;
            let planet = prepared
                .system
                .planets
                .iter()
                .find(|p| p.id == job.id)?
                .clone();
            Some((planet, refinement))
        })
        .expect("supported layered fixture")
}

#[test]
fn complete_projection_preserves_identity_orbits_spin_and_forcing() {
    let (planet, refinement) = fixture();
    let next = refinement.project(&planet).unwrap();
    assert_eq!(next.id, planet.id);
    assert_eq!(next.orbit, planet.orbit);
    assert_eq!(next.spin, planet.spin);
    assert_eq!(next.page, planet.page);
    assert_eq!(next.orbit_index, planet.orbit_index);
    assert_eq!(next.designation, planet.designation);
    assert_eq!(next.initial_result.as_ref().unwrap().revision.sequence, 1);
    assert_eq!(
        next.initial_result.as_ref().unwrap().provenance,
        space_shared::ResultProvenance::Layered
    );
    assert_eq!(
        next.environment.as_ref().unwrap().equilibrium_temperature_k,
        planet
            .environment
            .as_ref()
            .unwrap()
            .equilibrium_temperature_k
    );
    validate(&next).unwrap().unwrap();
    assert!(refinement.project(&next).is_err(), "no duplicate adoption");
    assert_eq!(
        refinement.project(&planet).unwrap(),
        next,
        "replay onto original base only"
    );
    let mut stale = planet.clone();
    stale
        .initial_result
        .as_mut()
        .unwrap()
        .revision
        .inputs
        .push_str(":changed");
    assert!(refinement.project(&stale).is_err());
    assert_eq!(planet.initial_result.as_ref().unwrap().revision.sequence, 0);
}

#[test]
fn refusal_keeps_the_partitioned_fallback_geometry_and_records_the_reason() {
    let prepared =
        generated_system_with_mode("g13b6ffffcfeba.2.1", [0.0; 3], Mode::PrepareRefinement)
            .unwrap();
    let job = &prepared.jobs[0];
    let refused = Outcome::Fallback(Box::new(crate::system::structure::fallback::solve(
        &job.budget,
        &job.environment,
        crate::system::structure::coupled::Error::DenseGasUnsupported,
    )));
    let refinement = prepare_refinement(&job.initial_result, &job.environment, &refused).unwrap();
    let planet = prepared
        .system
        .planets
        .iter()
        .find(|p| p.id == job.id)
        .unwrap();
    let next = refinement.project(planet).unwrap();
    assert_eq!(next.radius_km, planet.radius_km);
    assert_eq!(next.physical, planet.physical);
    assert_eq!(next.environment, planet.environment);
    assert_eq!(
        refinement.result.record.layers,
        job.initial_result.record.layers
    );
    assert!(matches!(
        refinement.result.refinement,
        RefinementStatus::Refused { .. }
    ));
}

#[test]
fn inspected_cold_water_candidate_replaces_the_gas_coordinate_with_its_ice_boundary() {
    let prepared =
        generated_system_with_mode("g3c0200000.2.128", [0.0; 3], Mode::PrepareRefinement).unwrap();
    let id = "g3c0200000.2.128#A/6";
    let job = prepared.jobs.iter().find(|j| j.id == id).unwrap();
    let old = prepared.system.planets.iter().find(|p| p.id == id).unwrap();
    let refinement =
        prepare_refinement(&job.initial_result, &job.environment, &job.resolve()).unwrap();
    let next = refinement.project(old).unwrap();
    let result = next.initial_result.as_ref().unwrap();
    // Generation already decided a condensed surface under its column: the
    // layered solve adds the phase, never a change of reference.
    assert_eq!(
        old.initial_result.as_ref().unwrap().surface_knowledge,
        space_shared::SurfaceKnowledge::Unresolved
    );
    assert_eq!(
        old.initial_result.as_ref().unwrap().record.surface,
        "unresolved water"
    );
    assert_eq!(
        result.record.surface_temperature_k,
        old.initial_result
            .as_ref()
            .unwrap()
            .record
            .surface_temperature_k
    );
    assert_eq!(
        result.surface_knowledge,
        space_shared::SurfaceKnowledge::SolidPhase
    );
    assert_eq!(result.record.surface, "ice");
    assert!(result.record.surface_pressure_pa > 1e5);
    let env = next.environment.as_ref().unwrap();
    assert!(result.record.surface_temperature_k > env.equilibrium_temperature_k);
    assert_eq!(
        env.atmosphere.as_ref().unwrap().pressure_pa,
        result.record.surface_pressure_pa
    );
    assert_eq!(
        env.atmosphere.as_ref().unwrap().reference_level,
        space_shared::AtmosphereReference::Surface
    );
    validate(&next).unwrap().unwrap();
}
