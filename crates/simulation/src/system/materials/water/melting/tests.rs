use super::*;

fn relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual / expected - 1.0).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

#[test]
fn all_branches_match_published_verification_pressures() {
    // R14-08(2011), table 3: independently printed reference values in MPa.
    // Tolerance follows the least precise printed value, not scientific accuracy.
    for (branch, temperature, pressure_mpa) in [
        (Branch::Ih, 260.0, 138.268),
        (Branch::III, 254.0, 268.685),
        (Branch::V, 265.0, 479.640),
        (Branch::VI, 320.0, 1356.76),
        (Branch::VII, 550.0, 6308.71),
    ] {
        relative(
            pressure_pa(branch, temperature).unwrap(),
            pressure_mpa * 1e6,
            4e-6,
        );
    }
}

#[test]
fn source_triple_points_are_not_artificially_stitched() {
    // Table 1. Source rounding produces small differences between an upper
    // endpoint and the next branch's pressure anchor; preserve both evaluations.
    for (left, right, temperature, pressure_mpa) in [
        (Branch::Ih, Branch::III, 251.165, 208.566),
        (Branch::III, Branch::V, 256.164, 350.1),
        (Branch::V, Branch::VI, 273.31, 632.4),
        (Branch::VI, Branch::VII, 355.0, 2216.0),
    ] {
        let a = pressure_pa(left, temperature).unwrap();
        let b = pressure_pa(right, temperature).unwrap();
        relative(a, pressure_mpa * 1e6, 4e-6);
        assert_eq!(b, pressure_mpa * 1e6);
        assert_ne!(a, b);
    }
    assert_eq!(pressure_pa(Branch::Ih, 273.16).unwrap(), 611.657);
    assert_eq!(Branch::Ih.relative_pressure_uncertainty(), 0.02);
    assert_eq!(Branch::III.relative_pressure_uncertainty(), 0.03);
    assert_eq!(Branch::V.relative_pressure_uncertainty(), 0.03);
    assert_eq!(Branch::VI.relative_pressure_uncertainty(), 0.03);
    assert_eq!(Branch::VII.relative_pressure_uncertainty(), 0.07);
}

#[test]
fn branches_remain_bounded_finite_monotone_and_deterministic() {
    for branch in [Branch::Ih, Branch::III, Branch::V, Branch::VI, Branch::VII] {
        let (min, max) = branch.temperature_bounds_k();
        for t in [
            min - 1e-6,
            max + 1e-6,
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let e = pressure_pa(branch, t).unwrap_err();
            assert_eq!(e.reason, Unsupported::TemperatureDomain);
            assert_eq!(e.pressure_pa, None);
            assert_eq!(e.temperature_k.to_bits(), t.to_bits());
        }
        let mut previous = pressure_pa(branch, min).unwrap();
        for n in 1..=1000 {
            let t = if n == 1000 {
                max
            } else {
                min + (max - min) * f64::from(n) / 1000.0
            };
            let p = pressure_pa(branch, t).unwrap();
            assert!(p.is_finite() && p > 0.0);
            assert_eq!(p, pressure_pa(branch, t).unwrap());
            if branch == Branch::Ih {
                assert!(p < previous);
            } else {
                assert!(p > previous);
            }
            previous = p;
        }
    }
}

#[test]
fn melting_knowledge_does_not_enable_high_pressure_density_or_phase_selection() {
    // At 260 K there are two different solid/liquid coexistence pressures.
    // Temperature alone cannot select a solid phase or a unique melting branch.
    assert!(pressure_pa(Branch::Ih, 260.0).unwrap() < pressure_pa(Branch::V, 260.0).unwrap());
    // The cold fixture lies below every melting branch, not in an invented Ih
    // continuation. Its deep solid/solid transitions need additional physics.
    for branch in [Branch::Ih, Branch::III, Branch::V, Branch::VI, Branch::VII] {
        assert!(pressure_pa(branch, 250.0).is_err());
    }
    let pressure = pressure_pa(Branch::VI, 300.0).unwrap();
    for result in [
        super::super::phase(pressure, 300.0).map(|_| ()),
        super::super::state(pressure, 300.0).map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().reason, Unsupported::PressureDomain);
    }
    for t in [251.165, 260.0, 270.0, 273.16] {
        assert_eq!(
            super::super::melting_pressure_ih_pa(t).unwrap(),
            pressure_pa(Branch::Ih, t).unwrap()
        );
    }
}
