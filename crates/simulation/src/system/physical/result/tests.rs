use super::*;
use crate::{system::composition::Inventory, units::EARTH_MASS_KG};

fn revision(sequence: u64) -> PhysicalRevision {
    PhysicalRevision {
        inputs: "fixture:supply-and-forcing:v1".into(),
        model: "fixture:physics:v1".into(),
        sequence,
    }
}

fn environment(temperature: f64) -> PlanetEnvironment {
    PlanetEnvironment {
        irradiation_w_m2: 1361.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: temperature,
        age_gyr: 4.5,
        atmosphere: None,
    }
}

fn budget(water: f64, gas: f64) -> Budget {
    let retained = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, (0.7 - water) * EARTH_MASS_KG),
        (Material::Water, water * EARTH_MASS_KG),
        (Material::Hydrogen, gas * EARTH_MASS_KG),
    ])
    .unwrap();
    Budget::new(
        retained,
        retained,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap()
}

fn round_trip(result: &PlanetResult) {
    validate_result(result).unwrap();
    let encoded = serde_json::to_string(result).unwrap();
    let decoded: PlanetResult = serde_json::from_str(&encoded).unwrap();
    assert_eq!(&decoded, result);
    validate_result(&decoded).unwrap();
}

#[test]
fn startup_fallback_has_honest_provenance_and_keeps_usable_facts_during_failure() {
    for (water, gas) in [(0.0, 0.0), (0.3, 0.0), (0.0, 0.1)] {
        let b = budget(water, gas);
        let env = environment(250.0);
        let initial = initial_result(&b, &env, revision(0)).unwrap();
        assert_eq!(initial.provenance, ResultProvenance::Fallback);
        assert_eq!(initial.refinement, RefinementStatus::Pending);
        assert_eq!(initial.record.fallback_reason, None);
        assert_eq!(
            initial.surface_knowledge,
            if gas > 0.0 {
                SurfaceKnowledge::GasReference
            } else {
                SurfaceKnowledge::Unresolved
            }
        );
        round_trip(&initial);
        for status in [
            RefinementStatus::Running,
            RefinementStatus::Unavailable {
                reason: "H/He resource absent".into(),
            },
            RefinementStatus::Failed {
                reason: "worker panicked".into(),
            },
        ] {
            let mut active = initial.clone();
            active.refinement = status;
            round_trip(&active);
            assert_eq!(active.record, initial.record);
        }
        let refused =
            structure::fallback::solve(&b, &env, structure::coupled::Error::DenseGasUnsupported);
        let candidate = resolved_result(
            &structure::prototype::Outcome::Fallback(Box::new(refused)),
            revision(1),
        )
        .unwrap();
        validate_refinement(&initial, &candidate).unwrap();
        assert_eq!(candidate.record.physical, initial.record.physical);
        assert_eq!(candidate.record.layers, initial.record.layers);
        round_trip(&candidate);
    }
}

#[test]
fn layered_phases_and_hot_rock_are_not_inferred_from_legacy_ground_labels() {
    for (water, temperature, expected) in [
        (0.0, 250.0, SurfaceKnowledge::Unresolved),
        (0.0, 1800.0, SurfaceKnowledge::Unresolved),
        (0.3, 250.0, SurfaceKnowledge::SolidPhase),
        (0.3, 280.0, SurfaceKnowledge::Liquid),
    ] {
        let b = budget(water, 0.0);
        let env = environment(temperature);
        let solved = structure::prototype::solve(&b, &env, Default::default()).unwrap();
        let candidate = resolved_result(
            &structure::prototype::Outcome::Layered(Box::new(solved)),
            revision(1),
        )
        .unwrap();
        assert_eq!(candidate.surface_knowledge, expected);
        let initial = initial_result(&b, &env, revision(0)).unwrap();
        validate_refinement(&initial, &candidate).unwrap();
        round_trip(&candidate);
        let mut invented = candidate.clone();
        invented.surface_knowledge = SurfaceKnowledge::GasReference;
        assert!(validate_result(&invented).is_err());
    }
}

#[test]
fn live_layered_temperature_is_not_replaced_by_equilibrium_forcing() {
    let forcing = environment(1800.0);
    let b = budget(0.0, 0.0);
    let solved = structure::prototype::solve(&b, &forcing, Default::default()).unwrap();
    let result = resolved_result(
        &structure::prototype::Outcome::Layered(Box::new(solved)),
        revision(1),
    )
    .unwrap();
    let facts = &result.record.physical;
    let planet = space_shared::Planet {
        mass_earth: facts.retained_mass_kg / EARTH_MASS_KG,
        radius_km: facts.reference_radius_m / 1000.0,
        inertia_factor: Some(facts.inertia_factor),
        physical: Some(facts.clone()),
        // Stellar forcing and the solved boundary temperature are separate.
        environment: Some(environment(250.0)),
        initial_result: Some(result),
        ..Default::default()
    };
    super::super::validate(&planet).unwrap().unwrap();
    let geometry = crate::system::surface::geometry(&planet).unwrap();
    assert_eq!(geometry.condensed_phase, SurfaceKnowledge::Unresolved);
}

