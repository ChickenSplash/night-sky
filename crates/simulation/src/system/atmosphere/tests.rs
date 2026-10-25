use super::*;
use crate::units::{EARTH_SOLAR, SUN_GM};

fn mixture(parts: &[(Gas, f64)]) -> Composition {
    Composition::new(parts).unwrap()
}

fn nitrogen(pressure: f64, temperature: f64) -> GasAtmosphere {
    GasAtmosphere::new(
        mixture(&[(Gas::Nitrogen, 1.0)]),
        ReferenceLevel::Surface,
        pressure,
        temperature,
        None,
    )
    .unwrap()
}

fn close(got: f64, expected: f64, relative: f64) {
    assert!(
        (got / expected - 1.0).abs() < relative,
        "{got} vs {expected}"
    );
}

#[test]
fn composition_checks_fractions_and_only_normalises_roundoff() {
    for parts in [
        vec![],
        vec![(Gas::Nitrogen, 0.9)],
        vec![(Gas::Nitrogen, 0.8), (Gas::Oxygen, 0.3)],
        vec![(Gas::Nitrogen, -0.1), (Gas::Oxygen, 1.1)],
        vec![(Gas::Nitrogen, f64::NAN)],
        vec![(Gas::Nitrogen, f64::INFINITY)],
        vec![(Gas::Nitrogen, 0.5), (Gas::Nitrogen, 0.5)],
        vec![(Gas::Nitrogen, 1.0), (Gas::Argon, 0.0), (Gas::Argon, 0.0)],
    ] {
        assert!(Composition::new(&parts).is_err(), "{parts:?}");
    }
    let air = mixture(&[(Gas::Nitrogen, 0.8), (Gas::Oxygen, 0.200_000_000_1)]);
    close(
        Gas::ALL.iter().map(|gas| air.fraction(*gas)).sum(),
        1.0,
        1e-14,
    );
    assert_eq!(air.fraction(Gas::CarbonDioxide), 0.0);
    for gas in Gas::ALL {
        let pure = mixture(&[(gas, 1.0)]);
        close(
            pure.mean_molecular_mass_kg() / ATOMIC_MASS_KG,
            gas.molecular_mass_u(),
            1e-14,
        );
    }
}

#[test]
fn reference_world_columns_have_sensible_physical_values() {
    // Dry terrestrial air, a simplified Martian mixture and Jupiter-like H₂/He.
    // These are physical fixtures, not generated climates or renderer presets.
    let earth = mixture(&[
        (Gas::Nitrogen, 0.7808),
        (Gas::Oxygen, 0.2095),
        (Gas::Argon, 0.0093),
        (Gas::CarbonDioxide, 0.0004),
    ]);
    let mars = mixture(&[
        (Gas::CarbonDioxide, 0.953),
        (Gas::Nitrogen, 0.027),
        (Gas::Argon, 0.020),
    ]);
    let giant = mixture(&[(Gas::Hydrogen, 0.86), (Gas::Helium, 0.14)]);
    for (composition, level, pressure, temperature, gm, radius, mass_u, density, height) in [
        (
            earth,
            ReferenceLevel::Surface,
            101_325.0,
            288.15,
            SUN_GM * EARTH_SOLAR,
            6.371e6,
            28.97,
            1.225,
            8_430.0,
        ),
        (
            mars,
            ReferenceLevel::Surface,
            600.0,
            210.0,
            4.282_837e13,
            3.3895e6,
            43.50,
            0.01495,
            10_770.0,
        ),
        (
            giant,
            ReferenceLevel::OneBar,
            ONE_BAR_PA,
            165.0,
            1.266_865_34e17,
            6.9911e7,
            2.294,
            0.1672,
            23_070.0,
        ),
    ] {
        let atmosphere =
            GasAtmosphere::new(composition, level, pressure, temperature, None).unwrap();
        let column = atmosphere.column(gm, radius).unwrap();
        close(column.molecular_mass_kg / ATOMIC_MASS_KG, mass_u, 0.002);
        close(column.density_kg_m3, density, 0.005);
        close(column.scale_height_m, height, 0.005);
        close(column.gravity_m_s2, gm / radius.powi(2), 1e-14);
        assert_eq!(column.validity, ColumnValidity::Thin);
        close(
            column.extent_m,
            column
                .profile
                .height_for_drop(profile::Profile::cutoff_drop(pressure))
                .unwrap(),
            1e-14,
        );
    }
}

