# Native HTML UI: remaining implementation plan

## Goal and scope

Evaluate sharing HTML/CSS/JavaScript panels between the browser and native
Bevy clients without moving the native simulation or scene rendering into a
browser wrapper. Replace native egui panels only after input, performance and
packaging checks pass and the user explicitly approves migration.

## Current state

- Experiment branch: `prototype-cef-overlay`.
- Latest implementation commit at this handoff: `443dc06`.
- Earlier milestones: `093459a` (transparent overlay), `1f19574` (input and pause bridge).
- The GTK/native HTML prototypes remain on `prototype-native-html-ui`.
- `main` has not been changed by this experiment. Nothing has been pushed.
- Bevy remains pinned to 0.18.1; optional native CEF uses `bevy_cef` 0.11.0.
- Browser builds and ordinary native launches do not initialise CEF.
- Native Rust simulation, Bevy windowing and Vulkan rendering remain in place.
- Existing egui panels remain present. There has been no full UI migration.
- Data source remains the built-in demo galaxy.

### Implemented

1. Opt-in CEF dependency, project-local SDK setup and launch script.
2. One transparent full-window HTML surface with a floating test panel.
3. Explicit input ownership: keyboard focus, Escape handoff, UI wheel routing,
   press-to-release drag ownership, held-key suppression and egui input filtering.
4. Minimal bridge: HTML sends `UiCommand::SetPaused`; Rust returns pause state
   and simulated days through `space:state` and `space:tick` at 10 Hz.
5. Benchmark infrastructure for separate release builds and repeated workloads.

The standard HTML `<select>` did not work reliably because Chromium paints its
options in a separate popup surface that this plugin does not compose correctly.
The test panel now uses HTML radio buttons instead.

### Verification and outstanding acceptance

- The initial transparent overlay was visually checked on Linux/Wayland with
  NVIDIA Vulkan, before the input/bridge and benchmark additions.
- Both benchmark release configurations compile.
- Automated checks passed: 74 CEF-enabled client tests, 65 normal-native client
  tests, strict CEF-enabled Clippy, browser compilation, runner self-tests and a
  headless HTML test using a mock CEF transport. The core navigation-policy test
  also passed during the input/bridge work.
- The user reported that the initial panel worked, except for the dropdown and
  camera movement while typing. Those reports preceded the input-isolation fix.
- The updated native input/bridge acceptance checklist is not recorded as passed.
- The native benchmark launch/measurement sequence has not been run on the
  user's behalf. No overhead results or performance acceptance exist yet.

## Next session: start here

1. Read `AGENTS.md`, this file and the benchmark section of `README.md`.
2. Inspect the working tree and current branch. Do not discard newer user work.
3. Ask whether manual input checks or benchmark runs have happened since this
   handoff. Look for results under `data/logs/cef-benchmark-*`.
4. Continue at the first unfinished gate below. Do not assume migration has been
   approved merely because the prototype looks good.

## Gate 1: finish native input and bridge acceptance

Launch the current panel:

```sh
./scripts/run-cef-overlay.sh
```

The SDK is already installed on this machine. If needed on a restored checkout:

```sh
./scripts/setup-cef-overlay.sh
```

The user does manual interaction testing; the agent builds and fixes issues.
Keep any agent-opened visual test windows on the secondary monitor only, without
taking focus or covering windows on primary `DP-1`.

Checklist:

- Type WASD, R/F and Q/E without camera travel or roll.
- Select text, copy/paste, and navigate with Tab and Shift+Tab.
- Scroll the panel and its text box without zooming the scene.
- Click and drag in empty space to control the scene normally.
- Press Escape after focusing HTML to return keyboard ownership to the scene.
- Drag from HTML into empty space and release; repeat in the other direction.
- Check fast clicks, window focus loss and held keys during a focus handoff.
- Confirm no stuck keys, mouse buttons or focus.
- Check hide/show, position controls, window resizing and scaling.
- Pause/resume through HTML, egui and the scene's Space shortcut. Both UIs must
  agree, and the simulated-day readout must freeze/resume appropriately.

If these fail, fix the prototype before treating benchmark results as evidence
for migration. The benchmark's camera route deliberately overrides navigation;
it is not a test of camera input isolation.

