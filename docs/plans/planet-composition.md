# Planet composition replacement

## Status and goal

**P1 complete and signed off on 27 October 2026. Next: P2 step 6, cheap
layered wins.** The live, generator-backed rock-and-water prototype provides
conserved, layered planets alongside the legacy model. Population-wide cutover
and legacy removal remain P3.

Generated planets still use the approximate model in `structure::derive` by
default. The opt-in prototype joins dry, water, protected-trace and
atmosphere-coupling solvers, with background loading and live inspector results.
Rendering and navigation still use the legacy model.

This is a simulator, not a thermodynamics research project. Prefer the simplest
scientifically plausible model that supports the needed behaviour. Named,
bounded approximations and explicitly unresolved phases are acceptable. Exact
phase diagrams and publication-grade accuracy are optional improvements, needed
only when a demonstrated simulation or game problem requires them.

**Policy revised, 28 October 2026:** every planet gets a new-model result.
The layered solver covers what it can; every refusal gets a simple, total,
explicitly labelled **fallback** structure inside the same entry point and
result type, carrying its refusal reason. Target about **95% layered** coverage,
with the rest on the fallback and **no refusals**. Further physics slices are
optional later work, not P2 exit gates.

Use named, scoped approximations with visible global and region
fidelity/limitations. Conservation, numerical closure, finite physical
quantities and no hidden legacy substitution remain non-negotiable. Fallback
results are always distinguishable from layered solves and never counted as
layered coverage. No universal physical radius-error tolerance is agreed.

Legacy removal waits for P3: rendering, shape, landing, navigation, optics and
snapshots still read the legacy record, and the fast synchronous first answer
is still legacy.

| Document | Owns |
| --- | --- |
| This plan | Delivery order, scope, decisions and acceptance |
| [Composition and structure guide](../simulation/planet-composition.md) | Implemented behaviour, APIs, equations, limitations and numerical tests |
| [Scientific reference](../simulation/planet-composition-reference.md) | Sources, generated-input bounds and candidate higher-fidelity fixtures |
| [Water physics](../simulation/water-physics.md) | Implemented water prescriptions and their evidence |

### Resume here (October 2026)

