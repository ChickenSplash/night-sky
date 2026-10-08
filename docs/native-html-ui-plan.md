# Shared HTML UI: implementation and handoff

## Decision and architecture

On 2026-10-05 the user accepted the small-panel CEF overhead and explicitly
approved replacing the native UI with the full shared HTML UI as the default.
Keep the complete egui interface only as a separately selected launch mode.

- `web/ui/panels.js`, `panels.css`, `bridge.js` and `annotations.js` are the
  single frontend source of truth for native and browser clients.
- Native simulation, Bevy windowing and Vulkan scene rendering stay native.
- HTML mode does not initialise egui, including its old canvas overlays.
  Shared transparent canvas annotations replace labels, selection rings and
  velocity markers. Rust retains f64 projection and planet/star picking.
- Native `--ui=egui` and browser `?ui=egui` select the full fallback instead.
- `ui_bridge.rs` serialises galaxy/state/tick contracts for both transports.
  Browser receives window events; native CEF's adapter forwards `cef.listen`
  events to the same window events and sends the same ID-based `UiCommand`s.
- `build.rs` bundles shared source assets into one embedded native page.
  Native changes require rebuilding; browser development keeps live source assets.
- Native hit rectangles cover panels, dropdown menus and modal backdrops,
  including animated bounds. Drag ownership persists through release; Escape,
  focus loss and held-key handoffs remain explicit.
- Standard popup dropdowns are not used. The shared panels already have in-page
  dropdown menus. CSS backdrop blur cannot sample the native Bevy scene.
- Native Save bug report remains available in Settings. Explicit task-link
  clicks open HTTP(S) URLs through the desktop, not inside the unsandboxed CEF page.

## Launch and checks

```sh
./scripts/setup-native-ui.sh  # one-time project-local CEF setup
./scripts/run.sh             # shared HTML, native by default
./scripts/run.sh --ui=egui   # builds without CEF, full egui fallback
./scripts/build-web.sh       # no CEF dependencies in WASM
./scripts/check.sh
node scripts/test-native-ui.mjs
SPACE_URL=http://127.0.0.1:4321 node scripts/test-html-ui.mjs
```

The prototype page, prototype-only launcher and its DOM test are removed.
CEF runtime files/cache/tools stay under `data/`; compiled output stays under
`target/`. There are no global desktop/Cargo changes. Run visual checks only on
HDMI-A-1, without taking focus from the user's work.

For development-only native UI inspection, `SPACE_UI_DEBUG_PORT=<port>` opts
into loopback Chromium DevTools (port must be at least 1024). It is off by default.
This permits frontend/bridge tests, not proof that native OS input routing works.

## Accepted historical small-panel benchmark

- Branch: `prototype-cef-overlay`; implementation: `443dc06`; handoff: `9997380`.
- Full unattended suite completed 60/60 runs, three repetitions per group:
  `data/logs/cef-benchmark-20261005T081419-090162Z/summary.json`.
- Active uncapped median: stationary 2.27 -> 2.67 ms; flight 8.54 -> 9.37 ms.
- Active uncapped flight p95: 18.06 -> 19.26 ms; p99: 24.50 -> 25.74 ms.
- Added active PSS: approximately 393 to 485 MiB. Added aggregate CPU:
  approximately 13 to 27 percentage points, where 100% is one logical core.
- Normal presentation follows the approximately 75 Hz monitor cadence.
- The user accepts these overhead tradeoffs. They are not full-UI measurements.

The retained `benchmark-cef-overlay.sh` runner now compares complete egui and
shared HTML clients, not an otherwise identical egui client plus a tiny panel.
Do not mix new results with old results as if the workloads/UI were identical.
Frame intervals are wall-clock, not GPU timestamps. GPU samples are system-wide;
CPU paint counters do not measure texture upload. Uncompressed runtime size is
not compressed installer size. No new performance acceptance is implied.

## Remaining acceptance and distribution work

### Native latency investigation, 2026-10-05

The user reports that labels/planet markers trail the native bodies/orbit lines,
and task-list scrolling feels late. The attempted Chromium smooth-scroll toggle
did not help and has been reverted. No production cadence change has been made.

The upstream plugin's `BeginFrameInterval` defaults to 33 ms (approximately
30 Hz), even though its CEF browser settings allow 60 Hz. Bevy renders the scene
independently. Native annotations published in `Last` travel through renderer IPC,
JavaScript drawing, CEF composition/OnPaint, a latest-pixel slot, Bevy texture
update and the render pipeline. Latest-slot delivery is bounded, but does not
make the delivered pixels belong to the current scene frame.

