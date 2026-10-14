# Performance benchmarks

The user runs GPU, core-load and star-flight benchmarks on their own focused
window. Agents read results from `data/logs/`; do not launch benchmark runs.
The figures below are recorded baselines, not newly measured results.
See [development checks](../development.md#checks) for ordinary verification
and [diagnostics](devtools.md) for interpreting timing readouts.

## Core loading

The native debug window's **core load test** (`tools` tab,
`crates/client/src/devtools/core_load.rs`) jumps 1 pc from the black hole,
restarts streaming from nothing (`StreamRestart`: a new loader thread with
an empty `Sky` cache, every cell dropped) and times until no cells are pending.
Baseline before generation speed-ups: 157 cells, 114,992,223 stars in 141.6 s.
With the nuclear stellar disc: 145,751,381 stars. The star count is deterministic,
so a changed count means generation changed.

## Native GPU rendering

The native **GPU test** (`tools` tab, `crates/client/src/devtools/gpu_test.rs`)
is the fixed before-and-after benchmark for star rendering. It places the camera
500 ly from the black hole facing it, restarts streaming the same way, waits
until every cell is loaded and uploaded plus `SETTLE_S` (2 s), then measures
`MEASURE_S` (20 s) of frame times and the main view's GPU pass times.

Each run is logged (`GPU test (...)`) and appended as one JSON line to
`data/logs/gpu-test.jsonl`, with the commit (`+dirty` for uncommitted changes),
window size, MSAA state and magnitude limit. The star pass sums
`main_transparent_pass_3d` and `star_points`, and records which rendering path ran.

## Browser star flight

The browser debug window's **run star-stream benchmark** (`tools` tab) reloads
into a fresh, empty starfield at home, then follows -X (towards the galactic centre)
at 1,000 ly/s for 15 s without auto-visits. `crates/client/src/devtools/benchmark.rs`
records each `StreamMetrics::sample`, logs the results to the console and shows
them in an egui window with a JSON download.

`?benchmark=star-flight` is the identical route used by
`SPACE_HEADFUL=1 node scripts/benchmark-star-flight.mjs <label>`; it writes JSON
results under `data/logs/`. Headless SwiftShader is too slow for meaningful
comparisons, so use headed Chromium on Wayland. Visual tests belong on the
secondary monitor.

## Search measurements

The original [search implementation history](../plans/object-search.md#performance)
retains the native search probe results and
[browser worker measurements](../plans/object-search.md#session-4--browser-search-worker-done).
The engine probe is
`cargo test -p space-simulation --release search_probe -- --ignored --nocapture`.
It is an ignored measurement test, not part of normal checks. These measurements
describe release builds and particular work caps; do not treat them as current
performance guarantees.
