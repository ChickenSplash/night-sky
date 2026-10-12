# Galaxy model

Design notes for generation, streaming, galactic rendering and navigation.
Paths below are relative to the repository root, or to the named crate's
`src/` directory. See [agent rules](../AGENTS.md) and
[realistic rendering](realistic-rendering.md) before changing these systems.

## Population and generation

- `space_simulation::galaxy::GalaxyModel` (parameterised, `milky_way()` by
  default) gives stars per cubic parsec at any point, per `Component`: nuclear
  cluster, bar and bulge, thin disc, young arm stars, thick disc, halo and
  nuclear stellar disc (an exponential `Disc`, 75 pc by 45 pc, last in
  `Component::ALL` so the others keep their star ID indices). Axes
  are parsecs from the central black hole, disc in XZ, Y to the north pole;
  home is 8.2 kpc out on +X, between arms.
- Every profile has a closed-form integral, so components hold exactly their
  share of `stars` (2e11). Spiral modulation averages to one around every
  circle; keep it that way, or the totals drift. The test integrates the
  density numerically to within 1%.
- Galactic stars (`galactic_star(id, component)`, `GalaxyModel::star(id, p)`)
  draw a Kroupa birth mass and an age from the component's formation history,
  then evolve (main sequence, giant or supergiant, white dwarf). Neutron stars
  and stellar black holes are redrawn: `stars` counts star systems only. Their
  systems come from `galactic_bodies(id, component)`.
- Multiple stars (`crates/simulation/src/multiple.rs`): every drawn star is a system's
  primary, its brightest star, and draws its companions from the same
  sequence (no separate seed): `{id}:star` for page and galactic stars, and
  for tiered stars `{id}:tier` for what sets their light, `{id}:star` for
  the rest. Multiplicity rises with mass (about 26% of red dwarfs, 46% of
  Sun-like stars, most B stars), a quarter of multiples have three or four
  stars, and about a quarter of white dwarfs have companions (all tested).
  Companions share the primary's age, so some born heavier are white dwarfs;
  ones a giant phase would have engulfed are gone. Pairs nest hierarchically
  (`Barycentre`, innermost first, the last holding the system), each a
  two-body orbit widened to stay stable (Mardling & Aarseth); stars are
  lettered down the hierarchy (a close pair AB, its distant third C). Planets
  orbit one star inside its stable zone, or a pair closer than 1 AU from
  outside it (Holman & Wiegert); pairs under 50 AU keep planets a third of
  the time. `bodies(key)`/`cell_bodies(id)` build a whole system;
  `member_position` and `planet_position` place stars and planets from the
  page's outermost barycentre. `place` and `galaxy_nav::generated` both build
  from `Bodies`; a page's capacity is its total planets. The census and IMF
  describe primaries; companions add about a third more stars, mostly red
  dwarfs.
- Page stars (`star(id)`) keep the fixed solar-neighbourhood census until
  lists move onto galactic stars. Both share the builders in `crates/simulation/src/star.rs`; census
  draws must keep their order, or every page star changes.
- Galactic stars are generated from cells (`space_simulation::cells`), never
  stored. `Cell` is a locational code (a leading 1, then octant bits x, y, z per
  level); the root is `ROOT_PC` (131,072 pc) across, centred on the black hole.
- `population` sorts stars into `TIERS` brightness bands, four times dimmer per
  tier (`tier_limit`). Tier t's stars belong to level-t cells, so cell size and
  reach (`reach_pc`) both halve per tier. Tiered stars are sampled directly
  within their band; together they match `galactic_star`'s mix (tested).
- `Sky` holds the root's exact counts per component and tier and splits each
  cell's finer counts among its children by a seeded multinomial, weighted by
  the model's stars in each child (`child_masses`). Children always add up to
  their parent, and the galaxy holds exactly 2e11 stars. Seeds depend only on
  cell codes, so results never depend on visiting order or the cache
  (`Sky::retain` only drops cached work).
- A cell star's ID is `g{cell hex}.{component}.{index}`. `cell_star(id)` and
  `cell_planetary_system(id)` rebuild it from the ID alone; its position needs
  `Sky::star(id)`.

## Planetary systems and orbital simulation

- Calculations use SI metres and `f64`; the interface uses AU and light years.
  Orbital positions are solved analytically from Kepler's equation. Periods
  follow the semi-major axis and the host's mass, including barycentre hosts.
