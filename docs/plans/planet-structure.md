# Layered planetary structure · model contract

Checkpoint 1 of the [composition plan's next phase](planet-composition.md#next-phase--layered-planetary-structure).
Specification prepared 2026-10-25; the user subsequently authorised checkpoint 2
after discussing its dry/cold scope and headless tests. The separate dry solver
is implemented; the broader water/envelope model remains future work. Current
inventory, reservoir and radius approximations remain documented in the
[composition guide](../simulation/planet-composition.md).

## Completion target

Derive radial material distribution, meaningful outer boundaries and supported
phase states from conserved material supply and explicit physical assumptions.
Eventually distinguish exposed rock/metal, ice, liquid-water boundaries and
continuous fluid envelopes. Always distinguish a conditional model prediction
from unresolved physics. A single global boundary does not describe continents,
patchy oceans or local ice cover.

A giant's reference-pressure radius is not ground. No condensed surface in the
supported outer profile does not imply that no solid material exists deeper
inside. Giant core composition, mixing and solidity are separate questions; the
first solver cannot answer them. Helium is not automatically sorted into a core.

## One owner for every kilogram

1. Formation/source and escaped inventories are conservation bookkeeping.
   `retained[material] = source[material] - escaped[material]`.
2. Retained inventory is the immutable input to one structure/partition solve.
3. A successful solution owns disjoint physical regions. Each region has a
   material inventory; their per-material sum equals retained inventory.
4. Interior, condensed and atmospheric reservoir totals are derived projections
   of those allocations. They are never a second set of mutable allocations.

Use existing `Material` identities and `Inventory` validation. Material masses
remain molecular/material budgets, not independent elemental budgets. No
reactions, dissociation, rock-to-gas conversion or extra hydrogen extracted from
water are implied. Later molecular dissociation requires an explicit extension
to this accounting contract before importing an EOS that assumes it.

Regions may contain mixtures. A differentiated metal core and silicate mantle
are a prescribed arrangement, not the result of simulating differentiation or
sorting materials by their ambient density. Zero-mass regions are omitted.
Composition interfaces and phase boundaries are different: the same water
inventory can span multiple phases; a mixed H/He envelope is one material mixture.

The envelope and its outer atmosphere cannot both own the same material. Either
the atmosphere is a diagnostic slice of an envelope region, or that region is
split into disjoint mass intervals. A cutoff used for rendering does not delete
unrendered material. Any physical atmosphere cutoff and tail-mass convention
must be part of the mass-normalisation model, not chosen by the renderer.

## Inputs and outputs

### Solve request

- Immutable retained inventory in kilograms; source/escape bookkeeping belongs
  to the caller and must already have been checked.
- Explicit physical model/version, differentiation prescription and material
  EOS selections. Class names, UI labels and random draws are not inputs.
- Outer boundary condition: imposed absolute pressure and the thermal condition
  required by that model. Vacuum pressure zero is valid for the first dry solve.
- Thermal prescription and anchors where supported. The initial cold model
  explicitly ignores thermal pressure and does not invent an interior temperature.
- Bounded numerical controls. Use existing SI constants and f64 throughout.

Irradiation, age and entropy are not interchangeable. The later generation adapter
may choose a thermal prescription from irradiation/age, but must retain its
provenance and assumptions. Equilibrium temperature is not a solved surface
temperature and does not establish an ocean or greenhouse state.

### Successful structure

- Ordered regions with inner/outer radius, enclosed mass at their endpoints,
  material inventories and a named physical prescription.
- Radial pressure/density profiles, and temperature/phase only where known.
  Preserve one-sided states at discontinuities; do not interpolate across an
  interface as if it were a homogeneous mixture.
- Located material boundary and reference-pressure radii with explicit semantics.
  A requested pressure level that does not exist returns absent, not a fabricated
  radius. A truncated profile reports its deepest supported boundary as a model
  limit, not as the planet's surface or core boundary.
- Retained mass, reference radius, mean density, reference gravity and axial
  moment of inertia, with model provenance and limitations. The spherical solve
  gives `I = (8π/3) ∫ρ(r)r⁴ dr`; report `I/(M R_ref²)` with its reference specified.
  Continue to treat rotational shape separately, not as a solved 2D interior.
- Numerical diagnostics: central pressure, work counts, boundary/mass residuals
  and resolution checks where requested. Numerical convergence is not scientific
  accuracy, and a model caveat is not a statistical error bar.

Keep phase knowledge independent of mechanical support. A cold proxy can return
resolved material boundaries while reporting refractory phase as unresolved.
For later phase-capable models, record solid, liquid, vapour, supercritical,
coexisting phases or unresolved, with material-specific phase IDs where justified.
Supercritical requires the applicable material model, not a generic pressure rule.
At a coexistence point, pressure/temperature alone need not determine phase
fractions: require an energy/entropy or allocation constraint, or return unresolved.

## First implementation: cold differentiated dry bodies

Select the modified-polytropic **density EOS**, not the existing whole-body
mass-radius interpolation, from [Seager et al. (2007), equation 11 and table 3](https://ar5iv.labs.arxiv.org/html/0707.2895):

```text
ρ(P) = ρ₀ + c Pⁿ
```

| Material proxy | ρ₀ (kg/m³) | c (kg/m³ · Pa⁻ⁿ) | n |
| --- | ---: | ---: | ---: |
| Iron/nickel budget represented by Fe | 8300 | 0.00349 | 0.528 |
| Silicate budget represented by MgSiO₃ | 4100 | 0.00161 | 0.541 |

The paper describes fits below 10¹⁶ Pa. Our initial engineering domain is narrower:
retained mass 0.0001–20 Earth masses, pressure 0–10¹³ Pa throughout the profile,
and **only metal and silicate**. These bounds are a conservative implementation
scope, not measured accuracy limits. Pure-material endpoints are valid; place
all metal inside all silicate when both are present. No homogeneous mixture
rule, porosity, alloy partitioning, thermal expansion, melting or mineral phase
claim is supported. The EOS's cold/room-temperature provenance does not prescribe
a physical 300 K planet or prove a solid core.

The mass and pressure bounds intersect, not every mass/mixture/load combination
is promised a solution. Checkpoint 2 confirms that the 20-Earth-mass 70%-metal
and nearly/pure-metal fixtures cannot be bracketed within the pressure ceiling;
they fail explicitly rather than extending the domain.

The first public solver must reject any positive water, H/He or secondary-gas
mass, including traces. Do not silently drop it, absorb it into rock or apply the
old water proxy. Nonzero imposed outer pressure may be used as an explicitly
specified external load for tests; it does not allocate or solve an atmosphere.

Constant-density profiles are analytic test fixtures, not a selectable physical
model for generated planets. For a uniform sphere, check
`R = (3M/(4πρ))^(1/3)`, `P_c - P_out = 3GM²/(8πR⁴)` and `I/(MR²) = 2/5`.

## Radial and numerical contract

Solve mass conservation and hydrostatic balance, closing density with the selected
EOS and, later, temperature with the selected thermal model:

```text
dm/dr = 4πr²ρ
dP/dr = -Gmρ/r²
```

The material masses fix interface mass coordinates, not guessed interface radii.
The centre satisfies `m(0)=0` with finite density and pressure. Do not evaluate
`m/r²` at zero; use a centre expansion or an equivalent regularised formulation.
The outer endpoint has the requested total mass and pressure. Across an interface,
radius, enclosed mass and pressure are continuous; density may jump. Thermal
interface conditions must be declared by a future thermal model, not inferred
from a density jump.

Checkpoint 2 may choose the integration coordinate and method without changing
this contract. Initial central-pressure bracketing starts above the imposed outer
pressure, expands at most 64 times and never exceeds the EOS pressure ceiling.
Use at most 128 shooting iterations, 32,768 attempted integration steps per trial
and 2,000,000 EOS evaluations per solve, including rejected steps and refinements.
An exhausted budget is an explicit failure, not the last trial presented as a
solution. These are initial deterministic safety ceilings to calibrate, not a
performance promise or a search-time budget.

Initial acceptance targets, to be exercised and justified in checkpoint 2:

- Shooting residual: `|P_outer - P_requested| ≤ max(1 Pa, 10⁻⁸ P_c)`; this is a
  numerical residual only and is not sufficient for phase/low-pressure accuracy.
- Integrated total and each region's mass agree with their positive requested
  mass to relative 10⁻⁸. Do not scale a trace region's tolerance by planet mass.
- Allocation bookkeeping agrees per material to relative 10⁻¹⁰ of that material's
  retained mass; a zero requested mass permits no positive allocation. Numerical
  shell quadrature and exact inventory ownership are separate checks.
- Tightening integration accuracy by a factor of four changes radius and inertia
  factor by at most relative 10⁻⁵ on the acceptance fixtures. Analytic fixtures
  independently meet relative 10⁻⁶ in radius, pressure increment and inertia.
- Pressure decreases outward, enclosed mass increases, all nonempty regions have
  positive thickness, and all sampled values are finite and within EOS domain.

Do not rescale a failed profile or adjust its pressure after solving to hide
failed closure. Check results with independent quadrature and analytic limits,
not just identities calculated from the same cached totals. Later surface-phase
work must add local pressure/temperature and phase-boundary convergence tests;
the dry solver's global pressure tolerance cannot certify liquid water.

## Water, atmospheres and giants: explicit extension gates

| Regime | Selected direction | Gate before enabling |
| --- | --- | --- |
| Water-bearing structure | Phase-aware P–T EOS, starting evaluation with AQUA and IAPWS | Pin source/data version, licence, units, domain masks, phase selection, interpolation tests and thermal closure |
| Dilute outer gas | Existing ideal-gas mixture and spherical column, within its supported domain | Solve boundary load/partition and column mass together; preserve explicit weak-binding limitations |
| Dense H/He | Dedicated mixture EOS and prescribed entropy/thermal profile | Validate dense/outer matching, self-gravity, mass and energy conventions; no ideal-gas extrapolation |
| Refractory melting/core solidity | Thermal, phase-aware metal/silicate models | Validate phase boundaries and thermal assumptions; cold density EOS is insufficient |

[AQUA (Haldemann et al., 2020)](https://arxiv.org/abs/2009.10098) combines water
EOS over a broad domain; its [MIT-licensed repository](https://github.com/mnijh/AQUA)
documents the component models and tabulated quantities. It is a candidate, not
an imported or validated dependency. Check subsequent corrections, particularly
entropy, before selecting data for adiabatic integration. EOS table coverage
alone does not prove accurate phase labelling; verify a phase selector separately.

[IAPWS-95](https://iapws.org/technical-guidance/release/IAPWS-95) covers stable
fluid water from the melting curve to 1273 K and up to 1000 MPa. It is not a
complete high-pressure ice model. Its ice/melting companion formulations must
be selected with their own domains; never extend the low-pressure freezing point
of water to every depth. A water-rich shell needs an explicit P–T path and
supported phase interfaces before it can locate an ice or liquid surface.

For dense H/He, evaluate published mixture models such as
[Chabrier & Debras (2021)](https://doi.org/10.3847/1538-4357/abfc48) and
[Howard & Guillot (2023)](https://arxiv.org/abs/2302.07902), including their
mixing assumptions. No giant EOS/data version is selected in this checkpoint.
Helium rain, diffuse-core mixing and heavy-material solubility are not produced
by the differentiated dry solver. Do not manufacture pure-He or solid-core labels.

## Example requests and expected coverage

These are input/coverage fixtures, not fabricated solved profiles. Masses use
the project's existing Earth-mass constant; omitted materials have zero mass.
Full source inventory equals retained inventory and escaped mass is zero in
these examples. All temperatures below are prescribed, not climate predictions.

| Fixture | Retained inventory (Earth masses) | Boundary / thermal request | First dry solver | Later acceptance intent |
| --- | --- | --- | --- | --- |
| Dry differentiated | Metal 0.325, silicate 0.675 | Vacuum, cold density approximation | Solve core/mantle, phase unresolved | Independent mass/radius/inertia tests, not exact Earth reconstruction |
| Cold water-rich | Metal 0.2, silicate 0.5, water 0.3 | Prescribed outer 1 bar, 250 K; water thermal path required | Unsupported material | Locate supported ice boundary and deeper phases under explicit assumptions |
| Warm water-rich | Same as cold fixture | Prescribed outer 1 bar, 300 K; water thermal path required | Unsupported material | Distinguish liquid boundary from ice under supported phase physics |
| Thin atmosphere | Metal 0.325, silicate 0.674999, nitrogen 0.000001 | Prescribed outer-column 250 K; base pressure is an output | Unsupported material | Match atmospheric inventory/load and condensed boundary, never add gas mass again |
| Small envelope | Metal 1.5, silicate 3.5, hydrogen 0.075, helium 0.025 | Outer thermal condition and envelope entropy required | Unsupported material | Continuous mixed H/He envelope; find 1 bar only if present |
| Giant | Metal 10, silicate 10, hydrogen 223.5, helium 74.5 | Giant EOS and thermal anchors required | Unsupported material | Explicit approximation until dense-mixture model is validated; no claimed core solidity |

One-bar imposed loads in the water fixtures represent an external boundary
condition, not a hidden atmosphere with unaccounted mass. Coupled-atmosphere
tests must supply its inventory explicitly instead. Missing thermal anchors
are missing inputs, never permission to invent a temperature or entropy.

## Failure, migration and module boundaries

The future API distinguishes invalid input, unsupported constituent/thermal/EOS
domain, unbracketed solution, nonconvergence, work-budget exhaustion and invalid
output closure. Errors should identify the material/region and offending state.
No partial profile is a successful planet; diagnostics may retain it separately.

Initially this solver runs only on prescribed tests, not generation or search.
During migration, an adapter selects either the layered model or the existing
explicitly approximate legacy model and records which supplied the entire
result. Unsupported layered requests may use that explicit legacy path;
unexpected convergence failures must remain visible. Do not fill failed regions
with the legacy radius fit and present the hybrid as a solved structure.

`system::composition` keeps inventory conservation. Shared identities/properties
move to `system::materials` when the first EOS implementation needs them, with
compatibility re-exports as appropriate; do not copy the catalogue or make
material physics depend on atmospheric gas types. `system::structure` owns the
radial solver, regions and thermal/boundary matching. Generation owns retention
and orchestration, not hydrostatic maths. Transport, inspector and rendering
consume immutable validated results. Avoid speculative public types, a new
crate or an extensible plugin framework before the solver establishes the API.

## Checkpoint 1 acceptance

- [x] Specify inventory/region ownership, inputs, outputs and unresolved states.
- [x] Select a cited first EOS and explicit supported/unsupported domains.
- [x] Specify thermal assumptions, boundary conditions, deterministic work limits,
  initial tolerances, independent validation and typed failure categories.
- [x] Record dry, water, atmosphere, envelope and giant coverage fixtures.
- [x] Keep population behaviour unchanged and identify future extension gates.
- [x] User authorises checkpoint 2 after clarification of the dry/cold scope.

## Checkpoint 2 implementation

- [x] Separate cold metal/silicate solver with no generation or population switch.
- [x] Regular centre, mass-located interfaces, imposed outer load and integrated inertia.
- [x] Immutable region-owned allocations, one-sided profiles and unresolved phase.
- [x] Bounded bracketing, shooting, adaptive integration and total EOS work.
- [x] Analytic uniform/two-layer solutions, independent radial quadrature,
  refinement, trace regions, domain refusals and explicit budget exhaustion tests.

Implementation, numerical targets exercised and diagnostic fixtures are canonical
in the [solver guide](../simulation/planet-composition.md#headless-dry-body-solver).
This does not complete water phases, dense envelopes, generation migration or
visual acceptance, and does not establish scientific accuracy of the cold proxies.

Verification: `./scripts/check.sh` passes formatting, workspace tests and strict
Clippy. Simulation: 298 passed, 2 ignored (9 new dry-solver tests). Client:
363 passed, 4 ignored. Documentation file links and `git diff --check` pass.
