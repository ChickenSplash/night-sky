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
- [Planetary atmospheres](plans/atmospheres.md): the rendering prototype first,
  then cleaner, cheaper steps and many planets, then the simulated
  atmosphere and climate.
- [Planet surfaces](plans/planet-surfaces.md): bodies flattened by their spin,
  landing on the ground, then geology, water and giants' depths.

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
- Atmospheres are a rendering prototype: hand-set air (presets and sliders)
  on the selected planet only, with lookup tables and multiple scattering but
  no light from the sky on the ground. No planet has a simulated atmosphere yet.
- Bodies are spheres with no terrain, drawn as a faceted mesh, and the camera
  can pass through the ground.

## Organisation

Folders follow logical code ownership, not individual source files. Cross-cutting
setup stays in `development.md`; plans/history stay separate from current behaviour.
Each topic has one owning explanation, linked from related guides. Paths beginning
with `crates/` or `scripts/` are repository-relative; shorter source paths
are relative to the crate named by their guide. Source code remains authoritative
for implementation details.