- Systems are generated as a whole from stable star IDs, with planets around
  component stars or close pairs as their stability zones allow. A single star
  typically has 7 to 14 planets; multiple systems can have more, fewer or none.
  Tasks fill stable slots on these bodies and never determine their physical properties.
- Composition follows the snow line: rocky worlds and sub-Neptunes inside,
  giants farther out and dwarf planets beyond. Radii range from Pluto-like to
  just beyond Jupiter. Neighbours stay Hill-stable without crossing orbits;
  some pairs have resonant period ratios (2:1, 3:2, 4:3, 5:4, 5:3, 3:1).
- Spins have independent seeds and tilted axes. Close-in planets can be tidally
  locked; others draw spin-orbit states, slow spins or large tilts. Spin does not
  move an orbit. Task status, deadlines, ordering and removals never change
  surviving bodies' orbital elements or permanent slots.
- `big_space` supplies a grid-cell floating origin. Render positions subtract
  the camera in double precision before conversion to GPU floats. Bevy's
  reverse-Z perspective depth and a distance-adjusted near plane support the
  changing scale; this is not a logarithmic depth buffer.
- Bodies retain physical radii by default. **Readable planet sizes** exaggerates
  displayed planet radii, not positions or periods. Wireframes use procedural
  latitude/longitude lines rather than native polygon wireframe mode.
- This is not an N-body simulation. Collisions, relativistic effects and
  physical warp travel are not implemented. Camera flights are navigation aids,
  not simulated spacecraft trajectories.

## Streaming and spatial queries

- Streaming makes about 100 million stars at the galactic centre, so the
  per-star path (`Sky::try_point`) builds no strings or `Star`: `StarKey`
  hashes the ID exactly as `stable_hash` would, `tiered_light` takes only
  luminosity, temperature and radius from the same `tiered_shape` draws as
  `tiered_formation`, plus its companions' light (`companion_light`), so a
  point is its whole system: under four times its primary, at most one tier
  brighter than its band, at its barycentre. It also replays the pairs'
  layout from the same draws (`Hierarchy::arrange` on masses and radii
  alone) for `StarPoint::separation_au`, the widest its stars get. Each star
  descends the tree once. It must give the
  same stars as `Sky::build`, bit for bit (`star_points_match_the_full_stars`).
  Keep both in step when changing star generation.
- `try_point` only reads the `Sky`; it reports a missing integration or
  rejection bound as a `Need`, which `supply`/`complete` work out. So
  `Loader::with_threads(n)` shares each cell's stars among n threads (native
  `star_threads`, three quarters of the machine's threads, at least 1;
  the browser uses 1; the perf tab's **threads** row shows them and how many
  were busy, from `Loader::take_worked`): stars needing something become
  holes, made again once it is worked out, so a cell is identical with any
  thread count (tested). With several threads, a made cell's grid
  (`StarCell::new`) builds on a thread of its own while the next cell's stars
  are made (`Loader::gridding`, at most `GRIDDING` at once, delivered in
  order). Moving away from a cell whose grid is building discards it without
  a `Dropped` event, as it was never delivered.
- `space_simulation::stream`: `wanted(camera, magnitude)` lists, per tier, the
  level-t cells within `reach_pc`, most visible first. `Loader` builds them one
  unit at a time (one child box integrated, via `Sky::integrate`, or one star)
  and stops when the caller's `more()` says so, so no step blocks for long.
  `counts_need`/`place_need` report the masses a step still needs; they share
  `Sky::descend` with `place`, so sliced loading gives the same stars. Loaded
  cells drop beyond `KEEP` × reach. Stars stream as compact `StarPoint`s.
