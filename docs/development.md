# Development

Run commands from the repository root. Native Linux on Vulkan is the only
client target. The client provides graphical smoke tests and
screenshot-based bug reports.

## Prerequisites

- Rust via rustup; `rust-toolchain.toml` pins the toolchain and components.
- Linux native builds: Clang, LLD, pkg-config and Wayland/X11 development libraries,
  plus a Vulkan-capable driver and desktop session to launch the client.
- Scripts: Bash, curl and Python 3.

See [the README](../README.md) for launch commands.
`./scripts/run.sh` builds and starts both processes, cleaning up the backend
on client exit. It enables the [developer API](client/developer-api.md)
by default; `NIGHT_SKY_API=0 ./scripts/run.sh` disables it for that run.

## Project structure

```text
crates/client/       Bevy client and egui interface
  src/main.rs        Entry point and system scheduling
  src/navigation/    Destination, visits, picking, camera flights, presentation and identities
  src/input/         Named bindings, input capture and ship piloting
  src/format.rs      Shared text formatting
  src/search/        Object search requests, settings and the shared store
  src/scene/         Camera, controls, location, placement and wireframes
  src/stars/         Galactic star streaming, batching, drawing and shaders
    stream/          Loader thread backend and streaming metrics
  src/render/        Bodies, exposure, glow, black hole and shaders
    atmosphere/      Air settings, scattering tables, reference tests and shell shader
  src/ui/            HUD, breadcrumbs, inspector, tasks and the debug console's skin
  src/ui/kit/        Shared themes, controls, windows, scrolling, animation and halo text
  src/ui/browser/    Object browser: query chips, filter picker, results
  src/ui/tasks/      Task-panel regression tests
  src/devtools/      Debug window, typed commands, tests, benchmarks and diagnostics
    tabs/            One module per debug tab
crates/server/       Axum demo galaxy API
crates/shared/       Serializable models
crates/simulation/   Orbital mechanics, clock, galaxy and deterministic generation
  src/lib.rs         Module map and explicit exports of common operations and types
  src/galaxy/        Density model, populations, cells, streaming, neighbours and glow
  src/star/          Stellar evolution and companion hierarchies
  src/system/        Planet generation, wire systems, shape and surface
  src/orbit/         Analytic orbits, states and nested barycentric motion
  src/clock.rs       Accumulated simulation time
  src/units.rs       Shared physical constants
  src/seed.rs        Stable hashing, random draws and seeded placement
  src/light.rs       Star and planet photometry
  src/objects.rs     ObjectRef: stable identities for inspection and search
  src/search/        Object registry, query model, search engine, runner and thread
docs/               Explanations grouped by simulation, client, shared and server
  simulation/        Galaxy, stars, planetary systems, orbits and search
  client/            Navigation, streaming, rendering and developer tooling
    ui/              Scene interface, object browser and shared UI kit
  shared/            Lists/tasks and their relationship to generated bodies
  server/            Demo backend and serving behaviour
  plans/             Implementation history and future proposals
scripts/            Launch, build and verification commands
assets/             Reserved project assets
data/               Local logs, fixtures and screenshots (ignored)
target/             Compiled output (ignored)
tools/              The TUI's Python environment and UI prototypes (ignored)
vendor/             Documented Bevy renderer patch
```

Large test blocks live in sibling `tests.rs` files. Shaders are beside their Rust
modules; internal asset paths are relative to the loading `.rs` file.

Simulation modules are grouped by domain, not by technical layer. `galaxy/`
owns spatial populations and their loading, `star/` owns stellar properties and
companions, and `system/` owns planets and natural-body geometry (including
stellar spheroids). Shared deterministic primitives live in `seed.rs`, rather
than making every generator depend on planetary generation for its RNG.
Photometry stays separate because it covers both emitted and reflected light.
The crate root is a facade with explicit exports for common operations and
types, not an implementation file; specialist APIs use their domain paths.

Client `navigation/` owns destinations and selection plus star picking, camera
flight paths, presentation layouts and inspection identity helpers. `scene/`
owns the displayed camera, physical location and drawing. Named key bindings
and piloting live in `input/`; `scene/controls.rs` applies those named actions
to the free camera. Typed command parsing lives in `devtools/command_line.rs`
and still sends the same `UiCommand` messages as panels.

The [UI kit guide](client/ui/kit.md) describes module ownership, shared interaction
rules and how to add controls without duplicating theme or animation logic.
The [object browser notes](client/ui/browser.md) cover query controls, filters,
range editors and responsive results. The debug console uses a thin
`ui/terminal.rs` adapter over the same kit, not a second widget implementation.

Runtime files and build output stay in the project. Rust toolchains and Cargo's
dependency cache use their normal user-level locations. Set
`CARGO_HOME="$PWD/.cargo-home"` to isolate future dependency downloads if needed.

Bevy, big_space and bevy_egui are pinned as a compatible set; check
compatibility before upgrading any one. No global desktop or Cargo
configuration changes are needed.

## Backend

See [the galaxy backend](server/backend.md) for serving, configuration, polling
and network exposure. It serves demo data, not the client's developer API.

## Logs and bug reports

- Backend output: `data/server.log`.
- Client events: `data/logs/client.log`; previous session: `client.previous.log`.
- Panics: `data/logs/crash-*.log`, with backtraces.
- Debug window, `tools` tab, **save bug report**: matching `bug-*.log` and `.png`
  files with camera state, recent events and a screenshot. Share both when reporting
  visual bugs, after checking them for anything you do not want to publish.
- Smoke test log: `data/logs/smoke-client.log`.

Recoverable surface timeouts skip a frame and log a rate-limited warning. The
local Bevy renderer patch is documented in [vendor/README.md](../vendor/README.md).
Keep it in step with Bevy upgrades.

## Checks

```sh
./scripts/check.sh
```

It runs the `ns` CLI tests, then checks formatting, workspace unit tests and
strict Clippy.
Tests cover orbital and clock invariants, deterministic generation, multiple
systems, streaming, persistent task slots, presentation layout and camera flights.
UI regressions cover long presses, window movement/resizing, title alignment,
selection-driven rows, responsive result panes and reset placement, numeric
precision/validation, dropdown geometry, themed tooltips and scrollbar easing.

Additional graphical check:

```sh
./scripts/run.sh --smoke-test
```

The smoke test requires a desktop session and saves eighteen screenshots
under `data/`: system views, tracking, galaxy travel, pagination, constellation
framing, camera flights and realistic lighting at both compression extremes.
These include a 50-task pagination fixture, mid-flight list/page jumps, and
seven realistic captures: at 0 stops, system at 100%/0% compression, lit planet,
constellation and star at both compression settings; then corona structure
at -16 stops and 0% compression.
Set `SPACE_SMOKE_TASKS=1400` for a larger constellation fixture.

The user runs performance benchmarks in their own focused window. Agents read
results rather than launching benchmarks. Routes, baselines and output files are
documented in [Performance benchmarks](client/benchmarks.md).
