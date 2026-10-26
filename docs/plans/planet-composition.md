# Planet composition replacement

## Status and goal

**One active roadmap.** Replace the generated whole-body radius, envelope and
inertia approximations with one conserved, region-owned structure system covering
the declared generated population. Generated planets still use the existing
approximate model; the replacement solvers remain headless and opt-in.

Scientific plausibility with published physics and explicitly bounded local
approximations is the standard, not research-complete thermodynamics. Missing
coverage must not be concealed by dropping traces, relabelling origins, narrowing
generation inputs or returning an unlabelled legacy fallback.

| Documentation | Owns |
| --- | --- |
| This plan | Current status, replacement contract, remaining work and exit gates |
| [Implemented composition and structure](../simulation/planet-composition.md) | Runtime behaviour, headless APIs, model domains, equations and measured validation |
| [Water physics](../simulation/water-physics.md) | Implemented water prescriptions and water-source evaluation |
| [Scientific reference](../simulation/planet-composition-reference.md) | Generated-input bounds, selected/candidate models, source pins, unresolved scientific issues, fixed requests B1–B12/B3w and proposed budgets |

Checkpoint numbers below refer only to the layered replacement. Completed
implementation history is available in Git.

## Completed foundation

The generated inventory/escape/reservoir model, composition-first labels,
transport and inspector are implemented. Their current approximations remain
documented in the simulation and client guides until replaced.

