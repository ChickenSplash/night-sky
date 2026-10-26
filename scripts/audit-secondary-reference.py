#!/usr/bin/env python3
"""Offline checks of pinned secondary-fluid equations, not a structure solver.

No network, dependencies, density inversion or phase selection. Evaluate only
supplied (T,rhomolar) reference states. Pressure and same-temperature Gibbs
differences need residual Helmholtz energy only; this does NOT audit calorics.
Sources, masks and limitations: docs/plans/planet-coverage.md.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path


# Published fluid ceilings, deliberately not the more permissive library limits.
SOURCES = {
    "CO2": ("08e27e1a5a6029e508976496b9d1fd4f7f0caa2c46fd35c84b7bf520260a5217", 1100, 8e8),
    "CH4": ("534b6a8906be63dd491972ddb29f2856c565c2c209320044858e67b90cb7a171", 625, 1e9),
    "N2": ("791432d1685eafcc75b43e776e8fe4e30f7cf73a2a73b67de0c1a94c57a2b9aa", 1000, 2.2e9),
    "O2": ("2296d3164338a9fadeb2e7a04c98fb5c403a07851c3b59bb7634887043c44fc3", 300, 8e7),
    "Ar": ("3dd2932a954084990add53fe583d48ff2d5ab99f9efa57b7f997377d7ea5322a", 700, 1e9),
}
LICENSE_HASH = "9bf835333ef602af4cb19338b9f9d43671e174fa029b00280e9bdba6ea4719b2"


def verify(path, expected):
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise ValueError(f"SHA-256 mismatch: {path.name}")
    return actual


def terms(block, columns):
    arrays = [block[k] for k in columns]
    if not arrays[0] or any(len(a) != len(arrays[0]) for a in arrays):
        raise ValueError("empty or unequal coefficient arrays")
    if not all(math.isfinite(v) for a in arrays for v in a):
        raise ValueError("nonfinite coefficient")
    return zip(*arrays)


def residual(blocks, delta, tau):
    """Return alpha_r and d(alpha_r)/d(delta) at nonsingular positive inputs."""
    if not all(math.isfinite(v) and v > 0 for v in (delta, tau)):
        raise ValueError("nonpositive or nonfinite reduced state")
    values, derivatives = [], []
    for block in blocks:
        kind = block["type"]
        if kind == "ResidualHelmholtzPower":
            for n, d, t, ell in terms(block, ["n", "d", "t", "l"]):
                damp = delta ** ell if ell else 0
                value = n * delta ** d * tau ** t * math.exp(-damp)
                values.append(value)
                derivatives.append(value * (d - ell * damp) / delta)
        elif kind == "ResidualHelmholtzGaussian":
            for n, d, t, eta, epsilon, beta, gamma in terms(
                    block, ["n", "d", "t", "eta", "epsilon", "beta", "gamma"]):
                value = n * delta ** d * tau ** t * math.exp(
                    -eta * (delta - epsilon) ** 2 - beta * (tau - gamma) ** 2)
                values.append(value)
                derivatives.append(value * (d / delta - 2 * eta * (delta - epsilon)))
        elif kind == "ResidualHelmholtzNonAnalytic":
            if delta == 1:
                raise ValueError("nonanalytic axis requires a separately audited limit")
            for n, a, b, beta, aa, bb, cc, dd in terms(
                    block, ["n", "a", "b", "beta", "A", "B", "C", "D"]):
                q = delta - 1
                theta = 1 - tau + aa * abs(q) ** (1 / beta)
                theta_d = aa / beta * abs(q) ** (1 / beta) / q
                big_delta = theta ** 2 + bb * abs(q) ** (2 * a)
                big_delta_d = 2 * theta * theta_d + 2 * a * bb * abs(q) ** (2 * a) / q
                psi = math.exp(-cc * q ** 2 - dd * (tau - 1) ** 2)
                values.append(n * delta * big_delta ** b * psi)
                derivatives.append(n * psi * (big_delta ** b * (1 - 2 * cc * delta * q)
                                              + delta * b * big_delta ** (b - 1) * big_delta_d))
        else:
            raise ValueError(f"unsupported residual term: {kind}")
    result = math.fsum(values), math.fsum(derivatives)
    if not all(map(math.isfinite, result)):
        raise ValueError("unusable residual evaluation")
    return result


def state(eos, temperature, rhomolar):
    if not all(math.isfinite(v) and v > 0 for v in (temperature, rhomolar)):
        raise ValueError("nonpositive or nonfinite SI state")
    reducing = eos["STATES"]["reducing"]
    delta = rhomolar / reducing["rhomolar"]
    ar, ard = residual(eos["alphar"], delta, reducing["T"] / temperature)
    z = 1 + delta * ard
    # Ideal alpha_0's T-only terms and +1 cancel in same-T Gibbs differences.
    return rhomolar * eos["gas_constant"] * temperature * z, math.log(delta) + ar + z


def ancillary(curve, temperature):
    if not math.isfinite(temperature) or not curve["Tmin"] <= temperature <= curve["Tmax"]:
        raise ValueError("outside ancillary temperature interval")
    theta = 1 - temperature / curve["T_r"]
    series = math.fsum(n * theta ** t for n, t in terms(curve, ["n", "t"]))
    if curve["using_tau_r"]:
        series *= curve["T_r"] / temperature
    if curve["type"] in ("pL", "pV", "rhoV"):
        return curve["reducing_value"] * math.exp(series)
    if curve["type"] == "rhoLnoexp":
        return curve["reducing_value"] * (1 + series)
    raise ValueError("unsupported ancillary")


def melting_pressure(curve, temperature):
    if len(curve["parts"]) != 1:
        raise ValueError("audit supports one melting segment only")
    part = curve["parts"][0]
    if not math.isfinite(temperature) or not part["T_min"] <= temperature <= part["T_max"]:
        raise ValueError("outside melting-curve interval")
    tr = temperature / part["T_0"]
    if curve["type"] == "Simon":
        return part["p_0"] + part["a"] * (tr ** part["c"] - 1)
    if curve["type"] == "polynomial_in_Tr":
        series = math.fsum(a * (tr ** t - 1) for a, t in terms(part, ["a", "t"]))
    elif curve["type"] == "polynomial_in_Theta":
        series = math.fsum(a * (tr - 1) ** t for a, t in terms(part, ["a", "t"]))
    else:
        raise ValueError("unsupported melting curve")
    return part["p_0"] * (1 + series)


def additive_volume(masses, densities):
    """Audit supplied same-P/T states only, NOT a mixture EOS or phase model."""
    if not masses or len(masses) != len(densities):
        raise ValueError("empty or unequal mixture arrays")
    if not all(math.isfinite(m) and m >= 0 for m in masses):
        raise ValueError("invalid origin mass")
    # Absent origins need no EOS evaluation; positive origins cannot be skipped.
    volumes = []
    for mass, density in zip(masses, densities):
        if mass == 0:
            continue
        if not math.isfinite(density) or density <= 0:
            raise ValueError("invalid active constituent density")
        volumes.append(mass / density)
    mass, volume = math.fsum(masses), math.fsum(volumes)
    if not all(math.isfinite(v) and v > 0 for v in (mass, volume)):
        raise ValueError("unusable mixture mass or volume")
    density = mass / volume
    if not math.isfinite(density) or density <= 0:
        raise ValueError("unusable mixture density")
    return volume, density


def coexistence_quality(specific_volume, liquid_volume, vapor_volume):
    """Lever-rule algebra with explicit conditioning failure, no phase search."""
    if not all(math.isfinite(v) and v > 0 for v in
               (specific_volume, liquid_volume, vapor_volume)):
        raise ValueError("invalid specific volume")
    contrast = vapor_volume - liquid_volume
    if contrast <= 1e-6 * max(liquid_volume, vapor_volume):
        raise ValueError("unresolved coexistence contrast")
    if not liquid_volume <= specific_volume <= vapor_volume:
        raise ValueError("outside coexistence volume interval")
    return (specific_volume - liquid_volume) / contrast


def quality_error_bound(specific_volume, liquid_volume, vapor_volume, errors):
    """Bound ratio perturbations from supplied absolute volume-error estimates."""
    q = coexistence_quality(specific_volume, liquid_volume, vapor_volume)
    if len(errors) != 3 or not all(math.isfinite(e) and e >= 0 for e in errors):
        raise ValueError("invalid specific-volume error estimates")
    ev, el, eg = errors
    denominator = vapor_volume - liquid_volume - el - eg
    if denominator <= 0:
        raise ValueError("uncertainty spans coexistence contrast")
    bound = (ev + el + abs(q) * (el + eg)) / denominator
    if not math.isfinite(bound):
        raise ValueError("unusable quality error bound")
    return bound


def liquid_pressure_work(temperature, molar_volume_bound, pressure, saturation_pressure, gas_constant):
    """Bound |ln Poynting factor| using a supplied upper bound on liquid v_m."""
    if not all(math.isfinite(v) and v > 0 for v in
               (temperature, molar_volume_bound, pressure, saturation_pressure, gas_constant)):
        raise ValueError("invalid liquid pressure-work input")
    bound = molar_volume_bound * abs(pressure - saturation_pressure) / (gas_constant * temperature)
    if not math.isfinite(bound):
        raise ValueError("unusable liquid pressure-work bound")
    return bound


def audit(directory):
    licence = verify(directory / "CoolProp-v8-license.txt", LICENSE_HASH)
    results = {}
    for name, (pin, tmax, pmax) in SOURCES.items():
        path = directory / f"CoolProp-v8-{name}.txt"
        verify(path, pin)
        fluid = json.loads(path.read_text())
        eos = fluid["EOS"][0]
        anchors = []
        for key in ("hs_anchor", "sat_min_liquid", "sat_min_vapor"):
            s = eos["STATES"][key]
            pressure, _ = state(eos, s["T"], s["rhomolar"])
            error = abs(pressure / s["p"] - 1)
            anchors.append({"name": key, "pressure_relative_residual": error,
                            "within_1e_minus_6": error <= 1e-6})
        saturation = []
        for s in eos["SUPERANCILLARY"]["check_points"]:
            t, p = s["T / K"], s["p(mp) / Pa"]
            rl, rv = s["rho'(mp) / mol/m^3"], s["rho''(mp) / mol/m^3"]
            pl, gl = state(eos, t, rl)
            pv, gv = state(eos, t, rv)
            errors = [abs(pl / p - 1), abs(pv / p - 1), abs(gl - gv)]
            if max(errors) > 1e-6:
                raise ValueError(f"{name} saturation reference residuals {errors}")
            curves = fluid["ANCILLARIES"]
            # Some source T values differ from curve endpoints by roundoff.
            # Report outside-domain instead of silently clipping to make it pass.
            estimates = {}
            for curve, ref in [("pS", p), ("rhoL", rl), ("rhoV", rv)]:
                c = curves[curve]
                estimates[curve] = (ancillary(c, t) / ref - 1
                                    if c["Tmin"] <= t <= c["Tmax"] else None)
            saturation.append({"temperature_k": t, "pressure_pa": p,
                               "density_kg_m3_liquid_vapor": [r * eos["molar_mass"] for r in (rl, rv)],
                               "pressure_relative_residual_liquid_vapor": errors[:2],
                               "abs_gibbs_difference_over_RT": errors[2],
                               "ancillary_relative_errors_pS_rhoL_rhoV": estimates})
        curve = fluid["ANCILLARIES"]["melting_line"]
        part = curve["parts"][0]
        super_meta = eos["SUPERANCILLARY"]["meta"]
        results[name] = {
            "sha256": pin, "citation": eos["BibTeX_EOS"],
            "fitted_R_J_molK": eos["gas_constant"], "molar_mass_kg_mol": eos["molar_mass"],
            "published_ceiling_K_Pa": [tmax, pmax], "JSON_ceiling_K_Pa": [eos["T_max"], eos["p_max"]],
            "triple_temperature_k": eos["Ttriple"],
            "nominal_critical_temperature_k": fluid["STATES"]["critical"]["T"],
            "mathematical_critical_temperature_k": super_meta["Tcrittrue / K"],
            "melting": {"citation": curve["BibTeX"], "type": curve["type"],
                        "endpoint_K_Pa": [[t, melting_pressure(curve, t)]
                                          for t in (part["T_min"], part["T_max"])]},
            "pressure_anchors": anchors, "coexistence_checks": saturation,
            "sublimation_curve_in_JSON": "sublimation_line" in fluid["ANCILLARIES"],
        }
    # Actual pinned methane coexistence row; no new density inversion or planet.
    methane = min(results["CH4"]["coexistence_checks"], key=lambda row: row["temperature_k"])
    vl, vv = [1 / rho for rho in methane["density_kg_m3_liquid_vapor"]]
    closure = []
    for q in (0, .25, .5, .75, 1):
        volume = (1 - q) * vl + q * vv  # m = 1 kg
        closure.append({"input_vapor_mass_fraction": q, "volume_m3_for_1kg": volume,
                        "recovered_vapor_mass_fraction": coexistence_quality(volume, vl, vv)})
    return {"scope": "reference transcription and branch consistency, NOT full physical validation",
            "license_sha256": licence, "fluids": results,
            "methane_inventory_volume_closure": {
                "temperature_k": methane["temperature_k"], "pressure_pa": methane["pressure_pa"],
                "rows": closure}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    try:
        report = audit(args.directory)
    except (OSError, ValueError, KeyError, OverflowError) as error:
        parser.exit(1, f"secondary reference audit failed: {error}\n")
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