- `crates/client/src/stars/stream/` aims the loader at the camera
  (`CameraRig::galactic_ly`, lists anchored at home) to
  `Navigation::magnitude` (6.5 by default): a
  worker thread natively (`native.rs`), a dedicated module Web Worker in the browser
  (`web.rs`), into the `Starfield` resource. The worker's generation-only WASM crate is
  `crates/star-worker`, with `web/star-worker.js` as its entry point; both WASM
  bundles are built by `scripts/build-web.sh`. `stream_wire` transfers cells in
  binary with f64 positions, and browser startup/worker errors fall back to
  the old 4 ms per-frame loader. Native loads about 1.06 million stars in 175
  cells near home in 2.3 s; the pre-worker browser took about 8 to 14 s.
  `space-simulation` builds at opt-level 3 in dev, or loading and tests crawl.
  `StreamMetrics` (`stars/stream/metrics.rs`) samples generation time, cancelled jobs,
  mesh backlog and wait, render-sync time and frame-time percentiles once per second;
  the debug window's `perf` tab shows them on both clients.
  Instrumentation does not change loading policy.
  `StarCell::new` (simulation `stream.rs`) sorts a cell's stars into a uniform
  grid of boxes (about 8 stars each, never under 0.1 pc, a power of two per
  axis, numbered in Morton order so aligned cubes are contiguous runs), on the loader's
  thread, so `stars` is in grid order, not generation order. Build cells only
  through `StarCell::new`; changing `stars` afterwards leaves the grid stale.
  Spatial questions use the grid: `StarCell::nearest` (rings of boxes outward)
  behind `Starfield::nearest` for automatic arrival in `scene::locate` and the
  close-up render scale in `place_camera`, and `StarCell::near` (boxes within a
  radius) for precise-sprite `candidates`. Never scan `Starfield::cells` star
  by star per frame or per streaming change: the cells containing the camera
  hold tens of millions of stars at the galactic centre. The perf tab's
  nearest star section times the lookups.

## Benchmarks

- The native debug window's **core load test** (`tools` tab,
  `crates/client/src/devtools/core_load.rs`) jumps 1 pc from the black hole,
  restarts streaming from nothing (`StreamRestart`: a new loader thread with
  an empty `Sky` cache, every cell dropped) and times until no cells are pending.
  Baseline before any generation speed-ups: 157 cells, 114,992,223 stars in
  141.6 s. With the nuclear stellar disc: 145,751,381 stars. The star count is
  deterministic, so a changed count means generation changed.
- The native **GPU test** (`tools` tab, `crates/client/src/devtools/gpu_test.rs`)
  is the fixed before-and-after benchmark for star rendering: it places the camera
  500 ly from the black hole facing it, restarts streaming the same way, waits until
  every cell is loaded and uploaded plus `SETTLE_S` (2 s), then measures
  `MEASURE_S` (20 s) of frame times and the main view's GPU pass times. Each
  run is logged (`GPU test (...)`) and appended as one JSON line, with the
  commit (`+dirty` for uncommitted changes), window size, MSAA state and
  magnitude limit, to `data/logs/gpu-test.jsonl`. The user runs the GPU test and other
  benchmarks on their own focused window; agents read the results from the
  logs and do not launch benchmark runs themselves.
- The browser debug window's **run star-stream benchmark** (`tools` tab) reloads into a fresh,
  empty starfield at home, then follows -X (towards the galactic centre) at
  1,000 ly/s for 15 s without auto-visits. `crates/client/src/devtools/benchmark.rs`
  records each `StreamMetrics::sample`, logs the results to the console and shows them in an
  egui window with a JSON download. `?benchmark=star-flight` is the
  identical route used by `SPACE_HEADFUL=1 node scripts/benchmark-star-flight.mjs
  <label>`; it writes JSON results under `data/logs/`. Headless SwiftShader is
  too slow for meaningful comparisons, so use headed Chromium on Wayland.

## Galactic star rendering

