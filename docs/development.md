# Development

Run commands from the repository root. Native Linux and browser/WASM are both
supported targets and share the same Rust simulation and egui interface.
Browser rendering uses WebGL 2; the native client
also provides graphical smoke tests and screenshot-based bug reports.

## Prerequisites

- Rust via rustup; `rust-toolchain.toml` pins the toolchain and components.
- Linux native builds: Clang, LLD, pkg-config and Wayland/X11 development libraries,
  plus a Vulkan-capable driver and desktop session to launch the client.
- Scripts: Bash and curl. Web builds additionally need Python 3 available as
  `python`, tar and gzip. `build-web.sh` currently downloads an x86-64 Linux
  wasm-bindgen binary matched to `Cargo.lock` and installs the WASM target.
- Browser tests: Node.js, npm, Chromium and
  `npm install --prefix tools/browser playwright-core`.

See [the README](../README.md) for native and browser launch commands.
`./scripts/run.sh` builds and starts both processes, cleaning up the backend
on client exit. It enables the [native developer API](client/developer-api.md)
by default; `NIGHT_SKY_API=0 ./scripts/run.sh` disables it for that run.
`./scripts/build-web.sh` builds WebGL 2 WASM into `web/dist/`.
`./scripts/dev-web.sh` builds the browser client and starts the server with the
HTML and worker entry points linked to their sources. Refresh for changes to
those files; rerun the script after Rust changes.
Both workers load the `sim-worker` WASM in separate instances.

## Project structure

```text
crates/client/       Bevy client and egui interface
  src/main.rs        Entry point and system scheduling
  src/navigation.rs  Destination, visits and selection
  src/format.rs      Shared text formatting
  src/search/        Object search requests, settings, the shared store and the
                     browser's search worker transport
  src/scene/         Camera, controls, location, placement and wireframes
  src/stars/         Galactic star streaming, batching, drawing and shaders
    stream/          Native/web backends and streaming metrics
  src/render/        Bodies, exposure, glow, black hole and shaders
  src/ui/            HUD, breadcrumbs, inspector, tasks and the debug console's skin
  src/ui/kit/        Shared themes, controls, windows, scrolling, animation and halo text
  src/ui/browser/    Object browser: query chips, filter picker, results
  src/ui/tasks/      Task-panel regression tests
  src/devtools/      Debug window, tests, benchmarks and diagnostics
    tabs/            One module per debug tab
crates/server/       Axum API and static browser file server
crates/shared/       Serializable models
crates/simulation/   Orbital mechanics, clock, galaxy and deterministic generation
  src/objects.rs     ObjectRef: stable identities for inspection and search
  src/search/        Object registry, query model, search engine, runner and native thread
crates/sim-worker/   Generation-only WASM for the browser's star and search workers
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
web/                Browser page, worker entry points and generated dist/
tools/              Downloaded build/test tools (ignored)
vendor/             Documented Bevy renderer patch
```

Large test blocks live in sibling `tests.rs` files. Shaders are beside their Rust
modules; internal asset paths are relative to the loading `.rs` file. Keep both
native and WASM configurations working when moving code.

The [UI kit guide](client/ui/kit.md) describes module ownership, shared interaction
rules and how to add controls without duplicating theme or animation logic.
The [object browser notes](client/ui/browser.md) cover query controls, filters,
range editors and responsive results. The debug console uses a thin
`ui/terminal.rs` adapter over the same kit, not a second widget implementation.

Runtime files and build output stay in the project. Rust toolchains and Cargo's
dependency cache use their normal user-level locations. Set
`CARGO_HOME="$PWD/.cargo-home"` to isolate future dependency downloads if needed.

Keep native threading/reqwest and browser fetch/timers behind target-specific
code. Bevy, big_space and bevy_egui are pinned as a compatible set; check
compatibility before upgrading any one. No global desktop or Cargo
configuration changes are needed.

## Backend

See [the galaxy backend](server/backend.md) for serving, configuration, polling
and network exposure. It serves demo data and the web build, not the native
client's developer API.

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

It checks formatting, workspace unit tests and strict Clippy, natively and
for the browser's WebAssembly build.
Tests cover orbital and clock invariants, deterministic generation, multiple
systems, streaming, persistent task slots, presentation layout and camera flights.
UI regressions cover long presses, window movement/resizing, title alignment,
selection-driven rows, responsive result panes and reset placement, numeric
precision/validation, dropdown geometry, themed tooltips and scrollbar easing.

Additional graphical and browser checks:

```sh
./scripts/run.sh --smoke-test
./scripts/build-web.sh
node scripts/test-browser.mjs
SPACE_URL=http://127.0.0.1:4321 node scripts/test-breadcrumbs.mjs
node scripts/test-star-worker.mjs
node scripts/test-exposure.mjs
```

Browser checks need a running server serving the current build. Set
`SPACE_CHROMIUM` to your Chromium executable and `SPACE_URL` to override a test's
server URL. The general browser test defaults to localhost port 4321; some
specialised checks default to port 4322, so set `SPACE_URL` explicitly when needed.

The native smoke test requires a desktop session and saves eighteen screenshots
under `data/`: system views, tracking, galaxy travel, pagination, constellation
framing, camera flights and realistic lighting at both compression extremes.
These include a 50-task pagination fixture, mid-flight list/page jumps, and
seven realistic captures: at 0 stops, system at 100%/0% compression, lit planet,
constellation and star at both compression settings; then corona structure
at -16 stops and 0% compression.
Set `SPACE_SMOKE_TASKS=1400` for a larger constellation fixture. The browser smoke
test covers startup, navigation, resize, static-file isolation and a 50-task
fixture through `window.space`. The breadcrumb test covers remote selection,
inspection without travel and narrow viewports, saving captures under `data/logs/`.

The user runs performance benchmarks in their own focused window. Agents read
results rather than launching benchmarks. Routes, baselines and output files are
documented in [Performance benchmarks](client/benchmarks.md).
