use super::*;

fn gas(parts: &[(Gas, f64)], pressure: f64, temperature: f64) -> GasAtmosphere {
    GasAtmosphere::new(
        Composition::new(parts).unwrap(),
        ReferenceLevel::Surface,
        pressure,
        temperature,
        None,
    )
    .unwrap()
}

#[test]
fn earth_clear_gas_matches_reference_scattering_without_inventing_ozone_or_haze() {
    let air = gas(
        &[(Gas::Nitrogen, 0.79), (Gas::Oxygen, 0.21)],
        101325.0,
        288.0,
    );
    let optics = air.optics(3.986e14, 6.371e6).unwrap();
    let reference = [5.802e-6, 13.558e-6, 33.1e-6];
    for (got, expected) in optics.rayleigh_m_inv.into_iter().zip(reference) {
        assert!((got / expected - 1.0).abs() < 0.08, "{got} vs {expected}");
    }
    assert_eq!(optics.gas_absorption_m_inv, [0.0; 3]);
    assert_eq!(optics.aerosol_scattering_m_inv, [0.0; 3]);
}

#[test]
fn mixtures_are_linear_and_coefficients_follow_number_density_not_mass_density() {
    let pure = |species, p, t| gas(&[(species, 1.0)], p, t).optics(4e14, 6e6).unwrap();
    let nitrogen = pure(Gas::Nitrogen, 1e5, 280.0);
    let co2 = pure(Gas::CarbonDioxide, 1e5, 280.0);
    let mixture = gas(
        &[(Gas::Nitrogen, 0.5), (Gas::CarbonDioxide, 0.5)],
        1e5,
        280.0,
    )
    .optics(4e14, 6e6)
    .unwrap();
    for c in 0..3 {
        assert!(
            (mixture.rayleigh_m_inv[c]
                / ((nitrogen.rayleigh_m_inv[c] + co2.rayleigh_m_inv[c]) / 2.0)
                - 1.0)
                .abs()
                < 1e-12
        );
        assert_eq!(
            pure(Gas::Nitrogen, 2e5, 280.0).rayleigh_m_inv[c],
            nitrogen.rayleigh_m_inv[c] * 2.0
        );
        assert_eq!(
            pure(Gas::Nitrogen, 1e5, 560.0).rayleigh_m_inv[c],
            nitrogen.rayleigh_m_inv[c] / 2.0
        );
    }
    // CO2 still scatters blue most strongly, with no orange absorber added.
    assert!(co2.rayleigh_m_inv[2] > co2.rayleigh_m_inv[0]);
    assert_eq!(co2.gas_absorption_m_inv, [0.0; 3]);
    assert!(pure(Gas::Hydrogen, 1e5, 280.0).rayleigh_m_inv[1] < nitrogen.rayleigh_m_inv[1]);
    let methane = pure(Gas::Methane, 1e5, 280.0);
    assert!(methane.gas_absorption_m_inv[0] > methane.gas_absorption_m_inv[1]);
}

#[test]
fn reference_columns_and_extended_estimates_are_preserved() {
    for (parts, pressure, t, gm, r) in [
        (
            vec![(Gas::CarbonDioxide, 1.0)],
            600.0,
            210.0,
            4.283e13,
            3.3895e6,
        ),
        (
            vec![(Gas::CarbonDioxide, 0.965), (Gas::Nitrogen, 0.035)],
            9.2e6,
            737.0,
            3.249e14,
            6.052e6,
        ),
        (
            vec![(Gas::Nitrogen, 0.95), (Gas::Methane, 0.05)],
            1.5e5,
            94.0,
            8.978e12,
            2.575e6,
        ),
        (
            vec![(Gas::Hydrogen, 0.86), (Gas::Helium, 0.14)],
            1e5,
            165.0,
            1.267e17,
            6.991e7,
        ),
    ] {
        let air = gas(&parts, pressure, t);
        let optics = air.optics(gm, r).unwrap();
        assert_eq!(optics.column, air.column(gm, r).unwrap());
        assert!(
            optics
                .rayleigh_m_inv
                .iter()
                .all(|v| v.is_finite() && *v > 0.0)
        );
    }
    let air = gas(&[(Gas::Hydrogen, 1.0)], 1e5, 500.0);
    let optics = air.optics(4e14, 6e6).unwrap();
    assert_eq!(optics.column.validity, ColumnValidity::Extended);
    assert_eq!(
        optics.column.extent_m,
        air.column(4e14, 6e6).unwrap().extent_m
    );
}

#[test]
fn prescribed_aerosol_tracks_mass_loading_and_particle_size_and_rejects_unsupported_sizes() {
    let clear = gas(&[(Gas::Nitrogen, 1.0)], 1e5, 280.0);
    let optics = |density, radius| {
        GasAtmosphere::new(
            *clear.composition(),
            ReferenceLevel::Surface,
            1e5,
            280.0,
            Some(
                AerosolLayer::new(AerosolMaterial::WaterDroplets, density, 1200.0, radius).unwrap(),
            ),
        )
        .unwrap()
        .optics(4e14, 6e6)
    };
    let base = optics(1e-8, 1e-6).unwrap();
    let double = optics(2e-8, 1e-6).unwrap();
    assert_eq!(
        double.aerosol_scattering_m_inv[0],
        base.aerosol_scattering_m_inv[0] * 2.0
    );
    assert_eq!(
        optics(1e-8, 2e-6).unwrap().aerosol_scattering_m_inv[0],
        base.aerosol_scattering_m_inv[0] / 2.0
    );
    assert!(optics(1e-8, 1e-8).is_err());
    assert!(base.aerosol_absorption_m_inv > 0.0);
}