- `crates/client/src/stars/starfield.rs` builds batches of up to `PACK` (65,536) uploaded stars,
  filled from `SLICE`-sized steps of a cell's stars within the upload budget,
  so a large cell may take several frames, and `stars/render.rs` draws
  them. Each star is one 20-byte `StarInstance` (f32 position from its
  block's centre, luminosity, temperature) in a vertex buffer uploaded once,
  drawn as an instance of a three-corner triangle the shader builds from
  `vertex_index`. Bevy's material path has no per-instance data, so this is
  a custom pipeline in `Transparent3d`, sharing Bevy's view bind groups and
  drawing only in the `SpaceCamera` view (extracted). Batches are not
  entities: each frame `sync` lists the draws in `StarFrame` and queues new
  and dropped batches in `StarUploads`; both are taken at extraction, so a
  frame's draws and buffers agree. A draw's phase item has no entity, so its
  batch range carries the draw's index (Bevy's batching finds no mesh for
  the placeholder and leaves it). A rebuilt
  cell's new batches wait in `Batches::fresh` and replace the old ones only
  once complete, so rebuilds never leave holes. Star positions are
  local to their grid block's centre (`StarCell::blocks`, at most `BLOCK`
  loaded stars, an aligned cube of the cell's grid; no batch spans two
  blocks); each draw's origin subtracts the camera in f64,
  then scales to render units. Filter points beyond 1.25 times their apparent
  magnitude reach before upload; the shader applies the exact cutoff. Rebuild
  retained cells after the camera travels 0.2 of the tier's reach.
  Natively, while the `point_stars` switch is on (default),
  `stars/points.rs` draws the same batches as single-pixel points, one
  vertex per star instead of three: the `StarPoints` node (between
  `MainOpaquePass` and `MainTransmissivePass`, GPU span `star_points`) draws
  them additively into an `Rgba16Float` image at `SUPERSAMPLE` (2) times the
  view's resolution per axis, without MSAA. `starfield_points.wgsl` applies
  the same cutoff, near hiding, colour and glare storage as `starfield.wgsl` (keep them
  in step) and depth-tests each star's centre against the opaque bodies (the
  space camera's depth texture has `TEXTURE_BINDING` natively for this;
  never enable that usage on WebGL's multisampled depth texture, which
  panics with "Tex storage 2D multisample is not supported"). One
  full-screen draw in `Transparent3d` (`starfield_points_composite.wgsl`)
  gives each point the 0.55 px Gaussian from the 6x6 supersampled pixels
  around each view pixel, so points land within a quarter pixel. Under
  points there is no alternate compact-footprint switch. The GPU test's star pass
  sums `main_transparent_pass_3d` and `star_points`, and records which path
  ran. The browser always uses triangles. A compute splat with atomics
  (`b6a078f`, reverted) was slower than triangles: 17.3 vs 14.3 ms.
  `starfield::Sight` skips batches outside the view each frame (the
  `cull_blocks` switch): a sphere around each batch's block, in f64 against
  the view's four side planes, widened by the point footprint.
  Batch positions are f32 offsets from their block's centre, so their error grows
  with that offset: within `PRECISE` times it of the camera a star is drawn
  instead as a precise sprite, from a per-frame buffer of offsets from the eye
  formed in f64 (gathered at `PREPARE` times the radius). The batch point
  remains until a quarter of that radius, so both overlap. The
  visited star is a body only while the camera is at it (`scene::body_range`,
  where its planets show): batches hide points within that range and it has
  no sprite. A multiple system is one point until its stars could appear
  `SPLIT_PX` (0.5) pixels apart, inside the point spread, so the split never
  shows; from there to arrival it is drawn star by star as precise sprites,
  from `member_position` at the clock's time in `scene::star_frame`, exactly
  where its bodies appear on arrival. `StarInstance` packs temperature and
  log2 separation as half floats (still 20 bytes), and both star shaders hide
  a batch point whose separation spans more than the draw's angle
  (`near.z`) within `near.w` parsecs; CPU and shader decode the same value.
  `StarCell::resolved` finds the candidates through a per-block pyramid of
  widest separations (`StarCell::widest`), within a reach that halves
  whenever a gather would measure over `SPLIT_BUDGET` stars, so crowded
  regions split only nearby systems. Its movement margin shrinks with the
  reach; at most four attempts run per gather. Overload temporarily keeps
  combined points but retains a positive retry reach. Cheap searches expand
  again on 250 ms retries even without movement; streaming restarts reset
  this adaptive state. Empty searches also expand, even if a coarse grid leaf
  measured many stars outside the search radius. Disabling splitting or
  galactic points stops split searches; re-enabling starts a fresh search.
  Examined counts include retries. The perf tab's collapsible `split systems`
  section measures search CPU elapsed
  time per search and per frame, searches and examined work per second,
  preparation CPU elapsed time per frame, cache-miss rebuilds and capacity
  clears, plus live cached systems and submitted/off-screen sprite counts.
  Timings use trailing-second averages and are also in `StreamSample` for
  benchmark reports. Preparation excludes GPU uploads and GPU rendering;
  off-screen counts use conservative system-sized spheres and do not cull.
  The UI additionally shows latest-frame and trailing-second peaks for
  search and preparation, uncached generation, orbital/sprite construction
  and rebuild counts. Generation and orbit timings are parts of preparation,
  not extra costs to add to it.
  Debug switch `split_multiples` is in the performance switches section.
  Outside the splitting reach, a system
  is a galactic point under the same magnitude limit. Galactic points use
  apparent flux, scaled by `sky::radiance_gain`. Their colour (and,
  realistic, visual efficiency) comes from 32-entry tables by temperature in
  the shader's uniforms: a blackbody palette, or `spectral_colour` when not
  realistic. Which stars load still follows bolometric luminosity. Every
  point is a
  0.55 px sigma Gaussian, the camera's point spread shared with body sprites
  (`render::PSF_SIGMA_PX`) (2 px footprint,
  widened up to 12 px for a star whose Gaussian shows
  beyond it), as in a perfect camera: brightness shows in its peak, linear,
  and above white in its glare (`hdr.wgsl`).
  The former logarithmic point knee remains only in wireframe mode. Crowds
  are never widened or blurred.
