use super::*;
use crate::system::{
    Body, Class,
    atmosphere::ColumnValidity,
    composition::{Budget, Inventory, Material, Reservoir},
};
use space_shared::{Barycentre, Orbit, Spin};

fn orbit_at(au: f64) -> Orbit {
    Orbit {
        semi_major_au: au,
        ..Default::default()
    }
}

fn star(id: &str, mass: f64, luminosity: f64) -> Star {
    Star {
        id: id.into(),
        page: 0,
        spectral: "G2 V".into(),
        kind: "Yellow dwarf".into(),
        mass_solar: mass,
        radius_solar: 1.0,
        luminosity_solar: luminosity,
        temperature_k: 5772.0,
        colour: [255; 3],
        spin: Spin::default(),
        component: String::new(),
    }
}

fn system(stars: Vec<Star>, pairs: Vec<Barycentre>) -> StarSystem {
    StarSystem {
        id: "test".into(),
        name: "Test".into(),
        position_ly: [0.0; 3],
        stars,
        barycentres: pairs,
        planets: Vec::new(),
        page_stars: Vec::new(),
    }
}

fn body() -> Body {
    use crate::system::{
        RetentionSolve,
        composition::{Budget, Inventory, Material},
        structure,
    };
    let inventory = Inventory::from_mass_fractions(
        crate::units::EARTH_MASS_KG,
        &[(Material::Metal, 0.325), (Material::Silicate, 0.675)],
    )
    .unwrap();
    let budget = Budget::new(
        inventory,
        inventory,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    Body {
        orbit: orbit_at(1.0),
        class: Class::Rocky,
        mass_earth: 1.0,
        radius_km: 6371.0,
        resonance: None,
        spin: Spin::default(),
        budget,
        structure: structure::derive(&budget, 1361.0, 5.0).unwrap(),
        formation_mass_earth: 1.0,
        formation_distance_au: 1.0,
        retention_solve: RetentionSolve::NoEnvelope,
    }
}

fn close(got: f64, want: f64, relative: f64) {
    assert!((got / want - 1.0).abs() < relative, "{got} vs {want}");
}

#[test]
fn isolated_sun_at_earth_distance_and_inverse_square_heating() {
    let system = system(vec![star("sun", 1.0, 1.0)], vec![]);
    let samples = stellar_samples(&system);
    let earth = heating(&system, &samples, "sun", orbit_at(1.0), 4.5);
    let far = heating(&system, &samples, "sun", orbit_at(2.0), 4.5);
    close(earth.flux, 1361.0, 0.001);
    close(far.flux, earth.flux / 4.0, 1e-12);
    close(earth.loss_flux, earth.flux, 0.01);
    let temperature = (earth.flux * 0.7 / (4.0 * STEFAN_BOLTZMANN)).powf(0.25);
    close(temperature, 254.6, 0.001);
}

#[test]
fn eccentric_orbit_average_uses_mean_anomaly_not_uniform_true_anomaly() {
    let system = system(vec![star("sun", 1.0, 1.0)], vec![]);
    let mut orbit = orbit_at(1.0);
    orbit.eccentricity = 0.3;
    let got = heating(&system, &stellar_samples(&system), "sun", orbit, 4.5).flux;
    let exact = light::flux(1.0, crate::AU_METRES) / (1.0 - 0.3f64.powi(2)).sqrt();
    close(got, exact, 1e-6);
}

#[test]
fn all_stars_contribute_for_both_stellar_and_circumbinary_hosts() {
    let mut system = system(
        vec![star("A", 1.0, 1.0), star("B", 1.0, 2.0)],
        vec![Barycentre {
            id: "AB".into(),
            page: 0,
            component: "AB".into(),
            members: ["A".into(), "B".into()],
            orbit: orbit_at(10.0),
        }],
    );
    let samples = stellar_samples(&system);
    let both = heating(&system, &samples, "A", orbit_at(1.0), 4.5);
    system.stars[1].luminosity_solar = 0.0;
    let samples = stellar_samples(&system);
    let one = heating(&system, &samples, "A", orbit_at(1.0), 4.5);
    assert!(both.flux > one.flux * 1.01);
    assert!(both.loss_flux > one.loss_flux);
    system.barycentres[0].orbit = orbit_at(0.05);
    system.stars[1].luminosity_solar = 2.0;
    let samples = stellar_samples(&system);
    let circumbinary = heating(&system, &samples, "AB", orbit_at(1.0), 4.5);
    close(
        circumbinary.flux,
        3.0 * light::flux(1.0, crate::AU_METRES),
        0.03,
    );
}

#[test]
fn nested_companions_contribute_and_initial_phases_do_not_set_generated_climate() {
    let mut system = system(
        vec![
            star("A", 1.0, 1.0),
            star("B", 0.8, 0.5),
            star("C", 0.6, 0.2),
        ],
        vec![
            Barycentre {
                id: "AB".into(),
                page: 0,
                component: "AB".into(),
                members: ["A".into(), "B".into()],
                orbit: orbit_at(0.1),
            },
            Barycentre {
                id: "ABC".into(),
                page: 0,
                component: "ABC".into(),
                members: ["AB".into(), "C".into()],
                orbit: orbit_at(10.0),
            },
        ],
    );
    let first = heating(&system, &stellar_samples(&system), "AB", orbit_at(1.0), 4.5);
    system.barycentres[0].orbit.phase_rad = 2.0;
    system.barycentres[1].orbit.phase_rad = 4.0;
    let mut orbit = orbit_at(1.0);
    orbit.phase_rad = 3.0;
    let changed = heating(&system, &stellar_samples(&system), "AB", orbit, 4.5);
    assert_eq!(first.flux, changed.flux);
    system.stars[2].luminosity_solar = 0.0;
    let no_third = heating(&system, &stellar_samples(&system), "AB", orbit, 4.5);
    assert!(first.flux > no_third.flux);
}

#[test]
fn retention_is_smooth_and_responds_to_escape_speed_heating_and_activity() {
    let base = secondary_retention(11_200.0, 1361.0);
    assert!(secondary_retention(5000.0, 1361.0) < base);
    assert!(secondary_retention(11_200.0, 13610.0) < base);
    assert!(secondary_retention(15_000.0, 1361.0) > base);
    let sun = star("sun", 1.0, 1.0);
    let dwarf = star("dwarf", 0.2, 0.01);
    assert!(activity_history(&dwarf, 4.5) > activity_history(&sun, 4.5));
    assert!(activity_history(&sun, 4.5) > activity_history(&sun, 0.1));
    for speed in [100.0, 1000.0, 10_000.0, 100_000.0] {
        for flux in [0.0, 1e-3, 1361.0, 1e9] {
            let got = secondary_retention(speed, flux);
            assert!(got.is_finite() && (0.0..=1.0).contains(&got));
        }
    }
}

#[test]
fn cold_condensation_reduces_partial_pressures_and_can_leave_airless_worlds() {
    let mut warm_body = volatile_body();
    let mut cold_body = warm_body.clone();
    let mut frozen_body = warm_body.clone();
    let warm = partition::atmosphere(&mut warm_body, 280.0)
        .unwrap()
        .unwrap();
    let cold = partition::atmosphere(&mut cold_body, 90.0)
        .unwrap()
        .unwrap();
    assert!(cold.pressure_pa() < warm.pressure_pa());
    assert!(
        cold.composition().fraction(Gas::CarbonDioxide)
            < warm.composition().fraction(Gas::CarbonDioxide)
    );
    let frozen = partition::atmosphere(&mut frozen_body, 20.0).unwrap();
    assert!(frozen.is_none());
    assert!(
        cold_body
            .budget
            .reservoir(Reservoir::CondensedVolatiles)
            .mass_kg(Material::CarbonDioxide)
            > warm_body
                .budget
                .reservoir(Reservoir::CondensedVolatiles)
                .mass_kg(Material::CarbonDioxide)
    );
    for body in [&warm_body, &cold_body, &frozen_body] {
        assert_conserved(body);
    }
    for gas in [Gas::CarbonDioxide, Gas::Nitrogen, Gas::Methane, Gas::Argon] {
        assert!(vapour_pressure(gas, 40.0) < vapour_pressure(gas, 80.0));
        assert!(vapour_pressure(gas, 80.0).is_finite());
        assert!(vapour_pressure(gas, 1000.0).is_infinite());
    }
}

#[test]
fn labels_cannot_create_gas_or_change_atmospheric_supply() {
    let mut dry = body();
    dry.class = Class::GasGiant;
    assert!(partition::atmosphere(&mut dry, 280.0).unwrap().is_none());
    let mut first = volatile_body();
    let mut relabelled = first.clone();
    relabelled.class = Class::GasGiant;
    let air = partition::atmosphere(&mut first, 280.0).unwrap();
    assert_eq!(air, partition::atmosphere(&mut relabelled, 280.0).unwrap());
    assert_eq!(first.budget, relabelled.budget);
}

fn volatile_body() -> Body {
    let mut body = body();
    let interior = *body.budget.retained();
    let volatiles = Inventory::new(&[
        (Material::Nitrogen, 1e19),
        (Material::CarbonDioxide, 1e19),
        (Material::Water, 1e21),
        (Material::Methane, 1e17),
        (Material::Argon, 1e17),
    ])
    .unwrap();
    body.budget = Budget::new(
        interior.checked_add(&volatiles).unwrap(),
        interior,
        volatiles,
        Inventory::default(),
    )
    .unwrap();
    body.structure = crate::system::structure::derive(&body.budget, 1361.0, 4.5).unwrap();
    body.mass_earth = body.structure.mass_kg() / crate::units::EARTH_MASS_KG;
    body.radius_km = body.structure.radius_m() / 1000.0;
    body
}

fn assert_conserved(body: &Body) {
    let budget = &body.budget;
    assert_eq!(body.structure.mass_kg(), budget.retained().total_mass_kg());
    for material in Material::ALL {
        let retained = budget.retained().mass_kg(material);
        let allocated: f64 = [
            Reservoir::Interior,
            Reservoir::CondensedVolatiles,
            Reservoir::Gas,
        ]
        .iter()
        .map(|&r| budget.reservoir(r).mass_kg(material))
        .sum();
        assert!((retained - allocated).abs() <= retained * 1e-14);
        let source = budget.source().mass_kg(material);
        assert!((source - retained - budget.escaped().mass_kg(material)).abs() <= source * 1e-14);
    }
}

#[test]
fn boundary_columns_use_mass_to_mole_conversion_and_obey_cold_caps() {
    for temperature in [20.0, 40.0, 90.0, 150.0, 280.0, 1000.0] {
        let mut body = volatile_body();
        let air = partition::atmosphere(&mut body, temperature).unwrap();
        assert_conserved(&body);
        if let Some(air) = air {
            let gas = body.budget.reservoir(Reservoir::Gas);
            assert_eq!(air.composition(), &gas.gas_composition().unwrap().unwrap());
            let weight = body.structure.reference_gravity_m_s2()
                / (4.0 * std::f64::consts::PI * body.structure.radius_m().powi(2));
            let column = air
                .column(
                    crate::units::GRAVITATIONAL_CONSTANT * body.structure.mass_kg(),
                    body.structure.radius_m(),
                )
                .unwrap();
            close(
                air.pressure_pa() * column.profile.mass_factor(air.pressure_pa()),
                gas.total_mass_kg() * weight,
                1e-12,
            );
            for species in Gas::ALL {
                assert!(
                    air.pressure_pa() * air.composition().fraction(species)
                        <= vapour_pressure(species, temperature) * (1.0 + 1e-12)
                );
            }
        }
    }
}

#[test]
fn sub_bar_light_gas_and_giant_columns_are_bounded_by_real_supply() {
    for light_mass in [1e12, 1e22, 1e27] {
        let mut body = volatile_body();
        let interior = *body.budget.reservoir(Reservoir::Interior);
        let condensed = *body.budget.reservoir(Reservoir::CondensedVolatiles);
        let light = Inventory::new(&[
            (Material::Hydrogen, 0.75 * light_mass),
            (Material::Helium, 0.25 * light_mass),
        ])
        .unwrap();
        let source = interior
            .checked_add(&condensed)
            .unwrap()
            .checked_add(&light)
            .unwrap();
        body.budget = Budget::new(source, interior, condensed, light).unwrap();
        body.structure = crate::system::structure::derive(&body.budget, 1361.0, 4.5).unwrap();
        let air = partition::atmosphere(&mut body, 90.0).unwrap().unwrap();
        assert_conserved(&body);
        assert_eq!(
            body.budget
                .reservoir(Reservoir::Gas)
                .mass_kg(Material::Hydrogen),
            0.75 * light_mass
        );
        let weight = body.structure.reference_gravity_m_s2()
            / (4.0 * std::f64::consts::PI * body.structure.radius_m().powi(2));
        let supply_pressure = body.budget.reservoir(Reservoir::Gas).total_mass_kg() * weight;
        assert!(air.pressure_pa() <= supply_pressure * (1.0 + 1e-12));
        if light_mass == 1e12 {
            assert_eq!(air.reference_level(), ReferenceLevel::Surface);
            assert!(air.pressure_pa() < ONE_BAR_PA);
        } else {
            assert_eq!(air.reference_level(), ReferenceLevel::OneBar);
            assert_eq!(air.pressure_pa(), ONE_BAR_PA);
        }
        for species in Gas::ALL {
            assert!(
                air.pressure_pa() * air.composition().fraction(species)
                    <= vapour_pressure(species, 90.0) * (1.0 + 1e-12)
            );
        }
    }
}

#[test]
fn secondary_escape_debits_only_accessible_source_and_does_not_compound() {
    use crate::system::generation::retention::{accessible_fraction, finalise};
    let initial = volatile_body();
    let mut body = initial.clone();
    finalise(&mut body, "escape-test", 1.0, 1361.0, 1e12, 4.5);
    assert_conserved(&body);
    assert!(body.budget.escaped().mass_kg(Material::Nitrogen) > 0.0);
    for material in [Material::Water, Material::Nitrogen, Material::CarbonDioxide] {
        let original = initial.budget.source().mass_kg(material);
        assert!(
            body.budget.escaped().mass_kg(material)
                <= original * accessible_fraction(material) * (1.0 + 1e-10)
        );
    }
    assert_eq!(body.budget.escaped().mass_kg(Material::Metal), 0.0);
    let first = body.clone();
    partition::atmosphere(&mut body, 90.0).unwrap();
    finalise(&mut body, "escape-test", 1.0, 1361.0, 1e12, 4.5);
    assert_eq!(body.budget, first.budget);
    assert_eq!(body.structure, first.structure);
    assert_eq!(body.spin, first.spin);
}

#[test]
fn population_is_deterministic_valid_and_varied_without_changing_orbits() {
    let mut absent = 0;
    let mut surface = 0;
    let mut envelope = 0;
    let mut extended = 0;
    for index in 0..120 {
        let key = format!("atmosphere-population:{index}");
        let first = crate::bodies(&key);
        let again = crate::bodies(&key);
        assert_eq!(first.planets.len(), again.planets.len());
        let expected_age = crate::system_age_gyr(&key);
        for (planet, repeated) in first.planets.iter().zip(&again.planets) {
            assert_conserved(&planet.body);
            assert_eq!(planet.id(), repeated.id());
            assert_eq!(planet.body.orbit, repeated.body.orbit);
            assert_eq!(planet.environment, repeated.environment);
            let environment = planet.environment.as_ref().unwrap();
            assert_eq!(environment.age_gyr, expected_age);
            assert!(environment.irradiation_w_m2.is_finite() && environment.irradiation_w_m2 > 0.0);
            assert!(environment.equilibrium_temperature_k.is_finite());
            let Some(data) = &environment.atmosphere else {
                absent += 1;
                continue;
            };
            let air = GasAtmosphere::try_from(data).unwrap();
            let column = air
                .column(
                    planet.body.mass_earth * EARTH_SOLAR * SUN_GM,
                    planet.body.radius_km * 1000.0,
                )
                .unwrap();
            assert_eq!(data.extended, column.validity == ColumnValidity::Extended);
            assert!(air.composition().fraction(Gas::Oxygen) == 0.0);
            assert!(data.pressure_pa >= MIN_PRESSURE_PA);
            if data.extended {
                extended += 1;
            }
            match air.reference_level() {
                ReferenceLevel::Surface => surface += 1,
                ReferenceLevel::OneBar => envelope += 1,
            }
            for species in Gas::ALL {
                let material = Material::from_gas(species);
                let gas_mass = planet
                    .body
                    .budget
                    .reservoir(Reservoir::Gas)
                    .mass_kg(material);
                if air.composition().fraction(species) > 0.0 {
                    assert!(gas_mass > 0.0);
                }
                assert!(
                    air.pressure_pa() * air.composition().fraction(species)
                        <= vapour_pressure(species, data.temperature_k) * (1.0 + 1e-9)
                );
            }
            let encoded = serde_json::to_string(environment).unwrap();
            let decoded: PlanetEnvironment = serde_json::from_str(&encoded).unwrap();
            close(
                decoded.irradiation_w_m2,
                environment.irradiation_w_m2,
                1e-14,
            );
        }
    }
    println!(
        "120 systems: {absent} airless, {surface} secondary, {envelope} envelopes, {extended} extended columns"
    );
    assert!(absent > 0 && surface > 0 && envelope > 0);
}

#[test]
fn changed_loss_history_preserves_orbits_and_source_but_reconciles_structure() {
    let original = crate::bodies("atmosphere-independent-streams");
    let mut changed = original.clone();
    populate(&mut changed, 0.01);
    assert_eq!(changed.stars, original.stars);
    assert_eq!(changed.barycentres, original.barycentres);
    for (before, after) in original.planets.iter().zip(&changed.planets) {
        assert_eq!(before.id(), after.id());
        assert_eq!(before.body.orbit, after.body.orbit);
        assert_eq!(before.body.budget.source(), after.body.budget.source());
        assert_eq!(
            before.body.formation_mass_earth,
            after.body.formation_mass_earth
        );
        assert_eq!(
            before.body.formation_distance_au,
            after.body.formation_distance_au
        );
        assert_eq!(
            after.body.structure.mass_kg(),
            after.body.budget.retained().total_mass_kg()
        );
        assert_eq!(before.body.resonance, after.body.resonance);
        assert_eq!(after.environment.as_ref().unwrap().age_gyr, 0.01);
    }
    let once = changed.clone();
    populate(&mut changed, 0.01);
    for (before, after) in once.planets.iter().zip(&changed.planets) {
        assert_eq!(before.body.structure, after.body.structure);
        assert_eq!(before.body.budget, after.body.budget);
        assert_eq!(before.body.spin, after.body.spin);
        assert_eq!(before.environment, after.environment);
    }
}

#[test]
fn wire_inputs_are_revalidated_and_derived_values_are_not_trusted() {
    let physical = GasAtmosphere::new(
        Composition::new(&[(Gas::Nitrogen, 1.0)]).unwrap(),
        ReferenceLevel::Surface,
        ONE_BAR_PA,
        280.0,
        Some(AerosolLayer::new(AerosolMaterial::MineralDust, 1e-8, 1000.0, 1e-6).unwrap()),
    )
    .unwrap();
    let mut data = physical.to_data(4e14, 6e6).unwrap();
    assert_eq!(GasAtmosphere::try_from(&data).unwrap(), physical);
    data.extent_m = -1.0;
    data.scale_height_m = f64::NAN;
    assert_eq!(GasAtmosphere::try_from(&data).unwrap(), physical);
    data.gases[0].gas = "unknown".into();
    assert!(GasAtmosphere::try_from(&data).is_err());
    data.gases[0].gas = "N2".into();
    data.gases[0].mole_fraction = 0.5;
    assert!(GasAtmosphere::try_from(&data).is_err());
}

#[test]
fn legacy_planet_records_remain_readable_and_airless_is_not_unknown() {
    let planet = space_shared::Planet::default();
    let mut json = serde_json::to_value(&planet).unwrap();
    json.as_object_mut().unwrap().remove("environment");
    let old: space_shared::Planet = serde_json::from_value(json).unwrap();
    assert!(old.environment.is_none());
    let mut populated = old;
    populated.environment = Some(PlanetEnvironment {
        irradiation_w_m2: 1361.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: 255.0,
        age_gyr: 4.5,
        atmosphere: None,
    });
    let json = serde_json::to_value(&populated).unwrap();
    assert!(json["environment"].is_object());
    assert!(json["environment"]["atmosphere"].is_null());
}
