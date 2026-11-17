use super::*;
use crate::{
    system::{forcing::Forcing, physical},
    units::EARTH_MASS_KG,
};
use space_shared::PhysicalRevision;

fn fixture(
    hydrogen: f64,
    nitrogen: f64,
    water: f64,
    temperature: f64,
) -> (Fallback, PlanetEnvironment) {
    let solid = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, 0.7 * EARTH_MASS_KG),
    ])
    .unwrap();
    let volatile =
        Inventory::new(&[(Material::Nitrogen, nitrogen), (Material::Water, water)]).unwrap();
    let gas = Inventory::new(&[(Material::Hydrogen, hydrogen)]).unwrap();
    let source = solid
        .checked_add(&volatile)
        .unwrap()
        .checked_add(&gas)
        .unwrap();
    let budget = Budget::new(source, solid, volatile, gas).unwrap();
    let mut env = Forcing::new(1361.0, 1361.0, 0.3, 4.5)
        .unwrap()
        .environment();
    env.equilibrium_temperature_k = temperature;
    (initial(&budget, &env), env)
}

fn check(initial: &Fallback, env: &PlanetEnvironment) -> Partitioned {
    let solved = resolve(initial).unwrap();
    assert_eq!(solved.fallback.source, initial.source);
    assert_eq!(solved.fallback.retained, initial.retained);
    assert_eq!(solved.fallback.escaped, initial.escaped);
    // A fully evaporated volatile layer is removed, shrinking the boundary.
    assert!(solved.fallback.radius_m <= initial.radius_m);
    assert!(solved.fallback.condensed_radius_m <= initial.condensed_radius_m);
    assert_eq!(
        solved.fallback.reference == RadiusReference::OneBar,
        initial.reference == RadiusReference::OneBar
    );
    physical::validate_record(&physical::fallback(&solved.fallback)).unwrap();
    // Generation enters through the surface decision, which keeps this
    // partition only when it leaves the surface undecided.
    let prepared = physical::initial_with_atmosphere(
        initial,
        env,
        PhysicalRevision {
            inputs: "partition-fixture".into(),
            model: MODEL_ID.into(),
        },
    )
    .unwrap();
    physical::validate_result(&prepared.result).unwrap();
    let decided = matches!(
        prepared.decision,
        crate::system::structure::prototype::Decision::Thin { .. }
    );
    // An envelope decision re-references a thick boundary column at one bar.
    let same_reference = (prepared.result.record.physical.radius_reference
        == RadiusReference::OneBar)
        == (solved.fallback.reference == RadiusReference::OneBar);
    if !decided && same_reference {
        assert_eq!(prepared.result.record.physical, {
            let mut p = physical::fallback(&solved.fallback).physical;
            p.approximations.push(MODEL_ID.into());
            p
        });
    }
    let record = &prepared.result.record;
    for m in &record.physical.materials {
        let material = Material::from_id(&m.material).unwrap();
        assert!(m.retained_kg <= initial.retained.mass_kg(material));
        if !decided {
            assert_eq!(m.retained_kg, initial.retained.mass_kg(material));
        }
        assert!(
            (m.interior_kg + m.condensed_kg + m.gas_kg - m.retained_kg).abs()
                <= 1e-12 * m.retained_kg
        );
    }
    if let Some(wire) = prepared.environment.atmosphere.as_ref() {
        assert_eq!(wire.pressure_pa, record.surface_pressure_pa);
        assert_eq!(wire.temperature_k, record.surface_temperature_k);
        crate::system::atmosphere::optics::boundary::OpticalColumn::from_result(
            &prepared.result,
            &GasAtmosphere::try_from(wire).unwrap(),
        )
        .unwrap();
    } else {
        assert_eq!(record.surface_pressure_pa, 0.0);
    }
    if !decided {
        assert_eq!(
            solved.atmosphere.is_some(),
            prepared.environment.atmosphere.is_some()
        );
    }
    assert_eq!(resolve(initial).unwrap().fallback, solved.fallback);
    solved
}

#[test]
fn dry_secondary_and_trace_columns_share_one_conserved_result() {
    for nitrogen in [0.0, 1e10, 1e18, 1e21] {
        let (initial, env) = fixture(0.0, nitrogen, 0.0, 280.0);
        let solved = check(&initial, &env);
        assert_eq!(solved.atmosphere.is_some(), nitrogen > 0.0);
        if let Some(air) = solved.atmosphere {
            assert!(air.pressure_pa() > 0.0);
            assert_eq!(
                air.reference_level(),
                crate::system::atmosphere::ReferenceLevel::Surface
            );
            let gas = solved
                .budget
                .reservoir(Reservoir::Gas)
                .mass_kg(Material::Nitrogen);
            assert!(gas > 0.0 && gas <= nitrogen * 0.1);
            assert_eq!(
                solved.fallback.regions.last().unwrap().kind,
                Kind::Atmosphere
            );
        }
    }
}

