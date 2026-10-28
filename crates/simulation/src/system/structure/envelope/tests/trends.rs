//! Plausibility experiments, not observational radius-accuracy claims.
use super::*;

#[test]
#[ignore = "local pinned source resource; run in release"]
fn source_backed_mass_heating_and_stripping_trends() {
    let run = |core, gas, equilibrium, intrinsic| {
        let request = inventory(core, gas);
        let (interior, envelope) = solve(
            &request,
            &Inventory::default(),
            equilibrium,
            intrinsic,
            Default::default(),
        )
        .unwrap_or_else(|e| {
            panic!("core={core} gas={gas} eq={equilibrium} int={intrinsic}: {e:?}")
        });
        let mass = interior
            .retained_inventory()
            .checked_add(&envelope.inventory)
            .unwrap();
        for m in Material::ALL {
            assert!((mass.mass_kg(m) - request.mass_kg(m)).abs() <= 1e-10 * request.mass_kg(m));
        }
        let inertia = envelope.inertia_kg_m2
            / (request.total_mass_kg() * envelope.reference_radius_m.powi(2));
        assert!(inertia.is_finite() && inertia > 0.0);
        assert!(envelope.gravity_residual().abs() < 1e-8);
        eprintln!(
            "TREND core={core} gas={gas} eq={equilibrium} int={intrinsic} radius_km={} condensed_km={} inertia={inertia} base_bar={} one_bar={}",
            envelope.reference_radius_m / 1e3,
            interior.radius_m() / 1e3,
            envelope.reference_pressure_pa() / 1e5,
            envelope.one_bar_radius_m().is_some()
        );
        (interior, envelope)
    };
    // More heavy mass at fixed small gas fraction increases radius and inertia.
    let mut previous = (0.0, 0.0);
    for core in [1.0, 3.0, 5.0, 10.0] {
        let (_, e) = run(core, core * 0.001, 350.0, 100.0);
        assert!(e.reference_radius_m > previous.0 && e.inertia_kg_m2 > previous.1);
        previous = (e.reference_radius_m, e.inertia_kg_m2);
    }
    // Heating a fixed inventory expands the envelope, not its cold-EOS rock.
    for intrinsic_sweep in [false, true] {
        let mut previous = 0.0;
        let values: &[f64] = if intrinsic_sweep {
            &[50.0, 100.0, 150.0, 200.0]
        } else {
            &[250.0, 350.0, 500.0, 750.0]
        };
        for &value in values {
            let (_, e) = run(
                5.0,
                0.01,
                if intrinsic_sweep { 350.0 } else { value },
                if intrinsic_sweep { value } else { 100.0 },
            );
            assert!(e.reference_radius_m > previous);
            previous = e.reference_radius_m;
        }
    }
    // Removing gas crosses the one-bar reference into a sub-bar condensed
    // reference. Compare the limiting radius AND dimensional inertia to bare rock.
    let bare = dry::solve(&inventory(5.0, 0.0), 0.0, Default::default()).unwrap();
    let mut previous = f64::INFINITY;
    for gas in [0.1, 0.01, 0.001, 1e-5, 1e-7] {
        let (_, e) = run(5.0, gas, 350.0, 100.0);
        assert!(e.reference_radius_m < previous);
        previous = e.reference_radius_m;
        if gas == 1e-7 {
            assert!(e.one_bar_radius_m().is_none());
            assert!((e.reference_radius_m / bare.radius_m() - 1.0).abs() < 1e-5);
            assert!((e.inertia_kg_m2 / bare.inertia_kg_m2() - 1.0).abs() < 1e-5);
        }
    }
}
