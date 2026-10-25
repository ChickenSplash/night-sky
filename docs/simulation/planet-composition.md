# Material inventories and approximate structure

The physical foundation lives in simulation `system/composition.rs`
and `system/structure.rs`. [Planet generation](planetary-systems.md) now draws
inventories first and derives retained mass, radius and inertia from them.
Atmospheric gas and condensed reservoirs now share the same conserved inventory;
validated inventory/reference transport and descriptive labels feed the inspector.
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

### Descriptive labels

Descriptive thresholds are centralised separately from structure inputs. Display
precedence uses retained mass fractions in this order:

1. H₂/He at least 50%: **Gas giant**.
2. H₂/He at least 10%: **Envelope world**.
3. H₂/He at least 0.1%: **Sub-Neptune**.
4. Water at least 25%: **Water-rich planet**.
5. Iron/nickel at least 50%: **Metal-rich planet**.
6. Otherwise: **Rocky planet**.

Equality takes the earlier rule. These are adjustable, disjoint display buckets,
not universal size definitions: conventional giant/sub-Neptune names can overlap
observationally. Water-rich does not mean an ocean, and envelope-world does not
assert an ice interior. No mass-based `Dwarf` class is generated or searchable:
dynamical orbital clearing is not established here. Changing these thresholds
changes descriptions/search matches only, never structure, spin, gas or optics.

Tests check pure and mixed material ordering, increasing compression, conserved
reservoirs/escape, mole/mass conversion, stripping and blend continuity, invalid
inputs, model warnings and deterministic finite outputs over a prescribed grid.
Broad checks allow 0.1 Earth radii around Earth, 0.06 around Mercury, 3–6 Earth
radii for a prescribed Neptune-like input, 8–12 for Saturn-like and 10–12 for
Jupiter-like. These are regression guardrails, **not measured error bars**.
The mixture/giant approximations have no established statistical uncertainty;
temperature, differentiation, entropy and phase errors can be substantial.
Population calibration remains future work.

## Formation priors and retained structure

`generation/materials.rs` isolates tunable population heuristics. These are not
observationally fitted disc-accretion models. For proposed formation distance
`x=a/snow`, ice availability is `I=x⁴/(1+x⁴)` and feeding factor is
`(1+2I)/(1+(x/5)²)`. Draw a log-uniform heavy mass from 0.0022–12 Earth masses
times feeding, with a 0.0022 minimum. A compact retry multiplies feeding by 0.01
before that minimum. Water is `I × uniform(0.05,0.6)` of heavy mass; secondary
molecules together take a log-uniform 1e-7–0.003 share. The remainder is metal
and silicate, with metal taking 15–70% of rock. Secondary material is 98% split
between CO₂ and N₂ (5–90% CO₂ share), 1% CH₄ and 1% Ar. No free O₂ is drawn.

Heavy material starts in interior (rock/metal) or unresolved condensed volatiles
(water/secondary molecules). This is an initial accounting allocation, not a
solved phase state. H/He acquisition uses gas/heavy mass ratio
`onset × (M_heavy/5)^2.5 × log_uniform(1e-4,30) × min(M_host,1)/(1+x/20)`.
`onset=clamp((M_protected-0.1)/0.1,0,1)` smoothly excludes unsupported small-envelope
bodies. `M_protected` subtracts all accessible water/secondary material from heavy
mass, so subsequent escape cannot move a trace envelope below the 0.1 Earth-mass
domain. This is a formation prior, not a post-loss mass clamp. Gas is 75% H₂ and
25% He by mass, limited to 3,900 minus heavy mass in
Earth units, within the structure domain. Lower-mass hosts therefore acquire
less gas at equal core/seed. All allocations are scaled together if formation
mass exceeds 0.4% of current host mass. The recomputed sum is the source budget.

The formation mass sets conservative Hill spacing. Two algebraic inequality
solutions replace unbounded outward stepping; the host-mass cap keeps both
denominators positive. Resonances and companion bounds still constrain final
placement, and the 1,000 AU compact retry remains. The proposed formation location
is retained even when spacing moves the final orbit outward. Evolved hosts keep
their existing progenitor snow/inner scales rather than using present luminosity.

