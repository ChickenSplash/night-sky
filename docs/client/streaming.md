# Star streaming and spatial queries

The client loads the galaxy's visible cells around the camera, not a complete
galaxy database. Generation and spatial indexing live in `crates/simulation`;
the native loader transport lives in `crates/client/src/stars/stream/`.
See [galaxy cells](../simulation/galaxy.md),
[galactic rendering](galactic-rendering.md) and
[benchmarks](benchmarks.md) for the surrounding systems.

## Compact generation and sliced loading

- Streaming makes about 100 million stars at the galactic centre, so the
  per-star path (`Sky::try_point`) builds no strings or `Star`: `StarKey`
  hashes the ID exactly as `stable_hash` would, `tiered_light` takes only
  luminosity, temperature and radius from the same `tiered_shape` draws as
  `tiered_formation`, plus its companions' light (`companion_light`).
- A point is its whole system: under four times its primary, at most one tier
  brighter than its band, at its barycentre. It replays the pairs' layout
  from the same draws (`Hierarchy::arrange` on masses and radii alone) for
  `StarPoint::separation_au`, the widest its stars get. Each star descends the
  tree once. It must give the same stars as `Sky::build`, bit for bit
  (`star_points_match_the_full_stars`). Keep both in step when changing generation.
- `try_point` only reads the `Sky`; it reports a missing integration or
  rejection bound as a `Need`, which `supply`/`complete` work out.
  `Loader::with_threads(n)` shares each cell's stars among n threads:
  `star_threads` uses three quarters of the machine's threads, at least
   1. This is now a shared work budget with background planet refinement,
   not an independent allowance for each loader. The perf tab's **CPU threads**
   section shows the shared ceiling and star-worker activity, from `Loader::take_worked`.
- Stars needing something become holes, made again once it is worked out,
  so a cell is identical with any thread count (tested). With several threads,
  a made cell's grid (`StarCell::new`) builds on a thread of its own while the
  next cell's stars are made (`Loader::gridding`, at most `GRIDDING` at once,
  delivered in order). Moving away from a cell whose grid is building discards
  it without a `Dropped` event, as it was never delivered.
- `space_simulation::galaxy::stream::wanted(camera, magnitude)` lists, per tier, the
  level-t cells within `reach_pc`, most visible first. `Loader` builds them one
  unit at a time (one child box integrated, via `Sky::integrate`, or one star)
  and stops when the caller's `more()` says so, so no step blocks for long.
  `counts_need`/`place_need` report the masses a step still needs; they share
  `Sky::descend` with `place`, so sliced loading gives the same stars.
  Loaded cells drop beyond `KEEP` × reach. Stars stream as compact `StarPoint`s.

## Loader thread

- `crates/client/src/stars/stream/` aims the loader at the camera
  (`CameraRig::galactic_ly`, lists anchored at home) to `Navigation::magnitude`
  (6.5 by default) on a worker thread (`native.rs`), into the `Starfield`
  resource.
- Recorded loading near home: about 1.06 million stars in 175 cells in
  2.3 s. This is a historical measurement, not a current performance guarantee.
- `space-simulation` builds at opt-level 3 in dev, or loading and tests crawl.
- `StreamMetrics` (`stars/stream/metrics.rs`) samples generation time, cancelled
  jobs, mesh backlog and wait, render-sync time and frame-time percentiles
  once per second; the debug window's `perf` tab shows them.
  Instrumentation does not change loading policy.

## Shared star and planet CPU budget

`simulation::background` admits active star and background planet work into
one process-local budget: `max(1, available_parallelism × 3 / 4)`. On a machine
reporting 20 threads that is **15 total**, not 15 per subsystem. The star loader's
coordinator, scoped star workers and asynchronous cell-grid builders all hold
permits. If no grid slot is free, the coordinator builds the grid itself; extra
star workers take only currently free slots. Separate magnitude loaders also
share the same budget while the old one winds down.

Star admission has priority. Planet preparation/solves are admitted only when
no star work is active or waiting. On multi-thread budgets one slot is reserved
for newly arriving star work, so planets alone can occupy at most 14 of 15 slots.
Existing solves are not preempted: stars use the reserved slot immediately and
reclaim more as solves finish. On a one-thread budget they must wait for the
current solve. Only the background star coordinator may wait for admission;
planet dispatch and result polling on the frame thread are nonblocking.

This is a concurrency budget, not CPU affinity, real-time scheduling or a
guarantee against dropped frames. Bevy rendering/ground jobs, search, craft
loading and other processes are outside this budget. Interactive frame-time
testing remains necessary. See [background planet refinement](../simulation/planet-composition.md#experimental-background-loading)
for the opt-in mode and cancellation/cache behaviour.

## Magnitude changes

The magnitude limit is the faintest apparent magnitude drawn as a star point:
default `VISIBLE_MAGNITUDE` (6.5), adjustable from 3 to 10.5 in tenths. A HUD
drag previews the value and applies it on release (`SetMagnitude`,
`set_magnitude 5.5`), because a change restarts streaming from nothing.
`stars/stream.rs` starts a new `Backend` whenever its magnitude differs.

The upload filter, shader cutoff, rebuild distance, star picking and glow's
unresolved light table all follow the limit. Fainter stars move into the glow,
leaving the total light unchanged in the model. Each magnitude fainter draws
about four times the stars. This setting is not persisted; page-star assignment
still uses `VISIBLE_MAGNITUDE`. See [galactic rendering](galactic-rendering.md).

## Spatial indexing

- `StarCell::new` (simulation `galaxy/stream/index.rs`, re-exported through
  `galaxy::stream`) sorts a cell's stars into a uniform
  grid of boxes (about 8 stars each, never under 0.1 pc, a power of two per
  axis, numbered in Morton order so aligned cubes are contiguous runs), on
  the loader's thread. `stars` is in grid order, not generation order.
- Build cells only through `StarCell::new`; changing `stars` afterwards leaves
  the grid stale, so any mutation needs a rebuilt grid.
- Spatial questions use the grid: `StarCell::nearest` (rings of boxes outward)
  behind `Starfield::nearest` for automatic arrival in `scene::locate` and
  close-up render scale in `place_camera`, and `StarCell::near` (boxes within
  a radius) for precise-sprite candidates.
- Never scan `Starfield::cells` star by star per frame or per streaming change:
  cells containing the camera hold tens of millions of stars at the centre.
  The perf tab's nearest-star section times the lookups.
