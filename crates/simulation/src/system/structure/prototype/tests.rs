use super::*;
use crate::{
    system::structure::{dry, water},
    units::EARTH_MASS_KG,
};

fn environment(t: f64) -> PlanetEnvironment {
    PlanetEnvironment {
        irradiation_w_m2: 0.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: t,
        age_gyr: 4.5,
        atmosphere: None,
    }
}

fn budget(water: f64, scale: f64) -> Budget {
    let retained = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG * scale),
        (Material::Silicate, (0.7 - water) * EARTH_MASS_KG * scale),
        (Material::Water, water * EARTH_MASS_KG * scale),
        (Material::Nitrogen, 1e-6 * EARTH_MASS_KG * scale),
    ])
    .unwrap();
    let source = retained
        .checked_add(
            &Inventory::new(&[(Material::Hydrogen, 1e-7 * EARTH_MASS_KG * scale)]).unwrap(),
        )
        .unwrap();
    // Deliberately no meaningful legacy reservoir split: the new result owns it.
    Budget::new(source, retained, Inventory::default(), Inventory::default()).unwrap()
}

fn check(s: &Solved, b: &Budget) {
    let structure = &s.structure;
    let retained = structure.retained_inventory();
    for m in Material::ALL {
        let initial = b.source().mass_kg(m);
        assert!((retained.mass_kg(m) + s.escaped.mass_kg(m) - initial).abs() <= 1e-10 * initial);
    }
    assert_eq!(s.source, *b.source());
    assert_eq!(s.escaped, b.escaped().checked_add(&s.blowoff).unwrap());
    assert!(structure.diagnostics().interface_residual_pa.abs() <= 0.02);
    let interior = structure.condensed().retained_inventory();
    assert!(
        (interior.mass_kg(Material::Nitrogen) / s.protected.mass_kg(Material::Nitrogen) - 1.0)
            .abs()
            < 1e-12
    );
    assert_eq!(s.protected.mass_kg(Material::Water), 0.0);
    let expected_gm = crate::units::GRAVITATIONAL_CONSTANT
        * (structure.condensed().mass_kg() + structure.deposit_inventory().total_mass_kg());
    assert_eq!(structure.column_gm_m3_s2(), expected_gm);
    assert!(structure.inertia_kg_m2().is_finite() && structure.inertia_kg_m2() > 0.0);
    if let Some(air) = structure.atmosphere() {
        let c = structure.column().unwrap().unwrap();
        let represented = 4.0 * std::f64::consts::PI * structure.surface_radius_m().powi(4)
            / expected_gm
            * air.pressure_pa()
            * c.profile.mass_factor(air.pressure_pa());
        assert!((represented / structure.gas_inventory().total_mass_kg() - 1.0).abs() < 1e-9);
    }
}

#[test]
fn dry_ice_and_ocean_join_closes_without_mutating_generation() {
    for (water, temperature) in [(0.0, 250.0), (0.3, 250.0), (0.3, 280.0)] {
        let b = budget(water, 1.0);
        let original = b;
        let env = environment(temperature);
        let s = solve(&b, &env, Controls::default()).unwrap();
        check(&s, &b);
        assert_eq!(b, original);
        assert_eq!(s, solve(&b, &env, Controls::default()).unwrap());
        match s.structure.condensed() {
            coupled::CondensedStructure::Dry(d) => {
                assert_eq!(d.model_id(), dry::protected::MODEL_ID)
            }
            coupled::CondensedStructure::Water(w) => {
                assert_eq!(w.model_id(), water::protected::MODEL_ID);
                assert_eq!(
                    w.regions().last().unwrap().phase(),
                    water::PhaseKnowledge::Water(if temperature < 273.15 {
                        crate::system::materials::water::Phase::IceIh
                    } else {
                        crate::system::materials::water::Phase::Liquid
                    })
                );
            }
        }
    }
}