#[test]
fn ideal_gas_and_hydrostatic_scalings_are_not_independent_slider_draws() {
    let gm = SUN_GM * EARTH_SOLAR;
    let radius = 6.371e6;
    let base = nitrogen(ONE_BAR_PA, 280.0).column(gm, radius).unwrap();
    let hot = nitrogen(ONE_BAR_PA, 560.0).column(gm, radius).unwrap();
    let dense = nitrogen(2.0 * ONE_BAR_PA, 280.0)
        .column(gm, radius)
        .unwrap();
    let heavy = nitrogen(ONE_BAR_PA, 280.0)
        .column(2.0 * gm, radius)
        .unwrap();
    close(hot.scale_height_m, 2.0 * base.scale_height_m, 1e-14);
    close(hot.density_kg_m3, 0.5 * base.density_kg_m3, 1e-14);
    close(dense.density_kg_m3, 2.0 * base.density_kg_m3, 1e-14);
    close(dense.scale_height_m, base.scale_height_m, 1e-14);
    close(heavy.scale_height_m, 0.5 * base.scale_height_m, 1e-14);
    // At one scale height, ideal-gas density and pressure have fallen by e.
    close(
        base.density_kg_m3 * base.gravity_m_s2 * base.scale_height_m,
        ONE_BAR_PA,
        1e-14,
    );
}

#[test]
fn dense_boundary_preserves_the_column_above_one_bar() {
    let gm = SUN_GM * EARTH_SOLAR;
    let radius = 6.371e6;
    let base = nitrogen(ONE_BAR_PA, 280.0).column(gm, radius).unwrap();
    for bars in [1.0, 92.0, 700.0, 1_000.0, 10_000.0] {
        let atmosphere = nitrogen(bars * ONE_BAR_PA, 280.0);
        let column = atmosphere.column(gm, radius).unwrap();
        let one_bar_height = atmosphere
            .pressure_level_altitude_m(ONE_BAR_PA, gm, radius)
            .unwrap()
            .unwrap();
        let expected_height = column.profile.height_for_drop(bars.ln()).unwrap();
        assert!((one_bar_height - expected_height).abs() < 1e-8);
        let upper = nitrogen(ONE_BAR_PA, 280.0)
            .column(gm, radius + one_bar_height)
            .unwrap();
        close(column.extent_m - one_bar_height, upper.extent_m, 1e-12);
        close(
            column.density_at(one_bar_height).unwrap(),
            base.density_kg_m3,
            1e-13,
        );
        close(
            column.density_at(column.extent_m).unwrap(),
            base.density_at(base.extent_m).unwrap(),
            1e-13,
        );
        // More gas increases optical column mass, not merely the extent label.
        close(
            column.density_kg_m3 * column.scale_height_m,
            bars * base.density_kg_m3 * base.scale_height_m,
            1e-13,
        );
    }
}

#[test]
fn pressure_levels_respect_the_reference_and_validate_inputs() {
    let gm = SUN_GM * EARTH_SOLAR;
    let radius = 6.371e6;
    let thin = nitrogen(600.0, 210.0);
    assert_eq!(
        thin.pressure_level_altitude_m(ONE_BAR_PA, gm, radius),
        Ok(None)
    );
    assert_eq!(
        thin.pressure_level_altitude_m(600.0, gm, radius),
        Ok(Some(0.0))
    );
    assert!(
        thin.pressure_level_altitude_m(60.0, gm, radius)
            .unwrap()
            .unwrap()
            > 0.0
    );
    let column = thin.column(gm, radius).unwrap();
    assert_eq!(
        column.extent_m,
        column
            .profile
            .height_for_drop(EXTENT_SCALE_HEIGHTS)
            .unwrap()
    );
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(thin.pressure_level_altitude_m(bad, gm, radius).is_err());
        assert!(
            thin.pressure_level_altitude_m(ONE_BAR_PA, bad, radius)
                .is_err()
        );
        assert!(thin.pressure_level_altitude_m(ONE_BAR_PA, gm, bad).is_err());
    }
    // Logarithmic evaluation remains finite even when p_reference/p_level
    // would overflow. This is numerical coverage, not a physical domain claim.
    assert!(
        thin.pressure_level_altitude_m(f64::from_bits(1), gm, radius)
            .unwrap()
            .unwrap()
            .is_finite()
    );
}

#[test]
fn airless_is_explicit_and_one_bar_level_is_not_arbitrary_surface_pressure() {
    assert_eq!(Atmosphere::default(), Atmosphere::Airless);
    let composition = mixture(&[(Gas::Hydrogen, 1.0)]);
    assert_eq!(
        GasAtmosphere::new(composition, ReferenceLevel::OneBar, 2e5, 165.0, None),
        Err(AtmosphereError::OneBarPressure),
    );
    let atmosphere =
        GasAtmosphere::new(composition, ReferenceLevel::OneBar, ONE_BAR_PA, 165.0, None).unwrap();
    assert_eq!(atmosphere.reference_level(), ReferenceLevel::OneBar);
    assert_eq!(atmosphere.pressure_pa(), ONE_BAR_PA);
    assert_eq!(atmosphere.temperature_k(), 165.0);
    assert_eq!(atmosphere.composition(), &composition);
    assert_eq!(atmosphere.aerosol(), None);
    assert_ne!(Atmosphere::Present(atmosphere), Atmosphere::Airless);
}

