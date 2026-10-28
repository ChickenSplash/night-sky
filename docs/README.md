# Documentation

Night Sky is a space simulator first. Its galaxy is synthetic and procedural,
grounded in observations and scientific research. Fictional lists and tasks
are an optional information overlay. Future external integrations belong in
optional plugins, not the simulator core; see [the overlay guide](shared/information-overlay.md).

Native Linux is the only client target. The browser/WASM build was dropped
(tag `web-final`); see [the native-only plan](plans/native-only.md).

Start with [development](development.md) for setup, project structure, logs and
checks, or [controls](client/controls.md) to use the app.
[AGENTS.md](../AGENTS.md) maps code areas to these guides and defines working
and documentation conventions.

## Simulation

Engine-independent models in `crates/simulation`, grouped into `galaxy/`,
`star/`, `system/`, `orbit/`, `craft/` and `search/`. Shared clock, units,
seed helpers, object identities and photometry stay at the crate root; see
[project structure](development.md#project-structure) for ownership.

- [Galaxy](simulation/galaxy.md): component shapes and ages, regional density,
  exact populations, cells, brightness tiers and deterministic identity.
- [Stars and multiple systems](simulation/stars.md): spectral labels, mass/age
  generation, evolved stars, companions, barycentres and stability zones.
- [Planetary systems](simulation/planetary-systems.md): generation sequence,
  snow line, mass–radius fits, Hill spacing, resonances, spins and stable body IDs.
- [Physical atmospheres](simulation/atmospheres.md): initial retention/generation,
  gas mixtures, reference pressure levels, isothermal columns and their limits.
- [Material inventories and structure](simulation/planet-composition.md): conserved
  formation/retained budgets, approximate generated radii, bounded H/He retention
  and separate headless dry/water radial solvers, protected traces in dry and
  water-bearing hosts, atmosphere coupling and opt-in background refinement
  shown in the inspector (rendering and navigation still use legacy data).
- [Pure-water physics](simulation/water-physics.md): headless liquid/ice-Ih EOS,
  compressed-liquid extension, prescribed 300 K ice-VII compression, melting
  curves, reference audits and deep-ice gates.
- [Composition scientific reference](simulation/planet-composition-reference.md):
  generated-input bounds, selected/candidate physics, source pins, unresolved
  scientific issues, fixed acceptance requests and proposed extension budgets.
- [Orbits and time](simulation/orbits.md): orbital elements, Kepler's equations,
  worked examples, nested barycentres, units and clock behaviour.
- [Object search](simulation/search.md): registry, query syntax, relationships,
  nearest-first search, completeness and the search thread.
- [Craft](simulation/craft.md): the man-made object model, gravity from every
  body, and fixed-step integration.

## Client

The Bevy/egui client in `crates/client`:

- [Controls](client/controls.md): shortcuts, travel, inspection and presentation modes.
- [Navigation](client/navigation.md): visits, physical arrival, selection,
  picking, camera frames and precision.
- [Streaming](client/streaming.md): compact generation, loading, magnitude
  changes, the loader thread and spatial queries.
- [Realistic rendering](client/rendering.md): physical light, reflectance and phase,
  exposure, compression, HDR, sprites, scale and cameras.
- [Galactic rendering](client/galactic-rendering.md): GPU point batches,
  precise sprites, multiple-system resolution and diffuse galaxy glow.
- [Debug tools](client/devtools.md): console, command path, performance
  readouts and snapshots.
- [Developer API and CLI](client/developer-api.md): local reads, discovery,
  commands, captures, inspector export and search endpoints.
- [Benchmarks](client/benchmarks.md): fixed routes, historical baselines,
  measurement procedures and result files.

### Interface and shared UI kit

- [Scene interface](client/ui/interface.md): windows, HUD, breadcrumbs,
  inspector, actions and input capture.
- [Object browser](client/ui/browser.md): query controls, filters, ranges,
  results and selected-object readouts.
- [UI kit](client/ui/kit.md): reusable themes, widgets, interaction,
  animation, windows, scrolling and extension rules.

## Shared data and server

- [Information overlay](shared/information-overlay.md): lists, stable task
  slots, pages, assigned stars and the relationship between tasks and planets.
- [Galaxy backend](server/backend.md): demo serving, polling, configuration,
  network exposure and the boundary with the native developer API.

## Plans and history

- [Object search](plans/object-search.md): original plan, completed sessions,
  historical measurements and future ideas. This is not the current UI specification.
- [Flight simulator](plans/flight-sim.md): alpha proposal for a walkable ship with
  warp and sublight drives, real gravity and layered autopilot.
- [Camera location](plans/camera-location.md): one location rule for the
  breadcrumb and the camera's motion, and star and barycentre markers.
- [Ship schema](plans/ship-schema.md): one source of truth for ship orders and
  state, generating `ns ship` and the TUI's Ship tab.
- [Native only](plans/native-only.md): why the browser/WASM build was dropped
  and what the removal covered.
- [Planetary atmospheres](plans/atmospheres.md): completed generation, optics and
  per-planet rendering work, validation history and explicitly deferred follow-ups.
- [Planet surfaces](plans/planet-surfaces.md): bodies flattened by their spin,
  landing on the ground, then geology, water and giants' depths.
- [Planet composition replacement](plans/planet-composition.md): rock-and-water
  prototype first, completed headless foundation, conserved-region contract, then
  envelopes and default cutover. Methane physics is parked on its own branch.

## Current limitations

- No N-body interactions, moons or collisions.
- Dust and lensing are not implemented. Star systems have galactic velocities,
  but stars do not move around the galaxy. Diffuse galaxy glow
  is an approximate smooth prototype, not a detailed dust-attenuated renderer.
- Camera flights are navigation aids, not physical spacecraft or warp simulations.
  Craft have six-axis CLI flight, pointing assists and a predicted coast
  path, can be followed across systems, and have unsafe CLI warp up to
  10,000 ly/s, steered by turning the nose or pointing, without a turn limit
  or collision or missing-data stops. Above light
  speed they enter no systems; below it, entering an uncharted system can
  briefly hold the clock while its region and bodies load. Explicit CLI dropout
  can match a local body's velocity or add speed for a rough orbit, without
  navigating to it. No in-app piloting
  controls, automatic arrivals, atmospheric exclusion, saving or collisions yet;
  predicted impacts outside warp are shown but not enforced.
- Bodies use gizmos or pooled per-body meshes; further body batching remains future work.
- Atmospheres use approximate generated columns and visible optics,
  not solved climate or atmospheric evolution. No generated clouds/aerosols or
  greenhouse solution yet. Supported columns use spherical variable gravity;
  weak-binding cases retain an explicitly unresolved exponential fallback.
  Bulk mass/radius, atmospheric gas and cold condensates now share conserved
  inventories. Descriptive labels never select physical properties; composition
  and reference facts are validated in transport and visible in the inspector.
  Table-readiness pop-in, grey view-path transmission and airless point/rim
   photometry remain (traversable envelopes omit the airless disc rims). Bulk H/He
   envelopes allow free-camera descent into approximate opaque darkness without
   a pressure stop, not physically simulated interiors. One-bar pressure references
   alone do not enable descent. Low-gas rocky bodies keep their approximate ground
    and inventory-normalised boundary pressure even with extended geometry.
   See [physical assumptions](simulation/atmospheres.md)
  and [rendering limits](client/rendering.md#atmospheres).
- Bodies are smooth spheroids flattened by their spin, with no terrain yet.
  Layered results can be calculated/cached through the opt-in
  [background prototype](simulation/planet-composition.md#experimental-background-loading),
  and shown in the planet inspector, but do not yet drive rendering, shape,
  atmosphere optics or navigation. The dry solver's
  protected-trace extension conserves
  CO₂/N₂/CH₄/O₂/Ar in dry hosts without resolving their chemistry or phase.
  Local water physics, including compressed liquid,
  is independently tested. A separate headless solver supports restricted
  liquid/ice-Ih shells and their narrow supported phase-interface overlap, but
  effective compressed deep ice now enables the 300 K water-rich fixture.
  Cold water down to 20 K and airless or thin-air ice surfaces also solve using
  explicitly labelled approximations.
  Pressure-melted liquid gaps are covered by a labelled density approximation.
  [Warm/deep and hot-water proxies](simulation/water-physics.md#hot-and-warm-deep-prototype-extension)
  extend the prototype to 2,000 K, without claiming resolved hot phases.
  Exact deep-ice polymorphs, precise supercooled-liquid physics and complete
  giant-envelope coverage remain unresolved. An optional locally prepared
  [H/He density resource and coupled envelope solve](simulation/planet-composition.md#self-gravitating-envelope-and-loaded-interior)
   now supports a bounded subset of envelopes, without driving live rendering.
   Cold mixed envelopes include approximate [locally suspended condensates](simulation/planet-composition.md#suspended-secondary-condensates),
   with conserved vapour/condensate splits but no settling or latent-heat closure.
  Thin atmosphere coupling matches supported dilute gas inventories to radial
  condensed boundaries; it does not itself cover dense envelopes or unsupported
  condensed molecules.
   A bounded hot ideal-steam approximation extends the column to 2,000 K and
   50 kg/m³ for ≥90%-water gas; denser columns remain unsupported. An opt-in
   [blow-off wrapper](simulation/planet-composition.md#unbound-column-blow-off)
   strips the lightest gases from weakly bound columns, with no escape rate.
  Nothing stops the camera at the ground, and close up the near plane cuts it
  away.

## Organisation

Folders follow logical code ownership, not individual source files. Cross-cutting
setup stays in `development.md`; plans/history stay separate from current behaviour.
Each topic has one owning explanation, linked from related guides. Paths beginning
with `crates/` or `scripts/` are repository-relative; shorter source paths
are relative to the crate named by their guide. Source code remains authoritative
for implementation details.