Final structure shares the atmosphere model's cached 32-phase all-star heating
and activity/history-weighted loss forcing. For original gas mass `M_g`, solve
retained fraction `s` such that `s=1/[1+(25/λ)^4]`, where `λ=GM μ_H2/(R kT_exo)`
is evaluated at the **retained** mass and radius. The structure solve uses
current flux and age, while `T_exo=max(1000 max(F_loss/1361,0.01)^0.2,2T_eq)`
and `T_eq=max(20,[0.7F/(4σ)]^0.25)` are fixed-albedo heuristic estimates.
These are not actual XUV spectra, Jeans escape rates or integrated evolution.
H₂ and He have the same retained fraction, deliberately deferring fractionation.
Rock/metal are fully retained. Accessible secondary loss is applied first, using
the fixed pre-loss source structure at the final irradiation and age (see below).
It is not iterated inside the H/He solve. All remaining heavy material enters
every H/He trial, including its protected and accessible volatile inventories.

Scalar bisection brackets `s` on [0,1], with at most 40 steps. Stop when absolute
survival residual or bracket width is ≤1e-8. This bounds gas-mass uncertainty to
1e-8 of original gas supply for a bracketed continuous root when the width
criterion is met. A residual stop only bounds the survival equation's residual,
not distance to a unique solution. It does not establish physical uncertainty.
The stripped endpoint is continuous in radius; bisection does not require a
monotonic radius law. No-envelope inputs bypass iteration. Unsupported structure
or exhaustion returns an explicit error; production asserts its supported draws
cannot reach that failure, rather than substituting an invented fallback world.
Re-evaluation always starts from immutable source H/He, so loss is not compounded.
Mass, radius, inertia and descriptive classification are updated together; neighbour
stability is checked again with retained masses, without moving any orbits.
The cost is bounded algebraic structure work per bisection step, not numerical
interior integration or repeated stellar phase sampling.

## Secondary escape and reservoir partition

The accessible fractions are tunable outgassing/exposure priors: `a=1e-5` of
source water and `a=0.1` of each secondary molecular constituent. The rest stays
protected in `CondensedVolatiles`. No mantle release, ocean evaporation history,
photochemistry, oxygen production or exchange between molecular budgets is solved.
For source mass `S_i`, accessible survival `r` gives retained mass
`S_i [1-a_i(1-r)]`, escaped mass `S_i a_i(1-r)` and available gas supply
`retained_i - S_i(1-a_i)`. Cold trapping does not delete the protected supply.

`r=1/[1+(F_loss/(1361 × 10 × (v_escape/11200)^4))^1.5]` is the existing
shoreline-inspired trend, now used as a surviving **mass fraction**, not a random
probability that air exists. Escape speed uses immutable pre-loss source structure
at current flux/age. This fixed reference avoids a second nonlinear solve but
does not reproduce feedback from secondary loss, envelope shielding or differential
escape. It is not an integrated escape rate or a fitted universal shoreline.
Re-evaluation starts from source, never from already depleted retained material.

