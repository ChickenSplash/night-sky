use super::*;
use space_shared::PlanetEnvironment;
use space_simulation::system::atmosphere::{Composition, Gas, ReferenceLevel};

#[test]
fn descent_rebases_the_column_without_moving_its_top_or_changing_upper_gas() {
    for preset in [Preset::HydrogenGiant, Preset::Earth, Preset::Venus] {
        let original = AirSettings::preset(preset).atmosphere().scaled(7e7);
        let deep = descending(original);
        let scale = f64::from(deep.radius_scale);
        assert!((0.8..1.0).contains(&deep.radius_scale));
        assert!((deep.top * deep.radius_scale - original.top).abs() < 2e-7);
        assert_eq!(deep.ground, Vec3::ZERO);
        for altitude in [0.0, 0.5 * f64::from(original.top - 1.0)] {
            let rebased_height = (1.0 + altitude) / scale - 1.0;
            let old = super::super::tables::extinction(&original, altitude);
            let new = super::super::tables::extinction(&deep, rebased_height) / scale;
            assert!((new - old).length() / old.length() < 0.002, "{preset:?}");
        }
    }
}

#[test]
fn descent_remains_finite_for_extended_columns_and_short_haze_profiles() {
    for radius in [1e3, 7e7, 1e10] {
        let air = AirSettings::preset(Preset::Earth)
            .atmosphere()
            .scaled(radius);
        let deep = descending(air);
        assert!(deep.radius_scale >= 0.8);
        assert!(deep.top.is_finite());
        assert!(deep.rayleigh.is_finite());
        assert!(deep.mie.is_finite());
        assert!(deep.shape.is_finite());
        assert!(deep.gas_absorption.is_finite());
    }
    let mut extreme = AirSettings::preset(Preset::HydrogenGiant)
        .atmosphere()
        .scaled(7e7);
    extreme.rayleigh.x = f32::MAX;
    assert_eq!(descending(extreme), extreme);
}

fn planet(gas: Gas) -> Planet {
    let physical = GasAtmosphere::new(
        Composition::new(&[(gas, 1.0)]).unwrap(),
        ReferenceLevel::Surface,
        1e5,
        280.0,
        None,
    )
    .unwrap();
    Planet {
        mass_earth: 1.0,
        radius_km: 6371.0,
        environment: Some(PlanetEnvironment {
            irradiation_w_m2: 1361.0,
            assumed_bond_albedo: 0.3,
            equilibrium_temperature_k: 255.0,
            age_gyr: 4.5,
            atmosphere: Some(physical.to_data(EARTH_SOLAR * SUN_GM, 6.371e6).unwrap()),
        }),
        ..default()
    }
}

#[test]
fn generated_air_is_independent_of_selection_and_override_is_opt_in() {
    let nitrogen = planet(Gas::Nitrogen);
    let methane = planet(Gas::Methane);
    let r = 6.371e6;
    let n = for_planet(&nitrogen, false, None, r).unwrap();
    let m = for_planet(&methane, false, None, r).unwrap();
    assert_ne!(n, m);
    assert_eq!(for_planet(&nitrogen, true, None, r), Some(n));
    let override_air = AirSettings::preset(Preset::Venus).atmosphere();
    assert_eq!(for_planet(&nitrogen, false, Some(override_air), r), Some(n));
    assert_eq!(
        for_planet(&nitrogen, true, Some(override_air), r),
        Some(override_air.scaled(r))
    );
    let mut airless = nitrogen.clone();
    airless.environment.as_mut().unwrap().atmosphere = None;
    assert_eq!(for_planet(&airless, false, None, r), None);
    assert!(for_planet(&airless, true, Some(override_air), r).is_some());
    assert_eq!(for_planet(&Planet::default(), false, None, r), None);
}

#[test]
fn wire_derived_values_are_recomputed_and_invalid_inputs_do_not_reach_gpu() {
    let mut world = planet(Gas::Nitrogen);
    let expected = atmosphere(&world);
    let data = world
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap();
    data.scale_height_m = f64::NAN;
    data.extent_m = -1.0;
    assert_eq!(atmosphere(&world), expected);
    world
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = -1.0;
    assert!(atmosphere(&world).is_none());
    assert!(for_planet(&planet(Gas::Nitrogen), false, None, f64::INFINITY).is_none());
}

