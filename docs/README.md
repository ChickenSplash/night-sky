# Documentation

Night Sky is a space simulator first. Its galaxy is synthetic and procedural,
grounded in observations and scientific research. Fictional lists and tasks
are an optional information overlay. Future external integrations belong in
optional plugins, not the simulator core; see [the overlay guide](shared/information-overlay.md).

Native Linux and browser/WASM clients are both supported, with neither treated
as the primary target. They share the simulation and interface; some capabilities
are platform-specific.

Start with [development](development.md) for setup, project structure, logs and
checks, or [controls](client/controls.md) to use the app.
[AGENTS.md](../AGENTS.md) maps code areas to these guides and defines working
and documentation conventions.

## Simulation

Engine-independent models in `crates/simulation`:

- [Galaxy](simulation/galaxy.md): component shapes and ages, regional density,
  exact populations, cells, brightness tiers and deterministic identity.
- [Stars and multiple systems](simulation/stars.md): spectral labels, mass/age
  generation, evolved stars, companions, barycentres and stability zones.
- [Planetary systems](simulation/planetary-systems.md): generation sequence,
  snow line, mass–radius fits, Hill spacing, resonances, spins and stable body IDs.
- [Orbits and time](simulation/orbits.md): orbital elements, Kepler's equations,
  worked examples, nested barycentres, units and clock behaviour.
- [Object search](simulation/search.md): registry, query syntax, relationships,
  nearest-first search, completeness and native/browser execution.
- [Craft](simulation/craft.md): the man-made object model, gravity from every
  body, and fixed-step integration.

## Client

The shared native/browser client in `crates/client`, with browser workers in
`crates/sim-worker` and `web/`:

- [Controls](client/controls.md): shortcuts, travel, inspection and presentation modes.
- [Navigation](client/navigation.md): visits, physical arrival, selection,
  picking, camera frames and precision.
- [Streaming](client/streaming.md): compact generation, loading, magnitude
  changes, native/web workers and spatial queries.
- [Realistic rendering](client/rendering.md): physical light, reflectance and phase,
  exposure, compression, HDR, sprites, scale and cameras.
- [Galactic rendering](client/galactic-rendering.md): GPU point batches,
  precise sprites, multiple-system resolution and diffuse galaxy glow.
- [Debug tools](client/devtools.md): console, command path, performance
  readouts, snapshots and browser automation.
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

## Current limitations

- No N-body interactions, moons or collisions.
- Dust and lensing are not implemented. Star systems have galactic velocities,
  but stars do not move around the galaxy. Diffuse galaxy glow
  is an approximate smooth prototype, not a detailed dust-attenuated renderer.
- Camera flights are navigation aids, not physical spacecraft or warp simulations.
  Craft are simulated and can be followed closely, but are spawned and
  thrusted only from the CLI so far: no in-app controls, trajectory
  prediction, warp, saving or collisions yet.
- Bodies use gizmos or pooled per-body meshes; further body batching remains future work.

## Organisation

Folders follow logical code ownership, not individual source files. Cross-cutting
setup stays in `development.md`; plans/history stay separate from current behaviour.
Each topic has one owning explanation, linked from related guides. Paths beginning
with `crates/`, `scripts/` or `web/` are repository-relative; shorter source paths
are relative to the crate named by their guide. Source code remains authoritative
for implementation details.
