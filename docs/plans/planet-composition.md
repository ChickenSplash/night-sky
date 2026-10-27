# Planet composition replacement

## Status and goal

**Next deliverable: a live, generator-backed rock-and-water prototype.** Replace
the old whole-body structure approximations with conserved, layered planets that
are useful in the simulation and game. Do not start another material-physics
sub-phase before that prototype exists.

Generated planets still use the approximate model in `structure::derive`. The new
dry, water, protected-trace and atmosphere-coupling solvers are headless and
opt-in. They are building blocks, not yet one generator-ready replacement.

This is a simulator, not a thermodynamics research project. Prefer the simplest
scientifically plausible model that supports the needed behaviour. Named,
bounded approximations and explicitly unresolved phases are acceptable. Exact
phase diagrams and publication-grade accuracy are optional improvements, needed
only when a demonstrated simulation or game problem requires them.

| Document | Owns |
| --- | --- |
| This plan | Delivery order, scope, decisions and acceptance |
| [Composition and structure guide](../simulation/planet-composition.md) | Implemented behaviour, APIs, equations, limitations and numerical tests |
| [Scientific reference](../simulation/planet-composition-reference.md) | Sources, generated-input bounds and candidate higher-fidelity fixtures |
| [Water physics](../simulation/water-physics.md) | Implemented water prescriptions and their evidence |

### Resume here (October 2026)

- **Done in P1:** steps 1–6, plus the frost deposit and bounded hot-steam extension.
  Nothing is pushed: neither
  the main branch nor `park/methane-physics` (parked methane physics). Ask the
  user before pushing either.
- **Coverage:** 839 of 1,029 non-envelope planets solve (82%), including all 280
  water-rich planets. See
  [measured starting point](#measured-starting-point).
- **Biggest blockers:** weak binding (175), dense gas (8) and warming beyond
  the bounded column (7). Step 5 removed the deep-water first failures.
- **Open decisions for the user:**
  - Steam escape: whether generation's escape should act on exposed water, so
    weakly bound small bodies lose their steam instead of failing.
  - Solve cost is about 156 ms at the median with the warming scan;
    address before step 8.
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
- **Envelopes come later.** Trace and thin H/He is an ordinary gas in the
  prototype. Bodies whose H/He exceeds the thin column's limits (about 14% of the
  population) are reported as unsupported in prototype mode.

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

**Current, after step 5:** 839/1,029 non-envelope planets solve (82%), with
515 H/He-bearing envelopes. This adds 91 solved bodies; another 66 are now
classified as envelopes because the wider warming scan exposes their gas-domain
failures. All 280 water-rich planets solve. Remaining first failures are weak
binding 175, dense gas 8 and warming outside the domain 7. Surfaces are frost 422,
ice 202, ocean 28 and rock 187. The bounded steam approximation is ideal gas at
1,000–2,000 K, ≥90% water by mass, ≤50 kg/m³ and ≤1% total gas mass, not a
general dense-fluid or envelope EOS. Cost on 20 threads: 156 ms median, 535 ms p95,
7.2 s maximum; report wall time 17.3 s. Radius difference from legacy:
1.9% median, 10.0% p95, 12.7% maximum.

Historical baseline before step 5:

- 27% are envelopes: H/He beyond the thin column's 1% gas-mass, dense-gas or
  20 Earth-mass limits. Trace H/He is an ordinary gas, not an envelope; counting
  any H/He as an envelope had misclassified 472 stripped rock-type worlds. Step 3
  raised envelopes from 219 to 417 (cold bodies with 0.1–1% H/He now exceed the
  dense-gas limit), step 4 to 454 (steam added to thin H/He columns); step 2
  left 449.
- Every rock planet starts with at least trace water. 103 solved bodies have
  evaporated all of it and are dry rock under steam.
- **748 of 1,095 non-envelope planets solve (68%)** after steps 2, 3, 4 and 6,
  up from 57 of 1,325. All 227 below 50 K, 391 of 429 at 50–250 K, 95 of 345 at
  250–623 K and 35 of 94 above (by equilibrium temperature). Step 4 alone had
  reached 88%; step 2 makes warm water worlds run away instead of keeping an
  ocean at their equilibrium temperature.
- First blockers now: gas triage 15%, almost all weak binding (155 small warm
  bodies whose steam column would escape), greenhouse runaway 15% (159 hot steam
  worlds beyond the thin column, P2), column above 1,000 K 1.4%, deep water 1.0%.
- Oceans: 21, at equilibrium 163–284 K warmed to 274–313 K.
- Radius against legacy: 2.4% at the median, 10.2% at p95, 12.7% at most. Small
  icy bodies grow because ice Ih is lighter than the legacy fit assumes.
- A solve costs about 110–190 ms at the median on 20 threads, up to about 9 s
  (varies between runs). The warming scan adds partition work.
  Cost needs attention before the experimental mode (step 8).

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

## P1. Rock-and-water prototype, next

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
   curve. **Open:** small warm bodies (155 after step 2) have a steam column
   too weakly bound to hold, so the escape prior and exposed water disagree
   there.
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
7. **One entry point.** Generated budget and environment in; one region-owned
   result out, with layers, atmosphere, model labels and boundary facts. It joins
   protected traces in the host with the atmosphere coupling, which today are
   separate routes.
8. **Experimental mode.** An explicit switch in `generated_system`, separate from
   the default. Envelope planets return a labelled unsupported result, never a
   quiet per-body substitution of the legacy solver.
9. **Consumers and inspector.** Map results onto `PlanetPhysical` so snapshots
   work unchanged. The inspector shows layers, model identity and failure reason.
   Preserve liquid-top semantics and free camera descent; no landing permission
   from unresolved phases.

Left as a documented limitation: each water layer has one temperature, so a deep
ocean is as warm as its surface. Revisit only if layer results look wrong.

**Exit:** the coverage report shows about 100% of non-envelope generated planets
solving, with every origin accounted for and measured cost. Representative dry,
ocean, icy and steam worlds can be inspected in the client. Run
`./scripts/check.sh`; the user performs visual review and interactive benchmarks.

**Stop:** no CO₂ or methane phase physics, exact Gibbs audits, envelope physics
or population-wide cutover in P1.

## P2. Envelopes and remaining coverage

Prioritise gaps measured by the P1 report.

| Missing capability | Minimum delivery requirement |
| --- | --- |
| H/He envelopes, small to giant | A named compressible prescription with envelope self-gravity, volume and inertia, plausible giant and stripping trends and explicit bounds. A simple justified model suffices; no specific published EOS required |
| Heavy cores above 20 Earth masses | Widen the radial mass domain or give these bodies a labelled route |
| Runtime cost and retention | Resolve retention dependencies without rerunning loss history. Cache per system with deterministic keys; validate any loosened tolerances |

**Exit:** every generated route has a documented prescription, the coverage
report has no unexplained holes, and mass/radius/inertia, stripping and heating
trends are plausible.

**Scope control:** before each change, state the observed gap, smallest useful
prescription, fixtures and stop condition. A slice ships only if it changes
something visible in game or closes a gap measured by the coverage report.

## P3. Default cutover and legacy removal

- Make the unified result authoritative for generated planets and retention.
  Migrate snapshots, shape and spin, atmosphere and optics, rendering and
  navigation to the same solved facts.
- Provide summary-first composition, ordered regions, phase knowledge,
  boundaries and model limitations in the inspector.
- Require measured generation and search performance suitable for the game, full
  project checks and the user's visual and interactive review before cutover.
- Remove obsolete whole-body fits, envelope surrogates and heuristic inertia only
  after all runtime callers migrate. Do not delete working legacy code early.

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