#[test]
fn cold_trapping_and_bulk_envelopes_do_not_double_count_gas() {
    for hydrogen in [0.0, 1e12, 0.1 * EARTH_MASS_KG] {
        for temperature in [20.0, 80.0, 280.0, 1000.0] {
            let (initial, env) = fixture(hydrogen, 1e20, 0.1 * EARTH_MASS_KG, temperature);
            let solved = check(&initial, &env);
            let gas_regions = solved
                .fallback
                .regions
                .iter()
                .filter(|r| matches!(r.kind, Kind::Atmosphere | Kind::Envelope))
                .count();
            // A one-bar reference splits the gas there: the envelope
            // beneath, the atmosphere record's own column above.
            let one_bar = initial.reference == RadiusReference::OneBar;
            assert_eq!(
                gas_regions,
                usize::from(solved.atmosphere.is_some()) * (1 + usize::from(one_bar))
            );
            if one_bar {
                let [envelope, above] = solved.fallback.regions.last_chunk().unwrap();
                assert_eq!(
                    [envelope.kind, above.kind],
                    [Kind::Envelope, Kind::Atmosphere]
                );
                assert_eq!(envelope.outer_radius_m, solved.fallback.radius_m);
                assert_eq!(above.inner_radius_m, solved.fallback.radius_m);
                assert_eq!(
                    [envelope.outer_pressure_pa, above.inner_pressure_pa],
                    [1e5; 2]
                );
                assert!(above.inventory.total_mass_kg() <= envelope.inventory.total_mass_kg());
                assert_eq!(solved.atmosphere.unwrap().pressure_pa(), 1e5);
            }
        }
    }
}

#[test]
fn column_inertia_agrees_with_independent_radial_mass_quadrature() {
    use crate::system::atmosphere::{Composition, Gas, ReferenceLevel};
    for gm in [1e11, 4e14] {
        let air = GasAtmosphere::new(
            Composition::new(&[(Gas::Nitrogen, 1.0)]).unwrap(),
            ReferenceLevel::Surface,
            1e5,
            280.0,
            None,
        )
        .unwrap();
        let column = air.column(gm, 6.371e6).unwrap();
        let n = 50_000;
        let step = column.extent_m / f64::from(n);
        let (mut mass, mut moment) = (0.0, 0.0);
        for i in 0..n {
            let height = (f64::from(i) + 0.5) * step;
            let radius = 6.371e6 + height;
            let dm = radius.powi(2) * column.density_at(height).unwrap() * step;
            mass += dm;
            moment += (2.0 / 3.0) * dm * radius.powi(2);
        }
        let expected = moment / mass;
        let actual = column_inertia(&column.profile, 1e5, 1.0);
        assert!((actual / expected - 1.0).abs() < 1e-6);
    }
}

#[test]
fn trace_cold_traps_use_representable_vapour_and_do_not_erase_pressure() {
    // Large protected reservoirs make some 20 K vapour transfers smaller than
    // one source-reservoir ulp. Bracket the gas that can actually be transferred.
    let solid = Inventory::new(&[(Material::Silicate, 0.06 * EARTH_MASS_KG)]).unwrap();
    let volatile = Inventory::new(&[
        (Material::Water, 1e22),
        (Material::Nitrogen, 1e20),
        (Material::Argon, 1e18),
        (Material::CarbonDioxide, 1e18),
        (Material::Methane, 1e18),
    ])
    .unwrap();
    let source = solid.checked_add(&volatile).unwrap();
    let budget = Budget::new(source, solid, volatile, Inventory::default()).unwrap();
    let mut env = Forcing::new(0.0, 0.0, 0.3, 4.5).unwrap().environment();
    env.equilibrium_temperature_k = 20.0;
    let solved = check(&initial(&budget, &env), &env);
    let air = solved.atmosphere.unwrap();
    assert!(air.pressure_pa() > 0.0 && air.pressure_pa() < 0.1);
    assert_eq!(
        solved
            .budget
            .reservoir(Reservoir::Gas)
            .mass_kg(Material::Water),
        0.0
    );
    for species in crate::system::atmosphere::Gas::ALL {
        assert!(
            air.pressure_pa() * air.composition().fraction(species)
                <= crate::system::atmosphere::condensation::vapour_pressure(species, 20.0)
                    * (1.0 + 1e-9)
        );
    }
}

#[test]
fn prepared_candidate_refuses_mixed_temperature_or_repartitioned_structure() {
    let (initial, env) = fixture(0.1 * EARTH_MASS_KG, 1e20, 1e21, 280.0);
    let revision = PhysicalRevision {
        inputs: "thermal-isolation".into(),
        model: MODEL_ID.into(),
    };
    let mut other = env.clone();
    other.equilibrium_temperature_k += 10.0;
    assert!(physical::initial_with_atmosphere(&initial, &other, revision.clone()).is_err());
    let solved = resolve(&initial).unwrap();
    assert!(resolve(&solved.fallback).is_err());
    other = env.clone();
    other.atmosphere = Some(
        solved
            .atmosphere
            .unwrap()
            .to_data(GRAVITATIONAL_CONSTANT * initial.mass_kg(), initial.radius_m)
            .unwrap(),
    );
    other.atmosphere.as_mut().unwrap().pressure_pa = -100.0;
    let prepared = physical::initial_with_atmosphere(&initial, &other, revision).unwrap();
    assert_eq!(
        prepared.environment.atmosphere.unwrap().pressure_pa,
        initial.surface_pressure_pa
    );
}
