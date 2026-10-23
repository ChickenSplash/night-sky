# Native only

Agreed with the user on 2026-10-23, before flight-sim phase 11 continues.
The browser/WASM build is dropped. Native Linux becomes the only client
target, so shaders, ship models, shadows and planet water never need a WebGL
version or a browser fallback.

## Where the work stands

- **Done (`native-only` branch):** steps 0 to 5 below. `warp-drive` was
  merged into `main` first, and `web-final` tags that merge (`956563f`),
  pushed to `origin`. `./scripts/check.sh` passes; local leftovers
  (`tools/wasm-bindgen*`, `tools/browser/`, `deploy/`, `web/dist/`) are deleted.
- **Pending:** the user's live check, then the merge to `main`. The homelab
  web demo is to be dealt with separately afterwards.

## Why

- Rendering parity with WebGL 2 blocks the planned work: no compute shaders,
  storage buffers or indirect draws, and every new shader would need checking
  on both renderers.
- A "lite" browser tier was considered and rejected. It frees the shaders, but
  not the rest of the cost: every feature must still compile for WASM, the
  browser's single thread needs Web Worker transports (stars, search, craft
  loading), shared drawing code stays limited to WebGL, and `check.sh`,
  upgrades and docs keep a second target.
- The last browser-capable commit is tagged `web-final`, so it can be recovered.

## Goal

- One client target: no `target_arch = "wasm32"` code, WASM dependencies,
  browser workers, browser scripts or browser docs remain.
- Native behaviour is unchanged. Every collapsed branch keeps the native side.
- `./scripts/check.sh` passes and is native only.

## Scope

**Removed:**

- `crates/sim-worker` and its workspace entry.
- `web/` (`index.html`, `star-worker.js`, `search-worker.js`,
  `craft-worker.js`) and the `/web/dist/` ignore rule.
- Scripts: `build-web.sh`, `dev-web.sh`, `test-browser.mjs`,
  `test-breadcrumbs.mjs`, `test-star-worker.mjs`, `test-craft-worker.mjs`,
  `test-exposure.mjs`, `test-galaxy-glow.mjs`, `benchmark-star-flight.mjs`.
- Client browser modules: `stars/stream/web.rs`, `search/web.rs`,
  `devtools/benchmark.rs`, `devtools/automation.rs`, the `browser` module in
  `craft/loading.rs`, the wasm `start`/`now_ms` in `network.rs`.
- Cargo: `[profile.web-release]`, the wasm32 target dependency tables in
  `crates/client` and `crates/simulation` (`gloo-*`, `wasm-bindgen*`,
  `js-sys`, `web-sys`, `getrandom` `wasm_js`, `wasm-bindgen-test`).
- Server: the `ServeDir` fallback for `web/dist/` and `SPACE_ROOT`.

**Kept:**

- `crates/server` with `/health` and `/galaxy`: the native client polls it
  (`network.rs`) and `scripts/run.sh` starts it.
- The **object browser** (`ui/browser/`, `docs/client/ui/browser.md`). It is
  the in-game search window, not the web build. Do not confuse the two when
  removing "browser" code or text.
- Time-sliced and pluggable designs in `crates/simulation` (search `Runner`
  steps, voyage `loading::Backend`, star `Loader::step`). Native uses them
  too. Remove only what the compiler or Clippy then reports as unused, such as
  `stream_wire` if only `sim-worker` and `stream/web.rs` used it.
- Docs in `docs/plans/`: they are history and stay as written.

**Out of scope (follow-up):**

- Retiring the triangle starfield path (`starfield.wgsl`) so stars are drawn
  only as points. With the browser gone it is just a native debug fallback,
  but the point path shares its buffers and the precise-sprite path needs
  checking first. Do this as its own change with a live visual check.

## Steps

### 0. Safety