| Checkpoint | Completed scope | Canonical result |
| --- | --- | --- |
| 1 | Conservation, region and numerical contract | Contract below; implemented guides describe actual support |
| 2 | Bounded cold metal/silicate radial solver | [Dry solver](../simulation/planet-composition.md#headless-dry-body-solver) |
| 3 | Restricted water shells and explicitly approximate deep ice | [Water solver](../simulation/planet-composition.md#headless-restricted-water-shell-solver), including solved 250/300 K, 30%-water fixtures |
| 4 | Restricted atmosphere/condensed-boundary coupling | [Coupling](../simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling) |
| 5 | Scoped readiness review, not complete physical coverage | [Scientific reference](../simulation/planet-composition-reference.md) |
| 6a | Protected CO₂/N₂/CH₄/O₂/Ar traces in dry hosts | [Protected-trace solver](../simulation/planet-composition.md#headless-protected-traces-in-dry-hosts) |
| 6b | Protected secondary traces in water-bearing hosts | [Water-host trace solver](../simulation/planet-composition.md#headless-protected-traces-in-water-bearing-hosts) |
| 6c | Offline methane density-inversion/coexistence numerical gate, not runtime phase coverage | [Methane numerical gate](../simulation/planet-composition-reference.md#methane-inversion-and-coexistence-numerical-gate) |

These are complete within their declared domains. Exact cold-water polymorphs and
matched deep-ice Gibbs surfaces do not reopen checkpoint 3. Further source surveys
do not reopen checkpoint 5. Water hosts, exposed secondary condensates and dense
envelopes do not extend checkpoint 6a's completed scope.

## Replacement contract

### Conservation and ownership

- Keep catalogue capability, per-body retained supply and solved allocation
  separate. The catalogue describes supported physics; formation/escape determine
  supply; immutable solved regions own its allocation.
- Retain ten stable constituent origins in kilograms. Each region's per-origin
  allocations sum to retained supply; retained plus escaped equals source.
  Interior, hydrosphere and atmosphere totals are projections, never a second set
  of editable allocations. The outer column cannot own envelope mass twice.
- EOS-internal H₂/H/H⁺/electrons do not create additional origin mass. Hydrogen
  bound in water/methane remains in those origins. Cross-origin reactions need a
  separately agreed elemental/reaction contract. Ionised states cannot use an
  unchanged dilute molecular-gas/optics adapter.
- Preserve protected versus exchangeable supply. A trace can occupy a declared
  mixed host region; it need not form a pure shell and must not vanish under a
  tolerance scaled to total planet mass. No automatic exposure or implied solubility.
- Use SI units, f64, deterministic generation and stable body identities. Labels
  remain descriptions, never selectors for physics, spin, shape or boundaries.

### Material, phase and thermal support

- A local density EOS, phase selector, thermal path and allocation/partition rule
  answer different questions. None alone determines a whole planet's arrangement.
  Prescribed differentiation or segregation is not simulated evolution or proven
  immiscibility. Do not sort shells by ambient density.
- Each capability needs identity/mass convention, model/version, units, physical
  validity mask, required thermal inputs, citations, licence/provenance and explicit
  approximation/unsupported states. A rectangular data grid is not a physical mask.
- Prefer published equations and measured reference properties. Selected source
  equations and successful offline algebra audits do not establish runtime coverage.
  A missing model becomes a specific blocker for its owning checkpoint, not an
  unbounded research programme.
- Mixture mechanics must declare component masks and nonideal treatment. Use the
  selected H/He block correction once, with H/He-normalised X/Y; explicit heavy
  origins must not also enter effective helium abundance. Pure-fluid critical
  points or averaged critical constants do not determine mixture phase splits.
- Density-only prescriptions promise no entropy, cp, latent heat or adiabat.
  Align energy/entropy zeros before thermal use. Coexistence needs an inventory/
  volume or energy closure, not arbitrary phase fractions from P/T alone.
  Near-critical ill-conditioning must preserve supply and report resolution limits.
- Age and irradiation do not uniquely determine entropy or interior temperature.
  Thermal prescriptions need anchors, masks and provenance, including age zero.
  Unknown refractory solidity is not a solid-core claim.

### Regions, numerics and boundaries

- Integrate all allocated mass in volume, self-gravity and axial inertia using
  `dm/dr=4πr²ρ`, `dP/dr=-Gmρ/r²` and `I=(8π/3)∫ρr⁴dr`, under explicit thermal and
  outer conditions. Retain spherical hydrostatics and separate spheroidal shape.
- Preserve a regular centre, mass-located interfaces and one-sided states. Radius,
  enclosed mass and pressure are continuous at material interfaces; density may
  jump. A change of density implementation is provenance, not automatically a
  physical phase interface. Declare thermal interface conditions separately.
- Validate independent mass/hydrostatic/inertia quadrature, analytic limits,
  refinement, domain edges and deterministic failures. Numerical convergence is
  not scientific accuracy. Preserve stricter existing tests when adding routes.
- Count rejected, nested inversion, partition and matching work against shared
  ceilings. No repaired pressure profile, rescaled radius or partial successful
  planet after failure. Distinguish missing capability, physical-domain exit,
  weak-binding/no-static-solution, nonconvergence and budget exhaustion.
- Report composition interfaces, phase interfaces, density-model seams, pressure
  references, optical extent and mechanical solidity independently, with explicit
  resolved/approximate/unresolved knowledge. No class, gas fraction or pressure
  threshold creates or removes ground. One bar is a reference, not a floor.
- A liquid top and submerged solid floor are different boundaries. Continuous
  supercritical fluid need not have a gas/ocean interface. A model stopping before
  a boundary is not proof that solids are absent deeper down.
- At consumer migration, supported exposed liquid may reuse smooth spheroidal
  ground geometry and its top radius for presentation/approach clearance. Preserve
  liquid identity and submerged boundaries; this grants no landing permission.
  Thickness follows allocated mass/density, never an arbitrary visual minimum.
  Protected deep liquid and trace water in a continuous envelope create no surface.

### Code ownership and migration

Under `crates/simulation/src/system/`:

| Area | Responsibility |
| --- | --- |
| `composition` | Inventory validation and conservation bookkeeping |
| `materials` | Shared identities, local EOS/phase/mixture capabilities and source data |
| `structure` | Regions, radial integration, thermal paths and boundary matching |
| `atmosphere` | Outer columns and optical adapters using solved facts |
| `generation` | Procedural inputs, retention and bounded orchestration |
| `physical` | Validated transport; clients consume facts rather than infer boundaries |

Consolidate reusable molecular/phase properties into `materials` as consumers
migrate, rather than copying the legacy catalogue or depending on presentation
types. Material extensions must cover stable IDs/serialization, inventory/gas
mapping, capability/domain checks, mixture/allocation, transport and reference
tests. Check completeness over `Material::ALL`.

Incrementally generalise the existing radial kernel and genuinely shared result/
control types when new regions require it. Preserve analytic, trace, quadrature
and work-accounting tests. Do not restart the solver or build a speculative plugin
framework. Temperature-dependent events must follow the chosen P–T path.

The old generated solver remains in use during development, not as the final
answer to missing physics. Delete its whole-body fits, envelope surrogate,
heuristic inertia and temporary adapters only after coverage, cost and caller
migration are demonstrated. Preserve inventories, useful local EOS and independent
fixtures. Old snapshots may report unknown facts or require regeneration; they
must not reactivate a deleted solver.

## Remaining checkpoints and exit gates

### 6. Secondary volatiles and condensed mixtures

**Partially complete: 6a and 6b are complete within their headless domains.** Remaining work is
runtime pure secondary fluid inversion and physical masks; solids/sublimation and
uncertain freezing edges; exposed condensates; mixed-region and partition rules.
The offline methane numerical gate (6c) is complete, not runtime fluid support.
Use the selected prescriptions only within their documented restrictions.

Before each implementation slice, fix its inputs, models, acceptance fixtures,
exclusions and stop condition. Exposed methane, dense envelopes and population
migration must not be appended to the protected-trace work automatically.

#### 6b. Protected traces in water-bearing hosts, frozen slice

**Complete.** Seven regressions and `./scripts/check.sh` pass, including 368
simulation tests (2 ignored), 363 client tests (4 ignored), offline references,
formatting and strict Clippy. The [canonical guide](../simulation/planet-composition.md#headless-protected-traces-in-water-bearing-hosts)
owns API semantics and measured closure, refinement and sensitivity results.

Inputs: separate metal/silicate/protected-water host and protected
CO₂/N₂/CH₄/O₂/Ar inventories, imposed water temperature and outer load, and existing
bounded controls. Require positive water, total mass 0.0001–20 Earth masses,
`f≤0.00301` and the unchanged water-host P–T masks. Use
`entrained_secondary_host_volume_v1` throughout every host region, including each
water phase/model segment. No exposed/exchangeable supply is accepted or inferred.
The caller owns that distinction. Trace chemistry and phase remain unresolved;
water phase metadata applies only to the host, not entrained origins.

Fixed acceptance:

- Host (metal,silicate,water)=(0.2,0.5,0.3) Earth masses, load 1e5 Pa, at
  250/260/300 K: zero traces recover the existing water result; mixed endpoint
  `f=0.00301`, shares (CO₂,N₂,CH₄,Ar)=(0.49,0.49,0.01,0.01), preserves all
  phase/model events. Set `M_trace=f M_host/(1-f)`.
- At 300 K, each of the five origins at trace/host ratios 1e-12, 1e-7 and 0.003;
  preserve every positive origin in every host segment, with species-local 1e-12
  allocation closure. Repeat/reversed construction must be deterministic.
- Independent radial quadrature: region and per-origin mass/inertia 1e-8,
  hydrostatic drops 2e-4. Fourfold step/tolerance refinement on the three mixed
  endpoints: radius/inertia/central pressure 1e-7, each region's origin allocation,
  thickness and pressure drop 1e-5. At nominal density, compare mechanics and
  event masses with the pure-host solve scaled by total/host mass.
- Test-only λ=0.5/1/2 sensitivity on those mixed endpoints: radius change ≤1%,
  dimensionless inertia change ≤2%. This is not a measured uncertainty interval.
- Pure-water and thin-water hosts; one-sided event loads, including Ih/liquid;
  invalid inventories, missing water, mass/P–T exits, numerical-resolution and
  deterministic work refusals, including nested EOS exact-budget success/failure.
  Existing dry/water/coupling behaviour, limits and regressions stay unchanged.

Stop after these gates, canonical documentation and `./scripts/check.sh` pass.
Report measured results or a concrete blocker. Do not append secondary-fluid
inversion, exposed methane, solid EOS, partition, thermal evolution or generation/
transport/rendering migration. These remain separate checkpoint-6/later work.

#### 6c. Methane inversion/coexistence reference gate, frozen slice

**Complete within this offline scope.** All three pinned source rows, ten new
synthetic regressions and the full check suite pass. Measured errors, numerical
limits and root-search restrictions live in the
[scientific reference](../simulation/planet-composition-reference.md#methane-inversion-and-coexistence-numerical-gate).

Implement an offline, standard-library numerical audit using the pinned methane
Helmholtz equation, not a runtime planetary route. Inputs are T/P and explicit
vapour/liquid molar-density brackets; coexistence additionally requires a pressure
bracket. Ancillaries supply estimates only. Differentiate power/Gaussian residuals
analytically through second density order, require positive compressibility at
every evaluated branch point, and solve equal pressure/Gibbs energy. Compare all
supplied fluid candidates by Gibbs energy; ties require additional volume closure.
This does not certify that caller brackets enumerate every fluid root.

Acceptance: all three pinned methane coexistence rows, off-saturation pressure
neighbours at ±1e-4, analytic derivatives against independent differences, repeated/
reversed branch order, fourfold tolerance refinement, and the existing five
mass/volume fractions. Recover pressure/densities to 1e-6 relative and equal Gibbs
to 1e-8 in RT units; recover quality to 1e-8. Exercise invalid/unstable/unbracketed
inputs, missing volume/critical contrast, iteration and shared nested work limits,
including exact-budget success and one-fewer failure. Every EOS probe is charged.
Synthetic tests run without downloaded files in `./scripts/check.sh`.

The finite audit window is 95–172 K / 1e4–3e6 Pa, intersected with the methane
freezing correlation. It is not a new general-purpose physical mask or an extension
of the published EOS. Stop after these tests, measured diagnostics and documentation.
Exclude runtime coefficient shipping, global root enumeration, solids/sublimation,
other species' uncertain freezing/critical edges, calorics, mixtures, exposed shells
and generation integration. Those remain open checkpoint-6/later requirements.

Exit: cold/warm secondary and mixed-water fixtures conserve every origin and pass
independent closure, one-sided interface/seam, refinement, domain and work-refusal
tests. Cover exposed methane and solid/liquid/gas/supercritical CO₂, not just gas
and ice. Replace heuristic cold traps on this new path; any retained dilute
shortcut needs its own error mask. Deep/hot extensions belong to 8, complete
planet matching to 9.

### 7. Dense hydrogen, helium and H/He mixtures

Implement the nonideal local EOS and thermal quantities, with pure/mixed limits,
physical masks, interpolation/inversion conventions, cold-dense/dilute joins and
bounded failures. No ideal-gas giant continuation. Heavy loading needs an explicit
prescription; no automatic helium core or implied helium rain.

Exit: reference equations, compressive stable branches, thermal derivative/adiabat
consistency, mixture limits and dilute joins pass. Cover required molecular,
dissociated/ionised states without confusing particle states and origin inventory.

### 8. Thermal profiles and hot/cold coverage

Choose prescribed adiabats or a labelled approximate equivalent, irradiated outer
anchors and an age/entropy or internal-luminosity prescription. Align thermodynamic
zeros. Extend hot/dense/vapour water and refractory/secondary pressure-temperature
coverage as required, without inferring melting from cold density.
Prefer valid entropy-contour adiabats with independent `d ln T/d ln P|S` checks;
a gradient proxy needs a selected value, anchor, mask and provenance.

Exit: finite cold/warm/strongly irradiated and young/old paths fit the selected
masks and pass thermal/interface tests. No full climate, evolutionary cooling
simulation or complete refractory phase diagram is required.

### 9. Unified structures and outer-column matching

Combine dry, volatile-rich, small-envelope and giant profiles under one region
contract. Include envelope self-gravity and inertia. Replace the dilute coupler's
1% restriction only on the newly validated path. Match mass, pressure and radius,
with explicit thermal, finite-tail and weak-binding conventions and one inventory
owner. Finish the currently incomplete planet-fixture inputs before claiming them.

Exit: independent closure/refinement, seams, stripping limits and deterministic
failures pass across complete planet fixtures. Giant mass/radius/inertia trends
must be plausible before generation integration, not left to visual judgement.

### 10. Population readiness, retention and bounded cost

Exercise sampled extrema and source-derived bounds, not just convenient planets.
Replace retention's old structure dependencies, including escape speed and H/He
survival, without rerunning loss history or inadvertently changing IDs/orbits.
Measure deterministic reuse/acceleration, total nested work, memory, cache keys/
versioning, eviction, concurrency and order-independent reproducibility.

Exit: no unexplained generated coverage holes or solve failures, no reliance on
legacy structure physics, and a measured generation/search/validation strategy.
Snapshot checks validate inventories, interfaces, provenance and boundaries without
repeating the entire solve or trusting arbitrary derived values. Proposed budgets
are in the scientific reference; interactive performance benchmarks remain user-run.
Generation-domain changes require documented physical rationale and user agreement.

### 11. Generation, transport, consumers and legacy deletion

Make unified structures drive retained planets at final-orbit forcing. Carry
consistent allocations, radius/reference levels, gravity, phase/unknowns and inertia
through snapshots, shape, atmosphere/optics, rendering and navigation. Preserve
unrestricted free-camera descent and liquid-top semantics.

Provide structured readout of assumptions, ordered regions, composition/phase,
mass/radius bounds, P/T endpoints, references and diagnostic limitations. Summaries
come first; full sampled profiles may be on demand.

Exit: all runtime callers, including retention and validation, migrate; code and
dependency searches plus tests prove the obsolete structure solver and fallbacks
are gone. Missing coverage keeps this gate open, not working code deleted early.

### 12. Calibration and acceptance

Check reference trends and population changes, conservation, interfaces, stripping,
shape/inertia, clearance and readout/rendering semantics. Run `./scripts/check.sh`
and affected shader checks. Supply reproducible natural examples of airless rock,
ice/liquid water, secondary volatiles, small envelopes and giants, including
approximate/unresolved phases. The user reviews changed presentation and runs
interactive benchmarks. Update canonical guides and confirm legacy deletion.

Commit, push and merge require their own authorisation.

## Scientific blockers and fixture ownership

The [reference](../simulation/planet-composition-reference.md) owns exact requests
and source findings. These are remaining route gates, not reasons to reopen
completed checkpoints or expand a slice mid-implementation.

| Missing capability / decision | Owner | Acceptance evidence |
| --- | --- | --- |
| Secondary fluid masks/inversion and solids/sublimation/freezing edges | 6; deep/hot extensions 8 | B3/B3w/B4/B5/B10; resolve or explicitly bound O₂/Ar edge uncertainties before affected support |
| Generated arrangement, exposure and mixed-interface partition | 6 local, 9 planet closure | B3/B3w/B9; jointly closed atmosphere/condensate mass, not arbitrary fractions |
| Additive-volume and H/He-block implementation | 6/7 local, 9 integration | B11/B12, pure/trace limits; no inferred mixture equilibrium or duplicated corrections |
| H/He masks, interpolation and joins | 7 | B6, including cold-He rejection and supported warm neighbour |
| Thermal anchors and hot/deep component routes | 8 | B1/B2/B7–B9 cold/hot/young/old paths, age zero and full forcing envelope |
| Complete planet inputs and unified matching | 9 | Complete B1–B10/B3w thermal, outer and partition inputs; independent hydrostatic/inertia/stripping/seam checks |
| Full generated envelope, retention and cost | 10 | B7/B8, sampled extrema, source bounds and measured work/data costs |
| Readout, liquid geometry, legacy removal and acceptance | 11–12 | Consistent consumer facts, no runtime fallback, reproducible examples and user review |

Real-time chemistry, convection, impacts, weather, helium rain and full planetary
evolution remain outside this replacement. Patchy lakes, continents, waves and
local redistribution belong to the [surface plan](planet-surfaces.md), using the
same conserved material rather than decorative extra inventory.
