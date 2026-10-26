#!/usr/bin/env python3
"""Offline regressions for reference parsing and diagnostics, no downloads."""

import hashlib
import importlib.util
import math
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "hhe_audit", Path(__file__).with_name("audit-hhe-reference.py")
)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class HheReferenceTests(unittest.TestCase):
    def test_hash_mismatch_is_not_an_allowed_source_update(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source"
            path.write_bytes(b"pinned")
            checksum = hashlib.sha256(b"pinned").hexdigest()
            self.assertEqual(audit.verify(path, checksum), checksum)
            path.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                audit.verify(path, checksum)

    def test_log_conversion_refuses_overflow_and_underflow(self):
        self.assertEqual(audit.decode_log(-3, 1000), 1)
        self.assertEqual(audit.decode_log(2, 1e6), 1e8)
        for value in (1000, -1000, float("nan"), float("inf")):
            self.assertIsNone(audit.decode_log(value, 1e6))

    def test_coordinate_converts_gpa_not_pa_or_cgs(self):
        self.assertEqual(audit.coordinate(3, -3), (20, 120))
        for point in [(99, -3), (3, 99), (3.01, -3), (3, -3.01)]:
            with self.subTest(point=point), self.assertRaises(ValueError):
                audit.coordinate(*point)

    @staticmethod
    def original_text():
        return "# pinned schema\n" + "\n".join(
            f"{2 + t * .05} {-9 + p * .05} -3 0 -1 -1 1 1 -1 .3"
            for t in range(3) for p in range(3)
        )

    def test_original_grid_requires_ten_columns_unique_complete_axes(self):
        text = self.original_text()
        with patch.object(audit, "SHAPE", (3, 3)):
            self.assertEqual(len(audit.original_grid(text)), 9)
            for broken in [text + "\n" + text.splitlines()[1],
                           "\n".join(text.splitlines()[:-1]),
                           text.replace("-3 0", "nan 0", 1),
                           text.replace("-3 0", "-3 0 99", 1)]:
                with self.subTest(text=broken), self.assertRaises(ValueError):
                    audit.original_grid(broken)

    def test_cgs_corrections_and_negative_values_are_preserved(self):
        text = "LOGP,LOGT,Vmix,Smix,Umix\n" + "\n".join(
            f"{1 + p * .05},{2 + t * .05},-2,3,4"
            for t in range(3) for p in range(3)
        )
        with tempfile.TemporaryDirectory() as directory, patch.object(audit, "SHAPE", (3, 3)):
            path = Path(directory) / "source.csv"
            path.write_text(text)
            rows = audit.correction_grid(path, energy=True)
            v, s, u = rows[(0, 0)]
            self.assertAlmostEqual(v, -0.002)
            self.assertAlmostEqual(s, 0.0003)
            self.assertEqual(u, 4)  # not converted without independently checked units
            for broken in [text.replace("Vmix", "wrong"), text + "\n1,2,-2,3,4",
                           text.replace("-2,3,4", "nan,3,4", 1),
                           text.replace("-2,3,4", "-2,3", 1),
                           text.replace("-2,3,4", "-2,3,4,5", 1)]:
                path.write_text(broken)
                with self.subTest(text=broken), self.assertRaises(ValueError):
                    audit.correction_grid(path, energy=True)

    def test_mixture_uses_specific_volume_and_preserves_endpoints(self):
        for y, expected in [(0, 2), (1, 4), (.5, 8 / 3)]:
            self.assertAlmostEqual(audit.mixture(2, 4, 0, y), expected)
        self.assertAlmostEqual(audit.mixture(2, 4, .1, .5), 2.5)
        for y in [-1, 2, float("nan")]:
            with self.assertRaises(ValueError):
                audit.mixture(2, 4, 0, y)
        with self.assertRaises(ValueError):
            audit.mixture(2, 4, -10, .5)
        with self.assertRaises(ValueError):
            audit.mixture(0, 4, 0, .5)

    @staticmethod
    def ideal_grid(gradient):
        # Independent ideal-gas entropy surface: ds=cp dlnT - R dlnP.
        rows = {}
        r, cp = 4000, 14000
        for ti in range(3):
            for pi in range(3):
                lt, lp = 3 + (ti - 1) * .05, -3 + (pi - 1) * .05
                t, p = 10 ** lt, 10 ** (lp + 9)
                s = 1e5 + cp * math.log(t / 1000) - r * math.log(p / 1e6)
                rows[(ti, pi)] = (lt, lp, math.log10(p / (r * t) / 1000),
                                  0, math.log10(s / 1e6), -1, 1,
                                  cp / s, -r / s, gradient)
        return rows

    def test_entropy_differences_recover_an_independent_ideal_adiabat(self):
        report = audit.grid_summary(self.ideal_grid(2 / 7))
        self.assertEqual(report["nonpositive_cp_entropy"], 0)
        self.assertEqual(report["nonpositive_cp_density_gradient"], 0)
        self.assertEqual(report["finite_difference_nodes"], 1)
        self.assertLess(report["gradient_relative_difference_min_median_p95_max"][-1], 1e-10)

    def test_clamped_gradient_is_reported_not_used_to_repair_entropy(self):
        report = audit.grid_summary(self.ideal_grid(.1))
        self.assertEqual(report["window_gradient_floor"], 9)
        self.assertGreater(report["gradient_relative_difference_min_median_p95_max"][-1], .6)

    def test_empty_diagnostics_are_not_claimed_zero_error(self):
        self.assertIsNone(audit.quantiles([]))

    def test_melting_screen_does_not_certify_outside_its_interval(self):
        helium = audit.melting_screen("He", 1e8, 60)
        self.assertEqual(helium["pure_melting_temperature_k"], 61)
        self.assertTrue(helium["below_pure_melting_curve"])
        self.assertTrue(audit.melting_screen("He", 1e11, 5011.872)["below_pure_melting_curve"])
        self.assertNotIn("below_pure_melting_curve", audit.melting_screen("H", 1e6, 1000))
        self.assertNotIn("below_pure_melting_curve", audit.melting_screen("He", 1e12, 20000))


if __name__ == "__main__":
    unittest.main()
