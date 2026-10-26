use super::*;

fn relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn residual_and_pressure_match_printed_iapws95_values() {
    // R6-95(2018), table 6. No third-party EOS implementation supplies these.
    let (value, derivative) = helmholtz::residual(838.025 / 322.0, 647.096 / 500.0);
    relative(value, -3.42693206, 2e-9);
    relative(derivative, -0.364366650, 2e-9);
    // Table 7. Private equation verification is not public state support.
    for (t, rho, pressure_mpa) in [
        (300.0, 996.556, 0.0992418352),
        (300.0, 1005.308, 20.0022515),
        (300.0, 1188.202, 700.004704),
        (500.0, 0.435, 0.0999679423),
        (500.0, 4.532, 0.999938125),
        (500.0, 838.025, 10.0003858),
        (500.0, 1084.564, 700.000405),
        (647.0, 358.0, 22.0384756),
        (900.0, 0.241, 0.100062559),
        (900.0, 52.615, 20.0000690),
        (900.0, 870.769, 700.000006),
    ] {
        relative(helmholtz::pressure_pa(rho, t), pressure_mpa * 1e6, 1e-8);
    }
    for (t, rho, pressure) in [
        (300.0, 1188.202, 700.004704e6),
        (500.0, 1084.564, 700.000405e6),
    ] {
        relative(state(pressure, t).unwrap().density_kg_m3, rho, 2e-9);
    }
}

#[test]
fn analytic_density_derivative_matches_independent_finite_differences() {
    for t in [273.16, 300.0, 400.0, 500.0, 623.15] {
        for rho in [800.0, 1000.0, 1200.0, 1400.0] {
            let delta = rho / 322.0;
            let tau = 647.096 / t;
            let h = 1e-4;
            let value = |x| helmholtz::residual(x, tau).0;
            // Independent five-point stencil checks the analytic derivative.
            let numerical = (value(delta - 2.0 * h) - 8.0 * value(delta - h)
                + 8.0 * value(delta + h)
                - value(delta + 2.0 * h))
                / (12.0 * h);
            let analytic = helmholtz::residual(delta, tau).1;
            assert!((analytic - numerical).abs() <= 2e-8 * analytic.abs().max(1.0));
        }
    }
}

#[test]
fn dense_liquid_mask_refuses_ice_coexistence_and_extrapolation() {
    for pressure in [f64::NAN, f64::INFINITY, -1.0, 0.0, 100e6 - 1.0, 1e9 + 1.0] {
        let e = state(pressure, 300.0).unwrap_err();
        assert_eq!(e.reason, Failure::PressureDomain);
        assert_eq!(e.pressure_pa.to_bits(), pressure.to_bits());
    }
    for t in [f64::NAN, f64::INFINITY, 250.0, 273.159, 623.151, 647.096] {
        let e = state(100e6, t).unwrap_err();
        assert_eq!(e.reason, Failure::TemperatureDomain);
        assert_eq!(e.temperature_k.to_bits(), t.to_bits());
    }
    for (t, branch) in [
        (273.16, melting::Branch::V),
        (273.31, melting::Branch::V),
        (280.0, melting::Branch::VI),
        (300.0, melting::Branch::VI),
    ] {
        let p = melting::pressure_pa(branch, t).unwrap();
        if p < MAX_PRESSURE_PA {
            assert_eq!(state(p, t).unwrap_err().reason, Failure::CoexistenceDensity);
            assert_eq!(state(p + 1.0, t).unwrap_err().reason, Failure::SolidDomain);
            assert_eq!(state(p - 1.0, t).unwrap().phase, Phase::Liquid);
        }
    }
    assert_eq!(state(1e9, 273.16).unwrap_err().reason, Failure::SolidDomain);
    assert!(state(100e6, 273.16).is_ok());
    assert!(state(1e9, 623.15).is_ok());
    assert_eq!(
        state_with_budget(700e6, 300.0, 0).unwrap_err().reason,
        Failure::EvaluationLimit
    );
}

#[test]
fn nested_work_is_exact_on_success_and_every_insufficient_budget() {
    let full = state_with_budget(700e6, 300.0, usize::MAX).unwrap();
    assert!(full.eos_evaluations > 3 && full.eos_evaluations <= MAX_EOS_EVALUATIONS);
    for budget in 0..full.eos_evaluations {
        let e = state_with_budget(700e6, 300.0, budget).unwrap_err();
        assert_eq!(e.reason, Failure::EvaluationLimit);
        assert_eq!(e.eos_evaluations, budget);
    }
    assert_eq!(
        state_with_budget(700e6, 300.0, full.eos_evaluations).unwrap(),
        full
    );
    assert_eq!(
        state_with_budget(1e9, 273.16, 0)
            .unwrap_err()
            .eos_evaluations,
        0
    );
}

#[test]
fn dense_states_close_pressure_and_compress_monotonically_without_if97_blending() {
    for t in [
        273.16, 273.31, 280.0, 300.0, 320.0, 355.0, 400.0, 500.0, 623.15,
    ] {
        let mut previous = None;
        for i in 0..=90 {
            let p = 100e6 + f64::from(i) * 10e6;
            match state(p, t) {
                Ok(s) => {
                    assert!(s.density_kg_m3.is_finite() && s.density_kg_m3 > 0.0);
                    assert_eq!(s, state(p, t).unwrap());
                    let residual = (helmholtz::pressure_pa(s.density_kg_m3, t) - p).abs();
                    assert!(residual <= (p * 1e-11).max(0.01));
                    if let Some(rho) = previous {
                        assert!(s.density_kg_m3 > rho);
                    }
                    previous = Some(s.density_kg_m3);
                }
                Err(e) => assert_eq!(e.reason, Failure::SolidDomain),
            }
        }
        // Models overlap at 100 MPa but are not forced to exactly coincide.
        let a = state(100e6, t).unwrap().density_kg_m3;
        let b = super::super::state(100e6, t).unwrap().density_kg_m3;
        assert_ne!(a, b);
        relative(a, b, 1e-4);
    }
}
