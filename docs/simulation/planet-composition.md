# Material inventories and approximate structure

The independently tested foundation lives in simulation `system/composition.rs`
and `system/structure.rs`. **Planet generation does not use it yet.** Generated
planets still follow [the current class-first recipe](planetary-systems.md).
Integration checkpoints are in [the refactor plan](../plans/planet-composition.md).

## Conserved inventory, not a classification

`Material` has stable string identifiers. Adding a constituent requires an
explicit catalogue entry and physical prescriptions, not just a density:

| Identifier | Meaning / supported prescription |
| --- | --- |
| `iron_nickel` | Unresolved metal; cold pure-Fe radius proxy |
| `silicate` | Unresolved silicate rock; cold MgSiO₃ proxy |
| `water` | H₂O mass, independent of reservoir/phase; cold water-ice radius proxy |
| `hydrogen`, `helium` | Separate H₂ and He mass budgets; joint light envelope model |
| `nitrogen`, `carbon_dioxide`, `methane`, `oxygen`, `argon` | N₂, CO₂, CH₄, O₂ and Ar molecular budgets |

These are material/molecular budgets, **not independent elemental inventories**.
The hydrogen bound in water or methane is included in that molecule's mass,
not counted again in `hydrogen`. Likewise oxygen in rock, water and CO₂ is not
also placed in `oxygen`. Metal alloys and mineral species are unresolved.
Oxygen support does not imply biological production or generated free oxygen.
Reactions, dissociation and conversions between species are unsupported: future
chemistry must explicitly conserve atoms and debit all source molecules.

`Inventory` accepts finite nonnegative kilograms, rejects duplicate constituents
and nonfinite totals, and keeps masses private. Empty reservoirs are valid; an
empty planet is not. Mass-fraction construction requires a positive total and
fractions summing to one within 1e-9, normalising only that rounding error.
The empty inventory has unknown fractions (`None`), not a fictitious composition.

`Budget` holds the formation/source inventory, retained inventory, three retained
reservoirs and escaped inventory. The reservoirs are interior, unresolved
condensed volatiles and the **entire** gaseous/fluid envelope. Source mass minus
allocations becomes escaped mass, separately for every constituent. Overspending
even a trace constituent is rejected without an absolute tolerance. No reservoir
assignment asserts equilibrium chemistry or a resolved ocean. Atmospheric columns
are descriptions of gas-reservoir material, never additional inventory.

Transfers between retained reservoirs are atomic on failure and do not change
source, retained or escaped totals. They conserve allocations to floating-point
roundoff, not arbitrary precision. A positive transfer too small to change either
source or destination mass is rejected rather than manufacturing a trace supply.
Escape is not silently recaptured. A later evolution step can construct a new
budget with revised retained allocations and explicit escaped remainder.

Gas conversion uses the atmosphere catalogue's existing molecular masses:
`w_i = x_i μ_i / Σ(x_j μ_j)` and `x_i = (w_i/μ_i) / Σ(w_j/μ_j)`.
Here `w` is mass fraction and `x` is mole fraction. Conversion preserves molecular
identity and total mass; solid rock/metal cannot be converted to gas by this API.
Condensation must explicitly transfer the same molecular material to condensates.

## Cold compressed heavy-material radius

Pure-material radii use Seager et al. (2007), equation 23 and the **modified-
polytropic** rows of table 4, not the separate unparameterised full-EOS rows:

```text
s = M / m1
R / r1 = 10^[k1 + log10(s)/3 - k2 s^k3]
```

| Proxy | m1 (Earth masses) | r1 (Earth radii) | k1 | k2 | k3 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fe | 5.80 | 2.52 | -0.209490 | 0.0804 | 0.394 |
| MgSiO₃ | 10.55 | 3.90 | -0.209594 | 0.0799 | 0.413 |
| H₂O ice | 5.52 | 4.43 | -0.209396 | 0.0807 | 0.375 |

We restrict direct fits to heavy-material mass ≤20 Earth masses. From 20 to
1,000 Earth masses, a continuous `R(20) (M/20)^0.2` continuation is explicitly
flagged. This is our bounded engineering extension, not the paper's EOS solution.
Below 0.0001 or above 1,000 heavy-material Earth masses the model returns an error;
it does not extrapolate indefinitely into asteroids or massive solid giants.

Mixtures use `R³ = Σ w_i R_i(M_heavy)³`, evaluating every pure curve at the **same
total heavy mass**, not each constituent's separate mass. This is compressed
volume interpolation, not addition of ambient-density volumes and not a layered
hydrostatic interior integration. Mixtures are flagged. Secondary molecular
volatiles have no pure condensed fit; only a combined ≤1% of heavy mass may use
an explicitly flagged water proxy. Larger supplies return an unsupported-domain
error rather than silently treating arbitrary bulk material as water.

Water-bearing results are flagged as cold-water proxies. Their radius is not
evidence of liquid water, a particular ice phase, an exposed ocean or solid ground.
An H/He planet's heavy-material radius is only a cold structural proxy, not a
located core/envelope discontinuity. Dissolved H₂/He is currently unsupported.

## Light envelopes and the giant transition