#[test]
fn rejects_nonphysical_and_nonfinite_inputs_and_derived_overflow() {
    let composition = mixture(&[(Gas::Nitrogen, 1.0)]);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            GasAtmosphere::new(composition, ReferenceLevel::Surface, bad, 280.0, None).is_err()
        );
        assert!(
            GasAtmosphere::new(composition, ReferenceLevel::Surface, ONE_BAR_PA, bad, None)
                .is_err()
        );
        let atmosphere = nitrogen(ONE_BAR_PA, 280.0);
        assert!(atmosphere.column(bad, 6e6).is_err());
        assert!(atmosphere.column(4e14, bad).is_err());
        assert!(AerosolLayer::new(AerosolMaterial::MineralDust, bad, 1000.0, 1e-6).is_err());
        assert!(AerosolLayer::new(AerosolMaterial::MineralDust, 1e-8, bad, 1e-6).is_err());
        assert!(AerosolLayer::new(AerosolMaterial::MineralDust, 1e-8, 1000.0, bad).is_err());
    }
    let atmosphere = nitrogen(ONE_BAR_PA, 280.0);
    assert!(atmosphere.column(f64::MAX, f64::MIN_POSITIVE).is_err());
    assert!(atmosphere.column(f64::MIN_POSITIVE, f64::MAX).is_err());
    assert!(
        nitrogen(f64::MAX, f64::MIN_POSITIVE)
            .column(4e14, 6e6)
            .is_err()
    );
}

#[test]
fn aerosol_can_set_the_extent_without_changing_gas_density_or_scale_height() {
    let aerosol = AerosolLayer::new(AerosolMaterial::OrganicHaze, 1e-8, 60_000.0, 1e-7).unwrap();
    assert_eq!(aerosol.material(), AerosolMaterial::OrganicHaze);
    assert_eq!(aerosol.reference_density_kg_m3(), 1e-8);
    assert_eq!(aerosol.particle_radius_m(), 1e-7);
    assert_eq!(aerosol.scale_height_m(), 60_000.0);
    let clear = nitrogen(ONE_BAR_PA, 280.0);
    let hazy = GasAtmosphere::new(
        *clear.composition(),
        ReferenceLevel::Surface,
        ONE_BAR_PA,
        280.0,
        Some(aerosol),
    )
    .unwrap();
    assert_eq!(hazy.aerosol(), Some(&aerosol));
    let clear = clear.column(4e14, 6e6).unwrap();
    let hazy = hazy.column(4e14, 6e6).unwrap();
    assert_eq!(hazy.scale_height_m, clear.scale_height_m);
    assert_eq!(hazy.density_kg_m3, clear.density_kg_m3);
    assert_eq!(hazy.extent_m, 750_000.0);
    assert_eq!(clear.validity, ColumnValidity::Thin);
    assert_eq!(hazy.validity, ColumnValidity::Extended);
}

#[test]
fn extended_gas_columns_are_flagged_without_silent_clamping() {
    let atmosphere = GasAtmosphere::new(
        mixture(&[(Gas::Hydrogen, 1.0)]),
        ReferenceLevel::Surface,
        ONE_BAR_PA,
        1000.0,
        None,
    )
    .unwrap();
    let column = atmosphere.column(4e14, 6e6).unwrap();
    assert_eq!(column.validity, ColumnValidity::Extended);
    assert!(column.extent_m > MAX_THIN_EXTENT_RATIO * 6e6);
    assert_eq!(
        column.extent_m,
        column
            .profile
            .height_for_drop(EXTENT_SCALE_HEIGHTS)
            .unwrap()
    );
    // Exercise both sides of the documented operational threshold.
    for (ratio, expected) in [
        (0.0999, ColumnValidity::Thin),
        (0.1001, ColumnValidity::Extended),
    ] {
        let radius = 6e6;
        let height = ratio * radius / ((1.0 + ratio) * EXTENT_SCALE_HEIGHTS);
        let gravity =
            BOLTZMANN_J_K * 1000.0 / atmosphere.composition().mean_molecular_mass_kg() / height;
        assert_eq!(
            atmosphere
                .column(gravity * radius * radius, radius)
                .unwrap()
                .validity,
            expected
        );
    }
}
