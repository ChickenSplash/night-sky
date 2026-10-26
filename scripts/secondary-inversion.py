#!/usr/bin/env python3
"""Bounded offline methane inversion gate, not a runtime/general phase selector.

Caller-supplied branch brackets are NOT a proof of complete fluid root enumeration.
Only pressure and same-T Gibbs differences are implemented, not caloric quantities.
"""

import argparse
from dataclasses import dataclass
import importlib.util
import json
import math
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "secondary_reference", Path(__file__).with_name("audit-secondary-reference.py"))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


class WorkExhausted(ValueError):
    pass


class ResolutionError(ValueError):
    pass


class MissingClosure(ValueError):
    pass


@dataclass
class Work:
    limit: int = 50000
    evaluations: int = 0

    def charge(self):
        if not isinstance(self.limit, int) or not 0 <= self.limit <= 50000:
            raise ValueError("invalid EOS work ceiling")
        if self.evaluations >= self.limit:
            raise WorkExhausted("shared EOS work exhausted")
        self.evaluations += 1


def controls(tolerance, iterations):
    if not math.isfinite(tolerance) or not 1e-13 <= tolerance <= 1e-6:
        raise ValueError("invalid relative tolerance")
    if not isinstance(iterations, int) or not 0 <= iterations <= 96:
        raise ValueError("invalid iteration ceiling")


def interval(bounds):
    if len(bounds) != 2 or not all(math.isfinite(x) and x > 0 for x in bounds):
        raise ValueError("invalid positive bracket")
    lo, hi = bounds
    if lo >= hi:
        raise ValueError("unordered bracket")
    return lo, hi


def evaluate(eos, temperature, density, work):
    """Return P, g/(RT) up to a T-only constant, and analytic dP/dn at fixed T."""
    work.charge()  # Includes failed probes and all nested inversion work.
    if not all(math.isfinite(x) and x > 0 for x in (temperature, density)):
        raise ValueError("invalid SI state")
    reducing = eos["STATES"]["reducing"]
    delta, tau = density / reducing["rhomolar"], reducing["T"] / temperature
    second = []
    for block in eos["alphar"]:
        if block["type"] == "ResidualHelmholtzPower":
            for n, d, t, ell in reference.terms(block, ["n", "d", "t", "l"]):
                damp = delta ** ell if ell else 0
                value = n * delta ** d * tau ** t * math.exp(-damp)
                slope = (d - ell * damp) / delta
                curvature = (-d - ell * (ell - 1) * damp) / delta ** 2
                second.append(value * (slope ** 2 + curvature))
        elif block["type"] == "ResidualHelmholtzGaussian":
            for n, d, t, eta, epsilon, beta, gamma in reference.terms(
                    block, ["n", "d", "t", "eta", "epsilon", "beta", "gamma"]):
                value = n * delta ** d * tau ** t * math.exp(
                    -eta * (delta - epsilon) ** 2 - beta * (tau - gamma) ** 2)
                slope = d / delta - 2 * eta * (delta - epsilon)
                second.append(value * (slope ** 2 - d / delta ** 2 - 2 * eta))
        else:
            raise ValueError("unsupported second derivative term")
    ar, ard = reference.residual(eos["alphar"], delta, tau)
    z = 1 + delta * ard
    rt = eos["gas_constant"] * temperature
    result = (density * rt * z, math.log(delta) + ar + z,
              rt * (1 + 2 * delta * ard + delta ** 2 * math.fsum(second)))
    if not all(map(math.isfinite, result)):
        raise ResolutionError("nonfinite EOS state")
    return result


def stable(eos, temperature, density, work):
    result = evaluate(eos, temperature, density, work)
    if result[2] <= 0:
        raise ValueError("mechanically unstable branch probe")
    return result