#[test]
fn stale_revisions_and_inconsistent_lifecycle_are_rejected() {
    let b = budget(0.0, 0.0);
    let env = environment(250.0);
    let active = initial_result(&b, &env, revision(0)).unwrap();
    let candidate = resolved_result(
        &structure::prototype::resolve(&b, &env, Default::default()),
        revision(1),
    )
    .unwrap();
    validate_refinement(&active, &candidate).unwrap();
    for corruption in 0..6 {
        let mut bad = candidate.clone();
        match corruption {
            0 => bad.revision.inputs.push_str(":changed"),
            1 => bad.revision.model.push_str(":changed"),
            2 => bad.revision.sequence = 0,
            3 => bad.revision.sequence = 2,
            4 => bad.refinement = RefinementStatus::Running,
            _ => bad.provenance = ResultProvenance::Fallback,
        }
        assert!(
            validate_refinement(&active, &bad).is_err(),
            "corruption {corruption}"
        );
    }
    let mut bad = active.clone();
    bad.refinement = RefinementStatus::Complete;
    assert!(validate_result(&bad).is_err());
    bad.refinement = RefinementStatus::Failed { reason: " ".into() };
    assert!(validate_result(&bad).is_err());
    bad.refinement = RefinementStatus::Refused {
        reason: "no invented refusal".into(),
    };
    assert!(validate_result(&bad).is_err());
    bad = active.clone();
    bad.revision.inputs.clear();
    assert!(validate_result(&bad).is_err());
}

#[test]
fn transport_validation_accepts_nonlegacy_radius_and_small_inertia_without_solving() {
    let mut result = initial_result(&budget(0.0, 0.0), &environment(250.0), revision(0)).unwrap();
    // A coherent differently-sized prescription need not reproduce a Seager or
    // legacy solve. Scale geometry, update its algebraic density/gravity only.
    let record = &mut result.record;
    record.physical.reference_radius_m *= 1.2;
    record.physical.condensed_radius_m *= 1.2;
    record.physical.mean_density_kg_m3 /= 1.2_f64.powi(3);
    record.physical.reference_gravity_m_s2 /= 1.2_f64.powi(2);
    record.physical.inertia_factor = 0.015;
    for layer in &mut record.layers {
        layer.inner_radius_m *= 1.2;
        layer.outer_radius_m *= 1.2;
    }
    round_trip(&result);
}

#[test]
fn corrupt_transport_is_rejected_species_locally_and_at_layer_boundaries() {
    let result = initial_result(&budget(0.3, 0.0), &environment(250.0), revision(0)).unwrap();
    for corruption in 0..19 {
        let mut bad = result.clone();
        let r = &mut bad.record;
        match corruption {
            0 => r.physical.materials[0].retained_kg *= 0.5,
            1 => r.physical.materials[0].source_kg = -1.0,
            2 => r.physical.materials[0].material = "unknown".into(),
            3 => {
                r.physical.materials.pop();
            }
            4 => r.physical.materials[0].material = r.physical.materials[1].material.clone(),
            5 => r.physical.mean_density_kg_m3 = f64::NAN,
            6 => r.physical.reference_gravity_m_s2 *= 2.0,
            7 => r.physical.condensed_radius_m *= 0.9,
            8 => r.physical.inertia_factor = f64::INFINITY,
            9 => {
                r.physical
                    .materials
                    .iter_mut()
                    .find(|m| m.material == "oxygen")
                    .unwrap()
                    .escaped_kg = 1e-100
            }
            10 => r.layers[0].mass_kg *= 2.0,
            11 => r.layers[0].inner_radius_m *= 0.9,
            12 => r.layers[0].temperature_k = Some(f64::NAN),
            13 => r.layers[0].inner_pressure_pa = -1.0,
            14 => {
                r.layers[0].phase_allocations = vec![space_shared::PhaseAllocation {
                    material: "water".into(),
                    vapour_kg: r.layers[0].mass_kg,
                    condensate_kg: r.layers[0].mass_kg,
                }]
            }
            15 => r.blowoff_kg = 1.0,
            16 => r.surface_pressure_pa = 1e4,
            17 => r.physical.radius_reference = RadiusReference::OneBar,
            _ => r.model = "invented".into(),
        }
        assert!(validate_result(&bad).is_err(), "corruption {corruption}");
    }
}

