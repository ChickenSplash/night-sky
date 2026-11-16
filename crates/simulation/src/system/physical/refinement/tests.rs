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
    assert_eq!(planet.environment, next.environment);
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
fn refinement_rejects_changed_bulk_boundaries_and_allocations() {
    let (planet, refinement) = fixture();
    let initial = planet.initial_result.as_ref().unwrap();
    for edit in [
        (|r: &mut space_shared::LayeredPlanet| r.physical.inertia_factor *= 1.0001)
            as fn(&mut space_shared::LayeredPlanet),
        |r| r.physical.reference_gravity_m_s2 *= 0.99999999,
        |r| r.surface_temperature_k *= 1.0001,
        |r| r.surface_pressure_pa = r.surface_pressure_pa.next_up(),
        |r| r.physical.condensed_radius_m = r.physical.condensed_radius_m.next_up(),
    ] {
        let mut candidate = refinement.result.clone();
        edit(&mut candidate.record);
        assert!(validate_refinement(initial, &candidate).is_err());
    }
}

#[test]
#[ignore = "local pinned H/He resource; run in release"]
fn prescribed_atmospheres_keep_all_five_previously_unsupported_cloud_columns() {
    use crate::{
        galaxy::Component,
        system::{
            atmosphere::{GasAtmosphere, optics::boundary::OpticalColumn},
            galactic_bodies,
            generated::PlanetJob,
            structure::prototype,
        },
    };
    for (component, index, suffix) in [
        (Component::Nucleus, 34, "/9"),
        (Component::Bulge, 14, "/5"),
        (Component::Arms, 11, "#A/10"),
        (Component::Arms, 32, "/4"),
        (Component::Halo, 44, "/6"),
    ] {
        let key = format!("composition-calibration:{component:?}:{index}");
        let hosted = galactic_bodies(&key, component)
            .planets
            .into_iter()
            .find(|p| p.id().ends_with(suffix))
            .unwrap();
        let job = PlanetJob {
            id: hosted.id(),
            designation: String::new(),
            budget: hosted.body.budget,
            environment: hosted.environment.clone().unwrap(),
            initial_result: hosted.body.initial_result.clone(),
            decision: hosted.body.decision,
        };
        let old = prototype::resolve(
            &job.budget,
            &job.environment,
            job.decision,
            Default::default(),
        );
        let Outcome::Layered(old) = old else {
            panic!("{} headless fixture", job.id)
        };
        assert!(
            layered(&old)
                .layers
                .iter()
                .any(|l| l.phase_allocations.iter().any(|p| p.condensate_kg > 0.0))
        );
        let outcome = job.resolve();
        assert!(matches!(outcome, Outcome::Layered(_)), "{}", job.id);
        let refinement =
            prepare_refinement(&job.initial_result, &job.environment, &outcome).unwrap();
        let planet = export_planet(hosted, 0, String::new(), None);
        let next = refinement.project(&planet).unwrap();
        assert_eq!(planet.environment, next.environment);
        let gas = GasAtmosphere::try_from(job.environment.atmosphere.as_ref().unwrap()).unwrap();
        assert_eq!(
            OpticalColumn::from_result(&job.initial_result, &gas).unwrap(),
            OpticalColumn::from_result(&refinement.result, &gas).unwrap()
        );
    }
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
