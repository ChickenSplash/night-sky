# Layered planetary structure · model contract

## Current roadmap and architecture review

Layered checkpoints 1–4 are complete within their declared headless domains;
generation still uses the earlier approximate structure model. Checkpoints 5–12
are the replacement roadmap below. Historical progress entries later in this
file do not reopen completed checkpoints.

The [composition architecture review](planet-composition.md#architecture-review--extensible-materials-and-boundaries)
owns the additional future contracts: one shared material-capability catalogue,
per-body conserved supply, region-owned allocation, mixture/exposure rules,
incremental kernel generalisation and boundary knowledge independent of class or
pressure thresholds. Its finite acceptance map includes thick H/He over rock/ice,
methane liquids, CO₂ critical/triple regimes and unresolved deep boundaries.
Use published reference physics where supported and explicit local approximations
elsewhere. This review changes the plan only, not the solver or completed scope.

## Revised completion scope · scientific plausibility, not research completeness

**User decision, 2026-10-26:** finish checkpoint 3 using scientifically plausible,
explicit approximations. Research-grade accuracy and a complete water phase
diagram are not requirements. This section supersedes the stricter deep-ice
selection/validation gates later in this document; those entries remain a record
of the earlier work, not permission to continue open-ended source investigation.
The user subsequently authorised resumption. **Checkpoint 3 is complete under
this revised scope**, with acceptance results in the
[canonical solver guide](../simulation/planet-composition.md#effective-deep-ice-and-checkpoint-3-acceptance).
The user subsequently authorised committing checkpoint 3 before beginning
checkpoint 4. Push and merge remain unauthorised.

### Fixed scope and model choice

- Keep the working dry, shallow-water and IAPWS implementations. Finish the
  missing deep-water path, rather than restarting the solver or replacing tested
  numerical machinery.
- Retain prescribed isothermal pure water, cold refractory proxies and imposed
  outer pressure. These are conditional structure models, not climate predictions.
- Use a cited analytic compression approximation for deep water ice. The default
  completion route is the Seager et al. (2007), equation 11/table 3 high-pressure
  water-ice proxy, after checking the source parameters and applicable pressure
  range. Give it a separate approximate model identity. It need not reproduce a
  complete Gibbs potential or individual VI/VII/VIII thermodynamics.
- Distinguish liquid water, low-pressure ice Ih and **effective high-pressure
  ice**. Do not label an effective compression branch as a specifically resolved
  ice polymorph. The earlier prescribed Lai VII model can remain a standalone
  comparison; joining it into the radial solution is not a completion requirement.
- At 300 K, use the existing IAPWS VI melting correlation as the approximate
  liquid/deep-ice boundary. An independently selected compression proxy and an
  empirical phase boundary are an explicit approximate composite model, not a
  claim of Gibbs-energy matching. Preserve its physical density discontinuity.
- At 250 K, use a documented approximate low-pressure/high-pressure ice boundary
  from a cited phase diagram, with deeper ice represented effectively. Exact
  intermediate II/III/V/VI/VIII shell sequences are out of scope. If a regime
  genuinely cannot be represented plausibly, return an explicit unsupported
  outcome and document it; do not restart phase-diagram research to unblock it.
- Source checking is limited to what is necessary to implement the selected
  equations, units, domains and attribution correctly. Do not add another EOS
  survey, dataset audit or independent experimental campaign. A material problem
  with the selected proxy must be surfaced as a specific implementation blocker,
  not silently expand the research scope.

### Implementation sequence when resumed

1. **Add the effective deep-ice material prescription.** Check the selected
   published parameters once; implement bounded density evaluation, explicit
   domains and approximate-phase/model metadata. Test transcription, finite
   positive density, compression trends and domain refusals. Do not require
   independent experimental validation across the whole domain.
2. **Integrate the water profile end to end.** Generalise the kernel's current
   single pressure-event support to the ordered events needed for the liquid
   model seam and approximate ice boundaries. Locate events, retain one-sided
   samples and allocate disjoint region inventories. Never integrate a density
   jump as a smooth mixture or repair pressure afterwards.
3. **Pass the finite acceptance suite below.** Fix numerical or accounting
   failures directly. Keep deterministic work limits, including failed/nested
   work; optimise inversions if necessary. Any control/tolerance change needs a
   documented numerical rationale and regression coverage, not hidden extra work.
4. **Close checkpoint 3.** Update canonical model/domain/limitation documentation,
   run the full checks, report the acceptance results and mark this scoped
   checkpoint complete. Do not delay closure for optional source research or
   begin checkpoints 4–6 within this work item.

### Definition of done

- [x] The one-Earth-mass inventory (metal 0.2, silicate 0.5, water 0.3 Earth masses)
  solves at 300 K and an imposed 1 bar, locating a liquid outer boundary and
  plausible compressed deep ice. This warm fixture is mandatory, not merely a
  local density test or a deliberately unsupported result.
- [x] The matching 250 K fixture resolves an ice outer boundary with an explicitly
  approximate deep-ice treatment. The user's follow-up explicitly requests
  approximate cold coverage rather than the former unsupported exception.
  Exact cold polymorph coverage is not a checkpoint-completion blocker.
- [x] Region inventories reproduce retained inventory without double counting;
  independently checked shell mass, hydrostatic balance and inertia close.
- [x] Pressure, enclosed mass and radius are continuous at located interfaces;
  one-sided density jumps and approximate phase/model identities remain explicit.
- [x] Refinement meets the established radius/inertia convergence target on the
  mandatory warm fixture. Profiles are finite, ordered and within declared model
  domains; failures and work exhaustion are deterministic and visible.
- [x] Existing dry, shallow-water, interface and budget regressions pass. Add
  representative deep-water loading, inventory-fraction/compression trend,
  out-of-domain and budget-failure tests. Exact Earth reconstruction or laboratory
  error-bar agreement is not required.
- [x] `./scripts/check.sh` passes and canonical guides describe the implemented
  approximation honestly. Generation, transport, rendering and inspector remain
  unchanged in checkpoint 3.

Completion verification: `data/checkpoint-3-completion-check.log` records the
passing full formatting, workspace-test and strict-Clippy run. The selected
effective-ice prescription, ordered pressure events and mandatory warm fixture
are implemented. The user's follow-up completion pass replaces the cold fixture's
unsupported result with a labelled intermediate-ice compression approximation,
rounded cold phase boundaries and an explicitly identified Ih pressure extension.
Pressure-melted liquid uses existing melting curves and a labelled density proxy,
so the Earth-mass acceptance inventory now solves across 250–300 K rather than
only at isolated warm/cold temperatures. Region metadata exposes approximation
notes. Cold and subfreezing profiles pass independent closure, refinement,
loading and budget checks without another source investigation. The canonical
guides record the assumptions, not claims of precise polymorph thermodynamics.
Follow-up cold/subfreezing completion verification is recorded in
`data/checkpoint-3-cold-completion-check.log`.

### Explicitly deferred, not hidden completion gates

Complete cold-water polymorph sequences; independently matched VI/VII/VIII Gibbs
surfaces; phonon-based thermal ice EOS; full experimental EOS calibration;
adiabatic interiors, climate and heat transport; atmosphere/envelope coupling;
generation/consumer migration and visual acceptance. The last three remain the
separate later checkpoints, not claims made by completing this headless solver.

## Checkpoint 4 implementation

The user authorised atmosphere/envelope coupling after completing checkpoint 3,
then requested committing the checkpoint-3 baseline before starting changes.
Baseline commit: `fc1dd43`; no push or merge was requested.

The opt-in `structure::coupled` API matches dry/water boundary radius, load and
finite atmospheric inventory, with immutable exchangeable supply and disjoint
allocation ownership. It shares cold-trap prescriptions, not generation priors.
The [canonical coupling guide](../simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling)
owns domains, tolerances, bounded work, acceptance fixtures and limitations.
Dense H/He remains the existing explicit envelope surrogate, not a new ideal-gas
interior. Unsupported condensates and weak-binding columns return typed failures.
No generation/consumer migration or camera behaviour change is part of this step.

**Checkpoint 4 is complete within this restricted coupling scope.** Verification:
`./scripts/check.sh` passes formatting, workspace tests and strict Clippy.
Simulation: 352 passed, 2 ignored; client: 363 passed, 4 ignored. Ten new coupling
tests include independent shell mass/inertia/hydrostatic quadrature and exact
aggregate-budget failure checks. All 57 structure tests pass.
Documentation file links and `git diff --check` pass. Full output is recorded in
`data/checkpoint-4-check.log`. The user subsequently requested committing this
checkpoint before generation integration: commit `158db7a`, after another passing
`./scripts/check.sh` run (`data/checkpoint-4-precommit-check.log`). Nothing was
pushed or merged.

## Expanded replacement roadmap · checkpoints 5–12

**User decision, 2026-10-26:** extend material and giant coverage so the old
structure implementation can ultimately be deleted, rather than permanently
selecting it for unsupported generated planets. Every current formation inventory
contains protected non-water volatiles, so the restricted coupler cannot yet
replace structure for natural planets. Dense H/He also needs mixture thermodynamics,
thermal closure and envelope self-gravity, not merely two new density functions.

The authoritative sequence and acceptance gates are in the
[composition roadmap](planet-composition.md#checkpoints-and-acceptance-gates).
Completed checkpoints 1–4 are unchanged. The new sequence is:

| Checkpoint | Scope |
| --- | --- |
| 5 | Full generated-domain coverage matrix and thermodynamic/accounting contract |
| 6 | Secondary volatiles and condensed mixtures, including protected traces |
| 7 | Dense hydrogen, helium and H/He mixture EOS |
| 8 | Thermal profiles and hot/cold water/refractory coverage |
| 9 | Unified self-gravitating structures and outer-column matching |
| 10 | Population coverage, retention migration, validated summaries and bounded cost |
| 11 (formerly 5) | Generation, transport, structured info readout, consumers and legacy deletion |
| 12 (formerly 6) | Calibration, reproducible live examples and user acceptance |

Scientific plausibility with cited, explicit local approximations remains the
standard. Research-complete thermodynamics, phase diagrams and evolutionary
cooling are not prerequisites. Each checkpoint must have finite fixtures and
independent numerical gates, not an indefinite source investigation. Physical
model gaps and numerical failures remain distinct and visible.

The completion boundary now requires removing the old whole-body radius fits,
giant envelope surrogate, heuristic structure inertia and temporary runtime
fallbacks after their generation, retention, validation and consumer dependencies
have migrated. Preserve reusable material/inventory/kernel code. Historical
snapshots may report unknown facts or require regeneration, but must not keep the
old physical solver alive. Do not delete it before replacement coverage and cost
are demonstrated. This request updates the plan only; no new solver implementation,
commit, push or merge is authorised by this planning step.

## Original contract and implementation history

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

Use existing `Material` identities and `Inventory` validation. The implemented
models retain molecular/material budgets, not independent elemental budgets; no
reactions, dissociation, rock-to-gas conversion or hydrogen extracted from water
are currently implied. Expanded checkpoint 5 must define constituent-origin
bookkeeping before enabling EOS-internal dissociation/ionisation. Such internal
states need not change origin inventory, but their particle counts and thermal
quantities must use the EOS consistently, not the dilute molecular gas adapter.
Cross-material chemical reactions remain outside scope unless separately agreed.

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
During temporary migration, an adapter selects either the layered model or the existing
explicitly approximate legacy model and records which supplied the entire
result. Unsupported layered requests may use that explicit legacy path;
unexpected convergence failures must remain visible. Do not fill failed regions
with the legacy radius fit and present the hybrid as a solved structure.
This is a staging policy, superseded as the final completion target by the
expanded roadmap above: checkpoint 11 removes the legacy path only after the
new model covers the declared generated population and all dependencies migrate.

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

## Checkpoint 3 implementation, in progress

User authorised starting checkpoint 3 after the checkpoint-2 commit. The first
material-physics baseline is implemented; this does **not** complete checkpoint 3.
Selection, pinned source releases, permissions, masks and candidate issues are
canonical in [pure-water physics](../simulation/water-physics.md).

- [x] Evaluate AQUA source/version/licence and reported entropy/interpolation issues.
- [x] Implement direct IAPWS liquid and ice-Ih density equations, bounded stable-phase
  selection and explicit coexistence/unsupported-density outcomes.
- [x] Verify against published reference densities, specific volumes and pressure
  curves; test domains, compression, freezing and deterministic failures.
- [x] Declare the first planned thermal closure: prescribed isothermal pure water,
  cold refractory proxies and explicit external load, not a climate prediction.
- [x] Implement and reference-test standalone IAPWS melting branches Ih/III/V/VI/VII,
  without widening the density or equilibrium-phase mask.
- [x] Audit pinned SeaFreeze source/licence/units and probe reference densities,
  melting comparisons and VI/VII Gibbs matching on the intended isotherms.
- [x] Implement a separate direct IAPWS95 compressed-liquid density API to 1 GPa,
  with melting masks, bounded inversion and published reference verification.
- [x] Evaluate corrected AQUA (2026), pin its CC-BY-4.0 dataset and audit phase
  labels, interpolation/extrapolation flags and cold/warm interface-cell coverage.
- [x] Implement and test the prescribed Lai et al. (2023) ice-VII compression
  branch at exactly 300 K and 3.5–20 GPa, with accountable bounded inversion;
  distinguish this from equilibrium selection and reject colder requests.
- [x] Check primary ice-VIII compression isotope coverage and Journaux thermal
  model inputs, without importing D₂O fits as H₂O or omitting phonon pressure.
- [ ] Select/validate high-pressure water and ice models for the water-rich fixtures.
- [x] Reuse the bounded radial kernel for restricted single-phase liquid/ice-Ih
  shells, with a 0.01 Pa outer residual and central-pressure-independent pressure
  error scaling, without changing generation or widening dry input coverage.
- [x] Test restricted water-bearing mass/inertia/hydrostatic closure independently,
  imposed loading, trace conservation, composition interfaces and refinement.
- [x] Locate low-pressure Ih/liquid interfaces where both densities exist,
  using pressure-coordinate endpoint integration without crossing density jumps.
- [x] Test disjoint phase allocations, one-sided endpoint states, independent
  closure, refinement, coexistence outer boundaries and bounded interface work.
- [x] Integrate compressed liquid to 1 GPa within its mask, locate the unforced
  IF97/IAPWS95 model boundary, debit nested inversion work including failures,
  and test conservation, one-sided states, independent closure and refinement.
- [ ] Extend interface matching and independent closure/refinement acceptance to
  high-pressure water-rich fixtures once validated density/phase coverage exists.

The original local API covers 250–623.15 K, 1,000–100,000,000 Pa, with ice Ih
and stable liquid density at ≥273.15 K. Colder pressure-stabilised liquid is
identified but has no enabled density EOS. This limited baseline must not be
represented as a solution of the contract's 30%-water, Earth-mass examples.
Generation, transport, inspector, rendering and dry radial behaviour are unchanged.

The continuation selected direct IAPWS high-pressure melting references and a
separate compressed-liquid density API over 100–1,000 MPa and 273.16–623.15 K,
restricted by the melting curves. A separate prescribed 300 K ice-VII density
branch is now available; no matched deep-ice equilibrium density or solid/solid
selection is enabled. Corrected AQUA offers a distributable candidate, but its
flagged density interpolation crosses some stored phase interfaces and does not
resolve cold ice VIII. SeaFreeze remains reference-only: GPL-3 distribution
terms require resolution, and its direct VI/VII-X Gibbs models did not bracket
equality in their shared pressure domain on either prescribed 250/300 K path.
Quantitative results and the remaining source/matching gate are canonical in
the [water guide](../simulation/water-physics.md#seafreeze-evaluated-reference-not-an-imported-eos).

Baseline verification: `./scripts/check.sh` passes formatting, workspace tests
and strict Clippy. Simulation: 304 passed, 2 ignored (6 new water-material tests).
Client: 363 passed, 4 ignored. Documentation file links and `git diff --check`
pass. Local output: `data/checkpoint-3-water-baseline-check.log`. No commit,
merge or publication is implied by this implementation step.

Continuation verification: `./scripts/check.sh` passes formatting, workspace tests
and strict Clippy. Simulation: 308 passed, 2 ignored (4 additional melting-curve
tests). Client: 363 passed, 4 ignored. `git diff --check` passes. Local output:
`data/checkpoint-3-melting-check.log`. Checkpoint 3 remains incomplete.

Compressed-liquid/revised-AQUA continuation verification: `./scripts/check.sh`
passes formatting, workspace tests and strict Clippy. Simulation: 312 passed,
2 ignored (4 additional compressed-liquid tests). Client: 363 passed, 4 ignored.
The pinned dataset structural audit passes and rejects an incorrect source hash.
Documentation file links and `git diff --check` pass. Local output:
`data/checkpoint-3-dense-liquid-check.log`. This still does not complete
high-pressure ice physics, radial integration or checkpoint 3.

Prescribed ice-VII continuation verification: `./scripts/check.sh` passes
formatting, workspace tests and strict Clippy. Simulation: 316 passed, 2 ignored;
client: 363 passed, 4 ignored. Four new branch-density tests cover numerical
reference evaluation, published lattice-fit deviations, pressure closure and
bounded work/failures. Documentation file links and `git diff --check` pass.
Local output: `data/checkpoint-3-ice-vii-check.log`. No VI/VII/VIII equilibrium
matching or water-bearing radial integration is claimed by this continuation.

The subsequent restricted radial continuation is implemented and described in the
[water-shell solver guide](../simulation/planet-composition.md#headless-restricted-water-shell-solver).
Its first version rejected phase crossings and the Earth-mass 30%-water fixtures.
Six initial tests
cover analytic three-layer mechanics, independent radial closure, conservation,
loading/refinement and explicit physical/numerical failures. The nine dry solver
regressions remain unchanged and pass against the shared numerical kernel.

Restricted radial verification: `./scripts/check.sh` passes formatting, workspace
tests and strict Clippy. Simulation: 322 passed, 2 ignored; client: 363 passed,
4 ignored. Local output: `data/checkpoint-3-water-radial-check.log`.

The supported Ih/liquid interface extension now uses one-sided named densities
and a pressure-coordinate endpoint step, preserving a common radius, enclosed
mass and nominal melting pressure without pressure repair. Four additional radial
tests and one material test cover analytic event location, independent closure,
phase allocations/refinement, coexistence endpoints and work limits. Cold liquid,
deep ice and the Earth-mass 30%-water fixtures remain unsupported, so checkpoint 3
is still incomplete. Numerical scope and tolerances remain canonical in the
[solver guide](../simulation/planet-composition.md#headless-restricted-water-shell-solver).

Interface-extension verification: `./scripts/check.sh` passes formatting,
workspace tests and strict Clippy. Simulation: 327 passed, 2 ignored; client:
363 passed, 4 ignored. Local output:
`data/checkpoint-3-water-interfaces-check.log`.

The compressed-liquid radial continuation now joins the separate liquid models
at a located 100 MPa model boundary, not a phase change or forced density match.
Three additional radial tests and one material-budget test cover the extension.
The existing request ceiling is preserved; the loaded pure-water stress fixture
explicitly exhausts it. Scientific and numerical scope is canonical in the
[solver guide](../simulation/planet-composition.md#headless-restricted-water-shell-solver).
Deep ice, cold liquid and the Earth-mass 30%-water fixtures remain unsupported.
Checkpoint 3 is not complete; generation and presentation remain unchanged.

Compressed-liquid radial verification: `./scripts/check.sh` passes formatting,
workspace tests and strict Clippy. Simulation: 331 passed, 2 ignored; client:
363 passed, 4 ignored. Local output: `data/checkpoint-3-dense-radial-check.log`.

The next deep-ice continuation prepares a reproducible independent H₂O ice-VI
density reference from Chodkiewicz et al. (2022). The hash-pinned source audit
separates isotope samples, counts the two pure-H₂O refinements as one observation,
and preserves its actual 293(2) K temperature and 1.5(1) GPa pressure. Five offline
regressions now run in `scripts/check.sh`. Source details and the discovered
formula/density metadata trap are canonical in the
[water guide](../simulation/water-physics.md#independent-ice-vi-density-reference-isotope-metadata-must-be-audited).
This is research/validation infrastructure only: no new runtime EOS or deep-water
profile is enabled. Distributable density coverage and supported VI/VII matching
remain open, and checkpoint 3 is still incomplete.

Reference-audit verification: both pinned-source hashes and the structural audit
pass, including the H₂O density calculation. `./scripts/check.sh` passes five
offline audit regressions, formatting, workspace tests and strict Clippy.
Simulation: 331 passed, 2 ignored; client: 363 passed, 4 ignored. Local output:
`data/checkpoint-3-ice-vi-reference-check.log`. A local SeaFreeze comparison at
the measurement's actual P–T is documented as a diagnostic, not an equilibrium
validation or a statistical accuracy claim. No generation/presentation changes,
commit, merge or publication are implied.
