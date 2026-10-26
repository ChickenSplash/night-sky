#!/usr/bin/env python3
"""Synthetic numerical gates, no downloaded coefficients or external packages."""

import importlib.util
import math
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "secondary_inversion", Path(__file__).with_name("secondary-inversion.py"))
solver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver)


def eos(blocks):
    return {"STATES": {"reducing": {"T": 1, "rhomolar": 1}},
            "gas_constant": 1, "alphar": blocks}


def toy():
    # Synthetic polynomial fluid, not a methane density or physical phase mask.
    return eos([{"type": "ResidualHelmholtzPower", "n": [-2, .5],
                 "d": [1, 2], "t": [0, 0], "l": [0, 0]}])


BRANCHES = [(.01, .32), (1.01, 1.5)]
PRESSURES = (.08, .14)


class SecondaryInversionTests(unittest.TestCase):
    def test_ideal_units_and_analytic_derivative(self):
        model = eos([])
        model["gas_constant"] = 8
        p, g, derivative = solver.evaluate(model, 300, 5, solver.Work())
        self.assertEqual((p, derivative), (12000, 2400))
        self.assertAlmostEqual(g, math.log(5) + 1)
        density, state = solver.invert(model, 300, p, (1, 10), solver.Work())
        self.assertAlmostEqual(density, 5, delta=1e-11)
        self.assertAlmostEqual(state[0] / p, 1, delta=1e-9)

    def test_second_derivatives_against_independent_pressure_differences(self):
        power = {"type": "ResidualHelmholtzPower", "n": [.7], "d": [3], "t": [.5]}
        gaussian = {"type": "ResidualHelmholtzGaussian", "n": [.7], "d": [2],
                    "t": [.5], "eta": [1.2], "epsilon": [.8], "beta": [2], "gamma": [1.1]}
        for block in [{**power, "l": [0]}, {**power, "l": [2]}, gaussian]:
            model = eos([block])
            for n in [.7, 1.3]:
                h, t = 1e-5, 1.2
                derivative = solver.evaluate(model, t, n, solver.Work())[2]
                left = solver.reference.state(model, t, n - h)[0]
                right = solver.reference.state(model, t, n + h)[0]
                self.assertAlmostEqual(derivative, (right - left) / (2 * h), delta=1e-8)

    def test_unsupported_second_derivative_and_invalid_states_are_charged(self):
        for model, t, n in [(eos([{"type": "ResidualHelmholtzNonAnalytic"}]), 1, 1),
                            (eos([]), 0, 1), (eos([]), 1, -1),
                            (eos([]), float("nan"), 1), (eos([]), 1, float("inf"))]:
            work = solver.Work()
            with self.assertRaises(ValueError):
                solver.evaluate(model, t, n, work)
            self.assertEqual(work.evaluations, 1)

    def test_unstable_bracket_crossing_and_unbracketed_roots_fail(self):
        for bounds in [(.4, .8), (.1, 1.5), (.01, .02)]:
            with self.subTest(bounds=bounds), self.assertRaises(ValueError):
                solver.invert(toy(), 1, .1, bounds, solver.Work())
        for bounds in [(0, 1), (2, 1), (1, 1), (1, float("inf")), (1,)]:
            with self.assertRaises(ValueError):
                solver.invert(toy(), 1, .1, bounds, solver.Work())

    def test_two_roots_coexistence_refinement_and_gibbs_not_density_selection(self):
        p, vapor, liquid = solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work())
        fine = solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work(), tolerance=2.5e-10)
        self.assertAlmostEqual(p / fine[0], 1, delta=1e-8)
        self.assertLess(vapor[0], liquid[0])
        for root in [vapor, liquid]:
            self.assertGreater(root[1][2], 0)
            self.assertAlmostEqual(root[1][0] / p, 1, delta=1e-9)
        self.assertAlmostEqual(vapor[1][1], liquid[1][1], delta=1e-10)
        for offset, index in [(-1e-4, 0), (1e-4, 1)]:
            roots = solver.candidates(toy(), 1, p * (1 + offset), BRANCHES, solver.Work())
            self.assertEqual(solver.select(roots), roots[index])

    def test_repeated_and_reversed_branches_have_identical_results_and_work(self):
        work, reverse_work = solver.Work(), solver.Work()
        first = solver.coexistence(toy(), 1, PRESSURES, BRANCHES, work)
        reverse = solver.coexistence(toy(), 1, PRESSURES, BRANCHES[::-1], reverse_work)
        self.assertEqual(first, reverse)
        self.assertEqual(work, reverse_work)
        self.assertEqual(first, solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work()))

    def test_coexistence_needs_additional_closure_not_arbitrary_bulk_density(self):
        _, vapor, liquid = solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work())
        with self.assertRaises(solver.MissingClosure):
            solver.select([liquid, vapor])
        vl, vv = 1 / liquid[0], 1 / vapor[0]
        for q in [0, .25, .5, .75, 1]:
            volume = (1 - q) * vl + q * vv
            recovered = solver.reference.coexistence_quality(volume, vl, vv)
            self.assertAlmostEqual(recovered, q, delta=1e-12)
            self.assertEqual(recovered + (1 - recovered), 1)
        for volume in [vl * (1 - 1e-6), vv * (1 + 1e-6)]:
            with self.assertRaises(ValueError):
                solver.reference.coexistence_quality(volume, vl, vv)
        with self.assertRaises(ValueError):
            solver.reference.coexistence_quality(1, 1, 1 + 1e-7)

    def test_invalid_pressure_controls_and_missing_or_overlapping_branches(self):
        for pressure in [0, -1, float("nan"), float("inf")]:
            with self.assertRaises(ValueError):
                solver.invert(toy(), 1, pressure, BRANCHES[0], solver.Work())
        for branches in [[], [BRANCHES[0]], [(1, 2), (1.5, 3)]]:
            with self.assertRaises(ValueError):
                solver.coexistence(toy(), 1, PRESSURES, branches, solver.Work())
        for tolerance in [0, 1e-14, 1e-5, float("nan")]:
            with self.assertRaises(ValueError):
                solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work(), tolerance=tolerance)
        for iterations in [-1, 97, .5]:
            with self.assertRaises(ValueError):
                solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work(), iterations=iterations)
        with self.assertRaises(ValueError):
            solver.coexistence(toy(), 1, (.08, .09), BRANCHES, solver.Work())
        with self.assertRaises(ValueError):
            solver.select([])

    def test_shared_nested_exact_budget_and_iteration_failures(self):
        work = solver.Work()
        result = solver.coexistence(toy(), 1, PRESSURES, BRANCHES, work)
        exact = solver.Work(limit=work.evaluations)
        self.assertEqual(result, solver.coexistence(toy(), 1, PRESSURES, BRANCHES, exact))
        self.assertEqual(exact.evaluations, work.evaluations)
        short = solver.Work(limit=work.evaluations - 1)
        with self.assertRaises(solver.WorkExhausted):
            solver.coexistence(toy(), 1, PRESSURES, BRANCHES, short)
        self.assertEqual(short.evaluations, short.limit)
        with self.assertRaises(solver.WorkExhausted):
            solver.invert(toy(), 1, .1, BRANCHES[0], solver.Work(limit=0))
        for limit in [-1, 50001, 1.5]:
            with self.assertRaises(ValueError):
                solver.evaluate(toy(), 1, .1, solver.Work(limit=limit))
        with self.assertRaisesRegex(solver.ResolutionError, "density iteration"):
            solver.invert(toy(), 1, .1, BRANCHES[0], solver.Work(), iterations=0)
        with self.assertRaisesRegex(solver.ResolutionError, "coexistence iteration"):
            solver.coexistence(toy(), 1, PRESSURES, BRANCHES, solver.Work(), iterations=0)

    def test_finite_window_is_not_permission_to_extrapolate_freezing_curve(self):
        fluid = {"ANCILLARIES": {"melting_line": {"type": "Simon", "parts": [
            {"T_min": 90, "T_max": 600, "T_0": 90, "p_0": 1e4, "a": 2e8, "c": 1.7}]}}}
        solver.methane_window(fluid, 100, 1e5)
        for t, p in [(94, 1e5), (173, 1e5), (100, 9999), (100, 3e6 + 1),
                     (float("nan"), 1e5), (100, float("inf"))]:
            with self.assertRaises(ValueError):
                solver.methane_window(fluid, t, p)
        fluid["ANCILLARIES"]["melting_line"]["parts"][0]["a"] = 1
        with self.assertRaisesRegex(ValueError, "freezing"):
            solver.methane_window(fluid, 100, 1e5)


if __name__ == "__main__":
    unittest.main()