Short controlled diagnostics at 1440 × 900 on the 75 Hz secondary monitor:

| Diagnostic configuration | Native source Hz | Received CEF paints Hz | Annotation source -> CPU pixels median / p95 | Source frames behind median / p95 |
| --- | --- | --- | --- | --- |
| Requested CEF 30 Hz | 74.98 | 30.00 | 61.17 / 65.64 ms | 4 / 4 |
| Requested CEF 60 Hz | 74.99 | 50.18 | 39.95 / 51.33 ms | 2 / 3 |

Twenty CEF DevTools wheel injections per run also tagged the first DOM scroll
update in returned pixels: median/p95 147/160 ms at 30 Hz, 94/107 ms at 60 Hz.
This excludes OS/Bevy input delivery and does not measure first physical display
presentation, nor the end of smooth scrolling. Annotation tags measure CPU pixel
receipt, before upload/render/presentation. These are short diagnostic runs, not
repeatability/performance acceptance. The experimental 60 Hz request reduces lag
but does not achieve frame alignment or even a sustained 60 received paints/s.

Raw samples and summaries:
- `data/logs/latency-1791201304971-30/`
- `data/logs/latency-1791201334531-60/`

Reproduce after building the native client:
```sh
node scripts/diagnose-native-ui-latency.mjs 30
node scripts/diagnose-native-ui-latency.mjs 60
```
The runner places a diagnostic on the visible secondary workspace without focus,
checks placement, and cleans up only its own uniquely tagged processes. The
opt-in `SPACE_UI_LATENCY_PROBE=1` code exits automatically after 25 seconds. It
uses tiny opaque pixel tags and scripted camera rotation/scrolling; normal
launches have no probe, synthetic motion or cadence override. Diagnostic-only
`SPACE_UI_PROBE_HZ` changes `BeginFrameInterval`; it is not a production fix.
Both test windows and their CEF helpers were closed after these measurements.

Recommended next work, not implemented: keep shared HTML panels, but render native
scene annotations directly in Bevy with the scene's current camera/frame, without
initialising egui. Independently improve/measure CEF panel scheduling/input latency.
Increasing CEF cadence alone cannot guarantee frame-accurate annotations. Delaying
the 3D scene to match CEF would trade away scene responsiveness and is not recommended.

### Prior verification and outstanding checks

Verification during migration:
- Workspace unit tests and strict Clippy passed; feature-free client tests passed.
- Browser WASM build and both HTML/egui browser integration suites passed.
- Bundled frontend mock-CEF tests passed.
- Real native CEF bridge checks passed: galaxy/state/tick, commands, task finding,
  table labels, FOV, HDR mode switching and pause/readout synchronisation.
- Native panels and table annotations were visually checked on HDMI-A-1.
  Test window, helper processes and test backend were closed after verification.

- Full native manual input: typing WASD/RF/QE, Tab/Shift+Tab, clipboard, wheel,
  modal/menu input, both drag-boundary directions, fast clicks, held keys,
  focus loss, resize and DPI changes. Verify no scene movement while typing.
- Full UI performance should be remeasured with the retained runner, especially
  canvas annotations during movement. The earlier small-panel result is not a
  full-UI performance guarantee.
- Linux/Wayland/NVIDIA is the current target. Windows/macOS distribution is not
  validated; setup scripts are development helpers, not finished installers.
- Practical packaging, licenses, updates, startup failure handling and runtime
  footprint still need distribution review.
- CEF's plugin disables the Linux sandbox. Only load the bundled trusted page,
  preserve restrictive CSP and the vendored pre-navigation inline-only gate,
  and deny new CEF windows. Do not load arbitrary websites/untrusted HTML.
- Built-in demo galaxy only. No task integrations, renderer rewrite, Bevy
  upgrade or zero-copy CEF work is part of this migration.

## Relevant files

- `crates/client/src/ui_bridge.rs`: shared serialised state contract.
- `crates/client/src/cef_overlay.rs`, `cef_input.rs`: native transport/composition/input.
- `crates/client/src/html_canvas.rs`: HTML viewport, annotations and scene picking.
- `crates/client/src/actions.rs`: shared application commands.
- `crates/client/src/ui.rs`: explicit egui fallback and native diagnostics.
- `web/native.html`, `web/ui/native.js`, `crates/client/build.rs`: native bundling/adapter.
- `web/index.html`, `web/ui/`: browser startup and shared frontend sources.
- `scripts/run.sh`, `setup-native-ui.sh`, `test-native-ui.mjs`: native workflow.