def invert(eos, temperature, pressure, bounds, work, tolerance=1e-9, iterations=96):
    """Bisect a supplied stable branch. No automatic expansion or hidden retry."""
    controls(tolerance, iterations)
    if not math.isfinite(pressure) or pressure <= 0:
        raise ValueError("invalid pressure")
    lo, hi = interval(bounds)
    # Sampling rejects obvious branch-crossing brackets, not a global stability proof.
    probes = [stable(eos, temperature, lo + (hi - lo) * i / 16, work)
              for i in range(17)]
    if any(a[0] >= b[0] for a, b in zip(probes, probes[1:])):
        raise ValueError("nonmonotone branch bracket")
    if not probes[0][0] <= pressure <= probes[-1][0]:
        raise ValueError("unbracketed density root")
    for density, state in [(lo, probes[0]), (hi, probes[-1])]:
        if state[0] == pressure:
            return density, state
    for _ in range(iterations):
        mid = lo + (hi - lo) / 2
        if mid in (lo, hi):
            raise ResolutionError("density bracket at floating-point resolution")
        state = stable(eos, temperature, mid, work)
        if abs(state[0] / pressure - 1) <= tolerance and hi - lo <= 1e-12 * mid:
            return mid, state
        if state[0] < pressure:
            lo = mid
        else:
            hi = mid
    raise ResolutionError("density iteration ceiling")


def candidates(eos, temperature, pressure, branches, work, tolerance=1e-9, iterations=96):
    if not branches:
        raise ValueError("missing fluid branches")
    # Canonical ordering makes diagnostics/work independent of caller order.
    ordered = sorted(interval(b) for b in branches)
    if any(a[1] >= b[0] for a, b in zip(ordered, ordered[1:])):
        raise ValueError("overlapping fluid branch brackets")
    return [invert(eos, temperature, pressure, b, work, tolerance, iterations)
            for b in ordered]


def select(roots):
    """Lowest Gibbs among supplied fluid roots only; a tie is not a phase fraction."""
    if not roots:
        raise ValueError("missing fluid roots")
    ordered = sorted(roots, key=lambda root: root[1][1])
    if len(ordered) > 1 and abs(ordered[0][1][1] - ordered[1][1][1]) <= 1e-8:
        raise MissingClosure("coexistence requires mass/volume closure")
    return ordered[0]


def coexistence(eos, temperature, pressures, branches, work, tolerance=1e-9, iterations=96):
    controls(tolerance, iterations)
    lo, hi = interval(pressures)
    if len(branches) != 2:
        raise ValueError("two distinct fluid branches required")

    def residual(pressure):
        vapor, liquid = candidates(eos, temperature, pressure, branches, work,
                                   max(1e-13, tolerance / 4))
        if 1 - vapor[0] / liquid[0] <= 1e-6:
            raise ResolutionError("unresolved coexistence density contrast")
        return liquid[1][1] - vapor[1][1], vapor, liquid

    left, right = residual(lo), residual(hi)
    if not left[0] > 0 > right[0]:
        raise ValueError("unbracketed Gibbs equality")
    for _ in range(iterations):
        mid = lo + (hi - lo) / 2
        if mid in (lo, hi):
            raise ResolutionError("pressure bracket at floating-point resolution")
        value, vapor, liquid = residual(mid)
        if abs(value) <= 1e-10 and hi - lo <= tolerance * mid:
            return mid, vapor, liquid
        if value > 0:
            lo = mid
        else:
            hi = mid
    raise ResolutionError("coexistence iteration ceiling")


def methane_window(fluid, temperature, pressure):
    """Finite reference window and independent freezing screen, not a global mask."""
    if not (math.isfinite(temperature) and 95 <= temperature <= 172
            and math.isfinite(pressure) and 1e4 <= pressure <= 3e6):
        raise ValueError("outside finite methane audit window")
    if pressure >= reference.melting_pressure(fluid["ANCILLARIES"]["melting_line"], temperature):
        raise ValueError("outside methane fluid freezing screen")


