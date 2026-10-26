#!/usr/bin/env python3
"""Offline CMS19/CD21/HG23 reference audit, not a runtime EOS or validity mask.

Download the three pinned artifacts separately. Reads named archive members
without extraction. Reports numerical warnings instead of treating a rectangular
grid, positive derivative or heuristic phase flag as physical validity evidence.
See docs/plans/planet-coverage.md for sources, licences and scope.
"""

import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import tarfile


PINS = {
    "EOS_Chabrier2021_HHe.tar.gz":
        "579658d918611f45e2ebc25c92bcb6c9d1d954b4f769897bff7c560c11f30507",
    "HG23_Vmix_Smix.csv":
        "86cf66c9551cce76eaff4e04f6746f30150cf3bcb10217d302d283c1f06d9bfe",
    "HG23_Vmix_Smix_Umix.csv":
        "bfd8b08a7869bd7f6b90d8d7c20c3ef7504c88a7ff67715ba78952114f320eb2",
}
PREFIX = "EOS_Chabrier2021_HHe/original_tables/"
MEMBERS = {
    "H": PREFIX + "TABLE_H_TP_v1",
    "He": PREFIX + "TABLE_HE_TP_v1",
    "CD21_Y0275": PREFIX + "TABLEEOS_2021_TP_Y0275_v1",
}
SHAPE = (121, 441)
LN10 = math.log(10.0)


def verify(path, expected):
    checksum = hashlib.sha256(path.read_bytes()).hexdigest()
    if checksum != expected:
        raise ValueError(f"SHA-256 mismatch: {path.name}")
    return checksum


def decode_log(value, factor):
    """Do not turn overflowing source energy/entropy into a usable sentinel."""
    try:
        decoded = 10.0 ** value * factor
    except OverflowError:
        return None
    return decoded if math.isfinite(decoded) and decoded > 0 else None


def coordinate(log_t, log_p_gpa):
    t, p = (log_t - 2.0) / 0.05, (log_p_gpa + 9.0) / 0.05
    key = round(t), round(p)
    if (abs(t - key[0]) > 1e-7 or abs(p - key[1]) > 1e-7
            or not 0 <= key[0] < SHAPE[0] or not 0 <= key[1] < SHAPE[1]):
        raise ValueError("point outside the pinned log-grid")
    return key


def rectangular(rows):
    expected = SHAPE[0] * SHAPE[1]
    if len(rows) != expected:
        raise ValueError(f"incomplete grid: {len(rows)} rows, expected {expected}")
    return rows


def original_grid(text):
    rows = {}
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        values = tuple(map(float, line.split()))
        if len(values) != 10 or not all(map(math.isfinite, values)):
            raise ValueError("invalid original TP row")
        key = coordinate(values[0], values[1])
        if key in rows:
            raise ValueError("duplicate original TP coordinate")
        rows[key] = values
    return rectangular(rows)


def correction_grid(path, energy=False):
    rows = {}
    with path.open(newline="") as source:
        reader = csv.DictReader(source)
        columns = ["LOGP", "LOGT", "Vmix", "Smix"] + (["Umix"] if energy else [])
        if reader.fieldnames != columns:
            raise ValueError("unexpected HG23 CSV schema")
        for row in reader:
            if None in row or any(row[k] is None for k in columns):
                raise ValueError("invalid HG23 CSV row length")
            values = tuple(float(row[k]) for k in columns)
            if not all(map(math.isfinite, values)):
                raise ValueError("nonfinite HG23 correction")
            # dyn/cm² -> Pa is x0.1; GPa -> dyn/cm² is x1e10.
            key = coordinate(values[1], values[0] - 10.0)
            if key in rows:
                raise ValueError("duplicate HG23 coordinate")
            # cm³/g -> m³/kg x1e-3; erg/(g K) -> J/(kg K) x1e-4.
            # Umix is retained raw, not assigned unverified units by analogy.
            rows[key] = (values[2] * 1e-3, values[3] * 1e-4) + values[4:]
    return rectangular(rows)


def mixture(h, he, correction, y):
    if not math.isfinite(y) or not 0 <= y <= 1:
        raise ValueError("helium mass fraction outside [0,1]")
    if not all(math.isfinite(rho) and rho > 0 for rho in (h, he)):
        raise ValueError("nonpositive pure density")
    x = 1.0 - y
    volume = x / h + y / he + x * y * correction
    if not math.isfinite(volume) or volume <= 0:
        raise ValueError("nonpositive mixture specific volume")
    return 1.0 / volume


def quantiles(values):
    if not values:
        return None
    values = sorted(values)
    return [values[int((len(values) - 1) * q)] for q in (0, 0.5, 0.95, 1)]


def melting_screen(species, p, t):
    """CMS19 Eqs.1/7, restricted screening, NOT a complete phase selector.

    Keep to a high-pressure comparison interval, avoiding low-pressure H2
    extrapolation and extending neither molecular curve to plasma conditions.
    """
    if species == "H" and 1e8 <= p <= 3e11:
        gpa = p / 1e9
        melt = 4.853 * (1 + gpa / .023) ** .748 * math.exp(-.0098 * gpa)
    elif species == "He" and 1e8 <= p <= 1e11:
        melt = 61 * (p / 1e8) ** .639  # pressure in kbar, not GPa
    else:
        return {"melting_curve_screen": "outside selected comparison interval"}
    return {"pure_melting_temperature_k": melt, "below_pure_melting_curve": t < melt}


def entropy(row):
    return decode_log(row[4], 1e6)


