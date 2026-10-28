#!/usr/bin/env python3
"""P2 step 2 decision probe, not a runtime envelope or physical validity mask.

Uses the existing separately downloaded, pinned CMS19/HG23 references. Does not
redistribute them or interpret finite density as evidence of a fluid phase.
"""

import argparse
import importlib.util
import json
import math
from pathlib import Path
import tarfile

DENSITY_PRESCRIPTION = "cms19_hg23_log_density_candidate_v1"
THERMAL_PRESCRIPTION = "grey_pressure_opacity_fixed_gradient_candidate_v1"


def reference_module():
    path = Path(__file__).with_name("audit-hhe-reference.py")
    spec = importlib.util.spec_from_file_location("hhe_reference", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def thermal(pressure_pa, equilibrium_k, intrinsic_k, gravity_m_s2,
            opacity_m2_kg=0.001):
    """Grey, plane-parallel radiative/convective *prescription*, constant g.

    kappa=kappa0 P/P0, tau=kappa0 P²/(2 g P0), P0=1 bar.
    T_rad^4=Teq^4+3/4 Tint^4 (tau+2/3). Cap dlnT/dlnP at 2/7.
    This molecular-gas gradient is prescribed, not taken from table entropy.
    """
    values = (pressure_pa, equilibrium_k, intrinsic_k, gravity_m_s2, opacity_m2_kg)
    if not all(math.isfinite(x) and x > 0 for x in values):
        raise ValueError("thermal inputs must be finite and positive")
    if not (20 <= equilibrium_k <= 2000 and 20 <= intrinsic_k <= 200):
        raise ValueError("outside selected thermal forcing bounds")
    if not (1e-2 <= pressure_pa <= 1e14 and 0.1 <= gravity_m_s2 <= 1000):
        raise ValueError("outside selected pressure/reference-gravity bounds")
    # Equate radiative logarithmic gradient to the prescribed molecular value.
    gradient = 2 / 7
    a = (equilibrium_k / intrinsic_k) ** 4 + 0.5
    tau_rcb = 4 * gradient * a / (0.75 * (2 - 4 * gradient))
    p_rcb = math.sqrt(2 * gravity_m_s2 * 1e5 * tau_rcb / opacity_m2_kg)
    tau = opacity_m2_kg * pressure_pa ** 2 / (2 * gravity_m_s2 * 1e5)
    if pressure_pa <= p_rcb:
        t = (equilibrium_k ** 4 + 0.75 * intrinsic_k ** 4 * (tau + 2 / 3)) ** 0.25
        slope = 0.375 * tau / (a + 0.75 * tau)
        branch = "radiative"
    else:
        t_rcb = intrinsic_k * (a + 0.75 * tau_rcb) ** 0.25
        t = t_rcb * (pressure_pa / p_rcb) ** gradient
        slope = gradient
        branch = "prescribed convective"
    return dict(temperature_k=t, gradient=slope, branch=branch,
                boundary_pressure_pa=p_rcb)


class DensityReference:
    def __init__(self, directory):
        self.audit = reference_module()
        for name, pin in self.audit.PINS.items():
            self.audit.verify(directory / name, pin)
        with tarfile.open(directory / "EOS_Chabrier2021_HHe.tar.gz") as archive:
            self.grids = {
                label: self.audit.original_grid(archive.extractfile(name).read().decode("ascii"))
                for label, name in self.audit.MEMBERS.items() if label in ("H", "He")
            }
        self.correction = self.audit.correction_grid(directory / "HG23_Vmix_Smix.csv")

    def density(self, p, t, y=0.25):
        if not (math.isfinite(p) and math.isfinite(t) and math.isfinite(y)):
            raise ValueError("nonfinite density input")
        # An inspection rectangle, explicitly NOT a physical validity domain.
        if not (100 <= p <= 1e14 and 100 <= t <= 1e5 and 0 <= y <= 1):
            raise ValueError("outside density inspection rectangle")
        ti, pi = (math.log10(t) - 2) / 0.05, math.log10(p) / 0.05
        i, j = math.floor(ti), math.floor(pi)
        u, v = ti - i, pi - j
        weights = [((i + di, j + dj), wt * wp)
                   for di, wt in ((0, 1 - u), (1, u))
                   for dj, wp in ((0, 1 - v), (1, v)) if wt * wp > 0]
        # Interpolate log pure density, but the signed volume correction linearly.
        pure = [10 ** sum(weight * self.grids[label][key][2]
                         for key, weight in weights) * 1000 for label in ("H", "He")]
        correction = sum(weight * self.correction[key][0] for key, weight in weights)
        density = self.audit.mixture(*pure, correction, y)
        screens = {species: self.audit.melting_screen(species, p, t)
                   for species in ("H", "He")}
        corners = {label: all(self.grids[label][key][6] > 0 for key, _ in weights)
                   for label in ("H", "He")}
        return dict(density_kg_m3=density, pure_phase_screens=screens,
                    positive_pure_compressibility_at_corners=corners,
                    physical_validity="not established by this probe")


def probe(directory):
    reference = DensityReference(directory)
    fixtures = []
    for p, t in [(1e6, 1000), (1e11, 10 ** 3.7), (1e11, 10000),
                 (1e12, 10 ** 4.3)]:
        fixtures.append(dict(pressure_pa=p, temperature_k=t,
                             **reference.density(p, t)))
    profiles = []
    # Reference scenarios, not inferred gravity or internal heat of generated bodies.
    for label, teq, gravity in [("cold small envelope", 20, 10),
                                 ("temperate envelope", 250, 10),
                                 ("giant reference", 110, 25),
                                 ("hot envelope", 1074, 10)]:
        for tint in (30, 50, 100, 200):
            samples = []
            for p in (1e5, 1e7, 1e9, 1e11, 1e12):
                state = thermal(p, teq, tint, gravity)
                try:
                    density = reference.density(p, state["temperature_k"])
                except ValueError as error:
                    density = dict(unresolved=str(error))
                samples.append(dict(pressure_pa=p, **state, density_reference=density))
            profiles.append(dict(scenario=label, equilibrium_k=teq,
                                 reference_gravity_m_s2=gravity, intrinsic_k=tint,
                                 samples=samples))
    return dict(scope="prescription selection only; no hydrostatic solve, EOS mask or radius accuracy claim",
                density_prescription=DENSITY_PRESCRIPTION,
                thermal_prescription=THERMAL_PRESCRIPTION,
                mixture="uniform H/He; HG23 volume correction once; no heavy mixing",
                thermal_reference=dict(intrinsic_k=50, opacity_at_one_bar_m2_kg=0.001,
                                       gradient=2/7, reference_only=True),
                density_fixtures=fixtures, thermal_sensitivity=profiles)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    print(json.dumps(probe(args.directory), indent=2, allow_nan=False))