- `crates/client/src/render/sky.rs` now contains only fixed unit conversions and a fixed zero-stop
  reference (the former dark-space calibration), with no `Sky` resource,
  view meter or adaptation. `exposure::gain` is `2^stops / reference_radiance`.
  `sky::radiance_gain` converts galactic light with that same gain for points
  and glow. Camera location, selection and streaming never adjust exposure.
- `crates/client/src/render/black_hole.rs` draws an opaque sphere at the central black hole's physical
  Schwarzschild radius; it is not visible from home. No accretion disc or
  lensing yet.

## Diffuse galaxy glow

- `simulation::glow` builds smooth, population-normalised Gaussian volume patches
  for the first galaxy-glow milestone, with spiral modulation averaged out only
  in the glow. A stratified, distance-indexed RGB luminosity table statistically
  excludes stars visible at the magnitude limit (`unresolved_table`,
  recomputed by `crates/client/src/render/glow.rs` when it changes). This is approximate at finite patch/table
  resolution. Patch grids reach past `FLOOR` (1e-5 of each component's
  densest patch), so profiles fade out instead of stopping at a grid edge
  (tested). The nuclear cluster is four Gaussians fitted to its Plummer
  profile. The client renderer batches them spatially and `glow.wgsl` integrates
  each ellipsoid analytically, without a full-galaxy raymarch loop, widening
  each to at least half a glow pixel (keeping its light) so sub-pixel patches
  never flicker. Perspective ellipsoid bounds and view-cone rejection limit
  overdraw. Every point inside the patch grid sits within about 50 patches per
  component, each covering the whole view, so cost is pixel fill: a separate
  glow camera (layer 30, order -1, no MSAA) draws patches into a half-float
  image at 1/8 of the view's resolution per axis (`DOWNSCALE`, chosen by
  comparing full to 1/32), and
  `glow_composite.wgsl` scales it up additively at the far plane, with a cubic
  B-spline (four bilinear taps), not plain bilinear. Patch light is radiance
  times `sky::radiance_gain`, visual (eye-weighted), so it matches the points
  and bodies under manual exposure. The image is redrawn only when the eye,
  axes, field of view, size or (beyond 2x) exposure change by more than a
  quarter glow pixel, plus `SETTLE` frames and while any pipeline compiles;
  otherwise the composite rescales it by the exposure ratio.
  Camera-relative origins are formed in f64 before converting to f32. Patches
  are built lazily and hidden when disabled.
  Glow is shown only with
  realistic lighting and the **Smooth galaxy glow (prototype)** switch enabled
  (debug window, `render` tab), which also has the Inside, Face-on and Edge-on
  reference camera commands on both clients. `node scripts/test-galaxy-glow.mjs` saves comparison
  screenshots and on/off frame timings to `data/logs/`, on the secondary monitor.
  Dust, detailed spiral structure and other galaxy types are not implemented yet.

## Navigation and selection

- There is one mode: the streamed galaxy. List pages are ordinary cell stars,
  drawn and picked like any other, with no markers of their own.
  `Navigation::visit` (`crates/client/src/navigation.rs`) is the system the camera is anchored to
  (at, flying to, or showing as a constellation): `Visit::List` (a list index, at
  `Navigation::page`) or `Visit::Generated` (task-free planets built from the
  star's ID, never added to the lists' `Universe`). `None` is open space,
  relative to home in galactic axes; a visit moves the camera frame to its
  star in f64, and the same `Flight` shifts the displayed pose. Simulation, table
  and constellation presentation modes work on any visited system.
