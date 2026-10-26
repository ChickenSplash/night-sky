#!/usr/bin/env python3
"""Synthetic offline regressions, no CoolProp installation or source downloads."""

import hashlib
import importlib.util
import math
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "secondary_audit", Path(__file__).with_name("audit-secondary-reference.py")
)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class SecondaryReferenceTests(unittest.TestCase):
    def test_hash_pin_rejects_changed_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fluid.json"
            path.write_bytes(b"pinned")
            pin = hashlib.sha256(b"pinned").hexdigest()
            self.assertEqual(audit.verify(path, pin), pin)
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                audit.verify(path, pin)

    def test_ideal_pressure_uses_molar_not_mass_density(self):
        eos = {"STATES": {"reducing": {"T": 100, "rhomolar": 10}},
               "gas_constant": 8, "alphar": []}
        p, g = audit.state(eos, 300, 5)
        self.assertEqual(p, 12000)
        self.assertAlmostEqual(g, math.log(.5) + 1)
        for t, r in [(0, 1), (1, -1), (float("nan"), 1), (1, float("inf"))]:
            with self.subTest(t=t, r=r), self.assertRaises(ValueError):
                audit.state(eos, t, r)

    @staticmethod
    def power(ell):
        return {"type": "ResidualHelmholtzPower", "n": [2], "d": [3], "t": [2], "l": [ell]}

    def test_power_zero_ell_means_no_exponential(self):
        a, d = audit.residual([self.power(0)], 2, 3)
        self.assertEqual(a, 144)
        self.assertEqual(d, 216)

    def test_damped_power_derivative(self):
        a, d = audit.residual([self.power(2)], 2, 3)
        self.assertAlmostEqual(a, 144 * math.exp(-4))
        self.assertAlmostEqual(d, a * (3 - 8) / 2)

    def test_gaussian_derivative_matches_independent_difference(self):
        block = {"type": "ResidualHelmholtzGaussian", "n": [.7], "d": [2], "t": [.5],
                 "eta": [1.2], "epsilon": [.8], "beta": [2], "gamma": [1.1]}
        self.check_derivative(block, 1.3, .9)

    def test_nonanalytic_derivative_on_both_sides_of_axis(self):
        block = {"type": "ResidualHelmholtzNonAnalytic", "n": [.5], "a": [3.5],
                 "b": [.85], "beta": [.3], "A": [.7], "B": [.3], "C": [10], "D": [20]}
        for delta in [.7, 1.3]:
            self.check_derivative(block, delta, .9)
        with self.assertRaisesRegex(ValueError, "axis"):
            audit.residual([block], 1, .9)

    def check_derivative(self, block, delta, tau):
        h = 1e-5
        a, derivative = audit.residual([block], delta, tau)
        left = audit.residual([block], delta - h, tau)[0]
        right = audit.residual([block], delta + h, tau)[0]
        self.assertTrue(math.isfinite(a))
        self.assertAlmostEqual(derivative, (right - left) / (2 * h), delta=1e-8)

    def test_unknown_malformed_and_nonfinite_terms_are_rejected(self):
        for block in [{"type": "unknown"}, {**self.power(0), "d": []},
                      {**self.power(0), "n": [float("nan")]},
                      {**self.power(0), "t": [1, 2]}]:
            with self.subTest(block=block), self.assertRaises(ValueError):
                audit.residual([block], 2, 3)
        for delta, tau in [(0, 1), (1, -1), (float("inf"), 1)]:
            with self.assertRaises(ValueError):
                audit.residual([], delta, tau)

    def test_ancillary_types_units_and_domain_are_explicit(self):
        curve = {"Tmin": 50, "Tmax": 100, "T_r": 100, "reducing_value": 1e6,
                 "n": [-2], "t": [1], "using_tau_r": True, "type": "pL"}
        self.assertAlmostEqual(audit.ancillary(curve, 50), 1e6 * math.exp(-2))
        for kind in ["pV", "rhoV"]:
            self.assertEqual(audit.ancillary({**curve, "type": kind}, 50),
                             audit.ancillary(curve, 50))
        liquid = {**curve, "type": "rhoLnoexp", "using_tau_r": False, "n": [2]}
        self.assertEqual(audit.ancillary(liquid, 50), 2e6)
        for t in [49, 101, float("nan")]:
            with self.assertRaises(ValueError):
                audit.ancillary(curve, t)
        with self.assertRaises(ValueError):
            audit.ancillary({**curve, "type": "unknown"}, 75)

    def test_three_distinct_melting_conventions(self):
        base = {"T_min": 10, "T_max": 20, "T_0": 10, "p_0": 100, "a": [2], "t": [2]}
        for kind, expected in [("polynomial_in_Tr", 700), ("polynomial_in_Theta", 300)]:
            curve = {"type": kind, "parts": [base]}
            self.assertEqual(audit.melting_pressure(curve, 10), 100)
            self.assertEqual(audit.melting_pressure(curve, 20), expected)
        curve = {"type": "Simon", "parts": [{**base, "a": 2, "c": 2}]}
        self.assertEqual(audit.melting_pressure(curve, 20), 106)
        with self.assertRaises(ValueError):
            audit.melting_pressure(curve, 21)
        with self.assertRaises(ValueError):
            audit.melting_pressure({"parts": []}, 10)

    def test_primary_temperature_limits_are_not_JSON_extrapolation_limits(self):
        self.assertEqual({k: v[1] for k, v in audit.SOURCES.items()},
                         {"CO2": 1100, "CH4": 625, "N2": 1000, "O2": 300, "Ar": 700})

    def test_additive_volume_is_not_mass_weighted_density(self):
        # Deliberately synthetic densities, not a physical water/methane state.
        volume, density = audit.additive_volume([.8, .2], [1000, 400])
        self.assertAlmostEqual(volume, .0013)
        self.assertAlmostEqual(density, 1 / .0013)
        self.assertNotAlmostEqual(density, .8 * 1000 + .2 * 400)
        self.assertEqual(audit.additive_volume([2], [400]), (.005, 400))

    def test_mixture_mass_scaling_order_and_absent_origin(self):
        masses, densities = [.8, .2, 1e-12], [1000, 400, 10]
        v, rho = audit.additive_volume(masses, densities)
        vr, rhor = audit.additive_volume(masses[::-1], densities[::-1])
        self.assertEqual((v, rho), (vr, rhor))
        scaled, scaled_rho = audit.additive_volume([m * 7 for m in masses], densities)
        self.assertAlmostEqual(scaled, 7 * v)
        self.assertAlmostEqual(scaled_rho, rho)
        self.assertEqual(audit.additive_volume([1, 0], [400, float("nan")]), (.0025, 400))
        self.assertGreater(v, audit.additive_volume(masses[:2], densities[:2])[0])

    def test_hhe_submixture_grouping_does_not_reapply_excess_volume(self):
        # Synthetic H/He specific volumes and excess term in m3/kg.
        x, y, z = .75, .25, .2
        vh, vhe, vex, heavy_density = .01, .002, -.001, 1000
        vhhe = x * vh + y * vhe + x * y * vex
        volume, _ = audit.additive_volume([1 - z, z], [1 / vhhe, heavy_density])
        expected = (1 - z) * vhhe + z / heavy_density
        self.assertAlmostEqual(volume, expected)
        # H/He fractions are normalised inside their block, not over heavy mass.
        wrong = (1 - z) * (x * vh + y * vhe) + ((1 - z) * x) * ((1 - z) * y) * vex + z / heavy_density
        self.assertNotAlmostEqual(volume, wrong)

    def test_mixture_rejects_missing_positive_origin_density_and_bad_inputs(self):
        for masses, densities in [([], []), ([0], [1]), ([1], []), ([-1], [1]),
                                  ([1], [0]), ([1], [float("nan")]),
                                  ([float("inf")], [1]), ([1, 1e-12], [1, -1])]:
            with self.subTest(masses=masses, densities=densities), self.assertRaises(ValueError):
                audit.additive_volume(masses, densities)

    def test_inventory_volume_lever_rule_endpoints_and_interior(self):
        vl, vv = 1 / 450, 1 / .5
        for q in [0, .25, .5, .75, 1]:
            v = (1 - q) * vl + q * vv
            self.assertAlmostEqual(audit.coexistence_quality(v, vl, vv), q)
        for v in [vl / 2, vv * 2, float("nan")]:
            with self.assertRaises(ValueError):
                audit.coexistence_quality(v, vl, vv)

    def test_small_or_reversed_contrast_is_unresolved_not_single_fluid(self):
        for vl, vv in [(1, 1), (1, 1 + 1e-7), (2, 1), (0, 1)]:
            with self.assertRaises(ValueError):
                audit.coexistence_quality(1, vl, vv)
        self.assertAlmostEqual(audit.coexistence_quality(1 + 1e-5, 1, 1 + 2e-5), .5)

    def test_poynting_bound_has_molar_units_and_detects_large_total_load(self):
        # Synthetic constant v_m makes this bound the exact log factor.
        self.assertEqual(audit.liquid_pressure_work(100, 4e-5, 1e5, 1e5, 8), 0)
        self.assertAlmostEqual(audit.liquid_pressure_work(100, 4e-5, 2e5, 1e5, 8), .005)
        self.assertGreater(audit.liquid_pressure_work(100, 4e-5, 1e9, 1e5, 8), 1)
        for t, vm, p, ps, r in [(0, 1, 1, 1, 1), (1, -1, 1, 1, 1),
                                (1, 1, float("nan"), 1, 1)]:
            with self.assertRaises(ValueError):
                audit.liquid_pressure_work(t, vm, p, ps, r)

    def test_quality_error_bound_contains_independent_endpoint_perturbations(self):
        v, vl, vv = 1.2, .2, 2.2
        errors = (1e-5, 2e-5, 3e-5)
        q = audit.coexistence_quality(v, vl, vv)
        bound = audit.quality_error_bound(v, vl, vv, errors)
        self.assertLess(bound, 1e-4)
        for dv in [-errors[0], errors[0]]:
            for dl in [-errors[1], errors[1]]:
                for dg in [-errors[2], errors[2]]:
                    perturbed = (v + dv - vl - dl) / (vv + dg - vl - dl)
                    self.assertLessEqual(abs(perturbed - q), bound)
        self.assertEqual(audit.quality_error_bound(v, vl, vv, (0, 0, 0)), 0)

    def test_quality_error_control_is_stricter_than_contrast_floor(self):
        v, vl, vv = 1 + 1e-5, 1, 1 + 2e-5
        audit.coexistence_quality(v, vl, vv)  # Contrast floor passes.
        self.assertGreater(audit.quality_error_bound(v, vl, vv, (1e-7, 1e-7, 1e-7)), 1e-4)
        for errors in [(0, 1, 1), (-1, 0, 0), (float("nan"), 0, 0), (0, 0)]:
            with self.subTest(errors=errors), self.assertRaises(ValueError):
                audit.quality_error_bound(v, vl, vv, errors)


if __name__ == "__main__":
    unittest.main()