## Gate 2: run the small-panel benchmark (steps 5 to 7)

### Preparation

- Stop an existing backend on port 4321 so the runner can start its own.
- Show workspace **8 on HDMI-A-1 at scale 1**, and keep it visible throughout.
- Avoid other heavy work. Do not change graphics settings during the suite.
- The runner places fixed 1440 × 900 windows on the secondary monitor without
  taking focus. It does not change global desktop settings.

First validate the launch/measurement plumbing:

```sh
./scripts/benchmark-cef-overlay.sh --repetitions 1 --warmup 5 --seconds 5
```

This short run is not an acceptance-quality performance comparison. If it fails,
inspect its logs and fix the runner before starting the full suite.

### Unattended suite, preferred for the user's planned run

```sh
./scripts/benchmark-cef-overlay.sh
```

This runs automatically, with scripted DOM activity. Defaults are three
repetitions, at least 15 s warm-up and 20 s measurement per run: 60 runs and at
least **35 minutes**, plus readiness, startup and build time.

### Optional attended suite

```sh
./scripts/benchmark-cef-overlay.sh --interactive
```

This adds real typing/scrolling runs. It pauses after warm-up for an Enter prompt,
then asks the user to interact with the HTML fields during measurement. It cannot
be left unattended. There are 72 runs, at least **42 minutes**, plus prompts and
other delays. Use only text input and scrolling, not pause or graphics controls.

### Other controls

```sh
./scripts/benchmark-cef-overlay.sh --build-only
./scripts/benchmark-cef-overlay.sh --dry-run
./scripts/benchmark-cef-overlay.sh --self-test
```

`--skip-build` exists but may reuse stale binaries. Prefer the normal command,
which rebuilds both configurations. Builds stay under
`target/cef-benchmark/{baseline,cef}`.

### Configurations and workloads

Keep the same egui UI and graphics settings in every configuration:

| Mode | Purpose |
| --- | --- |
| A: `baseline` | Native release build with no CEF dependency |
| B: `runtime` | Browser/page alive, HTML hidden and overlay camera inactive after sizing |
| C: `idle` | Visible static panel, no periodic readouts after initial state |
| D: `active` | Scripted text edits/scroll changes and Rust readouts at 10 Hz |
| `animation` | Separate continuous requestAnimationFrame repaint stress test |
| `interactive` | Optional real user input and 10 Hz readouts |

Each mode runs stationary and on a repeatable flight towards the galactic centre
at 1,000 ly/s. Automatic visits are disabled. Physical/realistic lighting, galaxy
glow and 50° FOV are fixed. Warm-up waits for loader/mesh readiness and panel
readiness where applicable. Order rotates/reverses between repetitions.

Both requested uncapped and normal vsync presentation are measured. Benchmark
launches disable Bevy's normal unfocused 60 Hz update throttle. The compositor or
driver can still constrain presentation: requested uncapped is not proof of
effective uncapped rendering.

### Results and comparison

Results live in timestamped `data/logs/cef-benchmark-*` folders:

- `metadata.json`: revision, binary hashes, compiler, monitor, graphics, ordering,
  runtime sizes and limitations.
- `*.result.json`: raw frame intervals, frame summary, startup readiness,
  CEF paint counters, process samples and system-wide GPU samples.
- `summary.json`: comparisons, completed/planned run counts and completion flag.
  It is updated after each successful run, so interrupted suites remain partial.
- Per-run logs, status files and configuration files for diagnostics.

The user can share the summary and per-run files, or the agent can read them
locally. Compare like-for-like presentation/workload groups, preserving individual
repetition results and variability. Report:

- Median, p95 and p99 frame times, with absolute milliseconds and relative changes.
- Long-frame counts/rates.
- Native-process and helper CPU, and added aggregate client-tree PSS in MiB.
- Idle versus active paint frequency, copied bytes and CPU callback cost.
- Startup readiness and added uncompressed runtime size.
- Available GPU figures, with attribution limitations clearly stated.

Do not interpret a fixed FPS counter as proof of zero overhead. Check whether
presentation was capped, and whether the suite is complete before drawing a
conclusion. There is no pre-agreed numerical acceptance threshold; present the
tradeoffs and let the user decide what is acceptable.

### Measurement limitations

