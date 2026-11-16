# Night Sky

A procedural space simulator at real distances and physical planet radii.
Native Linux is the only client target: a Bevy/egui client on Vulkan.
The browser/WASM build was dropped (tag `web-final`); see [the plan](docs/plans/native-only.md).
Lists and tasks are an optional information overlay, currently demo-only.
External integrations are optional extensions, not part of the simulator core.
Repository: `ChickenSplash/night-sky`.

## Code and documentation map

Read the relevant guides before changing an area, not every guide each session.
[docs/README.md](docs/README.md) is the full index; source code is authoritative
for implementation details.

| Working area | Code ownership | Read |
| --- | --- | --- |
| Galaxy populations and procedural cells | `crates/simulation/src/galaxy/` | [Galaxy](docs/simulation/galaxy.md) |
| Star types, evolution and companions | `crates/simulation/src/star/` | [Stars](docs/simulation/stars.md) |
| Planet generation, shape and ground | `crates/simulation/src/system/` | [Planetary systems](docs/simulation/planetary-systems.md) |
| Material budgets, material physics, structure and physical transport | Simulation `system/composition.rs`, `system/materials.rs`, `system/structure.rs`, `system/physical.rs` | [Composition foundation](docs/simulation/planet-composition.md) |
| Pure-water EOS, phase domains and scientific reference checks | Simulation `system/materials/water.rs` | [Water physics](docs/simulation/water-physics.md) |
| Optional local H/He density resource and self-gravitating envelope/interior solve | Simulation `system/materials/hhe.rs`, `system/structure/envelope.rs`, `scripts/prepare-hhe-density.py` | [Envelope model and bounds](docs/simulation/planet-composition.md#self-gravitating-envelope-and-loaded-interior) |
| Bounded H/He-rich molecular mixtures and component density | Simulation `system/materials/{hhe_mixture,molecular}.rs` | [Molecular mixture bounds](docs/simulation/planet-composition.md#hhe-rich-molecular-mixtures) |
| Enriched envelopes and explicit molecular/hot-deep component handoff | Simulation `system/materials/effective_secondary.rs`, `system/structure/envelope/mixture.rs` | [Enriched/deep approximation](docs/simulation/planet-composition.md#enriched-and-deep-secondary-envelopes) |
| Local vapour/condensate partition and suspended-cloud density | Simulation `system/materials/suspended.rs`, `system/structure/envelope.rs` | [Suspended secondary condensates](docs/simulation/planet-composition.md#suspended-secondary-condensates) |
| Explicit approximation policy, fidelity transport and shallow envelope-loaded water | Shared `composition.rs`, simulation `system/materials/water/loaded.rs`, `system/physical/layered.rs` | [Fidelity policy](docs/simulation/planet-composition.md#approximation-policy-and-fidelity-labels), [shallow water](docs/simulation/planet-composition.md#shallow-loaded-water-approximation) |
| Headless dry/water radial solvers, protected traces, effective deep ice and shared numerical kernel | Simulation `system/structure/{dry,water,radial}.rs`, `{dry,water}/protected.rs` | [Composition foundation](docs/simulation/planet-composition.md#headless-dry-body-solver) |
| Physical atmosphere model | Simulation `system/atmosphere.rs` | [Atmospheres](docs/simulation/atmospheres.md) |
| Headless atmosphere/interior boundary coupling | Simulation `system/structure/coupled.rs` | [Coupling model and limits](docs/simulation/planet-composition.md#headless-atmosphere-and-condensed-boundary-coupling) |
| Orbital motion, units and clock | Simulation `orbit/`, `units.rs`, `clock.rs` | [Orbits](docs/simulation/orbits.md) |
| Craft model, gravity, integration and live craft | `crates/simulation/src/craft.rs`, client `craft.rs` | [Craft](docs/simulation/craft.md) |
| Object registry, queries and search | `crates/simulation/src/search`, client `search/` | [Search](docs/simulation/search.md) |
| Lists, pages, tasks and wire models | `crates/shared`, simulation `system/overlay.rs`, `system/demo.rs` and client | [Information overlay](docs/shared/information-overlay.md) |
| Visits, selection, camera and presentation | Client `navigation/`, `scene/` | [Navigation](docs/client/navigation.md) |
| Visible-star loading and spatial queries | Client `stars/stream/`, simulation `galaxy/` | [Streaming](docs/client/streaming.md) |
| Total labelled fallback structure and the embedded table of layered radii it reads | Simulation `system/structure/{prototype,fallback,table}.rs`, `structure/table/build.rs`, `resources/structure-table.bin`, `scripts/prepare-structure-table.sh`, `system/physical/layered.rs` | [Fallback structure](docs/simulation/planet-composition.md#fallback-structure), [structure table](docs/simulation/planet-composition.md#structure-table) |
| Background planet refinement, safe adoption and shared star/search/planet CPU work budget | Client `planet_loading.rs`, `planet_loading/adoption.rs`, `craft/travel.rs`, simulation `system/physical/refinement.rs`, `background.rs`, `system/generated.rs`, `search/worker.rs` | [Safe adoption](docs/simulation/planet-composition.md#safe-background-adoption), [background loading](docs/simulation/planet-composition.md#experimental-background-loading), [streaming](docs/client/streaming.md#shared-background-cpu-budget) |
| Bodies, lighting, star points and galaxy glow | Client `render/`, `stars/` | [Rendering](docs/client/rendering.md), [galactic rendering](docs/client/galactic-rendering.md) |
| Key bindings and piloting | Client `input/` | [Controls](docs/client/controls.md) |
| HUD, breadcrumbs, inspector and panels | Client `ui/` | [Interface](docs/client/ui/interface.md), [controls](docs/client/controls.md), [browser](docs/client/ui/browser.md) |
| Reusable widgets, themes and interaction | Client `ui/kit/`, `ui/terminal.rs` | [UI kit](docs/client/ui/kit.md) |
| Debug console, snapshots, API and CLI | Client `devtools/`, `scripts/ns`, `scripts/ns-tui` | [Diagnostics](docs/client/devtools.md), [developer API](docs/client/developer-api.md), [benchmarks](docs/client/benchmarks.md) |
| Demo backend and web serving | `crates/server` | [Backend](docs/server/backend.md) |
| Builds, tests, logs and file moves | Workspace and `scripts/` | [Development](docs/development.md) |

Client paths without a repository prefix are relative to `crates/client/src/`.
Plans in `docs/plans/` are history/future proposals, not current-behaviour guides.
Current limitations are listed in [the documentation index](docs/README.md#current-limitations).

## Working rules

- Run `./scripts/check.sh` after code changes: formatting, tests and strict
  Clippy. Build/run details live in the development guide.
- The user runs GPU, core-load and star-flight benchmarks in their own focused
  window. Read `data/logs/` results; do not launch benchmark runs yourself.
- Keep runtime files in project-local `data/`, compiled output in `target/`.
  Do not change global desktop or Cargo configuration.
- Do not reintroduce browser/WASM support or WebGL fallbacks. Check
  Bevy/big_space/bevy_egui compatibility before upgrades, including the patch
  documented in `vendor/README.md`.
- Credentials belong only in the backend, never API models, logs or the client.
  Keep the developer API loopback-only.
- Preserve physical scale, f64 simulation/camera precision, deterministic
  generation and stable body identities. Read the relevant guides
  for these cross-system constraints before changing them.
- The project is in alpha: generation changes may freely regenerate existing
  stars and bodies, so favour better generation over preserving current output.

## Documentation standard

- Keep this file a high-level overview, routing map and essential working rules,
  not a technical manual. Put subsystem explanations in their owning guides.
- Mirror logical code ownership under `docs/simulation/`, `docs/client/`
  (including `ui/`), `docs/shared/` and `docs/server/`. Do not mirror every
  source file or create empty guides. Keep cross-cutting setup in `development.md`
  and proposals/history in `plans/`.
- Code should be self-documenting wherever possible. Docs explain what code
  alone cannot readily communicate: how systems work together, the physical
  models and maths, behaviour, assumptions, limitations and important
  cross-system constraints with their reasons. Do not duplicate function
  inventories, local implementation details or low-level decision histories.
- Prefer one canonical explanation per topic and links from other guides.
  Keep the documentation index, this routing map, links and project structure
  in step with code and document moves.
- When work reveals missing, misleading or outdated explanations, update the
  relevant canonical guide as part of the task. Keep updates scoped to the
  work, verify against code, and distinguish implemented behaviour from plans.