- **Done in P1:** steps 1–9, plus the frost deposit, bounded hot-steam extension
  and [unbound-column blow-off](../simulation/planet-composition.md#unbound-column-blow-off).
  Nothing is pushed: neither
  the main branch nor `park/methane-physics` (parked methane physics). Ask the
  user before pushing either.
- **Current coverage:** 1,380/1,544 layered (89.4%), 164 on the fallback,
  no refusals. Next: step 6, time-boxed.
- **Performance and next step:**
  - Refinement-only warming optimisation is implemented: a paired 20-thread
    run reduced report wall time from 19.5 to 18.2 s and solved median from
    152 to 127 ms, with unchanged outcomes and negligible numerical differences.
    Interior search hints are also implemented: a separate paired 20-thread run
    reduced wall time from 17.9 to 15.5 s, typical ocean solves from 3.05 to
    2.22 s and reported radial EOS work by 28%, with unchanged outcomes and
    negligible numerical differences. Gas-search caching was reverted at the
    user's request. Further optimisation is deferred: seconds per system are
    acceptable for the prototype. Step 7 removed the separate protected-host
    re-solve: coverage is unchanged, with a 13.9 s report on 20 threads.
  - Step 8 now queues/caches background refinement, sharing a ¾-CPU budget with
    star streaming (including grid workers), with star admission priority and
    one reserved star slot on multi-thread budgets. F3 → perf → experimental
    planet loading enables it. Fast initial generation remains on the main
    thread, logged per system; two preliminary headless samples measured
    1.4–5.6 ms. Interactive hitch testing was not required for P1 sign-off;
    address any issues as they arise.
  - Step 9 converts each solve to a `LayeredPlanet` record kept beside the
    legacy one, shown in the inspector's layered section and
    `/state/planet_loading` (`ns planets`). Rendering, shape, landing and
    atmosphere optics stay legacy for every planet; refusals show the labelled
    fallback (step 5). Full legacy removal remains P3.
   - **P1 signed off, 27 October 2026:** the user accepted the dry (Ab) and steam
     (Ae) in-client reviews as sufficient. Separate ocean/icy reviews and
     interactive benchmarks are waived for P1, not outstanding exit gates or
     automatic P2 tasks. Existing benchmarks target atmosphere rendering and
     galaxy flight/loading rather than the new layered facts. Future issues
     will be addressed as they arise. P2 steps 1 (input census), 2 (prescription
     selection) and 3 (first coupled structural delivery) are implemented.
     Runtime envelopes are opt-in and need a locally prepared EOS resource;
     step 4 closes the remaining measured domains and numerical gaps.
- **After each step:** rerun the coverage report
  (`cargo test --release -p space-simulation composition_coverage -- --ignored
  --nocapture`), update the baseline here and in the
  [guide](../simulation/planet-composition.md#prototype-coverage-report), then
  run `./scripts/check.sh`. The user runs benchmarks and visual review.
- **Key code:** `structure/coupled.rs` (loop, `Temperature`),
  `coupled/{column,warming,deposit}.rs`, `atmosphere/greenhouse.rs`,
  `structure/coverage.rs` (route, categories), `materials/water.rs`.

## Decisions

- **Rock and water first.** Metal, silicate and water get layered structure for
  every rock-planet condition the generator produces.
- **Other volatiles ride along, with no new physics.** CO₂, N₂, CH₄, O₂ and Ar
  keep the generator's existing split: protected supply entrained in the host
  through the existing trace prescription, accessible supply in the atmosphere
  or escaped. Methane is an ordinary gas like CO₂, not a separate material model.
- **Methane physics is parked.** The runtime methane fluid kernel, cold-state
  approximation, condensed shells, dilute exchange and the offline inversion gate
  live on the `park/methane-physics` branch. Revive only for a demonstrated need.
- **No water-amount threshold.** Water always gets its own layer, however thin.
  Earth's oceans are about 0.023% of its mass, so a trace cut-off would remove
  Earth-like oceans. Thin layers need the right phase (ocean, ice or none) and
  conserved mass, not a precise EOS.
- **Trace H/He is an ordinary gas.** Bodies beyond the thin column's limits take
  the self-gravitating envelope route (P2), else the fallback.

## Completed foundation

Preserve these implementations and their tests. Their declared domains stay
unchanged unless a step below deliberately widens them with a labelled model.

| Historical checkpoint | Reusable result |
| --- | --- |
| 1–5 | Conserved inventories, region/numerical contract, [dry](../simulation/planet-composition.md#headless-dry-body-solver) and [water](../simulation/planet-composition.md#headless-restricted-water-shell-solver) mechanics, [thin-atmosphere coupling](../simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling) |
| 6a–6b | Protected secondary origins in [dry](../simulation/planet-composition.md#headless-protected-traces-in-dry-hosts) and [water-bearing](../simulation/planet-composition.md#headless-protected-traces-in-water-bearing-hosts) hosts |

## Measured starting point

The [coverage report](../simulation/planet-composition.md#prototype-coverage-report)
(October 2026, 1,544 planets from 140 calibration systems, provisional joined
route, greenhouse surface temperature) gives the P1 baseline:

**P1 baseline, after unbound blow-off:** 1,017/1,032 non-envelope planets solve
(99%), with 512 envelopes. Weakly bound columns lose their lightest exchangeable
gases (178 planets, at most 3.7% of their mass) instead of failing. Remaining
first failures: dense gas 8, warming outside the domain 7. Surfaces: frost 422,
ice 202, ocean 28, rock 365. Cost on 20 threads: 156 ms median, 593 ms p95,
9.8 s maximum.

## Replacement contract

Keep these invariants while simplifying physics and delivery:

- **One conserved owner:** region allocations sum to retained supply for every
  constituent; retained plus escaped equals source. Atmosphere, hydrosphere and
  interior totals are projections, not extra editable mass.
- **No lost or relabelled origins:** retain every positive constituent, including
  traces. Protected supply is not automatically exchangeable.
- **Honest approximations:** every model has an identity, assumptions, domain and
  practical checks. Never extrapolate a local EOS silently, narrow generated
  inputs to fit it, or hide a legacy fallback behind a new API.
- **Full mechanics:** allocated mass counts in volume, gravity and inertia.
- **Consistent interfaces:** radius, enclosed mass and pressure agree across
  material boundaries. No pressure translation, radius rescaling or partial
  successful solve after failure.
- **Separate facts:** unknown solidity is not absent ground. Class labels and
  pressure thresholds must not create or remove surfaces. One bar is a
  reference, not a floor. Liquid tops and submerged floors differ.
- **Deterministic, bounded work:** SI/f64, stable IDs and orbits, counted work.
  Convergence tests do not establish physical accuracy.

Reuse the existing radial kernel and module ownership: `composition` owns
bookkeeping, `materials` local properties, `structure` allocation and mechanics,
`atmosphere` columns and optics, `generation` inputs and retention, and `physical`
transport. Do not build a parallel solver framework or a material plugin system.

## P1. Rock-and-water prototype, complete

Each step is small and labelled. Rerun the coverage report after each one.

1. **Coverage report. Done.** A reproducible command routes a fixed deterministic
   set of generated planets through the prototype path. It reports success,
   first failure reason by category, work counts and timings.
2. **Surface temperature. Done.** A labelled
   [grey greenhouse estimate](../simulation/atmospheres.md#grey-greenhouse-estimate)
   from partial and total pressure (H₂O, CO₂ and CH₄ absorb), fitted to Earth,
   Mars and Venus, sets one surface temperature for the column and the water.
   It is solved on the coupled column, taking the coolest balance warming up
   from equilibrium; a runaway past the column domain is refused. An Earth twin
   now has a liquid ocean near 290 K. Not a climate model.
3. **Cold and airless water. Done.** A labelled
   [surface-water model](../simulation/water-physics.md#cold-and-low-pressure-surface-water)
   gives IAPWS ice Ih below 250 K down to 20 K and at any surface pressure, and
   IF97 liquid below 1 kPa down to its saturation pressure. The cold
   intermediate and temperature-independent deep-ice branches extend to 20 K.
   The coupling's column floor is now generation's 20 K, with labelled notes
   for extrapolated cold-trap caps.
4. **Vapour allocation. Done.** All water trades with the air: exposed water
   evaporates until the air is saturated (IF97 saturation above 273.16 K), leaving
   an ocean or ice below, or all of it becomes steam over dry rock. Generation's
   protection shields water from escape only. The coupling loop re-solves the
   load; an ocean under pure steam takes the liquid-side limit on the saturation
   curve. Small warm bodies initially had steam columns too weakly bound to
   hold; the later unbound-column blow-off resolved these failures.
   **Frost:**
   cold-trapped CO₂, N₂, CH₄ and Ar above their vapour-pressure cap settle as a
   labelled [surface deposit](../simulation/planet-composition.md#frost-surface-deposit)
   on top of the rock or water, at fixed solid densities, with its weight on the
   body below.
5. **Hot and deep water. Done.** One labelled approximation for hot water
   above 623 K and warm deep ice (300–623 K beyond 1 GPa), using effective
   high-pressure ice at depth whatever the temperature.
   The [bounded hot/deep extension](../simulation/water-physics.md#hot-and-warm-deep-prototype-extension)
   preserves the original local APIs and cold boundaries, uses a warm effective
   depth proxy beyond 1 GPa, and an unresolved hot-fluid specific-volume law
   above 623.15 K through 2,000 K. The column extends to 2,000 K with a separately
   labelled ideal-steam approximation (≥90% water by mass, ≤50 kg/m³ above
   1,000 K). No arbitrary dense gas or self-gravitating steam envelope is solved.
   The gas-mass and binding guards remain; out-of-domain cases stay unsolved.
6. **Trace rounding. Done.** Water transfers below one machine epsilon of the
   planet's water are accepted with a documented, species-local allowance,
   instead of being rejected.
7. **One entry point. Done.** `structure::prototype::solve`: generated budget and environment in; one region-owned
   result out, with layers, atmosphere, model labels and boundary facts. It joins
   protected traces in the host with atmosphere coupling and blow-off. Protected
   material participates in every iteration, not an after-the-fact host solve.
   The result accounts for source, generation's prior escape and new blow-off.
   All 1,544 coverage labels and escape fractions remain unchanged; small physical
   changes reflect the formerly omitted protected mass in the coupled gravity.
8. **Experimental mode. Done.** An explicit mode in `generated_system_with_mode`, separate from
   the default. Envelope planets return a labelled unsupported result, never a
   quiet per-body substitution of the legacy solver. The client prepares inputs
   and solves planets off-thread under the shared star/planet work budget, with
   selected-planet queue priority, bounded result polling, request cancellation
   and a four-system in-memory result cache. The F3 perf switch defaults off.
   Live consumers deliberately stay unchanged until step 9. Coverage's 1,544
   non-timing rows are identical to step 7.
9. **Consumers and inspector. Done.** Results map to `LayeredPlanet`, whose
   `physical` keeps the `PlanetPhysical` shape, beside (not replacing) the legacy
   record, so existing snapshots and consumers work unchanged. The inspector
   shows layers, model identity, legacy comparisons and failure reasons; `ns
   planets` and `set_layered_planets` expose them. Only bare rock or ice-Ih is
   `solid_surface`; navigation and landing remain legacy, so liquid-top
   semantics and free camera descent are untouched. All 1,017 solved coverage
   planets convert with closed mass and contiguous layers; coverage rows are
   unchanged.

Left as a documented limitation: each water layer has one temperature, so a deep
ocean is as warm as its surface. Revisit only if layer results look wrong.

**Exit accepted, 27 October 2026:** 1,017/1,032 non-envelope planets solve (99%),
with conserved origins, measured cost and passing `./scripts/check.sh`. The user
signed off after dry and steam world reviews, waiving separate ocean/icy reviews
and interactive benchmarks. Known unsupported cases remain explicit for P2;
future issues will be handled as they arise.

**Stop:** no CO₂ or methane phase physics, exact Gibbs audits, envelope physics
or population-wide cutover in P1.

## P2. Envelopes and remaining coverage

Prioritise gaps measured by the P1 report.

### Step 1. Generated-input census, complete (28 October 2026)

Extended the existing probe, with no new solver physics. Reproduce with:

```sh
NS_COVERAGE_THREADS=20 cargo test --release -p space-simulation composition_coverage -- --ignored --nocapture
```

Outputs in `data/tmp/`: `composition-coverage.{md,tsv}` and
`composition-census.md`. The census includes grouped bounds and deterministic
candidate IDs; the TSV retains original columns and appends composition inputs.
All 1,544 rows match step 9 in every original non-timing column: 1,017 solved,
512 envelope-labelled refusals and 15 other refusals.

| Observed gap/input | Measured requirement |
| --- | --- |
| H/He mass guard | 168 bodies have more than 1% H/He; total masses 1.19–1,089.67 Earth masses |
| Dense-gas guard with H/He present | 342 bodies, 0.102–19.58 Earth masses, H/He fractions about 5.15 × 10⁻¹³ to 0.00986. These are not all giants; H/He presence alone does not establish which gas causes the refusal |
| Radial mass guard | Two first failures above 20 Earth masses; 11 envelope-labelled bodies have non-H/He inventory above 20 Earth masses, reaching 26.35. This inventory includes water and secondaries, not just a rock/metal core |
| H/He mixture | Helium is 25% of retained H/He by mass throughout the sampled envelope-labelled population |
| Water-bearing interiors | Water reaches 59.38% of non-H/He mass. A dry-core-only envelope model would miss generated inputs |
| Thermal inputs | Envelope-labelled raw equilibrium temperatures span 0.604–1,073.82 K. The existing prototype floors its thermal input at 20 K; raw irradiation is not deep temperature or evidence for an EOS domain below that floor |
| Other dense-gas failures | Eight H/He-free bodies, 0.0398–0.0908 Earth masses, equilibrium 80.5–317.6 K |
| Other warming failures | Seven bodies, 0.267–3.30 Earth masses, equilibrium 176.5–335.7 K; no successful warmed temperature is available |

Legacy classes within the 512 refusals: 38 gas giants, 42 envelope worlds,
190 sub-Neptunes, 142 rocky, 56 metal-rich and 44 water-rich planets. These
labels describe generator output, not solved new-model structure. First
blockers can mask later failures; coverage does not yet prove the sufficiency
of any proposed envelope prescription.

Representative starting fixtures (calibration IDs, not real galaxy locations):

| Input | Calibration planet ID | Total mass, Earth masses | H/He mass fraction |
| --- | --- | --- | --- |
| Trace H/He, dense-gas refusal | `composition-calibration:Bulge:10/4` | 0.102 | 5.15 × 10⁻¹³ |
| Small envelope | `composition-calibration:ThickDisc:9/2` | 4.91 | 0.0206 |
| Intermediate envelope | `composition-calibration:Bulge:18/5` | 9.03 | 0.218 |
| Gas giant | `composition-calibration:ThickDisc:16/8` | 110.52 | 0.875 |
| Largest giant | `composition-calibration:NuclearDisc:12/5` | 1,089.67 | 0.994 |
| Largest non-H/He inventory, water-bearing | `composition-calibration:Arms:9/6` | 29.24 | 0.0990 |
| Hottest envelope-labelled input | `composition-calibration:Arms:10/0` | 8.18 | 0.00141 |
| Coldest raw irradiation input | `composition-calibration:Halo:3/9` | 1.36 | 0.0492 |

The generated census also selects median-mass examples by legacy class and
first blocker, input extremes and all 15 other refusals. These are regression
candidate inputs, not reference answers for envelope structure.

### Step 2. Prescription selection, complete (28 October 2026)

The [canonical prescription and decision probe](../simulation/planet-composition.md#p2-envelope-prescription-decision-probe)
owns equations, assumptions, reference comparisons and reproduction commands.

- **Observed gap:** 168 H/He mass refusals plus dense-gas failures across thin
  remnants, intermediate envelopes and giants. Water-rich interiors must remain
  distinct. Thin molecular gas cannot be extended merely by lifting its limits.
- **Selected density:** original CMS19 pure H/He densities with HG23 additive
  volume correction, uniformly mixed with separate conserved species. No heavy
  mixing. Use existing separately downloaded and hash-audited sources for the
  decision probe; source packaging/permission remains a gate before runtime use.
- **Selected thermal form:** grey irradiation plus intrinsic flux, opacity
  proportional to pressure, joined continuously in value and slope to an
  effective 2/7 deep temperature gradient. Not table entropy integration or a
  claim to resolve molecular dissociation and true deep adiabats.
- **Internal heat:** 50 K intrinsic reference, with 30/100/200 K sensitivity
  experiments. This is not core temperature or an age prediction. Choose a
  runtime default only after coupled radius/reference tests, not by guessing
  an unvalidated cooling law.
- **Fixtures:** existing source density points, including the negative
  pure-He melting-screen case, and cold/temperate/giant/hot thermal experiments.
  Six synthetic regressions run in `check.sh` without external data. These check
  construction, not physical accuracy against independent planets.
- **Explicit gaps:** below-table cold atmosphere, cold dense phases and
  quantum/plasma masks, valid dilute joins, interpolation stability and source
  packaging. Finite table density or a positive corner derivative does not
  establish a physically supported envelope. No unlabelled extrapolation.
- **Stop condition met:** prescriptions selected and executable decision checks
  provided, with sensitivity assumptions and unresolved domains explicit.
  No runtime EOS, envelope hydrostatics, giant radii or new coverage successes
  are claimed. Coverage remains 1,017 solved / 512 envelope-labelled / 15 other
  refusals; step 3 is the first coupled structural delivery.
- **Validation:** `./scripts/check.sh` passes. A fresh 20-thread coverage run
  matches all 1,544 original non-timing rows; source-based probe output is in
  `data/tmp/p2-step2-prescription.json`.

### Step 3. Coupled structural path implemented (28 October 2026)

- **Observed gap:** thin atmospheres cannot carry envelope self-gravity or
  consistently compress the heavy interior. The step-2 probe alone solved no
  planets.
- **Delivery:** a centre-to-outer-shell integration through the existing radial
  kernel, with all parcel mass in gravity, volume and RK-integrated inertia.
  Shared material-boundary pressure/radius/mass are identical by construction.
  Reference gravity is iterated from the new one-bar pressure event, not legacy
  geometry. Separate origins, protected heavy traces and accessible envelope
  traces remain conserved. Positive water always has its own region.
- **Bounded approximations:** local pinned CMS19/HG23 density resource, dilute
  sub-100 K volume anchor, explicit pure/quantum screens and pressure-only
  unresolved water compression under at least 10,000 bar. Accessible secondary
  traces share host volume without gaining protection. Original thin and local
  EOS contracts are unchanged. The new route permits heavier interiors, but
  retains the existing heavy-EOS pressure ceiling.
- **Thermal reference:** experimental 100 K intrinsic forcing with a prescribed
  2/7 deep slope. It is not an age/heat-history inference or a validated
  universal default. Whole-population accuracy and cold/Jupiter-like reference
  coverage remain open; do not promise a percentage accuracy.
- **Packaging gate:** no dataset redistribution. A deterministic converter
  prepares a locally supplied, SHA-pinned resource; missing/corrupt data is
  explicitly unsupported. Deployable bundled-data permission remains future
  work, not silently assumed.
- **Results:** 117 new solves: 85 sub-Neptunes, 28 envelope worlds and four
  lower-mass gas giants by legacy class, up to 39.07 Earth masses. Four have
  non-H/He inventory above 20 Earth masses. Total coverage is 1,134/1,544;
  all 1,017 P1 successes are identical in every original non-timing column.
  Full giant coverage is not claimed.
- **Checks:** `./scripts/check.sh`, pinned density/negative-phase fixtures and
  paired source-backed half-step/tighter-integration checks pass. The small
  5-Earth-mass heavy body plus 0.1-Earth-mass H/He fixture has a one-bar radius
  of 16,208 km; radius and inertia refine within relative 10⁻⁷. This is numerical
  stability, not a physical accuracy measurement. Synthetic checks independently
  verify shell inertia, conservation, interfaces and non-landable transport.
- **Cost:** the final 20-thread report took 22.6 s. New-envelope solved times were
  about 0.92 s median and 1.28 s p95; load-dependent, not an interactive
  benchmark. No gas-search cache or looser P1 integration tolerances were added.

Current envelope-route first blockers (395 total):

| Reason | Count |
| --- | ---: |
| Accessible secondary traces outside bounded envelope prescription | 212 |
| Water boundary below the effective compression domain | 103 |
| Radial integration convergence | 25 |
| Conservative pure melting screen | 22 |
| Cold dense H/He | 12 |
| Work budget | 8 |
| Central shooting resolution | 7 |
| Reference-gravity convergence | 5 |
| Region mass resolution | 1 |

The remaining eight H/He-free dense-gas and seven warming failures are unchanged.
Fix measured blockers next, without a water-amount cutoff or silently accepting
unsupported giant phases. The [canonical implementation guide](../simulation/planet-composition.md#self-gravitating-envelope-and-loaded-interior)
owns equations, resource setup, numerical boundary contracts and limitations.

### Step 4 investigation: secondary trace domain (28 October 2026)

- **Scope:** diagnose the 212 first-failure trace cases without changing physical
  prescriptions, protection, generation or numerical acceptance.
- **Result:** all 212 exceed the accessible-envelope fraction; none exceed the
  protected-heavy fraction. The prior thin route refuses 203 for dense gas,
  eight for gas mass and one for heavy mass.
- **Key distinction:** 79 have at least 50% accessible secondaries in the outer
  parcel. A further 57 have 5–50%; only 76 are below 5% (33 below 1%). These are
  mostly small secondary atmospheres with residual H/He, not giant impurities.
  CO₂/N₂ dominate; oxygen is absent. No metallic-regime need has been established.
- **Next model decision:** distinguish an H/He-rich mixed-envelope extension
  from a dense secondary-atmosphere prescription. Do not raise the trace cap
  or move material to protected hosts. Existing phase/thermal bounds remain
  applicable; new prescriptions need justified state ranges and fixtures first.
- **Stop condition met:** repeatable per-body audit and synthetic allocation
  checks, with unsupported depth conditions explicitly unknown. Coverage stays
  1,134/1,544; no additional solves are claimed. Canonical findings and diagnostic
  output ownership are in the
  [composition guide](../simulation/planet-composition.md#secondary-trace-input-investigation).

### Step 4: bounded H/He-rich molecular foundation (28 October 2026)

- **Observed gap:** the passive host-volume trace model cannot represent the
  finite molecular volume of a non-trace secondary inventory.
- **Implemented scope:** additive-specific-volume mixing of the existing
  CMS19/HG23 H/He density with screened Peng–Robinson component densities.
  This first extension requires at least 95% H/He by envelope mass, retaining
  all origins and all separate protected/water allocations. It is a bounded
  molecular foundation, **not completed H/He-rich planetary coverage**.
- **Explicit restrictions:** 80–625 K, at most 1,000 bar for present molecular
  components, conservative pure-condensation and near-critical masks, no liquid
  secondary regions or metallic-depth chemistry. No changes to the thin solver,
  original passive-trace route, heavy-water domain or numerical tolerances.
- **Fixtures:** ten independent NIST component-density spot checks, dilute
  molecular-weight limit, compressibility/continuity, trace-overlap comparisons,
  positive-constituent and phase refusals, source-backed full-planet conservation
  and refinement. The dry 2%-secondary fixture solves at about 6,499 km one-bar
  radius with a 12.30-bar base. At this initial delivery, adding positive water
  explicitly refused rather than dropping it or folding it into the envelope;
  the subsequent shallow-water slice below removes that blanket refusal.
- **Coverage finding:** no new generated successes. All generated candidates
  retain water, and this molecular pressure domain has no accepted overlap with
  the existing loaded-water floor of 10,000 bar. The original 1,134 successes
  remain unchanged. A larger accepted-mixture fraction alone cannot resolve
  that structural mismatch.
- **Final census/checks:** 20-thread coverage remains 1,134/1,544; all 1,134
  previous successes match every original non-timing column. Of the original
  212, 136 exceed the new composition bound; the 76 eligible mixture attempts
  return pure-condensation screen (42), component state domain (25), shallow
  water (8) or radial convergence (1). Full project checks and source-backed
  mixed-envelope refinement pass. These are reclassified refusals, not wins.
- **Delivery gate:** do not call the original trace gap closed, widen molecular
  bounds to planetary depths, or proceed to default cutover. Shallow-water
  coupling was subsequently delivered below; component/mixture phase/deeper-density
  prescriptions remain separate future work. The
  [canonical molecular model](../simulation/planet-composition.md#hhe-rich-molecular-mixtures)
  owns the equations, masks, sources and resulting diagnostic outcomes.

### Step-4 slice 1: shallow loaded water and visible fidelity

- Prior mixture work checkpointed as `8188e43`, nothing pushed.
- Added `shallow_loaded_water_murnaghan_proxy_v1` below 10,000 bar,
  a pressure-only condensed-water engineering approximation. Existing deep
  compression is unchanged, with a located, unblended model interface. All
  positive water and protected allocations remain represented; no new amount
  cutoff, relaxed numerical tolerance or legacy fallback was introduced.
- Global/per-layer `model_fidelity` and `model_limitations` now travel with
  layered results and appear in the inspector. Current whole-planet closures
  are at least approximate. Extended-Ih regions are marked extrapolated;
  reference-checked local equations are not a whole-planet accuracy certificate.
- The 20-thread census gains 98 solves: 95 of the 111 former water refusals and
  three prior radial failures, including eight of the original molecular-mixture
  cohort. The other water cases expose numerical convergence
  (9), cold dense H/He (5), melting screen (1), or missing one-bar level (1).
  No old successes are lost; their maximum radius/inertia-factor changes are
  relative 1.36×10⁻⁹ / 2.72×10⁻⁹ due to altered shooting paths.
- Full `./scripts/check.sh`, source-backed shallow-water conservation/refinement,
  source-backed mixture tests and `git diff --check` pass. No interactive client
  test, renderer change or benchmark was launched.
- Physical limitations remain explicit: no shallow water thermal expansion,
  phase equilibrium, steam redistribution or H/He-water mixing. These results
  do not establish scientifically accurate oceans or interiors. The
  [canonical guide](../simulation/planet-composition.md#shallow-loaded-water-approximation)
  owns equations, tests and limitations.
- Next at that checkpoint: choose a bounded heavy-enriched/deeper-envelope approximation and phase
  treatment for the remaining 136 composition, 42 condensation and 25 molecular
  state refusals. Numerical and dense-H/He work remain separate. No renderer
  cutover or placeholder has been implemented in slice 1.

### Step-4 slice 2: enriched and deeper envelopes

- Slice 1 checkpointed as `16d101f` before starting; nothing pushed.
- **Observed gap:** 136 envelopes exceed the first molecular model's 5%
  secondary-mass scope; 25 hit its component pressure/temperature mask.
- **Delivered approximation:** an explicit enriched constructor, screened
  molecular components in their existing mask and an ideal-thermal-volume plus
  Seager-water-like compression surrogate beyond the first inward 625 K or
  1,000-bar boundary. Each component retains its identity and mass. This is not
  a published heavy-mixture EOS or chemistry calculation.
- **Handoff:** pressure located on each iterated thermal profile; explicit
  component endpoint normalisation makes density continuous without extrapolating
  PR. Separate disjoint radial regions, exact one-bar event, charged/cached
  anchor work and unchanged work ceilings/tolerances. Entirely hot envelopes use
  the unanchored proxy, labelled approximate, not a fake PR endpoint.
- **Limits:** no phase/critical refusal is bypassed. Positive H/He is still
  required. Enrichment retains the H/He-oriented opacity and fixed 2/7 thermal
  slope, a declared uncertainty even where secondaries dominate. No water or
  protected allocations are folded into the mixed parcel; no molecular mass is
  converted into surrogate H₂O. Metallic-depth chemistry remains unresolved.
- **Measured gain:** 72 new solves, reaching 1,304/1,544 (84.5%), with no lost
  successes and exactly unchanged prior reported radii/inertia factors. Of the
  136 composition cases, 50 solve, 62 reach condensation, 23 lack a one-bar event
  and one fails radial convergence. Of the 25 state-domain cases, 22 solve, two
  reach condensation and one fails radial convergence. First blockers are still
  not guaranteed wins.
- **Fixtures:** enriched dilute molecular-weight limit, compressibility, phase
  and domain refusals, continuous handoffs above/below the one-bar event, lazy
  anchor budget accounting; 20% and 90%-secondary full-planet source-backed and
  synthetic fixtures with separate water/protected material, independent radial
  mass/inertia/hydrostatic quadrature and relative 10⁻⁷ refinement gates.
- **Checks:** full `./scripts/check.sh`, all 14 envelope tests including the
  source-backed fixtures, final 20-thread census and `git diff --check` pass.
  Every previous non-work coverage column is unchanged. Rendering/default
  generation remain legacy; no interactive benchmark was launched.
- **Next:** secondary condensation is now the largest measured blocker (106).
  The other gaps include radial convergence (34), absent one-bar level (24),
  H/He melting screen (23), cold dense H/He (17), work budgets (8), dense H/He-free
  gas (8), warming domain (7), central shooting resolution (7), gravity
  convergence (5) and region mass resolution (1). Numerical errors remain errors,
  and a physically absent reference level cannot be invented.
- The [canonical enriched model](../simulation/planet-composition.md#enriched-and-deep-secondary-envelopes)
  owns the equations, IDs, physical caveats and measured fixture results.

### Step-4 slice 3: local suspended condensation

- Slice 2 checkpointed as `50984bb` before starting; nothing pushed.
- **Observed gap:** all 106 pure-condensation refusals involve CO₂, at last
  observed trial states of 84.17–189.52 K and 10–664.58 mbar. Old-cap
  partial-pressure estimates indicate 40 undersaturated and 66 supersaturated
  samples, not accepted planetary phases.
- **Delivered approximation:** deterministic cold-mixture selection before
  shooting; simultaneous ideal-partial-pressure saturation and separate,
  pressure-compressed condensate volumes. Total abundances remain uniform;
  condensate stays locally suspended, not protected or deposited into water.
  Phase amounts are recomputed with each radial density and gravity iteration.
- **Density scope:** ideal secondary vapour and simple liquid/solid-like density
  proxies in the existing 80–625 K / 0.01 mbar–1,000 bar molecular rectangle,
  explicitly not a non-ideal flash or PR extension. Deep effective components
  match the ideal-vapour endpoint, requiring all present secondaries above their
  critical temperatures there. Near-critical/H/He/numerical screens stay active.
- **Conservation and visibility:** each layer exposes constituent vapour and
  suspended-condensate suballocations, whose sum is already in its mass/gravity.
  Global material budgets distinguish vapour from suspended condensate; the
  inspector shows local integrated phase ratios and approximation caveats.
  Perfect suspension, ideal fugacity/activity, unresolved solid/liquid identity,
  neglected latent heat/settling and unchanged 2/7 thermal slope are explicit.
- **Measured gain:** 76 additional solves to **1,380/1,544 (89.4%)**, with no
  previous successes lost. Of these, 51 have integrated CO₂ condensate and 25
  solve without condensate anywhere in the accepted profile. All 49 previous
  cold explicit-mixture successes remain; their intentional model changes reach
  0.01144% in radius and 0.02287% in inertia factor. Warm and passive-trace
  successes retain their reported physical results.
- **Remaining original candidates:** cold dense H/He (15), outward envelope
  density inversion (12), radial convergence (2), central shooting resolution
  (1). Inversions now have their own label rather than being conflated with
  H/He interpolation. They remain refusals, not accepted unstable cloud layers.
- **Fixtures:** saturation onset/evaporation, dilution false alarms, simultaneous
  component saturation, independent NIST saturation/density spot checks,
  continuity at the deep handoff, protected/water/origin conservation, phase
  allocation budget exhaustion, independent envelope quadrature, and relative
  10⁻⁷ radius/inertia refinement with source-backed H/He. Reporting phase-mass
  quadrature has a separate relative 10⁻³ refinement check, observed below
  0.012%; this does not relax numerical structure or mass closure gates.
- **Next gaps across all 164 refusals:** radial convergence (36), cold dense
  H/He (32), absent one-bar level (24), H/He melting (23), density inversion
  (12), work budget (8), H/He-free dense gas (8), central shooting resolution
  (8), warming domain (7), gravity convergence (5), region mass resolution (1).
  No work on settling, revised thermal closure or these next gaps is implied.
- **Verification:** final 20-thread census and comparison, full
  `./scripts/check.sh`, all 17 envelope tests (including source-backed fixtures),
  all seven focused suspended-model tests and `git diff --check` pass. No
  interactive benchmark or visual test was launched. Rendering/default
  generation stay legacy. Slice 3 checkpointed at session close; nothing pushed.
  Full CLI JSON (`scripts/ns --json planets` or `scripts/ns state planet_loading`)
  serializes the complete layered record, including phase suballocations and
  model fidelity/limitations. Plain `scripts/ns planets` remains a short summary.
- The [owning suspended-condensate guide](../simulation/planet-composition.md#suspended-secondary-condensates)
  defines equations, scopes, transport semantics and remaining physical limits.

### Remaining delivery order

5. **Fallback structure. Done, 28 October 2026.** Every refusal gets a total,
   closed-form, labelled structure from the same retained inventory
   ([guide](../simulation/planet-composition.md#fallback-structure)). All
   164/164 census refusals get a checked fallback; 1,380 layered results are
   unchanged. 20,000 randomised extreme inventories pass in `check.sh`.
   Fallback radius vs legacy: median 0%, p95 30%, max 61%, the large ones being
   bodies with under 1% H/He, where layered solves also exceed legacy.
6. **Cheap layered wins, time-boxed.** Solver robustness (radial, shooting,
   gravity and region-mass convergence, work budgets) and a radius reference
   for bodies with no one-bar level. Stop at the time box; leftovers stay on
   the fallback. Hard physics (cold dense H/He, H/He melting, density
   inversion) is parked.
7. **Validate and close.** Mass/radius/inertia, heating and stripping trends,
   approach to the bare-body limit and conserved boundaries; rerun coverage and
   project checks.

**Exit:** about 95% layered, the rest on the labelled fallback, no refusals,
and step 7 trends pass documented plausibility checks. Coverage alone does not
establish scientific accuracy.

**Scope control:** before each change, state the observed gap, smallest useful
prescription, fixtures and stop condition. A slice ships only if it changes
something visible in game or closes a gap measured by the coverage report.

## P3. Default cutover and legacy removal

- Make the unified result authoritative for generated planets and retention.
  Migrate snapshots, shape and spin, atmosphere and optics, rendering and
  navigation to the same solved facts.
- Use the fallback as the fast synchronous first answer while the layered solve
  refines in the background; this replaces legacy's role.
- Decide the H/He density resource: bundling permission for CMS19, or a
  shippable replacement. Without it, envelope bodies drop to the fallback.
- Provide summary-first composition, ordered regions, phase knowledge,
  boundaries and model limitations in the inspector.
- Require measured generation and search performance suitable for the game, full
  project checks and the user's visual and interactive review before cutover.
- Remove `structure::derive`, its heuristic inertia and legacy consumers only
  after all runtime callers migrate. The Seager and envelope fits stay: the
  fallback uses them.

## Optional deeper physics, not prerequisites

Revisit only for a demonstrated game need or an explicit user request:

- CO₂, N₂, CH₄, O₂ and Ar phases beyond the fixed-density frost deposit:
  compressed or polar frost, lakes, supercritical states. Parked
  methane work is a starting point if methane lakes are ever wanted.
- Solubility, clathrates, phase separation and realistic mixtures.
- Finer water polymorphs, thermal profiles through oceans, refractory melting,
  detailed H/He thermodynamics and cooling evolution.

Chemistry, weather, helium rain, impacts and full planetary evolution remain out
of scope. Local lakes, continents and waves belong to the
[surface plan](planet-surfaces.md), using the same conserved inventory.

The offline reference audits in `scripts/` remain regressions for implemented
or candidate models. Move them out of `check.sh` into a separate audit command
if they slow day-to-day checks.
