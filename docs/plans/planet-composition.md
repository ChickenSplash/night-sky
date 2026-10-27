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
- **Envelopes come later.** H/He planets (about half the population) are not part
  of the first prototype and are reported as unsupported in prototype mode.

## Completed foundation

Preserve these implementations and their tests. Their declared domains stay
unchanged unless a step below deliberately widens them with a labelled model.

| Historical checkpoint | Reusable result |
| --- | --- |
| 1–5 | Conserved inventories, region/numerical contract, [dry](../simulation/planet-composition.md#headless-dry-body-solver) and [water](../simulation/planet-composition.md#headless-restricted-water-shell-solver) mechanics, [thin-atmosphere coupling](../simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling) |
| 6a–6b | Protected secondary origins in [dry](../simulation/planet-composition.md#headless-protected-traces-in-dry-hosts) and [water-bearing](../simulation/planet-composition.md#headless-protected-traces-in-water-bearing-hosts) hosts |

## Measured starting point

A throwaway probe (October 2026) routed 1,375 generated planets from 120
calibration systems through the existing headless solvers, using a provisional
surface load. The numbers are indicative, not a formal coverage report:

- 54% carry H/He and have no new route.
- No rock planet is completely dry: every one carries at least trace water, so
  all use the water solver.
- About 26% of non-envelope planets solve on any route. Most failures are water
  conditions outside the solver's window, not missing mass.
- Where solved, radius is within about 1% of the legacy model. The visible gain
  is layer structure and boundary meaning, not radius.
- A solve costs about 30–60 ms, roughly 0.5 s for a twelve-planet system.

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

1. **Coverage report.** A reproducible command routes a fixed deterministic set
   of generated planets through the prototype path. It reports success, failure
   reason by category, work counts and timings. It replaces the throwaway probe.
2. **Surface temperature.** A simple labelled greenhouse warming estimate from
   atmospheric pressure and composition, above the equilibrium temperature.
   Without it an Earth twin is about 255 K and frozen. Not a climate model.
3. **Cold and airless water.** Ice Ih below 250 K down to 20 K, and water surfaces
   below 1 kPa, use a labelled near-constant ice density. Extend the existing
   temperature-independent effective deep-ice model to cold bodies.
4. **Vapour allocation.** Exposed water that cannot stay liquid or frozen at the
   surface moves to the atmosphere inventory; protected water stays in the host.
   Reuse the existing coupling loop to re-solve the load.
5. **Hot and deep water.** One labelled approximation for supercritical water
   above 623 K and warm deep ice (300–623 K beyond 1 GPa), using effective
   high-pressure ice at depth whatever the temperature.
6. **Trace rounding.** Accept water transfers below floating-point resolution of
   the planet's water with a documented, species-local allowance, instead of
   rejecting them.
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

- CO₂, N₂, CH₄, O₂ and Ar phases: frost, lakes, supercritical states. Parked
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