The [atmosphere model](atmospheres.md#inventory-backed-generation) partitions the
accessible retained supply using its prescribed temperature and saturation caps.
All retained H₂ and He remain in `Gas`; other molecules not in gas remain in
`CondensedVolatiles`, including newly cold-trapped material. This reservoir name
does not assert a resolved phase, ocean or exposed ice layer. Atomic reservoir
transfers preserve authoritative retained/escaped totals, so partition does not
change bulk mass, structure or spin. Sub-precision gas transfers are omitted, not
manufactured. Trace gas below the 0.1 Pa display cutoff remains accounted for.

Boundary-column pressure is `p=M_gas g/(4πR²)`. One-bar envelopes instead use
the complete gas reservoir's molecular mixture at a fixed reference pressure;
that column is never inverted to infer total giant mass. Both obey partial-pressure
cold caps with explicit mass-to-mole conversion. The partition uses a fixed
64-step scalar bisection, not a deep atmosphere integration. Constant gravity,
ideal gas and well-mixed bulk gas are prescriptions, especially unreliable for
dense secondary air and extended columns. Heavy vapour still uses the cold bulk
radius proxy, with no steam-envelope expansion or greenhouse feedback. These
limitations are explicit rather than silently imposing an arbitrary pressure cap.
Emitted columns are checked against saturation and one-bar supply with a 1e-9
relative roundoff allowance; failure is explicit rather than a fabricated column.

## Inertia and spin

The structure model adds an explicitly heuristic inertia factor for Darwin-Radau:
`k=clamp((1-f) k_heavy (R_heavy/R)^2 + 0.25f,0.12,0.36)`, where
`k_heavy=0.36-0.08 × metal/heavy mass` and `f` is retained H/He fraction.
The radius ratio represents a compact heavy component under a larger envelope;
the bounds avoid claiming resolved central concentration. This is not a layered
moment-of-inertia integration or a prediction of giant interiors.

For spins, set `e=clamp(f/0.5,0,1)`, `q=clamp((0.02-M)/0.02,0,1)` (Earth mass
units). The lock distance is `(0.3-0.2e)(1-q) M_host^(1/3)` AU. Within 2.5 times
that distance, bodies with `f<0.1` can receive the existing slow or 3:2 states.
Otherwise free periods are log-uniform between `10-e-6q` and `60-45e+340q`
hours. With probability `(0.75+0.1e)(1-q)`, tilt is Rayleigh with scale
`22-4e` degrees; otherwise isotropic. These are prescribed Solar-System-inspired
outcomes, not integrated tidal histories. Spin streams are per stable body ID,
independent of orbital/material/atmosphere draws. Free spins are limited using
the physical inertia factor and the existing 0.2 flattening cap. Locked/slow
states keep their assigned periods, with the existing shape cap as a safeguard.

`Planet::inertia_factor` is optional for legacy compatibility, transported early
on both generated and task-overlay paths. Valid values override all class strings
in `planet_shape`. Only records without composition may use the isolated legacy
class fallback; malformed new records use a neutral factor, never their label.

## Transport and downstream consumers

`Planet::physical` carries stable material IDs with source, retained, escaped,
interior, condensed and whole-gas masses, plus reference structure and stable
approximation IDs. It is optional: absent means unknown, not zero inventory.
`system/physical` reconstructs validated inventories/budgets, re-derives structure
at transported irradiation/age and checks all duplicated facts, radius semantics
and caveats (1e-10 relative roundoff tolerance). Unknown/duplicate/missing materials,
nonfinite values and inconsistent records are refused explicitly. This bounded
algebraic check does not repeat stellar heating or retention iteration.

Generated systems and task overlays use one conversion. Their physical records
must match for the same body ID; task metadata never alters them. New reflectance
uses retained water/metal and H₂/He continuously, not classification. See the
[rendering guide](../client/rendering.md#planet-reflectance-and-phase) for this
unresolved appearance proxy. Old records keep isolated class-based reflectance
and shape compatibility; invalid new reflectance uses a neutral proxy. Geometry,
surface boundary semantics and atmospheric optics do not inspect class strings.
Search consumes descriptions only. The [typed inspector](../client/ui/interface.md#breadcrumbs-and-inspector)
and developer export share presentation fields, separate from these physical records.

## Sources

- [Seager et al. (2007), Mass-Radius Relationships for Solid Exoplanets](https://arxiv.org/abs/0707.2895),
  ApJ 669, 1279, DOI 10.1086/521346. Pure compressed-material fits only; our
  mixture and high-mass continuation are not their layered numerical solver.
- [Lopez & Fortney (2014), Understanding the Mass-Radius Relation for Sub-Neptunes](https://arxiv.org/abs/1311.0329),
  ApJ 792, 1, DOI 10.1088/0004-637X/792/1/1. Low-mass H/He thickness scaling only;
  our limiting, reference convention and giant surrogate are separate heuristics.
