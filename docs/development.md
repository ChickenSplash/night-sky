# Development

Run commands from the repository root. Both clients share the same Rust
simulation and egui interface. Browser rendering uses WebGL 2; the native client
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
`./scripts/dev-web.sh` builds the browser client and starts the server with the
HTML and worker entry point linked to their sources. Refresh for changes to
those files; rerun the script after Rust changes.

## Project structure

```text
crates/client/       Bevy client and egui interface
  src/main.rs        Entry point and system scheduling
  src/navigation.rs  Destination, visits and selection
  src/format.rs      Shared text formatting
  src/search.rs      Object search requests, settings and the API's shared store
  src/scene/         Camera, controls, location, placement and wireframes
  src/stars/         Galactic star streaming, batching, drawing and shaders
    stream/          Native/web backends and streaming metrics
  src/render/        Bodies, exposure, glow, black hole and shaders
  src/ui/            HUD, breadcrumbs, inspector, tasks and the debug console's skin
  src/ui/kit/        Themed widget kit (terminal and game themes), halo text
  src/ui/browser/    Object browser: query chips, filter picker, results
  src/devtools/      Debug window, tests, benchmarks and diagnostics
    tabs/            One module per debug tab
crates/server/       Axum API and static browser file server
crates/shared/       Serializable models
crates/simulation/   Orbital mechanics, clock, galaxy and deterministic generation
  src/objects.rs     ObjectRef: stable identities for inspection and search
  src/search/        Object registry, query model, search engine and native worker
crates/star-worker/  Generation-only WASM worker for browser streaming
docs/               Development and design notes
scripts/            Launch, build and verification commands
assets/             Reserved project assets
data/               Local logs, fixtures and screenshots (ignored)
target/             Compiled output (ignored)
web/                Browser page, worker entry point and generated dist/
tools/              Downloaded build/test tools (ignored)
vendor/             Documented Bevy renderer patch
```

Large test blocks live in sibling `tests.rs` files. Shaders are beside their Rust
modules; internal asset paths are relative to the loading `.rs` file. Keep both
native and WASM configurations working when moving code.

Runtime files and build output stay in the project. Rust toolchains and Cargo's
dependency cache use their normal user-level locations. Set
`CARGO_HOME="$PWD/.cargo-home"` to isolate future dependency downloads if needed.

## Backend and logs

The server serves only `web/dist/` and the built-in demo galaxy. It binds to
`127.0.0.1:4321` by default. `SPACE_BIND` overrides the listening address;
`SPACE_ROOT` overrides the project root used to locate `web/dist/`. Binding to a
non-loopback address exposes the service to that network, so configure access
controls and HTTPS separately for a public deployment.

- `GET /health`: service health.
- `GET /galaxy`: demo snapshot, with ETags and HTTP 304 for unchanged data.
- The native client polls every two seconds. It can run alone with
  `cargo run --locked -p space-client`, retaining demo data when no backend is available.
- `scripts/run.sh` writes backend output to `data/server.log`.
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
cargo check -p space-client --target wasm32-unknown-unknown
```

The first command checks formatting, workspace unit tests and strict Clippy.
Tests cover orbital and clock invariants, deterministic generation, multiple
systems, streaming, persistent task slots, presentation layout and camera flights.

Additional graphical and browser checks:

```sh
./scripts/run.sh --smoke-test
./scripts/build-web.sh
cargo clippy --locked -p space-client --target wasm32-unknown-unknown -- -D warnings
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
Set `SPACE_SMOKE_TASKS=1400` for a larger constellation fixture. The browser smoke
test covers startup, navigation, resize, static-file isolation and a 50-task
fixture through `window.space`. The breadcrumb test covers remote selection,
inspection without travel and narrow viewports, saving captures under `data/logs/`.

The user runs performance benchmarks in their own focused window. Agents read
results rather than launching benchmarks. Routes, baselines and output files are
documented in [Galaxy model: Benchmarks](galaxy-model.md#benchmarks).