def audit(directory):
    reference.verify(directory / "CoolProp-v8-license.txt", reference.LICENSE_HASH)
    reference.verify(directory / "CoolProp-v8-CH4.txt", reference.SOURCES["CH4"][0])
    fluid = json.loads((directory / "CoolProp-v8-CH4.txt").read_text())
    eos, curves = fluid["EOS"][0], fluid["ANCILLARIES"]
    results = []
    for row in eos["SUPERANCILLARY"]["check_points"]:
        t, expected = row["T / K"], row["p(mp) / Pa"]
        estimate = reference.ancillary(curves["pS"], t)
        pressures = (.98 * estimate, 1.02 * estimate)
        for pressure in pressures:
            methane_window(fluid, t, pressure)
        branches = [(.9 * reference.ancillary(curves[k], t),
                     1.1 * reference.ancillary(curves[k], t)) for k in ("rhoV", "rhoL")]
        work = Work()
        p, vapor, liquid = coexistence(eos, t, pressures, branches, work)
        fine_work = Work()
        fine = coexistence(eos, t, pressures, branches, fine_work, tolerance=2.5e-10)
        errors = [abs(p / expected - 1),
                  abs(vapor[0] / row["rho''(mp) / mol/m^3"] - 1),
                  abs(liquid[0] / row["rho'(mp) / mol/m^3"] - 1)]
        if max(errors) > 1e-6 or abs(liquid[1][1] - vapor[1][1]) > 1e-8:
            raise ValueError("methane reference recovery failure")
        refinement = [abs(p / fine[0] - 1), abs(vapor[0] / fine[1][0] - 1),
                      abs(liquid[0] / fine[2][0] - 1)]
        if max(refinement) > 1e-8:
            raise ValueError("methane refinement failure")
        derivative_errors = []
        for density, state in (vapor, liquid):
            h = 1e-5 * density
            left = reference.state(eos, t, density - h)[0]
            right = reference.state(eos, t, density + h)[0]
            derivative_errors.append(abs((right - left) / (2 * h) / state[2] - 1))
        if max(derivative_errors) > 1e-6:
            raise ValueError("methane analytic compressibility derivative failure")
        for offset, branch in [(-1e-4, 0), (1e-4, 1)]:
            methane_window(fluid, t, p * (1 + offset))
            roots = candidates(eos, t, p * (1 + offset), branches, Work())
            if select(roots) != roots[branch]:
                raise ValueError("wrong methane Gibbs branch")
            reverse = candidates(eos, t, p * (1 + offset), branches[::-1], Work())
            if roots != reverse:
                raise ValueError("branch-order dependence")
        try:
            select([vapor, liquid])
        except MissingClosure:
            pass
        else:
            raise ValueError("coexistence silently selected one density")
        # Validate closure against source volumes, not volumes made from solved roots.
        vl, vv = (1 / (r[0] * eos["molar_mass"]) for r in (liquid, vapor))
        ref_vl = 1 / (row["rho'(mp) / mol/m^3"] * eos["molar_mass"])
        ref_vv = 1 / (row["rho''(mp) / mol/m^3"] * eos["molar_mass"])
        qualities = []
        for q in [0, .25, .5, .75, 1]:
            # Endpoints use their own resolved volumes, without clipping a source
            # endpoint just outside the numerical solution interval.
            v = vl if q == 0 else vv if q == 1 else (1 - q) * ref_vl + q * ref_vv
            recovered = reference.coexistence_quality(v, vl, vv)
            if abs(recovered - q) > 1e-8:
                raise ValueError("methane quality recovery failure")
            qualities.append(recovered)
        # Deterministic shared accounting covers the entire nested solve.
        repeat_work = Work(limit=work.evaluations)
        if (p, vapor, liquid) != coexistence(eos, t, pressures, branches, repeat_work):
            raise ValueError("nonrepeatable coexistence")
        try:
            coexistence(eos, t, pressures, branches, Work(limit=work.evaluations - 1))
        except WorkExhausted:
            pass
        else:
            raise ValueError("uncharged nested work")
        results.append({"temperature_k": t, "pressure_pa": p,
                        "molar_density_vapor_liquid": [vapor[0], liquid[0]],
                        "reference_relative_errors_p_nv_nl": errors,
                        "refinement_relative_changes_p_nv_nl": refinement,
                        "abs_gibbs_difference_over_RT": abs(liquid[1][1] - vapor[1][1]),
                        "pressure_relative_residual_vapor_liquid": [
                            abs(vapor[1][0] / p - 1), abs(liquid[1][0] / p - 1)],
                        "derivative_difference_relative_errors": derivative_errors,
                        "qualities": qualities, "eos_evaluations": work.evaluations,
                        "refined_eos_evaluations": fine_work.evaluations})
    return {"scope": "offline methane, supplied fluid branches only", "rows": results}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    print(json.dumps(audit(args.directory), indent=2, allow_nan=False))
