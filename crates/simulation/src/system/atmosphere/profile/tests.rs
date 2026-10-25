use super::*;

#[test]
fn spherical_profile_obeys_hydrostatic_balance_and_pressure_inversion() {
    let profile = Profile::new(1.5e6, 30_000.0, 200.0);
    assert!(profile.spherical);
    for height in [0.0, 30_000.0, 400_000.0, 1e6] {
        let derivative = (profile.log_relative_density(height + 0.1)
            - profile.log_relative_density(height - 0.1))
            / 0.2;
        let expected = -1.0 / (profile.scale_height_m * (1.0 + height / profile.radius_m).powi(2));
        assert!((derivative / expected - 1.0).abs() < 1e-7);
        let drop = -profile.log_relative_density(height);
        assert!((profile.height_for_drop(drop).unwrap() - height).abs() < 1e-8);
    }
    assert!(profile.height_for_drop(50.0).is_none());
}

#[test]
fn finite_shell_mass_matches_independent_radial_quadrature() {
    for height in [0.001, 100.0, 10_000.0, 80_000.0] {
        let radius: f64 = 1.5e6;
        let gm = 4e12;
        let c2 = height * gm / radius.powi(2);
        let mass = 1e15;
        let pressure = Profile::boundary_pressure(mass, gm, radius, c2);
        let profile = Profile::new(radius, height, pressure);
        assert!(profile.spherical);
        let end = profile
            .height_for_drop(Profile::cutoff_drop(pressure))
            .unwrap();
        let dh = end / 100_000.0;
        let integral: f64 = (0..100_000)
            .map(|i| {
                let h = (i as f64 + 0.5) * dh;
                (radius + h).powi(2) * profile.log_relative_density(h).exp() * dh
            })
            .sum();
        let integrated = 4.0 * std::f64::consts::PI * pressure / c2 * integral;
        assert!(
            (integrated / mass - 1.0).abs() < 1e-6,
            "{integrated} vs {mass}"
        );
    }
}

#[test]
fn thin_limit_and_weak_binding_are_explicit() {
    let thin = Profile::new(1e9, 1000.0, 1e5);
    assert!((thin.mass_factor(1e5) - 1.0).abs() < 1e-5);
    let weak = Profile::new(1e6, 1e5, 1e5);
    assert!(!weak.spherical);
    assert_eq!(weak.mass_factor(1e5), 1.0);
    assert_eq!(weak.height_for_drop(12.5), Some(1.25e6));
}