def grid_summary(grid):
    counts = dict(rows=len(grid), unsafe_u=0, unsafe_s=0, window_nodes=0,
                  nonpositive_compressibility=0, nonpositive_cp_entropy=0,
                  nonpositive_cp_density_gradient=0, window_gradient_floor=0,
                  finite_difference_nodes=0)
    errors = []
    for (ti, pi), row in grid.items():
        counts["unsafe_u"] += decode_log(row[3], 1e6) is None
        counts["unsafe_s"] += entropy(row) is None
        # Inspection window ONLY, deliberately not called a physical mask.
        if not (2 <= row[0] <= 5 and -7 <= row[1] <= 5):
            continue
        counts["window_nodes"] += 1
        rho = decode_log(row[2], 1000)
        if rho is None:
            raise ValueError("unusable source density in inspection window")
        counts["nonpositive_compressibility"] += row[6] <= 0
        s = entropy(row)
        counts["nonpositive_cp_entropy"] += s is None or s * row[7] <= 0
        cp_from_density = -(10.0 ** (row[1] + 9)) * row[5] / (
            10.0 ** row[0] * rho * row[9]) if row[9] != 0 else float("nan")
        counts["nonpositive_cp_density_gradient"] += (
            not math.isfinite(cp_from_density) or cp_from_density <= 0)
        counts["window_gradient_floor"] += row[9] == 0.1
        neighbours = [(ti - 1, pi), (ti + 1, pi), (ti, pi - 1), (ti, pi + 1)]
        if any(k not in grid for k in neighbours):
            continue
        st0, st1, sp0, sp1 = [entropy(grid[k]) for k in neighbours]
        if any(v is None for v in (st0, st1, sp0, sp1)):
            continue
        cp = (st1 - st0) / (0.1 * LN10)
        if cp <= 0:
            continue
        gradient = -(sp1 - sp0) / (0.1 * LN10) / cp
        if gradient > 0:
            counts["finite_difference_nodes"] += 1
            errors.append(abs(row[9] / gradient - 1.0))
    counts["gradient_relative_difference_min_median_p95_max"] = quantiles(errors)
    return counts


def audit(directory):
    hashes = {name: verify(directory / name, pin) for name, pin in PINS.items()}
    grids, members = {}, {}
    with tarfile.open(directory / "EOS_Chabrier2021_HHe.tar.gz") as archive:
        for label, name in MEMBERS.items():
            info = archive.getmember(name)
            if not info.isfile() or info.size > 10_000_000:
                raise ValueError("unexpected source member type or size")
            data = archive.extractfile(info).read()
            members[label] = {"path": name, "sha256": hashlib.sha256(data).hexdigest()}
            grids[label] = original_grid(data.decode("ascii"))
    corrections = correction_grid(directory / "HG23_Vmix_Smix.csv")
    with_energy = correction_grid(directory / "HG23_Vmix_Smix_Umix.csv", energy=True)
    if any(corrections[k] != with_energy[k][:2] for k in corrections):
        raise ValueError("updated HG23 table changed volume/entropy coefficients")
    # Published Appendix B is an independent comparison, not a golden copy of
    # the downloaded coefficients. Preserve disagreements in the report.
    published = []
    for log_t, v, s in [(2.5, 0.21972032499215227, 145270983.02864826),
                        (2.85, 0.24501465036396686, 156237499.56270728)]:
        actual = corrections[coordinate(log_t, 0.5)]
        published.append({
            "logP_cgs_logT_k": [10.5, log_t],
            "printed_vmix_smix_SI": [v * 1e-3, s * 1e-4],
            "downloaded_vmix_smix_SI": list(actual),
            "relative_differences": [actual[0] / (v * 1e-3) - 1,
                                     actual[1] / (s * 1e-4) - 1],
        })
    fixtures = []
    for p, t in [(1e6, 1000), (1e11, 5000), (1e12, 20000)]:
        # Nearest source knot, explicitly not interpolation of a requested state.
        key = round((math.log10(t) - 2) / 0.05), round(math.log10(p) / 0.05)
        h, he, cd = [grids[label][key] for label in MEMBERS]
        rh, rhe, rcd = [decode_log(r[2], 1000) for r in (h, he, cd)]
        v, s = corrections[key]
        # HG23 Eq.5 uses X*=0.725/Y*=0.275 for pure contributions,
        # but Xtilde=0.755/Ytilde=0.245 for the correction amplitude.
        reconstructed_volume = 0.725 / rh + 0.275 / rhe + 0.755 * 0.245 * v
        fixtures.append({
            "requested_pa_k": [p, t],
            "nearest_knot_pa_k": [10.0 ** (h[1] + 9), 10.0 ** h[0]],
            "pure_density_kg_m3": [rh, rhe],
            "pure_phase_screens": {
                species: melting_screen(species, 10.0 ** (h[1] + 9), 10.0 ** h[0])
                for species in ("H", "He")
            },
            "HG23_density_Y025_Y0275": [mixture(rh, rhe, v, y) for y in (0.25, 0.275)],
            "HG23_smix_J_kgK": s,
            "CD21_Eq5_volume_relative_residual": abs(reconstructed_volume * rcd - 1),
        })
    return {
        "scope": "source integrity, units and numerical diagnostics, NOT physical validity",
        "sha256": hashes, "original_members": members,
        "inspection_window": {"temperature_k": [100, 1e5], "pressure_pa": [100, 1e14]},
        "original_grids": {label: grid_summary(g) for label, g in grids.items()},
        "HG23": {"rows": len(corrections), "pressure_pa": [1, 1e22],
                 "temperature_k": [100, 1e8], "published_fragment_comparisons": published,
                 "Umix_units": "not independently selected for thermal use"},
        "nearest_knot_fixtures": fixtures,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    try:
        report = audit(args.directory)
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f"reference audit failed: {error}\n")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