The input is retained H₂+He mass fraction `f`, total mass `M`, current bolometric
flux `F` and age, not class or surface pressure. Helium must be 15–35% of the
H/He mass and all H/He must be allocated to the gas reservoir. H/He planets require
0.1–4,000 total Earth masses; dry bodies allow 0.0001–4,000, subject to the separate
heavy-mass bound above. Pure H/He is supported; pure H or pure He is not.

The starting thickness is inspired by Lopez & Fortney (2014), equation 4:

```text
ΔR / R_earth = 2.06 M^-0.21 (f/0.05)^0.59 (F/F_earth)^0.044 (age/5 Gyr)^-0.18
```

The original low-mass envelope calculations are not a giant-planet prescription.
We bound its mass input to 1–20 Earth masses, fraction to at most 0.2, flux to
0.1–1,000 Earth fluxes and age to 0.1–10 Gyr, flagging any bound used. Age must
still be positive and finite; flux must be finite and nonnegative. Below the
published grid's minimum fraction (0.0001), the power law is deliberately
continued to zero for a continuous stripping limit, not a claim of fit accuracy.
Every envelope result carries an `EnvelopeSurrogate` caveat.

Our separate, tunable self-compressed giant surrogate is:

```text
R_g / R_earth = 11.2 (M/318)^(1/3)
               × [(1 + 318/95)/(1 + M/95)]^(1/3 + 0.04)
```

This tends from `M^(1/3)` to `M^-0.04` and normalises a 318-Earth-mass gas sphere
to 11.2 Earth radii. It is **not** a published giant evolution fit. It has no hot-
Jupiter inflation, explicit entropy/cooling integration or helium separation.
Set `R_g` no smaller than the heavy-material proxy. Limit the low-mass thickness
smoothly to its available radius using `d / [1+(d/c)^4]^(1/4)`, where `c=R_g-R_heavy`.
Zero capacity gives zero thickness. Blend from this radius to `R_g` across
`f=0.1–0.5` using clamped smoothstep `t²(3-2t)`. Thin-envelope mass/age/flux fits
therefore never directly set gas-giant radii.

These choices guarantee finite bounded radii and continuous transitions within
the domain, not uniquely correct structures. The reference radius is a one-bar
**proxy**, not the transit radius of the original low-mass model. No photospheric
radius correction is added, so this is not a reproduction of that published fit.

As a necessary supply check, bound that proxy with the thin-column relation
`M_column ≈ 4π R⁴ p/(GM)` at `p=100,000 Pa`. The implied upper support radius
limits excess radius with the same smooth limiter. This is not used to infer the
mass of the entire giant envelope. If even the heavy boundary cannot support
one bar, return its radius and condensed-boundary semantics with `SubBarEnvelope`.
Tiny H/He inventories therefore do not fabricate a one-bar reference level.
No giant envelope bottom, ocean bottom or atmospheric discontinuity is inferred.

## Constants, outputs and limits

`structure/parameters.rs` centralises empirical coefficients, model bounds and
engineering defaults separately from physical constants in `units.rs`. Earth
radius is 6,371,000 m and reference flux 1,361 W/m². The mass unit is derived from
the existing orbital `SUN_GM × EARTH_SOLAR / G`, using CODATA 2022 `G=6.67430e-11`.
This preserves the simulator's existing gravitational mass convention.

Outputs are retained kilograms, volume-equivalent metres, H/He mass fraction,
reference semantics, model caveats, `ρ=M/(4πR³/3)` and `g=GM/R²`. Escape is never
included in those quantities. Shape, spin and classification do not enter the
calculation. The solve is algebraic, bounded-cost and deterministic; there is
no iterative convergence failure or random fallback. Invalid/unsupported states
return typed errors. Inclusive mass-bound checks allow 16 machine epsilons for
summation roundoff only.

Provisional descriptive thresholds are centralised for checkpoint 4: metal-rich
at 50% mass, water-rich at 25%, significant H/He at 0.1%, and H/He-dominated at
50%. These are adjustable label choices, currently unused, never physics inputs.
Display precedence and conventional size names are still checkpoint 4 work.

Tests check pure and mixed material ordering, increasing compression, conserved
reservoirs/escape, mole/mass conversion, stripping and blend continuity, invalid
inputs, model warnings and deterministic finite outputs over a prescribed grid.
Broad checks allow 0.1 Earth radii around Earth, 0.06 around Mercury, 3–6 Earth
radii for a prescribed Neptune-like input, 8–12 for Saturn-like and 10–12 for
Jupiter-like. These are regression guardrails, **not measured error bars**.
The mixture/giant approximations have no established statistical uncertainty;
temperature, differentiation, entropy and phase errors can be substantial.
Population calibration and coupling to retention/atmospheres remain future work.

## Sources

- [Seager et al. (2007), Mass-Radius Relationships for Solid Exoplanets](https://arxiv.org/abs/0707.2895),
  ApJ 669, 1279, DOI 10.1086/521346. Pure compressed-material fits only; our
  mixture and high-mass continuation are not their layered numerical solver.
- [Lopez & Fortney (2014), Understanding the Mass-Radius Relation for Sub-Neptunes](https://arxiv.org/abs/1311.0329),
  ApJ 792, 1, DOI 10.1088/0004-637X/792/1/1. Low-mass H/He thickness scaling only;
  our limiting, reference convention and giant surrogate are separate heuristics.
