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
        space_simulation::system::generated_system("g13b6ffffcfeba.2.1", [0.0; 3]).unwrap();
    let world = system
        .planets
        .iter()
        .find(|p| p.id.ends_with("/1"))
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
    let mut reference = world.clone();
    reference
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .pressure_pa = 1e5;
    let one_bar = atmosphere(&reference).unwrap();
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
    let system = space_simulation::system::generated_system("g1e0024924b.2.231", [0.0; 3]).unwrap();
    let world = system
        .planets
        .iter()
        .find(|p| p.id == "g1e0024924b.2.231#B/4")
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

    // A different pressure-reference label must not turn trace gas into a giant.
    let mut unresolved = world.clone();
    unresolved
        .environment
        .as_mut()
        .unwrap()
        .atmosphere
        .as_mut()
        .unwrap()
        .reference_level = AtmosphereReference::OneBar;
    unresolved.physical.as_mut().unwrap().radius_reference = space_shared::RadiusReference::OneBar;
    assert!(envelope(&unresolved));
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