- `crates/client/src/presentation.rs` owns `PresentationMode`: `Simulation` (simulated orbital
  positions), `Table` (comparison row), and `Constellation` (connected stars).
  `Navigation::presentation_mode` describes scene presentation, not tracking or
  free flight. Switching presets also sets camera framing. The terminal command
  is `set_presentation_mode simulation|table|constellation`; JSON commands use
  `type: "set_presentation_mode"` and `presentation_mode`, also the browser state
  key. Camera views and physical `Orbit` elements keep their existing names.
- Where the camera is and which list is open are separate:
  `Navigation::location` (the visit while `Navigation::arrived`) is physical,
  and `Navigation::list` is the open list (a `Visit::List`, kept in
  constellation view). `scene::locate` (`scene/location.rs`) sets `arrived` every frame: within
  `system_range` of the page star (0.02 ly, or 4 times the system's extent,
  `system_extent_au`, stars and planets, if wider) it arrives, and it leaves beyond `LEAVE_FACTOR` (2) times that.
  Constellation view is never at a star, and a flight arrives only on getting
  there. Clicking Visit and drifting in arrive the same way. Leaving in simulation
  mode, however the visit began, returns to open space and closes the list;
  there is no restoring a previous visit.
- Free flight in open space automatically activates a loaded star's list page
  or generated system within 0.02 ly, without flying, reframing or changing
  movement speed. Frame changes preserve the camera eye despite f32 quaternion
  rounding. Bodies are drawn (`scene::body_range`) and render scale tightens
  only while `arrived`, which follows physical proximity, not the zoom/range
  setting, so the same point-to-body handoff works after clicking Visit or
  drifting in.
  Close-up star size exaggeration also follows physical eye distance, never a
  potentially interstellar camera range that would put the eye inside its shell.
- During free-look and free flight, `Navigation::eye` is the authoritative f64
  camera position in the current page frame. `target` remains for orbiting and
  flights; never reconstruct the close-up eye by cancelling a distant target
  against the camera range. Velocity uses differences of local eyes and ignores
  the rounding residual on star-frame switches. During right-drag the speed
  readout still measures the eye's orbit, but the direction marker uses only
  intentional keyboard travel, not rotation around the target.
- The render scale tightens near any loaded star within 20 ly even before its
  system activates, independently of the selected camera range. Keep the
  visited body's physical-proximity fallback when its cell has not loaded yet.
- `Navigation::selection` is the one clicked object: `Selection::Star` (any
  star, by stable ID) or `Selection::Planet` (a stable planet ID, including
  remote systems). Selecting one replaces the other; set it with
  `Navigation::select`, which keeps following only from planet to planet. A
  second click activates it: a star is visited, a planet followed. Esc while
  following reframes the system and clears selection; otherwise it clears
  selection, or with nothing selected goes up physically. Leaving a system
  keeps either selection and stops tracking. Planet location uses simulated
  f64 positions: enter within 20 physical planet radii, leave beyond 30.
  `scene::locate_planet` runs after camera placement, using the same simulation
  instant as the eye, including when following at high time rates.
  Render exaggeration, selection and following never affect this threshold;
  table and constellation layouts have no physical planet location.
  `crates/client/src/galaxy_nav.rs` picks and visits the selected star (`visit_selected`): a star with a list page (`galaxy_nav::mapped`) opens
  that page, any other its generated system. A new snapshot moves the visit
  with its star (`navigation::follow_visited_star`). Clicks not on a planet pick from
  loaded, visible points only, and only on click, never every frame.
  Picking traverses `StarCell::query_blocks` with a conservative sphere/cone
  test, then applies the original magnitude and angular tests only to leaf
  candidates, rather than scanning every loaded star. Each click logs elapsed
  time and stars examined as `Star picking`. `Starfield::picked` caches the
  last ID and point so applying selection and inspector reachability avoid
  rescanning its cell; replacement/removal of that cell or restart invalidates
  the cache, while unrelated streaming changes retain it. Uncached typed ID
  lookups still scan their cell once. Click
  again or Enter to visit. The bodies drawn are
  the visited system's stars and planets. Shift + scroll (either Shift key) zooms
  from 20,000 km to 40,000 ly; plain scroll adjusts movement speed independently.
- Unassigned cell stars use ID-derived `TS` catalogue names and lettered
  planets, with no name registry or per-star UI entities.
