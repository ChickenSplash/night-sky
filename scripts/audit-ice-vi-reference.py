#!/usr/bin/env python3
"""Audit an independent ice-VI reference, not a runtime EOS or phase selector.

Chodkiewicz et al. (2022), doi:10.1107/S2052252522006662, CC-BY-4.0.
Reads separately downloaded, hash-pinned supplementary CIF and PDF files.
No network access, third-party dependencies, temperature correction or EOS fit.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import shlex


CIF_SHA256 = "738b41d8d101c5d46f138fd8124eb829440f7397227adcfde0dd7325e01ebe5a"
PDF_SHA256 = "813c08809c7771f7d37ae654a82971e30a2c2642c1d3603ac7c659039b0a9d1c"
MOLAR_MASS_H2O_KG_MOL = 0.01801528
AVOGADRO_PER_MOL = 6.02214076e23
EXPECTED_BLOCKS = {
    "D2O_APS_HAR_mod1", "D2O_APS_HAR", "D2O_APS_IAM",
    "D2O_DESY_HAR", "D2O_DESY_IAM", "D2O_home_HAR", "D2O_home_IAM",
    "H2O_D2O_home_HAR", "H2O_D2O_home_IAM", "H2O_home_HAR", "H2O_home_IAM",
}
TAGS = {
    "_chemical_formula_sum", "_cell_volume", "_cell_formula_units_Z",
    "_cell_length_a", "_cell_length_b", "_cell_length_c",
    "_cell_angle_alpha", "_cell_angle_beta", "_cell_angle_gamma",
    "_cell_measurement_temperature", "_exptl_crystal_density_diffrn",
}


def pinned_bytes(path, checksum):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError(f"{path.name}: SHA-256 differs from pinned supplement")
    return data


def scalars(text):
    """Extract only the pinned file's single-line scalar fields, not general CIF.

    Semicolon-delimited text includes an embedded data_woda_IIIb block. It must
    not become another experiment or override the enclosing block's scalars.
    """
    blocks = {}
    current = None
    in_text = False
    for line in text.splitlines():
        if line.startswith(";"):
            in_text = not in_text
            continue
        if in_text:
            continue
        if line.startswith("data_"):
            name = line[5:].strip()
            if name in blocks:
                raise ValueError(f"duplicate data block: {name}")
            current = blocks[name] = {}
        elif line.startswith("_") and current is not None:
            parts = shlex.split(line, comments=True)
            if parts[0] in TAGS:
                if len(parts) != 2 or parts[0] in current:
                    raise ValueError(f"unexpected scalar: {line}")
                current[parts[0]] = parts[1]
    if in_text:
        raise ValueError("unterminated CIF text field")
    return blocks


def measured(value):
    """Central value and parenthesised standard uncertainty, retaining precision."""
    match = re.fullmatch(r"(\d+(?:\.\d+)?)(?:\((\d+)\))?", value)
    if match is None:
        raise ValueError(f"invalid measurement: {value}")
    central = float(match[1])
    digits = len(match[1].partition(".")[2])
    uncertainty = int(match[2]) * 10.0 ** -digits if match[2] else None
    if not math.isfinite(central) or central <= 0:
        raise ValueError(f"nonpositive/nonfinite measurement: {value}")
    return central, uncertainty


def audit(cif_path, pdf_path):
    blocks = scalars(pinned_bytes(cif_path, CIF_SHA256).decode("utf-8"))
    pinned_bytes(pdf_path, PDF_SHA256)
    if blocks.keys() != EXPECTED_BLOCKS:
        raise ValueError("unexpected CIF data blocks")
    records = []
    for name, fields in blocks.items():
        if fields.keys() != TAGS:
            raise ValueError(f"unexpected scalar schema: {name}")
        volume, volume_uncertainty = measured(fields["_cell_volume"])
        a, b, c = (measured(fields[f"_cell_length_{axis}"])[0] for axis in "abc")
        if any(measured(fields[f"_cell_angle_{angle}"])[0] != 90
               for angle in ("alpha", "beta", "gamma")):
            raise ValueError(f"non-orthogonal cell: {name}")
        if abs(a * b * c / volume - 1) > 5e-5:
            raise ValueError(f"inconsistent lattice volume: {name}")
        z = measured(fields["_cell_formula_units_Z"])[0]
        if z != 10:
            raise ValueError(f"unexpected formula units: {name}")
        temperature, temperature_uncertainty = measured(fields["_cell_measurement_temperature"])
        # This is deliberately labelled H2O-equivalent for excluded isotope samples.
        # It audits the deposited density calculation, NOT their physical density.
        h2o_equivalent_density = z * MOLAR_MASS_H2O_KG_MOL / (AVOGADRO_PER_MOL * volume * 1e-30)
        deposited_density = measured(fields["_exptl_crystal_density_diffrn"])[0] * 1000
        if abs(h2o_equivalent_density - deposited_density) > 0.51:
            raise ValueError(f"deposited density is not the rounded H2O-equivalent value: {name}")
        records.append({
            "block": name,
            "sample_isotope_from_publication": (
                "mixed" if name.startswith("H2O_D2O_") else
                "D2O" if name.startswith("D2O_") else "H2O"
            ),
            "deposited_formula": fields["_chemical_formula_sum"],
            "cell_volume_angstrom3": volume,
            "cell_volume_standard_uncertainty_angstrom3": volume_uncertainty,
            "temperature_k": temperature,
            "temperature_standard_uncertainty_k": temperature_uncertainty,
            "deposited_density_kg_m3": deposited_density,
            "h2o_equivalent_density_kg_m3": h2o_equivalent_density,
        })
    pure = [row for row in records if row["sample_isotope_from_publication"] == "H2O"]
    if len(pure) != 2 or pure[0]["cell_volume_angstrom3"] != pure[1]["cell_volume_angstrom3"]:
        raise ValueError("unexpected pure-H2O refinement pair")
    observation = pure[0]
    return {
        "source_doi": "10.1107/S2052252522006662",
        "license": "CC-BY-4.0",
        "cif_sha256": CIF_SHA256,
        "supporting_pdf_sha256": PDF_SHA256,
        "records": records,
        "independent_h2o_observations": 1,
        "h2o_observation": {
            "pressure_pa": 1.5e9,
            "pressure_uncertainty_pa": 0.1e9,
            "pressure_source": "supporting information, table 1Sa, H2O column",
            "temperature_k": observation["temperature_k"],
            "temperature_standard_uncertainty_k": observation["temperature_standard_uncertainty_k"],
            "density_kg_m3": observation["h2o_equivalent_density_kg_m3"],
            "density_volume_only_standard_uncertainty_kg_m3": (
                observation["h2o_equivalent_density_kg_m3"]
                * observation["cell_volume_standard_uncertainty_angstrom3"]
                / observation["cell_volume_angstrom3"]
            ),
        },
        "limitations": [
            "HAR and IAM are refinements of one measurement, not independent observations",
            "all deposited formulas and densities use H2O mass, including D2O/mixed samples",
            "volume-only density uncertainty excludes P/T and systematic uncertainty",
            "293 K is not 300 K; no temperature correction or model validation is performed",
            "one density point supplies neither an EOS nor a VI/VII equilibrium boundary",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cif", type=Path)
    parser.add_argument("supporting_pdf", type=Path)
    args = parser.parse_args()
    try:
        result = audit(args.cif, args.supporting_pdf)
    except (OSError, ValueError) as error:
        parser.exit(1, f"audit failed: {error}\n")
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
