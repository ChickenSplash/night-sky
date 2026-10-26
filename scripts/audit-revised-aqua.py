#!/usr/bin/env python3
"""Audit the pinned reference dataset, without generating a runtime EOS table.

Source: Cano Amoros et al. (2026), doi:10.5281/zenodo.18609452, CC-BY-4.0.
Dataset must be downloaded separately. This script has no network access or
third-party dependencies. Its report is structural evidence, not EOS validation.
"""

import argparse
import bisect
import collections
import csv
import hashlib
import json
import math
from pathlib import Path


SHA256 = "baaf15a2108c0690f3e8d544174d7cc923376fe0de52dee4a277d9b14675bdbf"
COLUMNS = [
    "pressure_Pa", "temperature_K", "density_kg_m3", "entropy_J_kgK",
    "internal_energy_J_kg", "dlnS_dlnP_T", "dlnS_dlnT_P", "ad_grad",
    "phase", "flag",
]


def audit(path):
    checksum = hashlib.sha256(path.read_bytes()).hexdigest()
    if checksum != SHA256:
        raise ValueError("dataset SHA-256 differs from pinned Zenodo record 18609452")
    columns = collections.defaultdict(list)
    phase_counts = collections.Counter()
    flag_counts = collections.Counter()
    with path.open(newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != COLUMNS:
            raise ValueError("unexpected table schema")
        for row in reader:
            p, t, rho = (float(row[k]) for k in COLUMNS[:3])
            if not all(math.isfinite(v) and v > 0 for v in (p, t, rho)):
                raise ValueError("nonfinite/nonpositive P, T or density")
            phase, flag = int(row["phase"]), int(row["flag"])
            columns[t].append((p, rho, phase, flag))
            phase_counts[phase] += 1
            flag_counts[flag] += 1
    temperatures = sorted(columns)
    pressures = [row[0] for row in columns[temperatures[0]]]
    if len(temperatures) != 278 or len(pressures) != 1093:
        raise ValueError("unexpected axis dimensions")
    if any(a >= b for a, b in zip(pressures, pressures[1:])):
        raise ValueError("pressure axis is not strictly increasing")
    if any([row[0] for row in column] != pressures for column in columns.values()):
        raise ValueError("inconsistent pressure axes")
    report = {
        "source_doi": "10.5281/zenodo.18609452",
        "sha256": checksum,
        "rows": sum(phase_counts.values()),
        "pressure_nodes": len(pressures),
        "temperature_nodes": len(temperatures),
        "pressure_bounds_pa": [pressures[0], pressures[-1]],
        "temperature_bounds_k": [temperatures[0], temperatures[-1]],
        "phase_counts": dict(sorted(phase_counts.items())),
        "flag_counts": dict(sorted(flag_counts.items())),
        "isotherm_neighbours": {},
    }
    for target in (250.0, 300.0):
        index = bisect.bisect_left(temperatures, target)
        neighbours = temperatures[index - 1:index + 1]
        summaries = []
        for t in neighbours:
            segments = []
            previous = None
            for p, rho, phase, flag in columns[t]:
                key = (phase, flag)
                if key != previous:
                    segments.append({
                        "first_pressure_pa": p, "density_kg_m3": rho,
                        "phase": phase, "flag": flag,
                    })
                previous = key
            summaries.append({"temperature_k": t, "segments": segments})
        # Mixed labels forbid naive rectangular interpolation as a single phase.
        # Same labels alone do NOT certify the rest of a cell's phase domain.
        mixed_cells = 0
        a, b = (columns[t] for t in neighbours)
        for i in range(len(pressures) - 1):
            if len({a[i][2], a[i + 1][2], b[i][2], b[i + 1][2]}) > 1:
                mixed_cells += 1
        report["isotherm_neighbours"][str(target)] = {
            "columns": summaries, "mixed_phase_cells": mixed_cells,
        }
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset", type=Path)
    args = parser.parse_args()
    try:
        report = audit(args.dataset)
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"reference audit failed: {error}\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
