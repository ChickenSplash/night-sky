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

## Atmospheres

The **atmosphere test** (`tools` tab, `crates/client/src/devtools/atmosphere_test.rs`)
measures the [atmosphere shader](rendering.md#atmospheres-prototype) on the
selected planet, for before-and-after comparisons such as the lookup tables
against the direct march. It pauses the clock and places the camera at three
views, with the star 30° above the ground below the camera:

- **orbit:** three radii from the centre, facing the lit disc and its terminator;
- **high:** at 90% of the top of the air, facing the horizon;
- **low:** 1 km up, facing the horizon.

Each view is measured with Earth's and Venus's air switched off, then at 16
and 24 view steps: 18 views, each settled 1.5 s and measured 5 s. The air's
cost is the GPU total with it on less the same view with it off, as the shell
has no pass of its own. The test then restores the air, the switch and the
clock, and frames the planet again. Each run is logged (`Atmosphere test (...)`)
and appended as one JSON line to `data/logs/atmosphere-test.jsonl`, with the
commit, planet, window size, MSAA, field of view and light steps, and per view
the frame mean and 95th percentile, the GPU total and the transparent pass.
Compare runs on the same planet, as the views scale with its radius.

## Search measurements

The original [search implementation history](../plans/object-search.md#performance)
retains the search probe results.
The engine probe is
`cargo test -p space-simulation --release search_probe -- --ignored --nocapture`.
It is an ignored measurement test, not part of normal checks. These measurements
describe release builds and particular work caps; do not treat them as current
performance guarantees.
