# Planet composition refactor

## Status and goal

Checkpoints 1 through 3 implemented. Generation draws inventories, accounts for
escape and reconciles retained structure at final orbits. Atmospheres and cold
condensates share that budget. Labels and full transport await checkpoint 4.
Physical prescriptions and limitations are in the
[composition guide](../simulation/planet-composition.md).
Work on `refactor/planet-composition`,
branched from checked and pushed `main` including flight clearance. The abandoned
galactic-dust and HTML UI branches are excluded. The user will visually test the
finished implementation before it is merged into `main`.

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

- [ ] Derive rocky/metal-rich, water-rich and envelope-world descriptions using
  documented physical thresholds. Conventional size labels may overlap in nature;
  deterministic display rules must be explicit and not feed back into physics.
- [ ] Remove composition-class use of `Dwarf`. Do not assert orbital clearing from
  a small mass alone; defer a dynamical dwarf classification unless justified.
- [ ] Audit generation, shape, surface/photometry, search filters, fixtures,
  snapshots and client rendering for class dependencies. Appearance proxies must
  use explicit physical inputs and be labelled as approximations where needed.
- [ ] Carry composition and structure facts through generated systems and task
  overlays, with validated wire conversion and explicit unknown legacy state.
- [ ] Show material fractions, retained mass, density, envelope fraction and
  solid/reference-radius meaning in the inspector and developer readout.
- [ ] Introduce a small typed inspector presentation model with stable field IDs,
  sections, labels, units, formatted values and explicit unknown values. HUD and
  developer export consume the same description; physical records remain separate.
  Use Overview, Composition, Orbit, Rotation and Atmosphere sections, leaving room
  for hydrospheres without introducing a general schema/configuration framework.
- [ ] Bound the expanded inspector layout deliberately, using collapsible sections
  or scrolling rather than appending unbounded rows. Preserve the existing HUD style.
- [ ] Update classification filters and registry tests. Additional composition
  search operators are optional follow-up, not required for this refactor.

Exit: changing a label threshold changes descriptions only, never physical output.

### 5. Calibration, regression checks and documentation

- [ ] Use prescribed Mercury/Earth-like, water-rich, Neptune-like, Saturn-like and
  Jupiter-like inputs to check sensible structure trends. Declare broad justified
  tolerances and model domains, not exact matches or formation reconstructions.
- [ ] Test mass conservation, radius/density trends, envelope-stripping limits,
  threshold continuity, convergence/fallbacks and invalid/extreme inputs.
- [ ] Sample deterministic populations across host masses, ages and multiplicity.
  Record mass, radius, density, material/envelope fractions and class frequencies;
  look for implausible spikes, missing populations and pathological outliers.
- [ ] Verify that each stable ID regenerates reproducibly regardless of generation
  order, cache state and supported search slicing/threading.
- [ ] Verify spin/shape, atmospheric optics, warp clearance, normal/overlay
  transport and inspector regressions. Run `./scripts/check.sh`.
- [ ] Update the canonical planetary-system and atmosphere guides, plus relevant
  search/interface documentation. Clearly separate implemented limits from plans.

Exit: automated checks pass and reviewable population summaries explain the change.

### 6. User visual review and integration

- [ ] Provide launch instructions and reproducible sample IDs covering airless
  rock, water-rich bodies, small envelopes and giants across representative hosts.
- [ ] User checks physical sizes, flattening, atmosphere appearance, inspector
  facts and craft clearance. Agents do not launch performance benchmarks.
- [ ] Fix issues and rerun the affected tests and full check script.
- [ ] Commit and push the refactor branch for review. Keep `main` at the tested
  baseline until the user approves integration.
- [ ] After approval, merge into `main`, rerun checks as needed and push `main`.

## Decisions and completion boundary

No further user decision blocks starting. Use the defaults above, choose and
document scientific prescriptions during implementation, and keep the user
informed if evidence requires materially changing scope or behaviour.

Success is a coherent, extensible approximate model with conserved inventories,
physical downstream inputs and descriptive labels. It is not a validated solution
for every interior, climate, hydrosphere or geological surface. Visual review
supplements physical tests; it cannot establish scientific accuracy by itself.
