# Galactic points and diffuse light

Streamed star systems are distant points, resolve into individual stars nearby,
and become bodies on arrival. Smooth galaxy glow approximates light from stars
below the visible magnitude limit. Both share the manual exposure described in
[realistic rendering](rendering.md). Client paths are relative to
`crates/client/src/` unless prefixed with `crates/`.

## GPU batches and points

- `stars/starfield.rs` builds batches of up to `PACK` (65,536) uploaded stars,
  filled from `SLICE`-sized steps of a cell's stars within the upload budget,
  so a large cell may take several frames; `stars/render.rs` draws them.
- Each star is one 20-byte `StarInstance` (f32 position from its block's centre,
  luminosity, temperature) in a vertex buffer uploaded once, drawn as an
  instance of a three-corner triangle built from `vertex_index`. Bevy's material
  path has no per-instance data, so this is a custom pipeline in `Transparent3d`,
  sharing Bevy's view bind groups and drawing only in the `SpaceCamera` view.
- Batches are not entities: each frame `sync` lists draws in `StarFrame` and
  queues new/dropped batches in `StarUploads`; both are taken at extraction,
  so a frame's draws and buffers agree. A draw's phase item has no entity;
  its batch range carries the draw index (Bevy finds no mesh for the placeholder
  and leaves it). A rebuilt cell's new batches wait in `Batches::fresh` and
  replace old ones only once complete, so rebuilds never leave holes.
- Positions are local to their grid block's centre (`StarCell::blocks`, at most
  `BLOCK` loaded stars, an aligned cube of the grid; no batch spans two blocks).
  Each draw's origin subtracts the camera in f64, then scales to render units.
  Filter points beyond 1.25 times their apparent-magnitude reach before upload;
  the shader applies the exact cutoff. Rebuild retained cells after the camera
  travels 0.2 of the tier's reach.
- Natively, `point_stars` (on by default) uses `stars/points.rs` to draw the
  same batches as single-pixel points, one vertex rather than three. The
  `StarPoints` node (between `MainOpaquePass` and `MainTransmissivePass`, GPU
  span `star_points`) draws additively into `Rgba16Float` at `SUPERSAMPLE` (2)
  times the view resolution per axis, without MSAA.
- `starfield_points.wgsl` applies the same cutoff, near hiding, colour and glare
  storage as `starfield.wgsl`; keep them in step. It depth-tests each centre
  against opaque bodies (the space camera's depth texture has `TEXTURE_BINDING`
  natively; never enable it on WebGL's multisampled depth texture, which panics
  with "Tex storage 2D multisample is not supported").
- One full-screen draw in `Transparent3d` (`starfield_points_composite.wgsl`)
  gives each point the 0.55 px Gaussian from the 6x6 supersampled pixels around
  each view pixel, so points land within a quarter pixel. There is no alternate
  compact-footprint switch. The browser always uses triangles.
