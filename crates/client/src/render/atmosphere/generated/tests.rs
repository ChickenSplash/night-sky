use super::*;
use space_shared::PlanetEnvironment;
use space_simulation::system::atmosphere::{Composition, Gas, ReferenceLevel};

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
    let extra_height = base.rayleigh_height * 700.0_f64.ln();
    assert!((thick.top - base.top - extra_height).abs() < 1e-8);
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
    assert!(atmosphere(&world).is_some());
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