- Main-loop frame intervals are wall-clock measurements, not GPU timestamps.
- CPU 100% means one logical core. The main process also hosts CEF browser work.
- Process totals cover the native client and descendants, including CEF helpers;
  the fixed demo backend and runner are excluded.
- PSS access failures produce null. Summed RSS can double-count shared pages.
- Nominal 1 Hz sampling can miss short-lived helpers and straddle phase boundaries.
- GPU utilisation and VRAM are system-wide, not application-attributed.
- Paint timing covers CPU OnPaint copy/publish, not Bevy texture updates or GPU
  uploads. Intermediate paint buffers can be discarded by latest-frame slots.
- GPU upload timing and compressed package size are explicitly unavailable.
- Startup is a fresh process, not a cleared OS/CEF disk cache, and excludes OS
  dynamic-loader time.
- B still creates a browser and pixel surface and can process paint buffers.
- Scripted DOM changes in D are not real keyboard, clipboard or input-delivery tests.

## Gate 3: evaluate the existing full HTML UI before committing

If the small-panel results are acceptable, propose and obtain approval for a
second, still opt-in experiment loading the existing HTML panels from `web/ui/`.
A tiny panel validates the architecture, not the eventual full UI cost.

Work to plan and implement at that point:

1. Share frontend panel code through a transport adapter rather than duplicating
   the UI for CEF. Keep browser behaviour intact.
2. Extend the native bridge using the existing ID-based `UiCommand` architecture
   and the browser's galaxy/state/tick event contracts.
3. Extend hit rectangles, focus and input ownership to all panels and overlays.
4. Review standard controls that create separate Chromium popup surfaces.
5. Repeat input acceptance, then benchmark realistic full-panel activity under
   equivalent conditions. Do not remove egui to make the comparison look better.
6. Validate styling and HDR composition. CSS backdrop blur cannot see the native
   Bevy scene behind the CEF texture; do not promise it will work automatically.

No full-panel experiment or bridge expansion has been implemented yet.

## Gate 4: packaging and deployment review

Before migration approval, establish a practical way to package and launch CEF
libraries, helper processes and runtime assets without the full development SDK.
Review added size, startup, library paths, licenses, updates and failure handling.
The existing setup/run scripts are development helpers, not a finished installer.

Linux/Wayland is the current test environment. Do not claim Windows/macOS
packaging or compatibility has been validated. Linux's current CEF plugin policy
disables the Chromium sandbox: keep content trusted and local, preserve CSP and
the pre-navigation gate, and review this policy before broader distribution.

## Gate 5: user decision and migration

Only proceed after explicit user approval based on:

- Small- and full-panel input acceptance.
- Acceptable measured overhead and memory use.
- Normal native and browser regression checks.
- Manageable packaging and deployment.

If approved, plan a staged replacement of native egui panels with shared HTML.
Decide separately what to do with egui's canvas labels, picking and markers:
replacing panels does not automatically mean removing the whole egui dependency.
Keep a usable fallback until the replacement is validated. Do not introduce a
Bevy upgrade, zero-copy renderer or unrelated renderer rewrite unless the
experiment identifies a specific need and the user approves that work.

## Relevant files

- `crates/client/src/cef_overlay.rs`: overlay cameras, bridge and benchmark mode setup.
- `crates/client/src/cef_input.rs`: native focus, drag ownership and egui filtering.
- `crates/client/src/native_benchmark.rs`: readiness, route, frame samples and output.
- `crates/client/src/actions.rs`: shared application commands.
- `crates/client/src/web_bridge.rs`: existing browser event/command contracts.
- `crates/client/src/ui.rs`, `scene.rs`, `render.rs`: egui, controls and HDR integration.
- `web/cef-overlay.html`: minimal panel and scripted activity workloads.
- `web/ui/bridge.js`, `panels.js`, `panels.css`: existing browser panels to evaluate later.
- `scripts/benchmark-cef-overlay.sh`, `.py`: build, launch, sampling and comparison runner.
- `scripts/test-cef-panel.mjs`: headless DOM/transport-contract test, not native acceptance.
- `vendor/bevy_cef_core/` and `vendor/README.md`: navigation gate and paint instrumentation.
- `README.md`, `AGENTS.md`: launch commands, architecture and project invariants.