- A compute splat with atomics (`b6a078f`, reverted) was slower than triangles:
  17.3 vs 14.3 ms. The [GPU benchmark](benchmarks.md#native-gpu-rendering) records
  which path ran and sums both relevant passes.
- `starfield::Sight` skips batches outside the view each frame (`cull_blocks`):
  a sphere around each block, in f64 against four side planes, widened by
  the point footprint.

## Precision handoff and multiple systems

- Batch positions are f32 offsets from their block centre, so their error grows
  with that offset. Within `PRECISE` times it of the camera, a star is drawn
  instead as a precise sprite, from a per-frame buffer of offsets from the eye
  formed in f64 (gathered at `PREPARE` times the radius). The batch point remains
  until a quarter of that radius, so both overlap.
- The visited star is a body only while the camera is at it (`scene::body_range`,
  where planets show): batches hide only its point, matching the packed offset
  exactly, and it has no sprite. Other stars inside body range keep the normal
  precision handoff. Candidates store numeric cell/component/index identity,
  not allocated ID strings, and exclude stars too faint anywhere within the
  rescan margin. Magnitude visibility constants are calculated per scan/frame,
  not per star.
- A multiple system is one point until its stars could appear `SPLIT_PX` (0.5)
  pixels apart, inside the point spread, so the split never shows. From there
  to arrival it is drawn star by star as precise sprites from `star_positions`
  (`member_position` for every star at once, each pair's orbit solved once)
  at the clock's time in `scene::star_frame`, exactly where bodies appear on arrival.
- A system whose widest-separation sphere is outside the view is neither
  generated nor placed. Generated systems stay cached; past `SPLIT_CACHE`
  systems beyond those split this frame, the cache drops only ones not split
  this frame, so crowded views never rebuild every frame.
- `StarInstance` packs temperature and log2 separation as half floats (still
  20 bytes), and both star shaders hide a batch point whose separation spans
  more than the draw's angle (`near.z`) within `near.w` parsecs; CPU/shader
  decode the same value.
- `StarCell::resolved` finds candidates through a per-block pyramid of widest
  separations (`StarCell::widest`), within a reach that halves whenever a gather
  would measure over `SPLIT_BUDGET` stars. Crowded regions split only nearby
  systems. Movement margin shrinks with reach; at most four attempts run per gather.
- Overload temporarily keeps combined points but retains a positive retry reach.
  Cheap searches expand again on 250 ms retries even without movement;
  streaming restarts reset this adaptive state. Empty searches also expand,
  even if a coarse grid leaf measured many stars outside the search radius.
  Disabling splitting or galactic points stops split searches; re-enabling
  starts a fresh search. Examined counts include retries.
- The perf tab's collapsible `split systems` section measures search CPU time
  per search/frame, searches and examined work per second, preparation CPU
  time per frame, cache-miss rebuilds and capacity prunes, plus cached systems,
  submitted sprites and culled systems. Timings use trailing-second averages
  and are in `StreamSample` for reports. Preparation excludes GPU uploads/rendering.
- The UI also shows latest-frame and trailing-second peaks for search and
  preparation, uncached generation, orbital/sprite construction and rebuild
  counts. Generation and orbit timings are parts of preparation, not extra
  costs to add to it. `split_multiples` is in the performance-switches section.

## Point light and central black hole

- Outside splitting reach, a system is a galactic point under the same magnitude
  limit. Points use apparent flux, scaled by `sky::radiance_gain`. Their colour
  (and, realistic, visual efficiency) comes from 32-entry temperature tables in
  shader uniforms: a blackbody palette, or `spectral_colour` when not realistic.
  Which stars load still follows bolometric luminosity.
- Every point is a 0.55 px sigma Gaussian, shared with body sprites
  (`render::PSF_SIGMA_PX`), with a 2 px footprint widened up to 12 px when the
  Gaussian shows beyond it. Brightness shows linearly in the peak and above
  white in glare (`hdr.wgsl`). The former logarithmic point knee remains only
  in wireframe mode. Crowds are never widened or blurred.
- `render/sky.rs` contains only fixed unit conversions and a fixed zero-stop
  reference (the former dark-space calibration), no `Sky` resource, view meter
  or adaptation. `exposure::gain` is `2^stops / reference_radiance`;
  `sky::radiance_gain` uses the same gain for points/glow. Location, selection
  and streaming never adjust exposure.
- `render/black_hole.rs` draws an opaque sphere at the central black hole's
  physical Schwarzschild radius; it is not visible from home. No accretion disc
  or lensing yet.

## Diffuse galaxy glow

- `simulation::glow` builds smooth, population-normalised Gaussian volume patches
  for the first galaxy-glow milestone, with spiral modulation averaged out only
  in glow. A stratified, distance-indexed RGB luminosity table statistically
  excludes stars visible at the magnitude limit (`unresolved_table`, recomputed
  by `render/glow.rs` when it changes). This is approximate at finite patch/table
  resolution.
- Patch grids reach past `FLOOR` (1e-5 of each component's densest patch), so
  profiles fade rather than stopping at a grid edge (tested). The nuclear cluster
  is four Gaussians fitted to its Plummer profile.
- The renderer batches patches spatially and `glow.wgsl` integrates each ellipsoid
  analytically, without a full-galaxy raymarch loop, widening each to at least
  half a glow pixel (keeping its light) so sub-pixel patches never flicker.
  Perspective bounds and view-cone rejection limit overdraw.
- Every point inside the grid sits within about 50 patches per component, each
  covering the whole view, so cost is pixel fill. A separate glow camera (layer
  30, order -1, no MSAA) draws into a half-float image at 1/8 of view resolution
  per axis (`DOWNSCALE`, chosen by comparing full to 1/32).
- `glow_composite.wgsl` scales it up additively at the far plane with a cubic
  B-spline (four bilinear taps), not plain bilinear. Patch light is visual
  (eye-weighted) radiance times `sky::radiance_gain`, matching points/bodies
  under manual exposure.
- The image redraws only when eye, axes, field of view, size or (beyond 2x)
  exposure change by more than a quarter glow pixel, plus `SETTLE` frames and
  while any pipeline compiles; otherwise the composite rescales by exposure ratio.
  Camera-relative origins are formed in f64 before converting to f32. Patches
  are built lazily and hidden when disabled.
- Glow is shown only with realistic lighting and **Smooth galaxy glow (prototype)**
  enabled (render tab), alongside Inside, Face-on and Edge-on reference camera
  commands on both clients. `node scripts/test-galaxy-glow.mjs` saves comparison
  screenshots and on/off timings to `data/logs/`, on the secondary monitor.
- Dust, detailed spiral structure, other galaxy types and galaxy rotation are
  not implemented. Glow is a smooth prototype, not a dust-attenuated renderer.
