use super::*;
use std::cell::Cell;

#[test]
fn trial_recovers_an_invalid_stage_without_losing_mass_or_inertia() {
    let inventory = Inventory::new(&[(Material::Hydrogen, 1e24)]).unwrap();
    let calls = Cell::new(0);
    let density = |_, _, _, _: &mut Work, _| {
        calls.set(calls.get() + 1);
        Ok(if calls.get() == 4 { 1e-100 } else { 4100.0 })
    };
    let mut solver = Solver::new(
        &inventory,
        0.0,
        Controls::default(),
        &density,
        Policy {
            boundary_tolerance_pa: Some(1.0),
            ..Default::default()
        },
    );
    let trial = solver.trial(1e12).unwrap().unwrap();
    assert_eq!(trial.regions.len(), 1);
    assert_eq!(trial.regions[0].inventory, inventory);
    let radius = (3.0 * inventory.total_mass_kg() / (4.0 * PI * 4100.0)).cbrt();
    assert!((trial.regions[0].outer().radius_m / radius - 1.0).abs() < 1e-10);
    assert!(
        (trial.inertia_kg_m2 / (0.4 * inventory.total_mass_kg() * radius.powi(2)) - 1.0).abs()
            < 1e-10
    );
}

#[test]
fn invalid_intermediate_volume_rejects_only_the_step() {
    let inventory = Inventory::new(&[(Material::Hydrogen, 1e24)]).unwrap();
    let calls = Cell::new(0);
    let density = |_, _, _, _: &mut Work, _| {
        calls.set(calls.get() + 1);
        // Force an over-large RK stage without ever giving the EOS an invalid
        // input. The following negative RK coefficient overshoots the volume.
        Ok(if calls.get() == 2 { 1e-9 } else { 4100.0 })
    };
    let policy = Policy {
        boundary_tolerance_pa: Some(1.0),
        ..Default::default()
    };
    let mut solver = Solver::new(&inventory, 0.0, Controls::default(), &density, policy);
    let start = [1.0, 10.0, 0.2];
    assert!(matches!(
        solver.step(Material::Hydrogen, 0.9, start, 0.01),
        Err(StepError::InvalidState)
    ));
    let retried = solver
        .step(Material::Hydrogen, 0.9, start, 0.0025)
        .unwrap()
        .unwrap();
    let constant = |_, _, _, _: &mut Work, _| Ok(4100.0);
    let mut reference = Solver::new(
        &inventory,
        0.0,
        Controls::default(),
        &constant,
        Policy::default(),
    );
    let expected = reference
        .step(Material::Hydrogen, 0.9, start, 0.0025)
        .unwrap()
        .unwrap();
    assert_eq!(retried.state, expected.state);
    assert_eq!(retried.error, expected.error);
    assert!(solver.work.eos_evaluations > reference.work.eos_evaluations);
}

#[test]
fn invalid_stages_do_not_probe_eos_or_become_low_pressure_trials() {
    let inventory = Inventory::new(&[(Material::Hydrogen, 1e24)]).unwrap();
    let density = |_, _, _, _: &mut Work, _| panic!("invalid stage reached EOS");
    let mut solver = Solver::new(
        &inventory,
        0.0,
        Controls::default(),
        &density,
        Policy {
            boundary_tolerance_pa: Some(1.0),
            ..Default::default()
        },
    );
    for state in [
        [-1.0, 1.0, 0.0],
        [1.0, f64::NAN, 0.0],
        [1.0, 1.0, f64::INFINITY],
    ] {
        assert!(matches!(
            solver.step(Material::Hydrogen, 0.9, state, 0.01),
            Err(StepError::InvalidState)
        ));
    }
    assert_eq!(solver.work.eos_evaluations, 0);
}

#[test]
fn eos_errors_and_work_limits_are_not_retried() {
    let inventory = Inventory::new(&[(Material::Hydrogen, 1e24)]).unwrap();
    let error = SolveError::InvalidInput("test EOS domain");
    let density = |_, _, _, _: &mut Work, _| Err(error);
    for ceiling in [0, 1] {
        let mut solver = Solver::new(
            &inventory,
            0.0,
            Controls {
                max_eos_evaluations: ceiling,
                ..Default::default()
            },
            &density,
            Policy::default(),
        );
        let expected = if ceiling == 0 {
            SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations)
        } else {
            error
        };
        assert!(matches!(
            solver.step(Material::Hydrogen, 0.9, [1.0, 10.0, 0.2], 0.01),
            Err(StepError::Solve(e)) if e == expected
        ));
        assert_eq!(solver.work.eos_evaluations, ceiling);
    }
}

#[test]
fn persistent_invalid_stages_fail_numerically_within_existing_budgets() {
    let inventory = Inventory::new(&[(Material::Hydrogen, 1e24)]).unwrap();
    for max_steps in [1, Controls::default().max_steps_per_trial] {
        let calls = Cell::new(0);
        let density = |_, _, _, _: &mut Work, _| {
            calls.set(calls.get() + 1);
            // Inject a persistent non-finite RK stage after the regular centre.
            Ok(if calls.get() >= 4 { f64::NAN } else { 4100.0 })
        };
        let mut solver = Solver::new(
            &inventory,
            0.0,
            Controls {
                max_steps_per_trial: max_steps,
                ..Default::default()
            },
            &density,
            Policy {
                boundary_tolerance_pa: Some(1.0),
                ..Default::default()
            },
        );
        let result = solver.trial(1e12);
        if max_steps == 1 {
            assert!(matches!(
                result,
                Err(SolveError::WorkBudgetExhausted(WorkLimit::Integration))
            ));
        } else {
            assert!(matches!(
                result,
                Err(SolveError::NonConvergence {
                    material: Material::Hydrogen,
                    ..
                })
            ));
        }
        assert!(solver.work.attempted_steps <= max_steps);
        assert!(solver.work.eos_evaluations <= solver.controls.max_eos_evaluations);
    }
}
