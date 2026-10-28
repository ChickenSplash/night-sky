#!/usr/bin/env python3
"""Audit the saved P2 census, not an observational validation or new solve.

First run the ignored saved_census_fallback_audit test to reconstruct fallback
fields. All thresholds below are review triggers, never new solver cutoffs.
"""
import csv
import json
import math
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "data/tmp"
G = 6.67430e-11
EARTH_MASS = (math.tau**2 * 149597870700.0**3 / (365.2568983 * 86400)**2
              * 3.0034896e-6 / G)


def audit():
    with (DATA / "p2-step7-399/composition-coverage.tsv").open() as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    with (DATA / "p2-plausibility-fallbacks.jsonl").open() as stream:
        fallbacks = {r["id"]: r["record"] for r in map(json.loads, stream)}
    results = []
    for row in rows:
        fallback = fallbacks.get(row["id"])
        mass = float(row["mass_earth"]) * EARTH_MASS
        if fallback:
            p = fallback["physical"]
            mass, radius = p["retained_mass_kg"], p["reference_radius_m"]
            temperature, pressure = fallback["surface_temperature_k"], fallback["surface_pressure_pa"]
            inertia, surface = p["inertia_factor"], fallback["surface"]
        else:
            mass *= 1 - float(row["escaped_fraction"])
            radius = float(row["new_radius_km"]) * 1000
            temperature, pressure = float(row["surface_k"]), float(row["surface_pa"])
            inertia, surface = float(row["inertia_factor"]), row["surface"]
        density = mass / (4 * math.pi * radius**3 / 3)
        # Envelope records use enclosed mass at the reference. The saved census
        # lacks that mass, so GM_total/R² is explicitly an upper bound, not g_ref.
        gravity_upper = G * mass / radius**2
        jeans_upper = G * mass * 2.3 * 1.66053906892e-27 / (radius * 1.380649e-23 * temperature)
        flags = []
        if density < 100:
            flags.append("very_diffuse")
        if density > 20000 or (density < 500 and surface != "envelope"):
            flags.append("condensed_density_review")
        if inertia < 0.03:
            flags.append("small_reference_inertia")
        if not 0.01 <= inertia <= 2 / 3:
            flags.append("inertia_review")
        if surface == "envelope" and jeans_upper < 20:
            flags.append("weak_binding_review")
        if surface == "rock" and temperature >= 1500:
            flags.append("refractory_phase_unresolved")
        if fallback and fallback["model_fidelity"] == "extrapolated":
            flags.append("labelled_fit_extrapolation")
        assert all(math.isfinite(v) and v > 0 for v in
                   (mass, radius, density, gravity_upper, inertia, temperature)), row["id"]
        assert math.isfinite(pressure) and pressure >= 0, row["id"]
        results.append(dict(
            id=row["id"], route="fallback" if fallback else "layered",
            surface=surface, mass_earth=mass / EARTH_MASS,
            radius_km=radius / 1000, density_kg_m3=density,
            gravity_upper_m_s2=gravity_upper, inertia_factor=inertia,
            temperature_k=temperature, pressure_bar=pressure / 1e5,
            hhe_fraction=float(row["hhe_fraction"]),
            water_fraction=float(row["water_fraction"]),
            jeans_hhe_upper=jeans_upper if surface == "envelope" else None,
            radius_vs_legacy=radius / (1000 * float(row["legacy_radius_km"])),
            flags=";".join(flags), refusal=row["detail"],
        ))
    assert len(results) == 4367 and len(fallbacks) == 315
    with (DATA / "p2-plausibility-audit.tsv").open("w") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(results[0]), delimiter="\t")
        writer.writeheader()
        writer.writerows(results)
    summary = {}
    for route in ("layered", "fallback"):
        subset = [r for r in results if r["route"] == route]
        summary[route] = dict(count=len(subset), flags=dict(Counter(
            flag for r in subset for flag in r["flags"].split(";") if flag)))
        summary[route]["bounds"] = {k: [min(r[k] for r in subset), max(r[k] for r in subset)]
                                    for k in ("mass_earth", "radius_km", "density_kg_m3",
                                              "gravity_upper_m_s2", "inertia_factor", "temperature_k")}
    (DATA / "p2-plausibility-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    audit()
