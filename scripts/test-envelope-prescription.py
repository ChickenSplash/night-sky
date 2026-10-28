#!/usr/bin/env python3
"""Synthetic regression checks; no downloaded tables or network needed."""

import importlib.util
import math
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "envelope_probe", Path(__file__).with_name("probe-envelope-prescription.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ThermalTests(unittest.TestCase):
    def test_join_value_and_slope(self):
        for teq in (20, 250, 1074, 2000):
            p = probe.thermal(1e5, teq, 50, 10)["boundary_pressure_pa"]
            lo = probe.thermal(p * (1 - 1e-7), teq, 50, 10)
            hi = probe.thermal(p * (1 + 1e-7), teq, 50, 10)
            self.assertAlmostEqual(hi["temperature_k"] / lo["temperature_k"], 1, places=6)
            self.assertAlmostEqual(lo["gradient"], 2 / 7, places=6)
            self.assertEqual(hi["gradient"], 2 / 7)

    def test_analytic_gradient_and_monotonicity(self):
        for p in (1, 1e5, 1e7, 1e9, 1e12):
            state = probe.thermal(p, 250, 50, 10)
            lo = probe.thermal(p * (1 - 1e-5), 250, 50, 10)["temperature_k"]
            hi = probe.thermal(p * (1 + 1e-5), 250, 50, 10)["temperature_k"]
            derivative = math.log(hi / lo) / math.log((1 + 1e-5) / (1 - 1e-5))
            self.assertAlmostEqual(derivative, state["gradient"], places=8)
            self.assertLessEqual(state["gradient"], 2 / 7)
            self.assertGreaterEqual(hi, lo)

    def test_internal_heat_sensitivity(self):
        for teq in (20, 250, 1074):
            cold = probe.thermal(1e11, teq, 30, 10)["temperature_k"]
            hot = probe.thermal(1e11, teq, 200, 10)["temperature_k"]
            self.assertGreater(hot, cold)

    def test_invalid_inputs(self):
        for value in (0, -1, math.inf, math.nan):
            for index in range(5):
                args = [1e5, 250, 50, 10, 0.001]
                args[index] = value
                with self.assertRaises(ValueError):
                    probe.thermal(*args)
        for args in ((1e5, 19, 50, 10), (1e5, 250, 201, 10), (1e15, 250, 50, 10)):
            with self.assertRaises(ValueError):
                probe.thermal(*args)

    def test_synthetic_density_interpolation_and_pure_limits(self):
        # Use an exact power-law grid to test interpolation without real data.
        ref = probe.DensityReference.__new__(probe.DensityReference)
        ref.audit = probe.reference_module()
        grid = {}
        correction = {}
        for i in range(4):
            for j in range(40, 44):
                row = [0.] * 10
                row[2] = 0.05 * j - (2 + 0.05 * i)
                grid[i, j] = row
                correction[i, j] = (0., 0.)
        ref.grids = {"H": grid, "He": grid}
        ref.correction = correction
        p, t = 10 ** 2.075, 10 ** 2.075
        for y in (0, 0.25, 1):
            self.assertAlmostEqual(ref.density(p, t, y)["density_kg_m3"], 1000)
        with self.assertRaises(ValueError):
            ref.density(1e5, 99)

    def test_signed_mixture_correction_is_applied_once(self):
        ref = probe.DensityReference.__new__(probe.DensityReference)
        ref.audit = probe.reference_module()
        ref.grids = {}
        for label, rho in (("H", 100), ("He", 500)):
            ref.grids[label] = {}
            for i in range(4):
                for j in range(40, 44):
                    row = [0.] * 10
                    row[2] = math.log10(rho / 1000)
                    row[6] = 1
                    ref.grids[label][i, j] = row
        ref.correction = {(i, j): (-0.001, 0.) for i in range(4) for j in range(40, 44)}
        p, t = 10 ** 2.075, 10 ** 2.075
        self.assertAlmostEqual(ref.density(p, t, 0)["density_kg_m3"], 100)
        self.assertAlmostEqual(ref.density(p, t, 1)["density_kg_m3"], 500)
        self.assertAlmostEqual(ref.density(p, t, 0.25)["density_kg_m3"], 128)
        self.assertEqual(ref.density(p, t)["physical_validity"], "not established by this probe")


if __name__ == "__main__":
    unittest.main()