#[test]
fn one_bar_is_a_gas_reference_not_solid_ground() {
    let result = initial_result(&budget(0.0, 0.1), &environment(250.0), revision(0)).unwrap();
    assert_eq!(result.surface_knowledge, SurfaceKnowledge::GasReference);
    for corruption in 0..4 {
        let mut bad = result.clone();
        match corruption {
            0 => bad.record.one_bar_radius_m = None,
            1 => bad.record.surface_pressure_pa = 0.9e5,
            2 => bad.record.one_bar_radius_m = Some(bad.record.physical.reference_radius_m * 1.01),
            _ => bad.surface_knowledge = SurfaceKnowledge::SolidPhase,
        }
        assert!(validate_result(&bad).is_err());
    }
}

#[test]
fn frost_boundary_is_separate_from_condensed_interior_and_has_unresolved_phase() {
    let retained = budget(0.0, 0.0)
        .retained()
        .checked_add(&Inventory::new(&[(Material::CarbonDioxide, 1e-6 * EARTH_MASS_KG)]).unwrap())
        .unwrap();
    let b = Budget::new(
        retained,
        retained,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    let solved = structure::prototype::solve(&b, &environment(50.0), Default::default()).unwrap();
    let candidate = resolved_result(
        &structure::prototype::Outcome::Layered(Box::new(solved)),
        revision(1),
    )
    .unwrap();
    assert_eq!(candidate.record.surface, "frost");
    assert_eq!(candidate.surface_knowledge, SurfaceKnowledge::Unresolved);
    assert!(
        candidate.record.physical.reference_radius_m > candidate.record.physical.condensed_radius_m
    );
    round_trip(&candidate);
}

#[test]
fn per_species_escape_and_blowoff_survive_refinement_without_replaying_history() {
    let retained = budget(0.0, 0.0)
        .retained()
        .checked_add(&Inventory::new(&[(Material::Nitrogen, 1e-6 * EARTH_MASS_KG)]).unwrap())
        .unwrap()
        .scaled(0.0002)
        .unwrap();
    let source = retained
        .checked_add(&Inventory::new(&[(Material::Hydrogen, 1e-7 * EARTH_MASS_KG)]).unwrap())
        .unwrap();
    let b = Budget::new(source, retained, Inventory::default(), Inventory::default()).unwrap();
    let env = environment(1000.0);
    let active = initial_result(&b, &env, revision(0)).unwrap();
    let outcome = structure::prototype::resolve(&b, &env, Default::default());
    let candidate = resolved_result(&outcome, revision(1)).unwrap();
    assert!(candidate.record.blowoff_kg > 0.0);
    validate_refinement(&active, &candidate).unwrap();
    round_trip(&candidate);
    assert!(validate_refinement(&candidate, &candidate).is_err());
    let mut unrelated = active.clone();
    unrelated.revision.sequence = 1;
    unrelated.refinement = RefinementStatus::Refused {
        reason: "test refusal".into(),
    };
    unrelated.record.fallback_reason = Some("test refusal".into());
    // Internally consistent, but not the same immutable formation supply.
    unrelated
        .record
        .physical
        .materials
        .iter_mut()
        .find(|m| m.material == "hydrogen")
        .unwrap()
        .source_kg *= 2.0;
    unrelated
        .record
        .physical
        .materials
        .iter_mut()
        .find(|m| m.material == "hydrogen")
        .unwrap()
        .escaped_kg *= 2.0;
    validate_result(&unrelated).unwrap();
    assert!(validate_refinement(&active, &unrelated).is_err());
}

#[test]
fn exact_one_bar_at_gas_base_is_valid_and_sub_bar_cannot_claim_that_level() {
    let mut result = initial_result(&budget(0.0, 0.1), &environment(250.0), revision(0)).unwrap();
    let r = &mut result.record;
    // Transport fixture for the event at the very base of an envelope. No EOS
    // inference: retain positive contiguous regions and algebraic bulk facts.
    let radius = r.physical.condensed_radius_m;
    r.physical.reference_radius_m = radius;
    r.physical.reference_gravity_m_s2 =
        GRAVITATIONAL_CONSTANT * r.physical.retained_mass_kg / radius.powi(2);
    r.physical.mean_density_kg_m3 = r.physical.retained_mass_kg / (4.0 / 3.0 * PI * radius.powi(3));
    r.one_bar_radius_m = Some(radius);
    r.layers[0].inner_pressure_pa = 1e5;
    r.layers[1].outer_pressure_pa = Some(1e5);
    round_trip(&result);
    let mut sub_bar = result.clone();
    sub_bar.record.surface = "rock".into();
    sub_bar.record.surface_pressure_pa = 0.5e5;
    sub_bar.record.physical.radius_reference = RadiusReference::SolidSurface;
    sub_bar.surface_knowledge = SurfaceKnowledge::Unresolved;
    sub_bar.record.layers[0].inner_pressure_pa = 0.5e5;
    sub_bar.record.layers[1].outer_pressure_pa = Some(0.5e5);
    sub_bar.record.one_bar_radius_m = None;
    round_trip(&sub_bar);
    sub_bar.record.one_bar_radius_m = Some(radius);
    assert!(validate_result(&sub_bar).is_err());
}
