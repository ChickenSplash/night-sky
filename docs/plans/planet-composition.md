# Planet composition refactor

## Status and goal

**Current layered-refactor status:** checkpoints 1–4 of the
[next-phase roadmap](#checkpoints-and-acceptance-gates) are complete as opt-in
headless models. Checkpoint 5's [coverage audit](planet-coverage.md) is complete as
a user-approved scoped readiness review, not full physical coverage. Checkpoints
6a, protected traces in dry hosts, is complete as an opt-in headless slice.
The rest of checkpoint 6 and checkpoints 7–12 remain planned.
Generated planets still use the
earlier approximate structure path. The completed checkpoints 1–5/5b/5c below
belong to the **original inventory foundation**, not the layered replacement.
The [architecture review](#architecture-review--extensible-materials-and-boundaries)
clarifies the replacement contract; this review authorises documentation only.

Checkpoints 1 through 5 implemented. Generation draws inventories, accounts for
escape and reconciles retained structure at final orbits. Atmospheres and cold
condensates share that budget. Derived labels, validated composition transport
and the sectioned, bounded inspector are implemented. Calibration and regression
review are recorded in the composition guide. Checkpoint 5b, the approved
condensed-boundary atmosphere correction, is implemented and checked. Checkpoint
5c (descent into deep envelopes) is implemented with the revised scope below.
Variable-gravity columns, inventory-normalised pressure, opacity corrections
and the revised inspector also pass automated checks. On 2026-10-25 the user
authorised integration into `main` and continued work on this branch. This does
not claim exhaustive visual or scientific validation.
Physical prescriptions and limitations are in the
[composition guide](../simulation/planet-composition.md).
Work on `refactor/planet-composition`,
branched from checked and pushed `main` including flight clearance. The abandoned
galactic-dust and HTML UI branches are excluded. The user will visually test the
implementation during development; the latest explicit integration request
supersedes the earlier hold on merging.

Replace class-first generation with explicit material inventories and approximate
physical structure. A planet's class becomes a summary of its properties, never
an input to its physics. Existing generated populations may change in this alpha.

Build on the [atmosphere foundation](atmospheres.md#checkpoint-5--every-planet-with-air).
Current behaviour is documented in [planetary systems](../simulation/planetary-systems.md)
and [physical atmospheres](../simulation/atmospheres.md). Future terrain,
hydrospheres and deep-envelope descent belong to the [surface plan](planet-surfaces.md).

## Model contract

Keep four concepts separate:

1. **Material catalogue:** supported constituents and their physical prescriptions.
2. **Inventory:** mass of each constituent belonging to the planet.
3. **Reservoir allocation:** where inventory resides, such as interior, condensed
   volatile reservoir or gaseous envelope/atmosphere.
4. **Classification:** descriptive labels derived from the resulting properties.

Initial bulk constituents are iron/nickel metal, silicate rock, water, hydrogen
and helium, with explicit secondary volatile constituents/budgets sufficient to
account for the generated nitrogen-, carbon- and argon-bearing gas supply.
Choose their exact representation in checkpoint 1 and document the bookkeeping.
Do not count trace gases as additional mass on top of a complete bulk inventory.
Keep hydrogen and helium distinguishable so atmospheric mixtures have a source.
Detailed metal alloys and mineral species are not initially resolved.

The catalogue must be extensible through stable constituent identifiers and
isolated property/partition prescriptions. A material is not just a name and
ordinary density: new constituents may require compression, phase and mixture
models. Unsupported physics must not silently use a generic density.

### Invariants

- Use explicit units, finite nonnegative inventories and positive total mass.
  Material fractions are mass fractions; atmospheric gas fractions remain mole
  fractions, with explicit molecular-mass conversions between them.
- Each constituent's retained mass equals the sum of its reservoir allocations.
  Condensation transfers material, rather than deleting it. Account separately
  for escaped material; it is not part of the retained planet mass.
- Atmospheric columns are descriptions of reservoir material, not additional
  reservoirs to add again. A giant's one-bar column is only part of its envelope.
- No class or displayed class string selects mass, radius, atmospheric supply,
  spin, flattening, surface semantics or physical optical properties.
- Preserve physical scale, f64 calculations, reproducibility, host/index body-ID
  rules and companion stability constraints. Population changes are allowed.
- Missing composition in older wire records means unknown, not an iron-free,
  airless or zero-mass planet. Any legacy fallback must be isolated and must not
  become the generation path for newly produced bodies.

## Physical scope and boundaries

Mass and composition alone do not uniquely determine a planet. Temperature,
rotation, internal entropy, differentiation and evolutionary history matter.
Use documented, tunable approximations, not a claim of solving those histories.

- **Condensed structure:** composition-dependent compressed mass-radius fits or
  a bounded layered approximation. Do not sum ambient-density material volumes.
- **Envelopes:** a separate mass/temperature-dependent prescription for radius,
  with a smooth transition into the self-compressed giant regime. Thin-envelope
  fits must not be extrapolated unchecked to gas giants.
- **Density and gravity:** derive mean density from retained mass and
  volume-equivalent radius; reference gravity is `GM/R²`. Existing spheroidal
  geometry remains authoritative for surface positions.
- **Flattening:** retain the existing Darwin-Radau approximation and flattening
  cap, but supply an approximate inertia factor from physical structure instead
  of a class lookup. Spin remains a prescribed outcome, not integrated tides.
- **Atmosphere:** reuse validated gas states, heating, optical conversion and
  rendering. Bound supply by actual inventories; approximate retention and
  partitioning do not become a full atmospheric evolution model.
- **Reservoirs:** track condensed and interior volatile material without asserting
  a resolved ocean, ice shell or mineral surface. Carbon/nitrogen chemistry is
  prescribed, not solved, and any species conversion must conserve its budget.

Solid-surface radii and envelope reference radii must be explicitly distinguishable.
On giants, gas grades into dense fluid and can mix with deeper material; there
need not be an atmosphere/interior discontinuity. A one-bar reference is neither
solid ground nor a measured cloud deck. Do not invent an ocean bottom or envelope
bottom from a reference pressure.

Water-rich worlds can contain vapour, ocean, ice, high-pressure ice or supercritical
fluid. Bulk water abundance alone must not label a world as having an ocean.
Resolved phase boundaries, hydrospheres, surface chemistry, greenhouse climate,
interior evolution, metallic hydrogen and deep descent are deferred.

## Proposed generation flow

1. Establish the host's formation scales and companion stability zone.
2. At each proposed orbit, draw solid inventory and gas acquisition using
   formation location, host properties and tunable continuous prescriptions.
   Draw physical quantities, not hidden class templates renamed as materials.
3. Bound formation mass by the host-mass cap, scaling inventories consistently.
   Establish neighbour spacing using formation masses, retained as conservative
   spacing bounds if subsequent escape reduces a planet's mass.
4. After the complete stellar hierarchy and final orbital positions are known,
   calculate all-star heating and approximate retention. Reconcile retained
   inventory, radius and envelope fraction through a bounded deterministic solve.
5. Finalise retained mass, reference radius, reservoir allocations and structure.
   Account for escaped material explicitly. Recheck final orbital stability.
6. Derive atmosphere, spin and inertia/shape parameters from the final physical
   state. Spin and atmosphere RNG streams must not perturb orbital draws.
7. Derive descriptive classification and transport the physical facts to clients.

Formation snow-line distance remains an approximate origin heuristic, distinct
from current irradiation. Preserve progenitor-based formation scales for evolved
stars and include companions in present-day heating. Orbital relocation during
spacing does not retroactively redraw formation material.

The structure/retention solve needs an iteration limit, declared convergence
tolerances and a tested conservative fallback or explicit failure path. It must
not hang or silently clamp invalid physics into a plausible-looking planet.
Account for this work in generation/search costs; do not run an expensive full
interior integration for every candidate in a search.

## Implementation checkpoints

### 1. Material model and physical prescriptions

- [x] Add focused composition/structure modules under simulation `system/`,
  following the existing `name.rs` plus sibling-directory pattern.
- [x] Define validated inventories, reservoir accounting, escaped-material
  bookkeeping and explicit radius/reference semantics.
- [x] Select and cite suitable mass-radius/envelope approximations. Record units,
  coefficients, domains, blending rules, uncertainties and fallback behaviour.
- [x] Define physical defaults and classification thresholds centrally. Separate
  measured constants from empirical fits and adjustable population heuristics.
- [x] Test pure/mixed materials, invalid inputs, budget conversions and continuity.

Exit: independently testable physical model, with no generation changes yet.

### 2. Composition-first populations and retained structure

- [x] Replace `draw_class`, class-based mass/radius selection and compact-retry
  templates with inventory draws and physically bounded acquisition.
- [x] Keep host occurrence trends, compact retries, stable identities, resonances,
  companion limits and bounded system extents, without requiring old outputs.
- [x] Integrate final-orbit heating, bounded retention/structure reconciliation
  and conservative formation-mass spacing across single and multiple hosts.
- [x] Derive spin and inertia parameters without classification dependencies;
  preserve the flattening cap and independently seeded spins.
- [x] Add deterministic population and stability tests, including evolved hosts.

Exit: body mass, radius, shape inputs and inventory agree before labels exist.

Checkpoint 2 transports the optional inertia factor early so client shapes and
generation spin limits share the same physical input. Full inventory/reference
transport and the classification audit remain checkpoint 4. H/He retention uses
a bounded scalar bisection against immutable formation supply; checkpoint 3 adds
secondary volatile escape/partitioning to that foundation.

### 3. Atmosphere and reservoir integration

- [x] Remove class-based envelope probabilities and unbacked volatile supply.
- [x] Source hydrogen/helium and secondary gases from retained material budgets.
  Bound atmospheric mass/pressure consistently within the declared column model;
  do not extrapolate a thin column to estimate the entire mass of a giant.
- [x] Allocate condensation back to a named reservoir and handle airless/trace
  outcomes without negative masses or fabricated gases.
- [x] Reuse atmospheric validation, all-star heating, optics and extended-column
  warnings. Preserve flight-clearance integration and its safety semantics.
- [x] Test inventory-to-gas conversions, pressure bounds and reservoir conservation.

Exit: atmosphere and interior reference the same conserved material inventory.

Integration regression: thick atmospheres now receive the same 1.5-times
preferred wrapping margin as solid bodies, measured outside the atmospheric
clearance height. Hard exclusion/swept refusal rules are unchanged. Client
route fixtures launch above computed exclusion rather than assuming 400 km
always clears procedural air.

### 4. Classification, transport and inspection

- [x] Derive rocky/metal-rich, water-rich and envelope-world descriptions using
  documented physical thresholds. Conventional size labels may overlap in nature;
  deterministic display rules must be explicit and not feed back into physics.
- [x] Remove composition-class use of `Dwarf`. Do not assert orbital clearing from
  a small mass alone; defer a dynamical dwarf classification unless justified.
- [x] Audit generation, shape, surface/photometry, search filters, fixtures,
  snapshots and client rendering for class dependencies. Appearance proxies must
  use explicit physical inputs and be labelled as approximations where needed.
- [x] Carry composition and structure facts through generated systems and task
  overlays, with validated wire conversion and explicit unknown legacy state.
- [x] Show material fractions, retained mass, density, envelope fraction and
  solid/reference-radius meaning in the inspector and developer readout.
- [x] Introduce a small typed inspector presentation model with stable field IDs,
  sections, labels, units, formatted values and explicit unknown values. HUD and
  developer export consume the same description; physical records remain separate.
  Use Overview, Composition, Orbit, Rotation and Atmosphere sections, leaving room
  for hydrospheres without introducing a general schema/configuration framework.
- [x] Bound the expanded inspector layout deliberately, using collapsible sections
  or scrolling rather than appending unbounded rows. Preserve the existing HUD style.
- [x] Update classification filters and registry tests. Additional composition
  search operators are optional follow-up, not required for this refactor.

Exit: changing a label threshold changes descriptions only, never physical output.

Checkpoint 4 uses ordered retained-fraction labels (including metal-/water-rich
and envelope worlds), with no dynamical dwarf or inferred ice-interior label.
Both wire paths carry one conserved physical record, validated by budget
reconstruction and algebraic structure re-derivation. Appearance is a continuous,
explicit composition proxy; only missing legacy records may use class fallbacks.
The halo inspector keeps the existing style with five sections and bounded
vertical scrolling. Typed fields feed the HUD and developer export; legacy
label/value API facts remain a projection. Tests cover label boundaries,
relabelling, normal/overlay parity, invalid/legacy records, filters, field metadata,
narrow viewports and scrolling through the final navigation action.

The user visually reviewed checkpoint 4 and approved it for commit. This is
checkpoint approval, not final approval to integrate into `main`.

### 5. Calibration, regression checks and documentation

- [x] Use prescribed Mercury/Earth-like, water-rich, Neptune-like, Saturn-like and
  Jupiter-like inputs to check sensible structure trends. Declare broad justified
  tolerances and model domains, not exact matches or formation reconstructions.
- [x] Test mass conservation, radius/density trends, envelope-stripping limits,
  threshold continuity, convergence/fallbacks and invalid/extreme inputs.
- [x] Sample deterministic populations across host masses, ages and multiplicity.
  Record mass, radius, density, material/envelope fractions and class frequencies;
  look for implausible spikes, missing populations and pathological outliers.
- [x] Verify that each stable ID regenerates reproducibly regardless of generation
  order, cache state and supported search slicing/threading.
- [x] Verify spin/shape, atmospheric optics, warp clearance, normal/overlay
  transport and inspector regressions. Run `./scripts/check.sh`.
- [x] Update the canonical planetary-system and atmosphere guides, plus relevant
  search/interface documentation. Clearly separate implemented limits from plans.

Exit: automated checks pass and reviewable population summaries explain the change.

Checkpoint 5 samples 1,792 systems and 19,440 planets, grouped by all seven
galactic components, host mass, age, multiplicity and class. Reference-world
tables, quantiles, class counts, histograms, reviewed outliers and known artefacts
are in the [calibration census](../simulation/planet-composition.md#calibration-census-and-regression-scope).
An 800-case formation/retention grid checks supported extreme forcing and repeat
evaluation; full wire records are compared under reverse-order/four-thread
regeneration, with additional class-filter search slicing/threading checks.
Every census planet passes conservation, physical transport, spin/shape, optical
conversion and navigation-bound checks. No physics coefficients were retuned.
The detected defect was a transport check that amplified source-scale roundoff
relative to a tiny escaped remainder after partition. A species-local subtraction
allowance fixes it, with negative/nonfinite/corrupt/absent-species regressions.
Existing route, overlay and inspector regressions remain covered by the full check.

### 5b. Condensed-boundary atmospheres and deep-envelope references

The user approved one focused model-correction checkpoint before checkpoint 6.
A heavy-material-dominated planet can retain a substantial atmosphere while still
being described as rocky. Reporting and rendering only the column above its
one-bar radius hides the pressure and optical depth above its condensed boundary.
This is a physical-reference correction, not a pressure-label substitution.

Implemented in `structure/boundary.rs`, after the unchanged escape solve. The
domain bounds, handoff and limits are in the
[composition guide](../simulation/planet-composition.md#supported-approximate-condensed-boundaries).
The motivating planet now reports about 729.6 bar at an 8,529.7 km boundary, with
one bar about 58.2 km above it and a 168.6 km rendered column. Catalogue giant
`g13b6ffffdda21.2.0/6` (TS 13b6ffffdda21-2-0 h) keeps an unresolved one-bar
reference for comparison. Escape is deliberately not rerun at the corrected radius.

- [x] Define and document a bounded, class-independent criterion for supporting
  an approximate condensed boundary beneath a low-mass, geometrically thin
  atmosphere. Use physical inventory/structure and column-validity inputs, not
  a displayed planet class or an arbitrary pressure threshold. Explicitly treat
  unsupported deep boundaries as unresolved, not as proof that ground is absent.
- [x] Derive approximate boundary radius and pressure consistently from retained
  gas inventory, gravity and reservoir partition. Keep mass conservation, molecular
  identity, cold trapping and escape accounting intact. Do not extrapolate a thin
  column into a full giant-interior model or hard-code the motivating estimate.
- [x] Distinguish condensed-boundary radius, atmospheric pressure levels and
  apparent/rendered extent. Preserve one-bar references for unresolved deep
  envelopes; where supported, expose both boundary pressure and the altitude of
  the one-bar level. A threshold of 1,000 or 10,000 bar must not make physical
  ground disappear; any future depth limit must be labelled as a modelling limit.
- [x] Align body geometry, gas-density/optical profiles, atmosphere rendering and
  navigation clearance with the chosen reference. Increased pressure must affect
  optical depth, not just the readout. Do not invent clouds from pressure alone.
  Review transition behaviour and avoid artificial physical jumps when reference
  semantics or approximation regimes change; keep retention solves bounded.
- [x] Update the left inspector, object-browser details and developer exports
  consistently. Clearly label approximate boundary pressure, reference pressure,
  unresolved deep boundaries and dense-gas/column limitations. Keep shared facts
  separate from physical records; section/tab layout changes are not required
  for this physical correction.
- [x] Add regressions for the motivating selected planet
  `g13b6ffffcfeba.2.1/1` (TS 13b6ffffcfeba-2-1 c), plus airless/trace cases,
  thin hydrogen-bearing rocky worlds, thick secondary atmospheres, water-bearing
  unresolved boundaries and deep envelopes/giants. The earlier roughly 700-bar
  weight-over-area estimate is a diagnostic, not an exact expected result.
- [x] Test reference transitions, inventory/pressure consistency, finite optical
  conversion, geometry/clearance safety, transport validation, normal/overlay
  parity and reproducibility. Rerun affected calibration summaries and reference
  checks; document changed results rather than preserving obsolete census values.
- [x] Update the canonical composition, atmosphere and relevant client guides;
  run `./scripts/check.sh`. Prepare reproducible boundary-atmosphere and giant
  examples for the user's checkpoint 6 visual review, without launching benchmarks.

Scope remains an explicitly approximate boundary/column model. Full deep-interior
integration, dense-gas equations of state, greenhouse climate, resolved water/rock
phase boundaries and generated clouds are deferred. High-pressure estimates must
carry those limitations rather than claim a solved surface climate or solid ground.

Exit: supported condensed-boundary worlds report and render their inventory-backed
atmospheres above a consistent approximate boundary; deep envelopes retain explicit
reference-level semantics. Automated checks pass, limitations and changed population
results are documented, and examples are ready for checkpoint 6. No further user
modelling decision is required to begin; final visual/integration approval is still
reserved for checkpoint 6.

### 5c. Descent into deep envelopes

Implemented with revised scope on 2026-10-25. The user chose unrestricted
free-camera descent, not a pressure stop. Bulk H/He envelopes have no opaque
one-bar surface; rendering continues approximately downward and fades to black.
This is not an adiabatic interior solve, a resolved deep boundary or a newly
modelled gas inventory. Rocky bodies retain their condensed boundaries. Current
behaviour and limits are canonical in the
[rendering guide](../client/rendering.md#giant-descent).

The original proposal below is retained as history, not an outstanding checklist
or a claim that its adiabatic profile, pressure limit and UI were implemented.

Original motivation: worlds without a supported condensed
boundary (giants, envelope worlds and planets with `deep_boundary_unresolved`)
still render an opaque sphere at their one-bar radius, and navigation treats that
radius as a hard exclusion. The column describes no gas below one bar, so there
is nothing to descend into. One bar is a reference label, not a surface: the
renderer and simulation must agree that the camera can fly down through the gas.

- [ ] Extend the deep-envelope column below its one-bar reference. Pressure and
  density keep rising with depth; temperature rises along a simple, documented
  adiabatic (polytropic) profile rather than staying isothermal. Keep the
  existing isothermal column above one bar. This is a descriptive profile of the
  conserved gas reservoir, never additional inventory or a giant-interior solve.
- [ ] Define a **deepest modelled level**: a declared, centralised pressure
  (initially about 1,000 bar), or the condensed proxy radius if that is shallower.
  It is a labelled modelling limit, never ground. Pressure alone still never
  creates or removes ground. Apply the same treatment to transition and other
  unresolved-boundary worlds.
- [ ] Remove the opaque one-bar sphere for these worlds. The shader's lower bound
  becomes the deepest modelled level, and the gas's own optical depth makes the
  disc opaque from outside. Use the existing opacity early-stop to bound cost.
  Generated clouds remain deferred: render plain gas, with no cloud or haze
  stand-in. Giants may look less familiar until clouds exist; record that.
- [ ] Support the camera inside the deep column: extend scattering tables and
  the view march over that depth range, with step spacing that tightens with
  density. Descending should grow darker and hazier, reaching black. Keep f32
  precision and spheroidal handling consistent with the existing shell.
- [ ] Render the gas all the way down to the deepest modelled level. Nothing
  happens at that limit yet: no stop, damage or collision, matching how the
  camera can currently pass through rocky ground. Camera ground rules, landing
  and collision belong to the [planet surfaces plan](planet-surfaces.md).
- [ ] Navigation route clearance keeps its preferred height above the
  atmosphere, so automatic flights still stay outside. Move the envelope's hard
  exclusion from the one-bar radius to the deepest modelled level. Drag and
  heating are deferred.
- [ ] Inspector, browser and developer export: show the deepest modelled level
  (labelled as a modelling limit) and indicate when the camera is below one bar.
- [ ] Tests: continuity of pressure, density and optics across one bar, finite
  optics to the model limit, camera-inside rendering inputs, unchanged supported
  5b boundary worlds, clearance ordering and validated transport. Update the
  atmosphere, composition and rendering guides; run `./scripts/check.sh`.

Exit: deep envelopes render gas continuously from space down to a labelled model
limit, with no surface at one bar; supported boundary worlds are unchanged.
Cost stays bounded. No further user modelling decision is required to begin.

### 6. User visual review and integration

- [ ] Provide launch instructions and reproducible sample IDs covering airless
  rock, water-rich bodies, small envelopes and giants across representative hosts,
  including checkpoint 5b's high-pressure condensed-boundary regression example
  and a deep-envelope one-bar reference for comparison, plus a checkpoint 5c
  descent into that giant's envelope.
- [ ] User checks physical sizes, flattening, atmosphere appearance, inspector
  facts and craft clearance. Agents do not launch performance benchmarks.
- [ ] Fix issues and rerun the affected tests and full check script.
- [ ] Commit and push the refactor branch for review. Keep `main` at the tested
  baseline until the user approves integration.
- [ ] After approval, merge into `main`, rerun checks as needed and push `main`.

Integration update (2026-10-25): the user explicitly requested merging the current
branch into `main` and then continuing on `refactor/planet-composition`.
`./scripts/check.sh` and all three atmosphere shader variants pass before merge.
The request authorises integration; unchecked visual-review items above remain
unverified, rather than being marked complete by inference. Remote publication
is separate from the local merge.

## Decisions and completion boundary

No further user decision blocks starting. Use the defaults above, choose and
document scientific prescriptions during implementation, and keep the user
informed if evidence requires materially changing scope or behaviour.

Success is a coherent, extensible approximate model with conserved inventories,
physical downstream inputs and descriptive labels. It is not a validated solution
for every interior, climate, hydrosphere or geological surface. Visual review
supplements physical tests; it cannot establish scientific accuracy by itself.

## Next phase · layered planetary structure

Draft started 2026-10-25 after the user requested integration of the current
foundation and continued development on `refactor/planet-composition`. This
section proposes future work, not implemented behaviour. The reservoir contract
above describes the current foundation; the region contract below is its planned
replacement, not a second independently authoritative allocation.

Checkpoint 1 specification is recorded in the
[layered structure contract](planet-structure.md), including the first dry EOS,
region ownership, unsupported cases, numerical targets and input/coverage
fixtures. The user authorised checkpoint 2 after discussing the cold/dry scope
and headless numerical validation. The separate `system::structure::dry` solver
is now implemented and numerically tested; see the
[canonical solver guide](../simulation/planet-composition.md#headless-dry-body-solver).
Generation, transport and presentation remain unchanged. The user has now
authorised checkpoint 3. Its [low-pressure water material baseline](../simulation/water-physics.md)
is implemented and tested, along with standalone high-pressure IAPWS melting
curves, a bounded IAPWS95 compressed-liquid extension and pinned SeaFreeze/revised
AQUA reference audits. A separate prescribed 300 K ice-VII compression branch is
also implemented, without equilibrium selection. A restricted headless radial
solver now conserves liquid/ice-Ih shells over cold refractories, including located
Ih/liquid interfaces where both density branches exist, and compressed liquid
to 1 GPa within its mask. The IF97/IAPWS95 boundary preserves separate model
identities and one-sided densities; nested inversion work is budgeted. A matched high-pressure
ice phase/density model and dense envelopes are not implemented.
The subsequent effective-ice implementation completes checkpoint 3 under the
revised scope below, not as a matched-polymorph model.

**Scope revised by the user on 2026-10-26:** scientific plausibility with explicit
approximations is the completion standard, not research-grade deep-ice accuracy.
The authoritative [checkpoint-3 completion plan](planet-structure.md#revised-completion-scope--scientific-plausibility-not-research-completeness)
now specifies a cited effective high-pressure ice compression proxy, approximate
phase boundaries, a mandatory solved 300 K/30%-water fixture and finite numerical
acceptance tests. Exact cold polymorphs and matched deep-ice Gibbs surfaces are
deferred, not blockers. Existing conservation, one-sided interfaces, bounded work
and honest model provenance remain required. The user subsequently authorised
resuming and finishing this scoped checkpoint. **Checkpoint 3 is complete:** the
300 K fixture solves with effective ice and passing independent numerical tests;
the 250 K deep path now also solves with labelled approximate ice physics.
The subsequent completion pass also fills the pressure-melted liquid gaps
between the cold and warm prescriptions, with explicit approximate density
metadata and independent numerical checks. See the
[acceptance results](../simulation/planet-composition.md#effective-deep-ice-and-checkpoint-3-acceptance).
The user subsequently authorised checkpoint 4 and requested committing the
checkpoint-3 baseline first (`fc1dd43`). Checkpoint 4 was then committed at the
user's request as `158db7a`, after another passing full check. Nothing was pushed.

Checkpoint 4's opt-in atmosphere/condensed-body coupling is complete within its
restricted domain and passes the full check script;
model, limits and acceptance fixtures are in the
[canonical coupling guide](../simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling).
It matches finite gas inventory, partition and pressure loading to dry/water
radial structures. Unsupported dense/weakly bound columns and non-water
condensates fail explicitly. Existing generated H/He envelopes remain labelled
surrogates, not ideal-gas giant interiors. Generation, transport, consumers and
unrestricted free-camera descent are unchanged.

**Plan extended by the user on 2026-10-26:** the requested generation/readout
integration exposed a coverage blocker: formation supplies protected non-water
volatiles on every naturally generated planet, outside the current condensed EOS
coverage. The user chose to expand material and giant coverage rather than make
the old model a permanent alternative. Completed checkpoints 1–4 retain their
numbers. New checkpoints **5–10** below precede the former last two checkpoints:
generation/transport/consumers becomes **11**, and calibration/user acceptance
becomes **12**. This supersedes the earlier selective-migration completion target.
The current request authorises this plan update, not implementation of all new
checkpoints, additional commits, push or merge.

The endpoint is one coherent region-owned structure system covering the declared
generated population, followed by deletion of the obsolete whole-body radius,
envelope and inertia approximations and their runtime adapters. Explicit local
EOS, mixture and thermal approximations remain acceptable; a universally accurate
EOS or research-complete phase diagram is not promised. Missing physics must not
be concealed by dropping traces, relabelling materials or silently falling back.

### Architecture review · extensible materials and boundaries

Review of `refactor/planet-composition`, 2026-10-26. Preserve the working
conservation model, independent numerical checks, one-sided interfaces and bounded
failure handling. No restart or new generic plugin framework is recommended.
The gaps below are extension requirements, not claims that completed restricted
models implement them. Existing uncommitted roadmap edits remain part of this plan.

#### One catalogue, one inventory, one solved allocation

These are three different authorities, not one mutable global planet store:

1. **Material catalogue:** what each stable constituent means and which physical
   models support it. `materials` owns this shared knowledge, with separate files
   for equations/tables rather than one enormous constants table.
2. **Per-body retained inventory:** how many kilograms of each constituent exist.
   Formation and escape determine this input; the catalogue does not choose it.
3. **Immutable solved structure:** where those kilograms reside under specified
   thermal, exposure and arrangement assumptions. Regions own allocations;
   atmosphere/hydrosphere/interior summaries, boundaries and labels are projections.

Today identities are centralised in `materials.rs`, but legacy fit/gas adapters
live in `composition/catalogue.rs`, molecular masses in `atmosphere.rs`, and
cold-trap curves in `atmosphere/condensation.rs`. This is a transitional split,
not the completed material-physics ownership. Checkpoints 5–6 should establish
one discoverable capability description per constituent: identity/formula and
mass convention, EOS/phase models and their domains, required thermal quantities,
sources/versions, approximation status and supported mixture/partition routes.
The description references model code; constants alone cannot implement an EOS.
Move reusable molecular/phase properties to `materials` as consumers migrate;
keep gas conversion and atmospheric optics as downstream adapters. Do not make
shared material physics depend on atmospheric presentation types or copy curves.
Retire old fit-only metadata with its solver at checkpoint 11.

An extension checklist must cover stable ID/serialization, inventory/gas mapping,
domain/phase capability, mixture policy, allocation, transport and reference tests.
An unsupported capability is explicit, not a default density or an empty phase.
Test catalogue/mapping completeness over `Material::ALL`, so a newly added material
cannot silently bypass accounting or claim support from its identifier alone.

#### Density, phase and allocation answer different questions

- A local EOS evaluates density and supported thermal properties at pressure,
  temperature and composition. A phase model selects stable branches or reports
  approximate/unresolved/coexisting states. Neither chooses a whole planet's
  temperatures, accessible supply or geological arrangement.
- The structure/partition solve couples hydrostatic balance, those local models,
  a prescribed thermal path and an explicit exposure/mixing/differentiation rule.
  Preserve protected versus exchangeable supply without treating protected
  molecules as automatically exposed surface deposits. A local equilibrium
  assumption is not global equilibration of a sealed interior with the atmosphere.
- For dilute gas over a nearly pure condensate, partial-pressure saturation caps
  can be a declared approximation. Dense mixtures need supported fugacity/activity
  or an explicitly bounded substitute. Pure-component critical points and
  vapour-pressure curves do not determine mixture phase splits. Never let a
  mass-weighted average of critical points become a mixture phase model.
- Specify miscible versus separate phases and where trace constituents reside.
  A conserved trace need not create an infinitesimally thin pure shell: it can
  occupy a declared mixed host region with an explicit density/phase approximation.
  Do not silently delete its mass, assume solubility or sort shells by ambient
  density. Unsupported reactions, clathrates and mineral binding remain excluded
  or explicitly unresolved, not implied by simple volume mixing.
- Near coexistence, supply/energy/entropy or an explicit allocation condition must
  close phase fractions where P–T alone does not. Near a critical point, disappearing
  density contrast must not manufacture an interface or make the solve chatter.
  A change of EOS implementation is model provenance, not a new physical layer.

The shared radial kernel currently iterates a fixed metal/silicate/water order;
water wraps `dry` region/control/error types and dispatches its pressure events.
This is appropriate for the tested baseline, but not yet a general region engine.
In checkpoints 6–9, incrementally supply ordered region/mixture prescriptions to
the existing kernel and move genuinely shared result/control concepts out of the
dry-specific API. Support temperature-dependent event location along the selected
P–T path, rather than assuming all phase boundaries are fixed pressure isotherms.
Keep model selection/partition separate from numerical integration. Retain the
current analytic, quadrature, trace and work-accounting tests during this change.

#### Boundary knowledge, not a planet-class ground switch

Derive a small boundary description from the solved regions. Keep independent:
composition interfaces, phase interfaces, density-model seams, reference-pressure
levels, optical extent and any mechanically solid boundary. Several can occur at
different radii; some need not exist. Each reported boundary needs its physical
meaning, radius/P/T where known and resolved/approximate/unresolved provenance.

- **Air over solid:** a supported solid rock/metal/ice interface can provide a
  physical floor even beneath 1,000 or 10,000 bar (100 MPa or 1 GPa). Test the
  thermal/phase model there; high pressure neither removes ground nor proves it
  remains solid. The existing cold refractory EOS leaves solidity unresolved.
- **Air over liquid over solid:** the liquid top and submerged solid floor are
  different boundaries. An ocean surface is not landable rock. Water or methane
  inventory alone cannot establish either exposed liquid or a solid substrate.
- **Continuous fluid envelope:** a gas-like to dense/supercritical-fluid path
  need not cross a first-order phase boundary. Do not invent a gas/ocean surface
  at the critical point or one bar. A deeper composition interface or solid core
  may still exist independently. “Ice giant” does not assert an exposed ice floor.
- **Model stops before an answer:** distinguish “no solid boundary within this
  solved interval” from “boundary unresolved below this model limit”. Neither
  means “the entire planet has no solids”. Missing phase knowledge is not a
  negative detection. Rendering/navigation consume that distinction; collision,
  landing and camera permissions remain separate policies.

Checkpoint 5 specifies these semantics, checkpoints 6/8/9 exercise them, and
checkpoint 11 transports them without UI/rendering reclassification. No universal
pressure, envelope fraction or display label switches physical ground on/off.
Spherical liquid shells are in scope; patchy methane lakes, continents and local
ice cover need terrain, local climate and redistribution in the
[surface plan](planet-surfaces.md). Their eventual material comes from the same
allocation, not a second decorative liquid inventory.

**User clarification, 2026-10-26: liquid surfaces before terrain.** A supported
exposed liquid allocation may form a smooth global shell. Its outer boundary is
the visible ground/reference surface for presentation and approach clearance,
using the existing spheroidal geometry without requiring a separate hydrosphere
renderer. Preserve liquid material/phase and any submerged boundary in the physical
record; reusing ground geometry does not turn liquid into solid rock or establish
landing support. This is a future checkpoint-11 consumer policy, not a change to
current rendering, collision or unrestricted camera permissions.

Shell thickness follows allocated liquid mass and the selected density/structure
model, not an arbitrary minimum visual depth. A small methane supply can therefore
produce a thin layer under the declared global-coverage approximation. Preserve
trace accounting and explicit numerical-resolution limits. Only genuinely exposed
liquid qualifies: protected liquid at depth and continuous supercritical envelopes
must not acquire a fabricated surface. Waves, shorelines, patchy lakes, local
coverage and redistribution remain later terrain/hydrosphere work. Acceptance must
check that presentation uses the liquid-top radius while transport still identifies
it as liquid, including thin-shell and freeze/evaporation transitions.

#### Finite scientific acceptance map

Checkpoint 5 freezes these case requests and assigns unresolved inputs/models to
later owners in its [scoped handoff](planet-coverage.md#scope-reset-and-frozen-handoff).
Each implementation slice must fix its own numerical inputs/domains before coding;
not every planet request is already a complete solvable fixture. They supplement,
rather than replace, existing numerical acceptance gates. Unsupported cases must
name the missing capability; replacement completion still requires coverage of the
agreed generated population.

| Case | Required distinction / test | Owning checkpoints |
| --- | --- | --- |
| Dry rock, exposed ice, airless/trace gas | Known material boundary versus unresolved refractory solidity; no fabricated 1-bar radius | 5, 8, 9 |
| Rock/ice-rich body under H/He at 100 MPa and 1 GPa | Locate an interface if supported; no pressure-triggered loss of ground; solid claim requires a phase prescription | 7–9 |
| Water ocean above rock or deep ice | Separate liquid top, submerged boundary and atmosphere; preserve water mass through freeze/evaporation | 6, 8, 9 |
| CH₄ at prescribed cold surface conditions near 94 K | Supply-limited vapour plus liquid where supported; frozen and evaporated neighbours; label idealised pure CH₄, not a Titan reconstruction | 6, 9 |
| CO₂ solid, liquid, gas and supercritical fluid | Triple/critical neighbourhoods, saturation and compressed-fluid branches; no liquid below the pure-fluid triple pressure | 6, 9 |
| N₂/O₂/Ar and actual protected traces | No missing mass, fictitious pure shells or automatic exposure; supported mixed-host policy | 6, 9 |
| Mixed volatile and H/He-rich fluid | Pure/trace/mixture limits, declared nonideal treatment, unsupported pair detection; no pure-component critical shortcut | 6–9 |
| Small envelope through giant | Self-gravity, thermal anchors, stripping limit, continuous fluid with independently described deep boundary | 7–9 |
| Phase appearance/disappearance and EOS seams | Stable one-sided states, closed allocations, refinement and no invented physical jump at a model seam | 6–9 |
| Future material registration and domain edges | Completeness checks, explicit missing capabilities and unchanged old fixtures; bounded unsupported/work failures | 5–10 |

#### Public scientific starting points, not a new research programme

Use measured/reference properties and published equations first; use bounded,
labelled local approximations where the selected sources do not cover the agreed
domain. Record a model's validity mask, not just the largest P/T its code accepts.
Public availability is not redistribution permission. Pin selected releases/data
and check their licences before import. These references are candidates/reference
checks, not newly selected dependencies or extensions to current runtime physics:

- [NIST CO₂ phase data](https://webbook.nist.gov/cgi/cbook.cgi?ID=C124389&Mask=4)
  and [methane phase data](https://webbook.nist.gov/cgi/cbook.cgi?ID=C74828&Mask=4)
  provide experimental reference points and bounded correlations. Different
  historical measurements are not interchangeable coefficients to average blindly.
- [Span–Wagner CO₂ via CoolProp](https://coolprop.org/fluid_properties/fluids/CarbonDioxide.html)
  identifies a published fluid EOS (1996, DOI `10.1063/1.555991`): triple point
  about 216.59 K/5.18 bar, critical point about 304.13 K/73.77 bar. In its stable
  fluid domain, supercritical labelling requires both T > Tc and P > Pc;
  exceeding critical pressure alone is insufficient. Check the solid stability
  boundary too. The fluid EOS does not supply arbitrary deep solid CO₂ physics.
- [Setzmann–Wagner methane via CoolProp](https://coolprop.org/fluid_properties/fluids/Methane.html)
  identifies the 1991 fluid EOS (DOI `10.1063/1.555898`), with triple point about
  90.69 K/0.117 bar and critical point about 190.56 K/45.99 bar. Pure methane
  boiling near 111.7 K at roughly atmospheric pressure is not a universal lake
  temperature; local pressure, mixture, supply and thermal assumptions matter.
- [CoolProp mixture documentation](https://coolprop.org/fluid_properties/Mixtures.html)
  explains the need for mixture interaction models and warns that guessed mixing
  rules can give entirely wrong results. Use it to scope pair coverage and
  reference fixtures, not to assume all combinations or planetary pressures work.
  Existing IAPWS and dense-H/He candidates remain in the
  [structure extension gates](planet-structure.md#water-atmospheres-and-giants-explicit-extension-gates).

Mapping a finite declared material/regime set is feasible. Covering every possible
composition, pressure, temperature, chemistry and history is not a completion
promise, even with known constants. Keep structural/phase approximations honest
and conditional on thermal/exposure assumptions; exact climates and real planetary
interior compositions are not all known. This review does not reopen checkpoint 3
or authorise implementing the new checkpoints.

### Ownership and scope

- Retained inventory is the conserved input, with source and escaped masses
  separately accounted. A solved planet allocates each material into physical
  regions whose per-material totals reproduce that inventory.
- A region carries composition and a radial profile of pressure, temperature,
  density and phase. Regions can contain mixtures; neither each material nor
  each phase automatically needs its own shell. Numerical integration cells
  are implementation detail, not separate named geological layers.
- Interior, condensed and atmospheric reservoir totals become derived views.
  During staged development only, keep the old model behind an explicit adapter;
  never allow both allocations to be edited as truth. That adapter is temporary
  and must be removed at checkpoint 11, not retained as the final unsupported path.
- The atmospheric column describes material already allocated to the atmosphere
  or envelope. A giant's outer column is a subset of its mixed envelope, not
  additional mass. A one-bar radius is a reference within that envelope.
- Begin with spherical hydrostatic structure and declared differentiation/mixing
  prescriptions, using local phase equilibrium only where supported. Select and
  declare thermal prescriptions; composition and irradiation alone do not fix
  an interior.
  Retain existing spheroidal geometry as a separate approximation.
- Do not claim real-time differentiation, convection, impacts, chemical reactions,
  climate, terrain or interior evolution. Bulk water does not imply an ocean.

### Intended module ownership

Under `crates/simulation/src/system/`, retain the domain namespaces:

- `composition`: inventories and conservation bookkeeping.
- `materials`: shared material identities, thermodynamic properties,
  equations of state (EOS), phase and mixture prescriptions. Move existing
  definitions when needed, rather than creating another material catalogue.
- `structure`: regions/layers, radial integration, thermal prescriptions and
  boundary/reference matching. Keep current radius fits isolated as approximate
  models or test comparisons, not hidden substitutes for failed solves.
- `atmosphere`: outer-column and optical calculations using the solved boundary.
- `generation`: procedural inputs, retention and bounded orchestration of the
  coupled solve. No interior differential equations in generation code.
- `physical`: validated transport of solved facts. Client UI, rendering and
  navigation consume those facts, never independently infer physical boundaries.

Use existing `name.rs` plus sibling-directory conventions, private implementation
modules and a small explicit public API. Add files as real responsibilities arrive.

### Checkpoints and acceptance gates

1. **Model contract and scientific prescriptions, before population changes.**
   Specify inputs, outputs, region semantics, mass ownership, thermal boundary
   conditions, supported EOS/phase/mixture domains and citations. Define numerical
   tolerances, iteration/work limits and explicit unsupported/failure outcomes.
   Include sample dry, water-rich, thin-atmosphere and H/He-envelope records.
   Exit: one unambiguous allocation model and a reviewed scope for the first solver.
2. **Independently tested radial solver.** Solve `dm/dr = 4πr²ρ` and
   `dP/dr = -Gmρ/r²`, closed by the selected EOS and temperature prescription.
   Handle the centre, interfaces and outer pressure boundary explicitly. Start
   with analytic constant-density and differentiated dry-body fixtures. Test
   mass closure, pressure continuity, convergence under resolution refinement,
   inertia and gravity independently of the implementation's own identities.
   Exit: bounded, deterministic solver, not yet the generation default.
3. **Water-bearing condensed structures.** Finish the headless solver using the
   revised [plausibility-focused completion scope](planet-structure.md#revised-completion-scope--scientific-plausibility-not-research-completeness).
   Keep explicit thermal assumptions, cited compression approximations and
   approximate ice boundaries. Verify equation implementation, conservation,
   hydrostatic closure, interfaces and convergence; research-complete phase
   matching is not required. Solve the mandatory 300 K/30%-water fixture and
   document cold-path limitations without expanding the research programme.
   Exit: plausible water-rich condensed structures with meaningful outer
   boundaries, explicit approximation metadata and passing bounded numerical tests.
4. **Atmosphere/envelope coupling.** Match boundary pressure, mass and radius
   iteratively, including volatile partitioning. Do not extend the ideal-gas outer
   column into dense H/He and call it a giant interior. Add giant structure only
   with a suitable EOS and thermal prescription; until then identify the existing
   envelope approximation explicitly. Preserve unrestricted free-camera descent.
    Exit: no duplicated gas inventory or independently chosen surface pressure.
5. **Replacement readiness review and thermodynamic contract. Complete under the
    user-approved scope reset, 2026-10-26.** Freeze the finite material/regime map,
    generated-input audit, origin/region/boundary contracts, existing source findings,
    fixture requests and proposed numerical/data budgets. Classify each route as
    ready to implement, needing a decision or deferred; assign unresolved physics
    and fixture completion to later checkpoints with concrete acceptance conditions.
    Define one bounded implementation slice using already selected physics.
    The [coverage specification and frozen handoff](planet-coverage.md#scope-reset-and-frozen-handoff)
    is authoritative. This replaces the earlier requirement to select every
    EOS/phase/mixture/thermal route before implementation. Source pins and successful
    algebra audits do not establish physical coverage. No further research or
    audit-tool expansion is required to close this checkpoint.
    Exit met: reviewed readiness map, fixed requirements and request list, owned
    gaps and finite checkpoint 6a. Later slices must select their own missing models,
    domains and finite inputs before implementing the affected routes. Full generated
    coverage still gates checkpoint 10; integration and legacy deletion still gate
    checkpoint 11. Generation-domain changes require physical rationale and user
    agreement, never suppression of unsupported draws.
6. **Secondary volatiles and condensed mixtures.** Implement the selected local
    density and approximate phase prescriptions for CO₂, N₂, CH₄, O₂ and Ar,
    including protected material, gas/condensed transitions and trace allocations.
    Declare prescribed layering versus mixed-region rules; separate pure-material
    equations alone do not solve a mixture or determine its phase fractions.
    Preserve molecular/constituent identity rather than treating all condensates
    as water. Extend the existing region/kernel machinery where needed, retaining
     one-sided interface states and accountable nested work.
     Replace heuristic cold traps with the selected phase/partition route on this
     new path; retain a labelled dilute approximation only within its declared
     domain. Exercise methane liquids and all CO₂ fluid states, not just gas/ice.
    Exit: cold/warm secondary-volatile and mixed-water fixtures, including the
    actual generated trace pattern, conserve every material and pass independent
    mass/inertia/hydrostatic, domain, seam, refinement and budget-failure tests.
    **6a: protected traces in dry hosts is complete**, with its fixed acceptance set and
    stop condition are in the [handoff](planet-coverage.md#next-implementation-slice-checkpoint-6a).
    This is one headless end-to-end slice, not all of checkpoint 6. Water-host traces,
    exposed condensates, solid/sublimation routes and mixture partition remain
    separate follow-on work; each needs its own bounded implementation inputs and
    any missing model decisions. Do not hold 6a open for those routes or claim
    checkpoint 6 complete from 6a alone. Deep/hot component extensions belong to 8;
    complete atmosphere/planet matching belongs to 9.
7. **Dense hydrogen, helium and H/He mixtures.** Implement the selected nonideal
    local EOS with density and the thermal quantities needed for adiabatic paths,
    not an ideal-gas continuation or a whole-body giant radius fit. Include pure-H,
    pure-He and mixed endpoints, molecular/dissociated/ionised regimes as required
    by the agreed coverage, and honestly labelled high-pressure transition knowledge.
    Pin interpolation/inversion conventions, no-extrapolation masks and mixture
    rules. Heavy-material loading in envelopes must have an explicit prescription;
    helium is not automatically a core, and helium rain is not implied.
    Exit: published/reference equation checks, positive/compressive stable branches,
    thermal derivative or adiabat consistency tests appropriate to the selected
    model, dilute-limit matching, mixture limits and bounded failure tests.
8. **Thermal profiles and hot/cold coverage.** Add prescribed convective adiabats
    or an explicitly approximate equivalent, irradiated outer thermal anchors and
    a documented age/entropy or internal-luminosity prescription. Entropy and energy
    zero points must agree wherever used across EOS components. Age plus irradiation
    is not a unique cooling history; transport the chosen assumptions. Extend water
    to vapour, dense/hot fluid and effective deep states needed by the coverage
    matrix, and extend refractory pressure/temperature density coverage as needed.
    Do not infer refractory melting/solidity from a cold density proxy. Locate or
    explicitly model thermal transitions; missing energy/coexistence constraints
    cannot be repaired with arbitrary phase fractions.
    Exit: finite cold/warm/strongly irradiated and young/old envelope paths spanning
    the agreed population, with tested temperature/entropy behaviour and interface
    conventions. No full climate, evolutionary cooling simulation or complete
    refractory phase diagram is required.
9. **Unified self-gravitating structures and outer matching.** Integrate dry,
    volatile-rich, small-envelope and giant bodies using the same conservation and
    region contract. Include envelope mass in `m(r)`, hydrostatic gravity and inertia;
    remove the dilute coupler's 1% restriction only on the newly validated path.
    Match the dense envelope to its outer column with continuous mass, pressure
    and radius, explicit thermal conditions and one gas inventory owner. Define
    finite outer/tail and weak-binding conventions without inventing a static
    atmosphere for an escaping state. One bar is a located reference, never ground.
    Exit: headless dry/water/secondary-volatile/sub-Neptune/giant fixtures pass
    independent closure and refinement, EOS-seam tests, stripping limits and
    deterministic failure tests. Broad giant mass/radius/inertia trends must be
    plausible before generation integration, not deferred entirely to visual review.
10. **Population readiness, retention and bounded cost.** Exercise the coverage
    matrix over deterministic population samples before making it the default.
    Replace the old structure dependency inside retention, including escape-speed
    and H/He-survival evaluations, without rerunning loss history or changing stable
    IDs/orbits inadvertently. Use deterministic reuse, interpolation or a separately
    validated bounded accelerator if needed; no disguised legacy radius-fit path.
    Establish total nested work/memory ceilings, cache keys/versioning, eviction,
    concurrency and order-independent reproducibility. Lightweight snapshot checks
    must validate transported inventories, region/interface summaries, provenance
    and boundary semantics without trusting arbitrary derived values or repeating
    the full solve on each read. Record cold/warm generation and search costs and
    cache-hit behaviour; the user runs interactive performance benchmarks.
    Exit: the representative declared population has no unresolved coverage holes,
    unexplained solve failures or dependency on legacy structure physics, and a
    measured cost/validation strategy is ready for integration. Budget exhaustion
    and nonconvergence stay visible, never silently select the old model.
11. **Generation, transport, consumers and legacy retirement (formerly 5).** Make
    the unified model drive generated retained planets at final-orbit forcing.
    Derive reservoir summaries from region allocations and carry consistent radius,
    reference levels, gravity, phases/unknowns and inertia through snapshots, shape,
    atmosphere/optics, rendering and navigation clearance. Preserve unrestricted
    free-camera descent. Ship structured inspector/info-readout sections for model
    and thermal assumptions, ordered layers (material, phase knowledge, mass,
    radius bounds, pressure/temperature endpoints), atmospheric references and
    numerical diagnostics/limitations. Mark approximate phases as approximate;
    summaries come first, full sampled profiles may remain on demand.
    Delete obsolete compressed-volume whole-body radius fits, envelope surrogates,
    heuristic structure-inertia rules and temporary model-selection adapters once
    all callers, including retention and validation, have migrated. Replace obsolete
    tests/docs rather than keep runtime fallback code for comparison. Retain shared
    inventories, useful tested local EOS and independent reference fixtures.
    Missing old snapshot facts may be explicitly unknown or require regeneration;
    compatibility must not reactivate the deleted structure solver.
    Exit: natural planets visibly expose the new system, all consumers agree, and
    code/dependency searches plus tests demonstrate no legacy structure runtime path
    remains. If the new model still cannot cover generated regimes, this checkpoint
    is incomplete: do not delete working code early or declare permanent fallback
    to be completion.
12. **Calibration and user acceptance (formerly 6).** Check reference trends,
    population changes, conservation, interfaces, stripping, shape/inertia,
    clearance, rendering and inspector semantics across all newly covered regimes.
    Run `./scripts/check.sh` and affected shader checks. Supply reproducible natural
    sample IDs and launch/readout instructions for airless rock, ice/liquid water,
    secondary volatiles, small envelopes and giants, including approximate and
    genuinely unresolved phase examples. The user runs performance benchmarks and
    visually reviews the changed presentation. Fix regressions, update canonical
    guides and confirm the old structure implementation is absent. Commit, push
    and merge still require their own authorisation.

Checkpoint 2 supplies the tested headless dry-body baseline. Checkpoint 3 supplies
the scoped water-bearing solver with approximate deep ice, without switching generation over
or replacing all planet types at once. Each checkpoint should leave a tested
usable baseline. The current old model remains available during development, not
as the final solution for missing EOS. New checkpoints 5–10 establish replacement
coverage; 11 is the integration and deletion gate; 12 is final user acceptance.
"Complete" means coherent within a declared, tested generated domain, not an EOS
for every physically possible planet. Outside that domain, failures remain
explicit. This roadmap does not add real-time chemistry, convection, weather,
terrain, helium rain or a full planetary-evolution research programme.
