#!/usr/bin/env python3
"""Offline regressions for the narrow reference audit, no source downloads."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "ice_vi_audit", Path(__file__).with_name("audit-ice-vi-reference.py")
)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class IceViReferenceTests(unittest.TestCase):
    def test_measurement_precision(self):
        self.assertEqual(audit.measured("215.818(19)"), (215.818, 0.019))
        self.assertEqual(audit.measured("293(2)"), (293, 2))
        self.assertEqual(audit.measured("10"), (10, None))
        for value in ("?", ".", "nan", "inf", "0", "-1", "293(2)junk"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                audit.measured(value)

    def test_embedded_refinement_is_not_an_experiment(self):
        blocks = audit.scalars("""data_H2O_home_IAM
_cell_volume 215.818(19)
_chemical_formula_sum 'H2 O'
_iucr_refine_fcf_details
;
data_woda_IIIb
_cell_volume 999
;
data_D2O_home_HAR
_chemical_formula_sum 'H2 O'
""")
        self.assertEqual(set(blocks), {"H2O_home_IAM", "D2O_home_HAR"})
        self.assertEqual(blocks["H2O_home_IAM"]["_cell_volume"], "215.818(19)")
        # The formula is not a trustworthy isotope discriminator in this source.
        self.assertEqual(blocks["D2O_home_HAR"]["_chemical_formula_sum"], "H2 O")

    def test_duplicate_and_malformed_fields_are_refused(self):
        for text in (
            "data_a\ndata_a\n",
            "data_a\n_cell_volume 1\n_cell_volume 2\n",
            "data_a\n_cell_volume\n",
            "data_a\n;\nunclosed text\n",
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                audit.scalars(text)

    def test_wrong_source_fails_before_parsing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "wrong.cif"
            path.write_text("data_not_the_reference")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                audit.audit(path, path)

    def test_full_report_excludes_isotopes_and_duplicate_refinements(self):
        # Synthetic lattice, not a substitute for the separately pinned source.
        fields = """
_chemical_formula_sum 'H2 O'
_cell_length_a 6
_cell_length_b 6
_cell_length_c 6
_cell_angle_alpha 90
_cell_angle_beta 90
_cell_angle_gamma 90
_cell_volume 216.000(20)
_cell_formula_units_Z 10
_cell_measurement_temperature 293(2)
_exptl_crystal_density_diffrn 1.385
"""
        text = "".join(f"data_{name}\n{fields}" for name in sorted(audit.EXPECTED_BLOCKS))
        with patch.object(audit, "pinned_bytes", side_effect=[text.encode(), b"pinned PDF"]):
            report = audit.audit(Path("source.cif"), Path("source.pdf"))
        self.assertEqual(report["independent_h2o_observations"], 1)
        self.assertEqual(len(report["records"]), 11)
        isotopes = [row["sample_isotope_from_publication"] for row in report["records"]]
        self.assertEqual(isotopes.count("H2O"), 2)
        self.assertEqual(isotopes.count("D2O"), 7)
        self.assertEqual(isotopes.count("mixed"), 2)
        observation = report["h2o_observation"]
        self.assertEqual(observation["temperature_k"], 293)
        self.assertEqual(observation["temperature_standard_uncertainty_k"], 2)
        # Independent 60-digit Decimal evaluation of 10 M / (N_A * 216 Å³).
        self.assertAlmostEqual(observation["density_kg_m3"], 1384.9572336146137, places=8)
        self.assertAlmostEqual(
            observation["density_volume_only_standard_uncertainty_kg_m3"],
            observation["density_kg_m3"] * 0.020 / 216, places=12,
        )

        broken = text.replace("_cell_volume 216.000(20)", "_cell_volume 999.000(20)", 1)
        with patch.object(audit, "pinned_bytes", side_effect=[broken.encode(), b"pinned PDF"]):
            with self.assertRaisesRegex(ValueError, "inconsistent lattice volume"):
                audit.audit(Path("source.cif"), Path("source.pdf"))


if __name__ == "__main__":
    unittest.main()
