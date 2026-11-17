use super::*;
use crate::system::{
    composition::{Budget, Inventory, Material},
    physical,
    structure::prototype,
};
use crate::units::EARTH_MASS_KG;
use space_shared::{PhysicalRevision, PlanetEnvironment};

fn result(water: f64, gas: f64, temperature: f64, layered: bool) -> PlanetResult {
    let inventory = Inventory::new(&[
        (Material::Metal, 0.3 * EARTH_MASS_KG),
        (Material::Silicate, (0.7 - water) * EARTH_MASS_KG),
        (Material::Water, water * EARTH_MASS_KG),
        (Material::Hydrogen, gas * EARTH_MASS_KG),
    ])
    .unwrap();
    let budget = Budget::new(
        inventory,
        inventory,
        Inventory::default(),
        Inventory::default(),
    )
    .unwrap();
    let environment = PlanetEnvironment {
        irradiation_w_m2: 1361.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: temperature,
        age_gyr: 4.5,
        atmosphere: None,
    };
    let revision = PhysicalRevision {
        inputs: "surface-test".into(),
        model: "test".into(),
    };
    if layered {
        let solved = prototype::solve(&budget, &environment, Default::default()).unwrap();
        physical::resolved_result(&prototype::Outcome::Layered(Box::new(solved)), revision).unwrap()
    } else {
        physical::initial_result(&budget, &environment, revision).unwrap()
    }
}

#[test]
fn condensed_geometry_and_phase_do_not_follow_the_radius_label() {
    for (water, temperature, phase) in [
        (0.0, 250.0, SurfaceKnowledge::Unresolved),
        (0.0, 1800.0, SurfaceKnowledge::Unresolved),
        (0.3, 250.0, SurfaceKnowledge::SolidPhase),
        (0.3, 280.0, SurfaceKnowledge::Liquid),
    ] {
        let r = result(water, 0.0, temperature, true);
        let geometry = Geometry::from_result(&r).unwrap();
        assert!(!geometry.gas_reference);
        assert_eq!(geometry.condensed_phase, phase);
        assert!((geometry.condensed_radius_m / geometry.reference_radius_m - 1.0).abs() < 1e-12);
    }
}

#[test]
fn sub_bar_air_references_the_condensed_top_without_inventing_a_one_bar_floor() {
    let r = result(0.0, 1e-9, 250.0, true);
    assert!(r.record.surface_pressure_pa > 0.0 && r.record.surface_pressure_pa < 1e5);
    let geometry = Geometry::from_result(&r).unwrap();
    assert!(!geometry.gas_reference && !geometry.has_gas_depth());
    assert_eq!(geometry.condensed_radius_m, geometry.reference_radius_m);
}

#[test]
fn gas_coordinates_never_erase_the_condensed_boundary_or_establish_its_phase() {
    for gas in [1e-8, 1e-3, 0.1] {
        let r = result(0.3, gas, 250.0, false);
        let geometry = Geometry::from_result(&r).unwrap();
        assert_eq!(geometry.has_gas_depth(), geometry.gas_reference);
        assert_eq!(geometry.condensed_phase, SurfaceKnowledge::Unresolved);
        let reference = Spheroid::of(geometry.reference_radius_m, 0.1);
        let condensed = geometry.condensed_shape(reference);
        assert!(condensed.equatorial <= reference.equatorial);
        assert!((condensed.flattening() - reference.flattening()).abs() < 1e-15);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serde_json::to_string(&geometry).unwrap())
                .unwrap()["gas_reference"],
            geometry.gas_reference
        );
    }
}

#[test]
fn frost_top_is_not_confused_with_its_loaded_host_or_the_atmosphere_edge() {
    let mut r = result(0.0, 0.0, 250.0, false);
    // A closed, equal-density subdivision leaves every physical invariant
    // unchanged while exercising a deposit outside its loaded host.
    let outer = &mut r.record.layers[0];
    let mut frost = outer.clone();
    let mid = (outer.inner_radius_m.powi(3) + outer.outer_radius_m.powi(3))
        .mul_add(0.5, 0.0)
        .cbrt();
    let pressure = (outer.inner_pressure_pa + outer.outer_pressure_pa.unwrap()) * 0.5;
    frost.kind = "frost".into();
    frost.inner_radius_m = mid;
    frost.inner_pressure_pa = pressure;
    outer.outer_radius_m = mid;
    outer.outer_pressure_pa = Some(pressure);
    // Each half keeps half of every material and phase, so its breakdown
    // still sums to its mass.
    for layer in [&mut *outer, &mut frost] {
        layer.mass_kg *= 0.5;
        for material in &mut layer.materials {
            material.mass_kg *= 0.5;
        }
        for phase in &mut layer.phase_allocations {
            phase.vapour_kg *= 0.5;
            phase.condensate_kg *= 0.5;
        }
    }
    r.record.layers.insert(0, frost);
    r.record.surface = "frost".into();
    r.record.physical.radius_reference = RadiusReference::UnresolvedVolatileBoundary;
    r.record.physical.condensed_radius_m = mid;
    let geometry = Geometry::from_result(&r).unwrap();
    assert_eq!(
        geometry.condensed_radius_m,
        r.record.physical.reference_radius_m
    );
    assert!(geometry.condensed_radius_m > r.record.physical.condensed_radius_m);
}
