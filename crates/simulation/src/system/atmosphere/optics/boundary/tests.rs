use super::*;
use crate::{
    system::{
        composition::{Budget, Inventory},
        forcing::Forcing,
    },
    units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use space_shared::PhysicalRevision;

fn gas(species: Gas, level: ReferenceLevel, pressure: f64, temperature: f64) -> GasAtmosphere {
    GasAtmosphere::new(
        Composition::new(&[(species, 1.0)]).unwrap(),
        level,
        pressure,
        temperature,
        None,
    )
    .unwrap()
}

fn candidate() -> PlanetResult {
    let solid = Inventory::new(&[(Material::Silicate, EARTH_MASS_KG)]).unwrap();
    let hydrogen = Inventory::new(&[(Material::Hydrogen, EARTH_MASS_KG)]).unwrap();
    let budget = Budget::new(
        solid.checked_add(&hydrogen).unwrap(),
        solid,
        Inventory::default(),
        hydrogen,
    )
    .unwrap();
    physical::initial_result(
        &budget,
        &Forcing::new(1361.0, 1361.0, 0.3, 4.5)
            .unwrap()
            .environment(),
        PhysicalRevision {
            inputs: "optics-test".into(),
            model: "test".into(),
        },
    )
    .unwrap()
}

#[test]
fn declares_reference_profile_proxy_and_finite_cutoff_without_claiming_ground() {
    let gm = GRAVITATIONAL_CONSTANT * EARTH_MASS_KG;
    let radius = 6.371e6;
    for (level, kind) in [
        (
            ReferenceLevel::Surface,
            LowerBoundaryKind::CondensedReferenceProxy,
        ),
        (ReferenceLevel::OneBar, LowerBoundaryKind::GasReferenceProxy),
    ] {
        let air = gas(Gas::Nitrogen, level, ONE_BAR_PA, 280.0);
        let column = OpticalColumn::new(&air, gm, radius).unwrap();
        assert_eq!(column.optics, air.optics(gm, radius).unwrap());
        assert_eq!(column.reference.pressure_pa, ONE_BAR_PA);
        assert_eq!(column.reference.temperature_k, 280.0);
        assert_eq!(column.reference.level, level);
        assert_eq!(
            column.lower_boundary,
            LowerBoundary {
                radius_m: radius,
                kind
            }
        );
        assert_eq!(column.density_model, DensityModel::SphericalIsothermal);
        assert_eq!(column.outer_cutoff.gas_density_drops, EXTENT_SCALE_HEIGHTS);
        assert_eq!(
            column.outer_cutoff.radius_m,
            radius + column.outer_cutoff.height_m
        );
        assert!(column.outer_cutoff.radius_m.is_finite());
        // Gas does not physically vanish at the operational rendering cutoff.
        assert!(
            column
                .optics
                .column
                .density_at(column.outer_cutoff.height_m)
                .unwrap()
                > 0.0
        );
        assert!(
            column
                .optics
                .column
                .density_at(2.0 * column.outer_cutoff.height_m)
                .unwrap()
                > 0.0
        );
    }
}

#[test]
fn composition_and_conditions_drive_optics_not_a_material_layer_edge() {
    let gm = GRAVITATIONAL_CONSTANT * EARTH_MASS_KG;
    let radius = 6.371e6;
    let nitrogen = OpticalColumn::new(
        &gas(Gas::Nitrogen, ReferenceLevel::Surface, ONE_BAR_PA, 280.0),
        gm,
        radius,
    )
    .unwrap();
    let methane = OpticalColumn::new(
        &gas(Gas::Methane, ReferenceLevel::Surface, ONE_BAR_PA, 280.0),
        gm,
        radius,
    )
    .unwrap();
    assert_ne!(
        nitrogen.optics.rayleigh_m_inv,
        methane.optics.rayleigh_m_inv
    );
    assert_eq!(nitrogen.optics.gas_absorption_m_inv, [0.0; 3]);
    assert!(methane.optics.gas_absorption_m_inv[0] > 0.0);
    assert_ne!(nitrogen.outer_cutoff, methane.outer_cutoff);
    let dense = OpticalColumn::new(
        &gas(
            Gas::Nitrogen,
            ReferenceLevel::Surface,
            100.0 * ONE_BAR_PA,
            280.0,
        ),
        gm,
        radius,
    )
    .unwrap();
    assert!(dense.outer_cutoff.height_m > nitrogen.outer_cutoff.height_m);
    assert_eq!(
        dense.outer_cutoff.gas_density_drops,
        EXTENT_SCALE_HEIGHTS + 100_f64.ln()
    );
    assert_eq!(dense.lower_boundary, nitrogen.lower_boundary);
}

#[test]
fn aerosol_extent_and_weak_binding_are_declared_without_clamping() {
    let gm = GRAVITATIONAL_CONSTANT * EARTH_MASS_KG;
    let mut air = gas(Gas::Nitrogen, ReferenceLevel::Surface, ONE_BAR_PA, 280.0);
    air.aerosol = Some(AerosolLayer::new(AerosolMaterial::WaterIce, 1e-8, 1e5, 1e-6).unwrap());
    let column = OpticalColumn::new(&air, gm, 6.371e6).unwrap();
    assert_eq!(column.outer_cutoff.height_m, EXTENT_SCALE_HEIGHTS * 1e5);
    assert_eq!(
        column.outer_cutoff.aerosol_scale_heights,
        EXTENT_SCALE_HEIGHTS
    );
    let diffuse = OpticalColumn::new(
        &gas(Gas::Hydrogen, ReferenceLevel::OneBar, ONE_BAR_PA, 1000.0),
        gm,
        1e8,
    )
    .unwrap();
    assert_eq!(diffuse.density_model, DensityModel::UnresolvedExponential);
    assert!(!diffuse.optics.column.profile.spherical);
    assert!(diffuse.outer_cutoff.height_m > diffuse.reference.radius_m);
}

#[test]
fn unified_result_uses_its_reference_and_rejects_mixed_atmospheres() {
    let result = candidate();
    let temperature = result.record.surface_temperature_k;
    let air = gas(
        Gas::Hydrogen,
        ReferenceLevel::OneBar,
        ONE_BAR_PA,
        temperature,
    );
    let expected = OpticalColumn::from_result(&result, &air).unwrap();
    assert_eq!(
        expected.reference.radius_m,
        result.record.physical.reference_radius_m
    );
    assert_eq!(
        expected.lower_boundary.kind,
        LowerBoundaryKind::GasReferenceProxy
    );
    // Moving the numerical outer edge alone must not move the optical cutoff.
    let mut expanded = result.clone();
    expanded.record.layers[0].outer_radius_m *= 1.5;
    physical::validate_result(&expanded).unwrap();
    assert_eq!(
        OpticalColumn::from_result(&expanded, &air).unwrap(),
        expected
    );
    // An interior reference uses enclosed-mass gravity, not total GM.
    expanded.record.physical.reference_gravity_m_s2 *= 0.75;
    physical::validate_result(&expanded).unwrap();
    let enclosed = OpticalColumn::from_result(&expanded, &air).unwrap();
    assert!(
        (enclosed.reference.gravity_m_s2 / expected.reference.gravity_m_s2 - 0.75).abs() < 1e-14
    );
    assert!(enclosed.optics.column.scale_height_m > expected.optics.column.scale_height_m);
    for mixed in [
        gas(
            Gas::Nitrogen,
            ReferenceLevel::OneBar,
            ONE_BAR_PA,
            temperature,
        ),
        gas(
            Gas::Hydrogen,
            ReferenceLevel::Surface,
            ONE_BAR_PA,
            temperature,
        ),
        gas(
            Gas::Hydrogen,
            ReferenceLevel::OneBar,
            ONE_BAR_PA,
            temperature + 10.0,
        ),
    ] {
        assert!(OpticalColumn::from_result(&result, &mixed).is_err());
    }
    let mut broken = result.clone();
    broken.record.physical.inertia_factor = f64::NAN;
    assert!(OpticalColumn::from_result(&broken, &air).is_err());
}

#[test]
fn invalid_reference_inputs_never_produce_optical_geometry() {
    let air = gas(Gas::Nitrogen, ReferenceLevel::Surface, ONE_BAR_PA, 280.0);
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(OpticalColumn::new(&air, value, 6.371e6).is_err());
        assert!(OpticalColumn::new(&air, 4e14, value).is_err());
    }
}

#[test]
fn condensed_secondary_proxy_cannot_borrow_a_legacy_atmosphere() {
    let solid = Inventory::new(&[(Material::Silicate, EARTH_MASS_KG)]).unwrap();
    let secondary = Inventory::new(&[(Material::Nitrogen, 1e18)]).unwrap();
    let budget = Budget::new(
        solid.checked_add(&secondary).unwrap(),
        solid,
        secondary,
        Inventory::default(),
    )
    .unwrap();
    let result = physical::initial_result(
        &budget,
        &Forcing::new(1361.0, 1361.0, 0.3, 4.5)
            .unwrap()
            .environment(),
        PhysicalRevision {
            inputs: "secondary-proxy".into(),
            model: "test".into(),
        },
    )
    .unwrap();
    let air = gas(
        Gas::Nitrogen,
        ReferenceLevel::Surface,
        ONE_BAR_PA,
        result.record.surface_temperature_k,
    );
    assert!(OpticalColumn::from_result(&result, &air).is_err());
    assert!(
        OpticalColumn::new(
            &air,
            GRAVITATIONAL_CONSTANT * budget.retained().total_mass_kg(),
            result.record.physical.reference_radius_m
        )
        .is_ok()
    );
}