- Work on a `native-only` branch or worktree from `master`.
- Tag the current `master` head `web-final` (local tag; push only with the
  user's approval).

### 1. Delete browser-only code and files

- Everything in **Removed** above.
- Drop `crates/sim-worker` from the workspace members, then let a plain
  `cargo build` prune `Cargo.lock` (expect it to shrink; no version bumps).

### 2. Collapse target branches to native

Keep the native side of every branch and drop the `cfg`. Files:

- `main.rs`: crash reports, benchmark mode, window/canvas options, the
  `core_load`/`gpu_test`/smoke/automation/HTTP registrations.
- `network.rs`, `stars/mod.rs`, `stars/starfield.rs` (`StarPointsPlugin`,
  `frame.points`), `stars/stream/mod.rs`, `search/mod.rs`, `craft.rs`,
  `craft/loading.rs`, `craft/forecast.rs` (`STEPS_PER_FRAME` 250),
  `scene/mod.rs` (depth `TEXTURE_BINDING` always; keep `NoIndirectDrawing`,
  which GPU culling at the 1e30 far plane needs natively), `scene/location.rs`,
  `render/debug.rs` (`point_stars` default true, diagnostics always,
  tooltip without "native only").
- devtools: `mod.rs`, `snapshot.rs`, `capture.rs`, `gpu_test.rs`,
  `core_load.rs`, `tabs/tools.rs`, `tabs/perf.rs` (threads row and buttons,
  GPU pass timings), `command_line.rs`.
- Remove every `cfg_attr(target_arch = "wasm32", allow(dead_code))` and
  `cfg_attr(not(...), allow(dead_code))`; fix any real dead code they hid.
- `crates/simulation`: `craft/voyage.rs` `threads()`,
  `craft/voyage/loading.rs`, `search/mod.rs`.
- The 90 `#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test...)]` /
  `#[cfg_attr(not(...), test)]` pairs in 10 simulation test files become
  plain `#[test]`.
- Comments and user-facing strings that mention the browser, WebGL or WASM
  (for example the WebGL note in `scene/mod.rs`).

### 3. Scripts and server

- `scripts/check.sh`: drop the WASM craft tests and WASM Clippy block.
- `crates/server/src/main.rs`: drop `ServeDir` and `SPACE_ROOT`; drop
  `tower-http` features or the dependency if nothing else needs them. The
  startup message no longer suggests opening a browser.

### 4. Docs

Rewrite current-behaviour docs as native only (plans untouched):

- `AGENTS.md`: intro (lines 4-5), streaming routing row (no `sim-worker`,
  `web/`), check rule (no WebAssembly), "Keep both client targets working"
  becomes Bevy upgrade compatibility only, credentials rule (no WASM or
  same-origin browser wording).
- `README.md`: intro, remove "Run in a browser".
- `docs/README.md`: intro, client section, devtools line, limitations
  ("browser flight visuals" sentence), path note about `web/`, plus an index
  entry for this plan.
- `docs/development.md`: prerequisites, build section, project tree, target
  rules, browser checks.
- `docs/client/streaming.md` (transports), `docs/simulation/search.md`,
  `docs/client/devtools.md` (tools row, browser automation),
  `docs/client/benchmarks.md` (browser star flight),
  `docs/client/rendering.md` (lines 207, 229),
  `docs/client/galactic-rendering.md` (lines 39, 44, 150),
  `docs/simulation/craft.md` (lines 193-197, 275, 587, 687, 715),
  `docs/server/backend.md`, `docs/client/ui/kit.md` (lines 123-125),
  `docs/client/controls.md` (Ctrl+K web-browser note).

### 5. Verify

- `./scripts/check.sh` passes.
- `rg -n 'wasm|WebAssembly|WebGL|web_sys|js_sys|gloo|sim-worker|build-web|web/dist'`
  over `crates/`, `scripts/`, `Cargo.toml`, `AGENTS.md`, `README.md` and
  `docs/` (excluding `docs/plans/`) finds nothing unintended.
- The user runs `./scripts/run.sh` for a live check: stars stream, search
  works, a craft flies and warps, realistic lighting and galaxy glow look as
  before, the perf tab shows thread counts and GPU pass timings.
- Commit on the branch, then merge to `main` once the user approves.

## Decisions

- `web-final` is pushed to `origin`, so the browser build can be found and
  checked out from anywhere.
- The ignored local leftovers are deleted; `build-web.sh` at `web-final`
  downloads its tools again if the browser build is ever revived.
- The homelab web demo (the ignored `deploy/` bundle and
  `scripts/deploy-homelab.sh` rule) is handled after the merge, separately.