fn generated(system: &str, id: &str) -> Planet {
    space_simulation::system::generated_system(system, [0.0; 3])
        .unwrap()
        .planets
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

#[test]
fn generated_optics_come_from_the_result_and_unchecked_records_draw_no_air() {
    use space_simulation::system::atmosphere::optics::boundary::LowerBoundaryKind;
    let world = generated("g13b6ffffcfeba.2.339", "g13b6ffffcfeba.2.339/7");
    let facts = &world.initial_result.as_ref().unwrap().record.physical;
    let column = optical_column(&world).unwrap();
    assert_eq!(column.reference.radius_m, facts.reference_radius_m);
    let gravity = column.reference.gravity_m_s2 / facts.reference_gravity_m_s2;
    assert!((gravity - 1.0).abs() < 1e-12);
    assert_eq!(
        column.lower_boundary.kind,
        LowerBoundaryKind::CondensedReferenceProxy
    );
    assert_eq!(
        column.outer_cutoff.height_m,
        atmosphere(&world).unwrap().top
    );
    assert!(!envelope(&world));

    // Mixed projections must not render a shell at a different geometry.
    let mut moved = world.clone();
    moved.radius_km *= 2.0;
    moved.mass_earth *= 3.0;
    assert!(optical_column(&moved).is_none());

    // Air that disagrees with its result, a corrupt result, or a physical
    // record without its result is never drawn.
    let mut mismatched = world.clone();
    let air = mismatched.environment.as_mut().unwrap();
    air.atmosphere.as_mut().unwrap().pressure_pa *= 2.0;
    assert!(atmosphere(&mismatched).is_none());
    let mut corrupt = world.clone();
    let result = corrupt.initial_result.as_mut().unwrap();
    result.record.physical.reference_gravity_m_s2 = f64::NAN;
    assert!(atmosphere(&corrupt).is_none());
    let mut unchecked = world.clone();
    unchecked.initial_result = None;
    assert!(atmosphere(&unchecked).is_none());
    assert!(!envelope(&unchecked) && !deep_envelope(&unchecked));
}

#[test]
fn envelope_follows_the_results_surface_knowledge() {
    let envelope_world = (0..40)
        .filter_map(|i| {
            space_simulation::system::generated_system(&format!("g13b6ffffcfeba.2.{i}"), [0.0; 3])
        })
        .flat_map(|system| system.planets)
        .find(|p| {
            p.initial_result.as_ref().unwrap().surface_knowledge
                == space_shared::SurfaceKnowledge::GasReference
        })
        .unwrap();
    assert!(envelope(&envelope_world));
    let column = optical_column(&envelope_world).unwrap();
    assert_eq!(
        column.lower_boundary.kind,
        space_simulation::system::atmosphere::optics::boundary::LowerBoundaryKind::GasReferenceProxy
    );
}

#[test]
fn dense_boundary_pressure_changes_optics_and_preserves_the_upper_column() {
    let thin = planet(Gas::Nitrogen);
    let mut dense = thin.clone();
    dense
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = 700e5;
    let base = atmosphere(&thin).unwrap();
    let thick = atmosphere(&dense).unwrap();
    assert!(base.spherical && thick.spherical);
    let drop = 12.5 + 700.0_f64.ln();
    let expected = drop * base.rayleigh_height / (1.0 - drop * base.rayleigh_height / 6.371e6);
    assert!((thick.top - expected).abs() < 1e-8);
    assert_eq!(thick.rayleigh_height, base.rayleigh_height);
    assert!((thick.rayleigh / base.rayleigh - DVec3::splat(700.0)).length() < 1e-9);
    assert!(!envelope(&dense));
    assert!(for_planet(&dense, false, None, 6.371e6).is_some());
}

#[test]
fn motivating_generated_boundary_reaches_renderer_and_is_not_an_envelope_proxy() {
    let system =
        space_simulation::system::generated_system("g13b6ffffcfeba.2.339", [0.0; 3]).unwrap();
    let world = system
        .planets
        .iter()
        .find(|p| p.id.ends_with("/7"))
        .unwrap();
    let data = world
        .environment
        .as_ref()
        .unwrap()
        .atmosphere
        .as_ref()
        .unwrap();
    assert!(!envelope(world));
    assert!(data.pressure_pa > 100e5);
    let generated = atmosphere(world).unwrap();
    assert!((generated.top - data.extent_m).abs() < 1e-8);
    assert!(for_planet(world, false, None, world.radius_km * 1e3).is_some());
    // A one-bar column at the same result reference, for comparison only:
    // optics never draw live air that disagrees with the result.
    let mut reference = world.clone();
    reference
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = 1e5;
    assert!(atmosphere(&reference).is_none());
    let column = optical_column(world).unwrap();
    let gas = GasAtmosphere::try_from(
        reference
            .environment
            .as_ref()
            .unwrap()
            .atmosphere
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    let thin = OpticalColumn::new(
        &gas,
        column.reference.gravity_m_s2 * column.reference.radius_m.powi(2),
        column.reference.radius_m,
    )
    .unwrap();
    let one_bar = Atmosphere {
        top: thin.outer_cutoff.height_m,
        rayleigh: DVec3::from_array(thin.optics.rayleigh_m_inv),
        ..generated
    };
    assert!(generated.top > one_bar.top);
    assert!(
        (generated.rayleigh / one_bar.rayleigh - DVec3::splat(data.pressure_pa / 1e5)).length()
            < 1e-8
    );
}

#[test]
fn debug_optical_changes_enable_override_but_quality_changes_do_not() {
    let mut tuning = AtmosphereTuning::default();
    assert!(!tuning.enabled);
    tuning.apply(&AtmosphereChange {
        view_steps: Some(32),
        ..default()
    });
    assert!(!tuning.enabled);
    tuning.apply(&AtmosphereChange {
        preset: Some(Preset::Mars),
        ..default()
    });
    assert!(tuning.enabled);
    let change = AtmosphereChange {
        enabled: Some(false),
        ..default()
    };
    assert!(change.check().is_ok());
    tuning.apply(&change);
    assert!(!tuning.enabled);
}

#[test]
fn one_bar_reference_marks_an_envelope_boundary_not_solid_terrain() {
    let mut world = planet(Gas::Hydrogen);
    assert!(!envelope(&world));
    let data = world
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap();
    data.reference_level = AtmosphereReference::OneBar;
    assert!(envelope(&world));
    assert!(!deep_envelope(&world));
    assert!(atmosphere(&world).is_some());
}

#[test]
fn rocky_boundary_and_bulk_envelope_descent_are_distinct_and_label_independent() {
    let system =
        space_simulation::system::generated_system("g13b6ffffcfeba.2.59", [0.0; 3]).unwrap();
    let world = system
        .planets
        .iter()
        .find(|p| p.id == "g13b6ffffcfeba.2.59/7")
        .unwrap();
    assert!(!envelope(world));
    assert!(!deep_envelope(world));
    let air = atmosphere(world).unwrap();
    let data = world
        .environment
        .as_ref()
        .unwrap()
        .atmosphere
        .as_ref()
        .unwrap();
    assert!(data.pressure_pa > 1e5);
    assert!(data.extended);
    assert_eq!(air.scaled(world.radius_km * 1e3).radius_scale, 1.0);

    // Live labels alone do not make an envelope; the result decides.
    let mut relabelled = world.clone();
    relabelled
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .reference_level = AtmosphereReference::OneBar;
    relabelled.physical.as_mut().unwrap().radius_reference = space_shared::RadiusReference::OneBar;
    assert!(!envelope(&relabelled));

    // Changing a coordinate without its physical record does not manufacture depth.
    let mut unresolved = world.clone();
    let result = unresolved.initial_result.as_mut().unwrap();
    result.record.physical.radius_reference = space_shared::RadiusReference::OneBar;
    result.surface_knowledge = space_shared::SurfaceKnowledge::GasReference;
    assert!(!envelope(&unresolved));
    assert!(!deep_envelope(&unresolved));

    let giants =
        space_simulation::system::generated_system("g13b6ffffdda21.2.0", [0.0; 3]).unwrap();
    let mut giant = giants
        .planets
        .iter()
        .find(|p| p.id.ends_with("/6"))
        .unwrap()
        .clone();
    assert!(deep_envelope(&giant));
    giant.class = "Rocky planet".into();
    assert!(deep_envelope(&giant));
}

#[test]
fn spherical_rebasing_preserves_gas_extinction_above_the_reference() {
    let air = for_planet(&planet(Gas::Nitrogen), false, None, 6.371e6).unwrap();
    assert!(air.spherical);
    let deep = descending(air);
    for height in [0.0, 0.005, 0.01] {
        let rebased_height = (1.0 + height) / f64::from(deep.radius_scale) - 1.0;
        let original =
            air.rayleigh.truncate().as_dvec3() * super::super::tables::gas_density(&air, height);
        let rebased = deep.rayleigh.truncate().as_dvec3()
            * super::super::tables::gas_density(&deep, rebased_height)
            / f64::from(deep.radius_scale);
        assert!((rebased / original - DVec3::ONE).abs().max_element() < 1e-4);
    }
}

#[test]
fn continuation_stops_at_the_condensed_top_not_an_abundance_threshold() {
    let air = for_planet(&planet(Gas::Nitrogen), false, None, 6.371e6).unwrap();
    let ratio = 0.9999;
    let (surface, reached) = rebase(air, ratio);
    assert!(reached);
    assert_eq!(surface.radius_scale, ratio);
    assert!(surface.radius_scale < 1.0);
    assert!(!surface.opaque_proxy);
    let (proxy, reached) = rebase(air, 0.1);
    assert!(!reached);
    assert!(proxy.opaque_proxy);
    assert!(proxy.radius_scale > 0.1);
    // Rebasing only changes coordinates, not the optical radius above them.
    assert!((surface.top * surface.radius_scale - air.top).abs() < 1e-6);
}

#[test]
fn thin_envelopes_can_show_the_condensed_top_through_their_air() {
    use space_simulation::system::surface::geometry;
    let world = (0..60)
        .filter_map(|i| {
            space_simulation::system::generated_system(&format!("g13b6ffffcfeba.2.{i}"), [0.0; 3])
        })
        .flat_map(|s| s.planets)
        .find(|p| {
            geometry(p).is_ok_and(|g| {
                g.has_gas_depth() && g.condensed_radius_m / g.reference_radius_m > 0.8
            }) && p.physical.as_ref().unwrap().h_he_mass_fraction < 0.02
        })
        .unwrap();
    assert!(deep_envelope(&world));
    let air = for_planet(&world, false, None, world.radius_km * 1000.0).unwrap();
    // A wide but explicitly hand-set optical profile reaches this shallow top.
    let air = Air {
        rayleigh: air.rayleigh.truncate().extend(0.1),
        ..air
    };
    let (lower, ground) = lower_boundary(&world, Some(air), true);
    assert!(ground);
    let g = geometry(&world).unwrap();
    assert!(
        (f64::from(lower.unwrap().radius_scale) - g.condensed_radius_m / g.reference_radius_m)
            .abs()
            < 1e-7
    );
    assert!(!lower_boundary(&world, Some(air), false).1);
    assert!(!lower_boundary(&world, None, true).1);
    let mut relabelled = world.clone();
    relabelled.class = "Completely different label".into();
    assert_eq!(
        lower_boundary(&world, Some(air), true),
        lower_boundary(&relabelled, Some(air), true)
    );
}

#[test]
fn population_columns_all_convert_without_clamping_or_missing_generated_air() {
    let mut present = 0;
    let mut extended = 0;
    for i in 0..120 {
        let bodies = space_simulation::bodies(&format!("atmosphere-population:{i}"));
        for hosted in bodies.planets {
            let planet = Planet {
                mass_earth: hosted.body.mass_earth,
                radius_km: hosted.body.radius_km,
                environment: hosted.environment,
                ..default()
            };
            let data = planet.environment.as_ref().unwrap().atmosphere.as_ref();
            let converted = for_planet(&planet, false, None, planet.radius_km * 1e3);
            assert_eq!(converted.is_some(), data.is_some());
            if let Some(data) = data {
                present += 1;
                extended += usize::from(data.extended);
                assert!((atmosphere(&planet).unwrap().top / data.extent_m - 1.0).abs() < 1e-12);
            }
        }
    }
    assert!(present > 0 && extended > 0);
}
