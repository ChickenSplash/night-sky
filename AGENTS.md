# Night Sky

A procedural space simulator at real distances and physical planet radii.
Native Linux and browser/WASM are both supported targets, neither is primary.
They share the Bevy/egui client, with some platform-specific capabilities.
Lists and tasks are an optional information overlay, currently demo-only.
External integrations are optional extensions, not part of the simulator core.
Repository: `ChickenSplash/night-sky`.

## Code and documentation map

Read the relevant guides before changing an area, not every guide each session.
[docs/README.md](docs/README.md) is the full index; source code is authoritative
for implementation details.

| Working area | Code ownership | Read |
| --- | --- | --- |
| Galaxy populations and procedural cells | `crates/simulation` | [Galaxy](docs/simulation/galaxy.md) |
| Star types, evolution and companions | `crates/simulation` | [Stars](docs/simulation/stars.md) |
| Planet generation and motion | `crates/simulation` | [Planetary systems](docs/simulation/planetary-systems.md), [orbits](docs/simulation/orbits.md) |
| Craft model, gravity, integration and live craft | `crates/simulation/src/craft.rs`, client `craft.rs` | [Craft](docs/simulation/craft.md) |
| Object registry, queries and search | `crates/simulation/src/search`, client `search/` | [Search](docs/simulation/search.md) |
| Lists, pages, tasks and wire models | `crates/shared`, simulation and client | [Information overlay](docs/shared/information-overlay.md) |
| Visits, selection, camera and presentation | Client `navigation.rs`, `scene/`, `galaxy_nav.rs`, `presentation.rs` | [Navigation](docs/client/navigation.md) |
| Visible-star loading and spatial queries | Client `stars/stream/`, `crates/sim-worker`, `web/` | [Streaming](docs/client/streaming.md) |
| Bodies, lighting, star points and galaxy glow | Client `render/`, `stars/` | [Rendering](docs/client/rendering.md), [galactic rendering](docs/client/galactic-rendering.md) |
| HUD, breadcrumbs, inspector and panels | Client `ui/` | [Interface](docs/client/ui/interface.md), [controls](docs/client/controls.md), [browser](docs/client/ui/browser.md) |
| Reusable widgets, themes and interaction | Client `ui/kit/`, `ui/terminal.rs` | [UI kit](docs/client/ui/kit.md) |
| Debug console, snapshots, API and CLI | Client `devtools/`, `command_line.rs`, `scripts/ns` | [Diagnostics](docs/client/devtools.md), [developer API](docs/client/developer-api.md), [benchmarks](docs/client/benchmarks.md) |
| Demo backend and web serving | `crates/server` | [Backend](docs/server/backend.md) |
| Builds, tests, logs and file moves | Workspace and `scripts/` | [Development](docs/development.md) |

Client paths without a repository prefix are relative to `crates/client/src/`.
Plans in `docs/plans/` are history/future proposals, not current-behaviour guides.
Current limitations are listed in [the documentation index](docs/README.md#current-limitations).

## Working rules

- Run `./scripts/check.sh` after code changes: formatting, tests and strict
  Clippy for native and WebAssembly. Build/run details live in the development guide.
- The user runs GPU, core-load and star-flight benchmarks in their own focused
  window. Read `data/logs/` results; do not launch benchmark runs yourself.
- Keep runtime files in project-local `data/`, compiled output in `target/`.
  Do not change global desktop or Cargo configuration.
- Keep both client targets working. Check Bevy/big_space/bevy_egui compatibility
  before upgrades, including the patch documented in `vendor/README.md`.
- Credentials belong only in the backend, never API models, logs or WASM.
  Browser galaxy requests stay same-origin. Keep the native developer API loopback-only.
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