#[test]
fn blowoff_preserves_protected_species_and_previous_escape() {
    let b = budget(0.0, 0.0002);
    let s = solve(&b, &environment(1000.0), Controls::default()).unwrap();
    check(&s, &b);
    assert!(s.structure.atmosphere().is_none());
    let n = b.retained().mass_kg(Material::Nitrogen);
    assert!((s.blowoff.mass_kg(Material::Nitrogen) / n - 0.1).abs() < 1e-12);
    assert_eq!(s.blowoff.mass_kg(Material::Hydrogen), 0.0);
    assert_eq!(
        s.escaped.mass_kg(Material::Hydrogen),
        b.escaped().mass_kg(Material::Hydrogen)
    );
}

#[test]
fn frozen_exchangeable_material_is_not_entrained_twice() {
    let b = budget(0.3, 1.0);
    let s = solve(&b, &environment(30.0), Controls::default()).unwrap();
    check(&s, &b);
    assert!(s.structure.deposit_inventory().mass_kg(Material::Nitrogen) > 0.0);
    assert!(s.structure.deposit_thickness_m() > 0.0);
    assert!(
        s.structure.condensed_load_pa() > s.structure.atmosphere().map_or(0.0, |a| a.pressure_pa())
    );
}

#[test]
fn unsupported_and_budget_failures_never_fall_back() {
    let b = budget(0.0, 1.0);
    for t in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert_eq!(
            solve(&b, &environment(t), Controls::default()).unwrap_err(),
            Error::InvalidInput("equilibrium temperature")
        );
    }
    let controls = Controls {
        max_total_eos_evaluations: 0,
        ..Controls::default()
    };
    assert_eq!(
        solve(&b, &environment(250.0), controls).unwrap_err(),
        Error::WorkBudgetExhausted
    );
    let source = b
        .retained()
        .checked_add(&Inventory::new(&[(Material::Hydrogen, 0.1 * EARTH_MASS_KG)]).unwrap())
        .unwrap();
    let envelope = Budget::new(source, source, Inventory::default(), Inventory::default()).unwrap();
    assert_eq!(
        solve(&envelope, &environment(250.0), Controls::default()).unwrap_err(),
        Error::GasMassOutsideDomain
    );
}

#[test]
fn protected_iterations_share_the_original_work_ceiling() {
    let b = budget(0.0, 1.0);
    let env = environment(250.0);
    let s = solve(&b, &env, Controls::default()).unwrap();
    let work = s.structure.diagnostics().total_eos_evaluations;
    assert_eq!(
        s,
        solve(
            &b,
            &env,
            Controls {
                max_total_eos_evaluations: work,
                ..Controls::default()
            }
        )
        .unwrap()
    );
    assert!(matches!(
        solve(
            &b,
            &env,
            Controls {
                max_total_eos_evaluations: work - 1,
                ..Controls::default()
            }
        ),
        Err(Error::WorkBudgetExhausted | Error::Radial(dry::SolveError::WorkBudgetExhausted(_)))
    ));
}

#[test]
fn all_water_can_evaporate_without_losing_entrained_origins() {
    let b = budget(1e-10, 1.0);
    let s = solve(&b, &environment(1000.0), Controls::default()).unwrap();
    check(&s, &b);
    assert!(matches!(
        s.structure.condensed(),
        coupled::CondensedStructure::Dry(_)
    ));
    assert_eq!(
        s.structure.gas_inventory().mass_kg(Material::Water),
        b.retained().mass_kg(Material::Water)
    );
}

#[test]
fn empty_protected_supply_matches_the_original_coupling_route() {
    let inventory = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, 0.7 * EARTH_MASS_KG),
    ])
    .unwrap();
    let b = Budget::new(
        inventory,
        inventory,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    let s = solve(&b, &environment(250.0), Controls::default()).unwrap();
    assert_eq!(s.protected, Inventory::default());
    let original = coupled::escape::solve_escaping(
        &Request {
            retained: inventory,
            exchangeable: Inventory::default(),
            temperature: Temperature::Greenhouse {
                equilibrium_k: 250.0,
            },
        },
        Controls::default(),
    )
    .unwrap();
    assert_eq!(s.structure, original.structure);
    assert_eq!(s.blowoff, original.escaped);
}
