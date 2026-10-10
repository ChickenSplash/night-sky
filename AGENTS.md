# Night Sky

Formerly Task Space. The repo is `ChickenSplash/night-sky`, the project folder
`~/Projects/Night-Sky`, and the homelab container and path `night-sky`.

## Scope and architecture

- Native Linux and browser/WASM clients. Rust workspace: `client` (Bevy/egui), `server` (Axum),
  `shared` (wire models), `simulation` (engine-independent orbital maths).
- Lists are constellations of system pages (7 to 14 slots each); tasks are planets. Deep navy background (black with realistic lighting) and line-based
  wireframe bodies. Preserve real distances, with explicit visual size exaggeration.
- A space simulator first. Lists, pages and tasks stay as a general way of
  tracking information; the ClickUp integration was removed and will be
  re-approached later. The backend serves only the built-in demo galaxy.
- Any future credentials belong only in the backend, never in API models or logs.
- All runtime files belong in project-local `data/`; compiled output in `target/`.

## Development

- `./scripts/run.sh`: build and start both processes, clean up backend on exit.
- `./scripts/check.sh`: formatting, unit tests and strict Clippy checks.
- `./scripts/run.sh --smoke-test`: desktop rendering check with seventeen screenshots, including a 50-task pagination fixture, mid-flight captures of a list jump and a page jump, and six realistic-rendering captures (assisted, physical, lit planet close-up, constellation, star close-up in both exposures).
- Client logs live in `data/logs/client.log`, with `client.previous.log` for the last
  run and `crash-*.log` for panics. The renderer's local surface-timeout patch is
  documented in `vendor/README.md`. Keep it in step with Bevy upgrades.
- The native **Save bug report** control (debug window, `tools` tab) captures
  state, recent client log lines and a screenshot under `data/logs/bug-*` for
  visual bugs without a panic.
- `./scripts/build-web.sh`: WebGL 2 WASM build into `web/dist/`.
- The browser is the main target; native is a dev fallback. Both use the same
  egui interface (`ui.rs`), drawn in the same frame as the scene, which fills
  the whole window: there is no sidebar. Two free-floating halo icons at the
  top right (`ui::icons`, ⚙ above ✔, `breadcrumb::halo_glyph`, hover blue,
  white while open) toggle two plain egui windows that first open just below
  them, right-aligned: Settings, and Tasks (`tasks.rs`, 01 lists and 02 task
  index, with the data-source note). Lists and tasks are kept apart there as
  the future integration point; the rest reaches them through `UiCommand`s.
  Both start closed. The debug window opens below the icons
  (`terminal::below_icons`). Body facts, presentation modes and system paging
  are in the floating breadcrumb inspector and its actions; there is no
  telemetry section, centre-star pick or C key.
- The breadcrumb (`breadcrumb.rs`) is free-floating text at the top left of
  the 3D view, under any window, with no container. A soft dark halo (two
  rings of translucent black copies, `HALO_EDGE` at 1 px and `HALO_FADE` at
  2 px) keeps it readable over star discs and the galaxy band; it vanishes
  on dark space. Reuse `halo_text`/`halo_wrapped_text` for scene text.
  `breadcrumb::trail` is physical location: `Milky Way`, the current star's
  list if any, the star, then `Navigation::location_planet`. An open list or
  destination is not a location until arrival. `selection_trail` independently
  shows the selected object's hierarchy, even for a remote planet. List stars
  are `System n`, other stars use their catalogue names. Crumbs carry stable
  `objects::ObjectRef`s, never camera commands. Clicking either trail sends
  `InspectObject` and activates a highlighted tab in the shared, borderless
  `inspector.rs` readout below both trails. Selection automatically activates
  its leaf. The inspector uses its natural content height, with no scroll area,
  scrollbar or bottom-HUD height reservation; long information uses the left
  side of the view. Its top stays anchored at the shared HUD inset.
  Inspection changes neither selection nor camera/tracking. Missing
  tabs fall back to the selected leaf, or current location with no selection.
  Inspector facts are cached until target/snapshot changes; generated remote
  systems are never rebuilt per frame. Below the facts, `inspector::actions`
  shows the inspected object's actions as clickable halo text separated by
  `·` (hover blue, `CURRENT` when already showing it): galaxy `View from
  above`; list `Constellation view`, `Previous system`/`Next system` and
  then one `System n · k tasks` line per page. These act exactly like
  clicking the star (`PickStar`: select, or visit once selected; list stars
  resolve unstreamed through `page_pick`), relative to the selected star in
  that list, else the open list's page. They re-send `InspectObject`, and
  `breadcrumb::holds` keeps a list's tab while that list is open, so the
  inspector stays on the list while stepping or flying; star `Fly to system`/`Frame system`,
  `Table view` and, for list stars, `Constellation view`
  (`UiCommand::VisitStar`); planet `Fly to planet` (`FlyToPlanet`, visiting
  its star first, then following, like a second click). Stars are reachable
  by ID if mapped, visited or streamed (`galaxy_nav::reachable`/`visit_star`);
  unreachable actions are hidden, not disabled. New object kinds should extend
  `ObjectRef`, hierarchy builders and inspector resolution (moons do not yet
  exist). `UiCommand::Up` is separate keyboard navigation: reframe tracking,
  else clear selection, else move up physically; no crumb contains travel code.
- Time controls (`ui.rs`) are two free-floating, centred lines at the bottom
  of the 3D viewport, using the breadcrumb font, colours and `halo_text`.
  Elapsed simulation time sits above the actual rate (two decimal places,
  seconds/minutes/hours/days/years per real second). Click the rate to toggle
  pause; drag it horizontally for continuous logarithmic adjustment from real
  time to 100 years/s (400 logical pixels covers the full range). Dragging
  preserves pause state, with `· paused` beside the retained rate. There is no
  bottom panel, preset ladder or separate speed editor.
  The bottom-left movement readout (`ui::speed_readout`) uses the same halo,
  font and two-line layout: measured velocity above draggable set speed.
  Horizontal dragging adjusts speed logarithmically from `scene::MIN_SPEED`
  to `scene::MAX_SPEED` (1 m/s to 10,000 ly/s), through `SetMoveSpeed`; it
  captures input like the time rate, and a single click does nothing.
  All three text HUDs share
  `breadcrumb::HUD_PADDING` (20 logical pixels from the 3D viewport edges);
  movement and time share `HUD_LINE_GAP` and their bottom line alignment.
  `ui::pointer_captured` protects the background-layer breadcrumb/inspector and draggable text and captures
  its full drag, including outside its bounds; scene camera input and canvas
  picking both use it so time interactions never also move/select the scene.
  While a mouse button is held, the HUD hit test uses its press origin, not
  hover position, so camera drags that begin outside the rate text continue
  uninterrupted when crossing it.
- Debug tooling stays out of the product UI, in `devtools.rs`: a floating,
  movable, resizable window toggled by ` or F3 on both clients (the public
  site included), with tabs `log` (an in-memory console of the last 2,000
  tracing events from `devtools::log_layers`, with level filter and grep; the
  tab shows errors and warnings logged while it was hidden), `terminal` (a
  command line and its output), `perf` (frame graph; star stream and star
  draw facts: draw calls, uploaded stars, vertices and vertex memory; natively,
  a GPU passes table of GPU/CPU time and vertex/fragment shader invocations
  per render pass from Bevy's `RenderDiagnosticsPlugin`),
  `render` (exposure readout, galaxy glow, physical exposure, wireframes over
  bodies, galaxy reference views, and performance switches), `state` (camera; navigation: location,
  open list, anchor, page, presentation mode, selection; anchor star; clock) and `tools` (bug report natively, star-stream benchmark in the
  browser). New debug readouts and switches go there, not in Settings or the
  canvas. The performance switches (`render_debug::RenderDebug`, not
  `Navigation`; `set_debug <switch> <on>`) are for A/B tests and reset on
  restart: freeze star streaming (the loader keeps its last aim), galactic
  star points, shade point pixels (off, points still run the vertex shader
  but are never rasterised), compact 1.25 px points, 4x MSAA (both window
  cameras) and bloom.
- Typed commands live in `command_line.rs`. The terminal tab and, natively,
  standard input (the terminal that ran `./scripts/run.sh`, or a pipe) send
  `Typed` messages; `command_line::run` parses them with `parse`
  (`set_presentation_mode table`, `set_display option=orbits on=false`, or `UiCommand`
  JSON) into the same `UiCommand`s as the panels, before `actions::apply`.
  `help` is built in; `clear` empties the terminal tab and is handled by the
  window. Echoes, help and errors are logged under `command_line::TARGET`
  (`console`), which is what the terminal tab shows, so they also reach the
  log file and the launching terminal. Any new input source should send
  `Typed` too. Every `UiCommand` needs a `command_line::USAGE` entry (tests
  compare it with serde's variant list and parse each entry, naming any
  mismatch).
- The debug window uses the terminal look (`terminal.rs`): a translucent
  black, Hyprland-like window with a light grey border and text-mode
  checkboxes and buttons.
- There is no HTML UI; the HTML/CEF prototypes were abandoned (branch
  `prototype-cef-overlay`). Every panel action is an ID-based `UiCommand`
  (`actions.rs`). Panel-only choices (list/task sort, status groups) live in
  `tasks::Tasks`, not `Navigation`.
- `automation.rs` (browser only) is a test hook, not a UI: `window.space.command`
  sends a `UiCommand` as JSON and `window.space.state()` reads a small snapshot
  (list, page, presentation_mode, display options, stream caption, benchmark results).
- `./scripts/dev-web.sh`: web build plus server with `index.html` and
  `star-worker.js` linked live. `node scripts/test-browser.mjs` clicks the egui
  panels at 1600 x 1000 and checks the results through `window.space`; update its
  coordinates when the panel layout changes.
- `./scripts/deploy-homelab.sh`: build locally and deploy to the homelab's
  `~/homelab/night-sky`. URL: `http://192.168.1.90:4321/`. Demo data only;
  it removes any `.env` and SQLite cache left there by the former ClickUp
  integration.
- Docker's restart policy manages the remote service, independently of SSH sessions.
- Keep native threading/reqwest and browser fetch/timers behind target-specific code.
- Browser requests use same-origin `/galaxy`. Never bake a LAN IP or token into WASM.
- Bevy, big_space and bevy_egui are pinned as a compatible set. Check compatibility
  before upgrading any one of them.
- No global desktop or Cargo configuration changes are needed.

## Invariants

- Use `f64` for physics and local camera positions. Subtract the camera before
  converting positions to `f32`; do not add interstellar offsets to GPU floats.
- Divide render positions and radii by `CameraPose::render_scale()` (displayed camera
  about 1,000 units from target). `Navigation` holds the destination; `CameraRig`
  flies the displayed pose there. Discrete camera changes set `Navigation::fly`.
  Free-look and roll update the camera's local quaternion; left-drag turns about
  its current screen axes and repositions the target to keep the eye fixed.
  Right-drag orbits the fixed target about the same screen axes. Every
  keyboard control (WASD, R/F travel and Q/E roll) eases through one
  `CameraMotion::thrust` (camera-axis travel as a fraction of `move_speed`,
  roll in `w`) over about 0.15 s; speed changes and mouse turns stay instant.
  The vertical field of view is `Navigation::fov_deg` (30° to 130°, default
  50°), set from settings (`UiCommand::SetFov`) or Shift + left-drag (up
  narrows). Hand projections (picking, overlays, table framing) use
  `Navigation::tan_half_fov`, never a fixed 50°.
  `Navigation` and orbital positions use the page's plane axes;
  `scene::page_frame` rotates them into galactic axes, which `CameraPose` and star
  offsets use. Each page star is a cell star at its own position
  (`PageStar::position_ly`), and page changes use the same `Flight` as list
  jumps; no page-swap animation.
  Raw metres push clip-space values to ~1e13 and the gizmo line shader then drops
  or fragments segments.
- Pick wireframe line density from on-screen radius; dense rings on small bodies
  merge into blobs.
- Integrate simulation time. Never multiply lifetime elapsed time by a mutable rate.
- Keep orbital identity stable across reordering and edits. Task slots are
  append-only per list, including when tasks disappear from a snapshot.
- Changing task status or deadline must not change orbital elements.
- Planets and tasks are separate: a `Planet` is a generated body with a stable
  ID (its star's ID and orbit index) and an optional `Task`. Select and follow
  planets by stable body ID (`Selection::Planet(String)`); resolve a local
  index only against the visited system with `selected_planet(galaxy)`. Remote
  `Inspect { planet }` resolves the encoded parent star without visiting or
  needing streamed meshes, ready for future search results. Find tasks with
  `StarSystem::tasks`/`task_planet`. Task data (status, assignees, URL) never
  lives on the body.
- Generation is keyed by star ID. A list page's star is `star_id(list_id, page)`;
  `space_simulation::star(id)`, `planetary_system(id)`, `planet_count(id)` and
  `plane_tilt(id)` take any star ID. Changing a key moves every body it seeds.
- Task pages are filled in order from append-only slots; each page's capacity is
  `planet_count(assigned_star_id)` for pages mapped to cell stars, or
  `page_capacity(list_id, page)` for legacy pages. A page's whole planetary system is generated at
  once: masses, radii and orbits follow the snow line, neighbours stay Hill-stable
  and some pairs are resonant. The slot's index within its page picks its body.
  Every page with a task gets all its planets, tasked or bare; a list without
  tasks shows page 0's. `place` builds this from tasks and `shape` repeats it
  from a system's own tasks, on the server and again on the client, so fixtures
  need only slots. Table layout is presentation-only; leaving it restores current
  simulated positions. The table keeps physical radii.
- Page-to-cell-star assignments live in the snapshot's
  `StarSystem::page_stars`, from `assign_page_stars`. They are unique and
  immutable per list/page; include removed tasks' allocated slots when deciding
  how many pages need a star. Mapped pages use the cell star's ID, position,
  formation and plane tilt, so visiting that star from the galaxy shows the same
  planets, with tasks as an optional overlay.
- Each legacy page (with no cell star) has its own star (`space_simulation::star(&star_id(list_id, page))`),
  drawn in solar-neighbourhood proportions (mostly red dwarfs, a few giants and
  white dwarfs). Its mass sets orbital periods (`StarSystem::mass(page)`), its
  radius and temperature colour (`spectral_colour`) are drawn, and it shapes
  planet formation. Stars and planets carry a `Spin`; planets and star spins
  draw from their own seeds, so they never move an orbit. Planet mass is capped
  at 0.4% of the star's, or Hill spacing cannot converge.
- Constellation view never moves stars: it keeps the current page's frame, fits
  all page stars from the current angle and draws links between them.
- Each page has its own orbital-plane tilt (`scene::page_tilt`, its star's
  `plane_tilt`), including pages in one constellation.
- Explain the data source (currently always the demo) in the interface.

## Galaxy model

- `space_simulation::galaxy::GalaxyModel` (parameterised, `milky_way()` by
  default) gives stars per cubic parsec at any point, per `Component`: nuclear
  cluster, bar and bulge, thin disc, young arm stars, thick disc and halo. Axes
  are parsecs from the central black hole, disc in XZ, Y to the north pole;
  home is 8.2 kpc out on +X, between arms.
- Every profile has a closed-form integral, so components hold exactly their
  share of `stars` (2e11). Spiral modulation averages to one around every
  circle; keep it that way, or the totals drift. The test integrates the
  density numerically to within 1%.
- Galactic stars (`galactic_star(id, component)`, `GalaxyModel::star(id, p)`)
  draw a Kroupa birth mass and an age from the component's formation history,
  then evolve (main sequence, giant or supergiant, white dwarf). Neutron stars
  and stellar black holes are redrawn: `stars` counts stars only. Their systems
  come from `galactic_planetary_system(id, component)`.
- Page stars (`star(id)`) keep the fixed solar-neighbourhood census until
  lists move onto galactic stars. Both share the builders in `star.rs`; census
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
- `space_simulation::stream`: `wanted(camera, magnitude)` lists, per tier, the
  level-t cells within `reach_pc`, most visible first. `Loader` builds them one
  unit at a time (one child box integrated, via `Sky::integrate`, or one star)
  and stops when the caller's `more()` says so, so no step blocks for long.
  `counts_need`/`place_need` report the masses a step still needs; they share
  `Sky::descend` with `place`, so sliced loading gives the same stars. Loaded
  cells drop beyond `KEEP` × reach. Stars stream as compact `StarPoint`s.
- `crates/client/src/stream.rs` aims the loader at the camera
  (`CameraRig::galactic_ly`, lists anchored at home) to `MAGNITUDE` 6.5: a
  worker thread natively, a dedicated module Web Worker in the browser, into
  the `Starfield` resource. The worker's generation-only WASM crate is
  `crates/star-worker`, with `web/star-worker.js` as its entry point; both WASM
  bundles are built by `scripts/build-web.sh`. `stream_wire` transfers cells in
  binary with f64 positions, and browser startup/worker errors fall back to
  the old 4 ms per-frame loader. Native loads about 1.06 million stars in 175
  cells near home in 2.3 s; the pre-worker browser took about 8 to 14 s.
  `space-simulation` builds at opt-level 3 in dev, or loading and tests crawl.
  `StreamMetrics` samples generation time, cancelled jobs, mesh backlog and
  wait, render-sync time and frame-time percentiles once per second; the
  debug window's `perf` tab shows them on both clients.
  Instrumentation does not change loading policy.
  `StarCell::new` (simulation `stream.rs`) sorts a cell's stars into a uniform
  grid of boxes (about 8 stars each, never under 0.5 pc), on the loader's
  thread, so `stars` is in grid order, not generation order. Build cells only
  through `StarCell::new`; changing `stars` afterwards leaves the grid stale.
  Spatial questions use the grid: `StarCell::nearest` (rings of boxes outward)
  behind `Starfield::nearest` for automatic arrival in `scene::locate` and the
  close-up render scale in `place_camera`, and `StarCell::near` (boxes within a
  radius) for precise-sprite `candidates`. Never scan `Starfield::cells` star
  by star per frame or per streaming change: the cells containing the camera
  hold tens of millions of stars at the galactic centre. The perf tab's
  nearest star section times the lookups.
  The browser debug window's **run star-stream benchmark** (`tools` tab) reloads into a fresh,
  empty starfield at home, then follows -X (towards the galactic centre) at
  1,000 ly/s for 15 s without auto-visits. `benchmark.rs` records each
  `StreamMetrics::sample`, logs the results to the console and shows them in an
  egui window with a JSON download. `?benchmark=star-flight` is the
  identical route used by `SPACE_HEADFUL=1 node scripts/benchmark-star-flight.mjs
  <label>`; it writes JSON results under `data/logs/`. Headless SwiftShader is
  too slow for meaningful comparisons, so use headed Chromium on Wayland.
- `starfield.rs` draws one oversized triangle per visible star in mesh batches
  of up to `PACK` (65,536) uploaded stars, filled from `SLICE`-sized steps of
  a cell's stars within the upload budget, so a large cell may take several
  frames. Keep batches few and large: Bevy's per-mesh-entity work
  (specialization, extraction, queueing) runs every frame, hidden or not, and
  17k small batches cost about 30 ms of CPU at the galactic centre. A rebuilt
  cell's new batches wait in `Batches::fresh` and replace the old ones only
  once complete, so rebuilds never leave holes. Star positions are
  local to the owning cell; per-frame transforms subtract the camera in f64,
  then scale to render units. Filter points beyond 1.25 times their apparent
  magnitude reach before upload; the shader applies the exact cutoff. Rebuild
  retained cells after the camera travels 0.2 of the tier's reach. New entities
  start hidden for one frame, avoiding Bevy's material specialization race.
  Batch vertices are f32 offsets from their cell's centre, so their error grows
  with that offset: within `PRECISE` times it of the camera a single-star sprite
  placed by an f64 subtraction takes over (built at `PREPARE` times the radius,
  so its hidden frame is spent). The batch remains until a quarter of that
  radius, and is not culled while the precise sprite is still unplaced. The
  visited star is a body only while the camera is at it (`scene::body_range`,
  where its planets show): batches hide points within that range and its
  sprite waits hidden. Elsewhere, it
  is a galactic point under the same magnitude limit. Galactic points use
  apparent flux, scaled by `sky::radiance_gain`. Realistic, their colour and
  visual efficiency come from a blackbody palette by temperature (the vertex
  corner attribute carries the corner index and temperature); which stars
  load still follows bolometric luminosity. Every point is a
  0.55 px sigma Gaussian (2 px footprint, a uniform so the compact debug
  switch can shrink it), as in a perfect camera: brightness
  shows only in its peak, linear up to a knee and logarithmic above it, never
  in its size. Crowds are never widened or blurred.
- `sky.rs` meters the view on the CPU (about 0.3 ms): the smooth galaxy model
  (coarse glow patches of every star, resolved or not, at their mean visual
  luminosity), so the exposure never changes as stars stream in, plus the
  visited system's stars in view. The average radiance in view shows at
  `SKY_BACKGROUND`. `Sky::key` is the physical key (radiance shown at display
  `KEY`), the brightest of that, the subject's and a filling star disc
  (`render::physical_key`), never below `dark_key` (a dark-adapted eye).
  `Sky::exposure` is the assisted/wireframe galactic multiplier, at most one.
  Both ease like an eye: about 0.1 s to a brighter view, 0.5 s to a darker.
  `sky::radiance_gain` is the one conversion from galactic light to display
  value, shared by points and glow.
- `black_hole.rs` draws an opaque sphere at the central black hole's physical
  Schwarzschild radius; it is not visible from home. No accretion disc or
  lensing yet.
- `simulation::glow` builds smooth, population-normalised Gaussian volume patches
  for the first galaxy-glow milestone, with spiral modulation averaged out only
  in the glow. A stratified, distance-indexed RGB luminosity table statistically
  excludes magnitude-visible stars. This is approximate at finite patch/table
  resolution. Patch grids reach past `FLOOR` (1e-5 of each component's
  densest patch), so profiles fade out instead of stopping at a grid edge
  (tested). The nuclear cluster is four Gaussians fitted to its Plummer
  profile. `client::glow` batches them spatially and `glow.wgsl` integrates
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
  and bodies under physical exposure. The image is redrawn only when the eye,
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
- There is one mode: the streamed galaxy. List pages are ordinary cell stars,
  drawn and picked like any other, with no markers of their own.
  `Navigation::visit` is the system the camera is anchored to (at, flying to,
  or showing as a constellation): `Visit::List` (a list index, at
  `Navigation::page`) or `Visit::Generated` (task-free planets built from the
  star's ID, never added to the lists' `Universe`). `None` is open space,
  relative to home in galactic axes; a visit moves the camera frame to its
  star in f64, and the same `Flight` shifts the displayed pose. Simulation, table
  and constellation presentation modes work on any visited system.
- `presentation.rs` owns `PresentationMode`: `Simulation` (simulated orbital
  positions), `Table` (comparison row), and `Constellation` (connected stars).
  `Navigation::presentation_mode` describes scene presentation, not tracking or
  free flight. Switching presets also sets camera framing. The terminal command
  is `set_presentation_mode simulation|table|constellation`; JSON commands use
  `type: "set_presentation_mode"` and `presentation_mode`, also the browser state
  key. Camera views and physical `Orbit` elements keep their existing names.
- Where the camera is and which list is open are separate:
  `Navigation::location` (the visit while `Navigation::arrived`) is physical,
  and `Navigation::list` is the open list (a `Visit::List`, kept in
  constellation view). `scene::locate` sets `arrived` every frame: within
  `system_range` of the page star (0.02 ly, or 4 times the outermost apoapsis
  if wider) it arrives, and it leaves beyond `LEAVE_FACTOR` (2) times that.
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
  `galaxy_nav.rs` picks and visits the selected star (`visit_selected`): a star with a list page (`galaxy_nav::mapped`) opens
  that page, any other its generated system. A new snapshot moves the visit
  with its star (`follow_visited_star`). Clicks not on a planet pick from
  loaded, visible points only, and only on click, never every frame. Click
  again or Enter to visit. The only bodies drawn are
  the visited star and its planets. Shift + scroll (either Shift key) zooms
  from 20,000 km to 40,000 ly; plain scroll adjusts movement speed independently.
- Unassigned cell stars use ID-derived `TS` catalogue names and lettered
  planets, with no name registry or per-star UI entities.

## Realistic rendering

- On by default (`Navigation::realistic`); the wireframe bodies must stay
  unchanged when it is off (galactic stars appear in both modes). The smoke
  test turns it off for its wireframe captures.
  `render::exposure_caption` (debug window, `render` tab) shows the key (and stops from `dark_key`), its
  target and which candidate set it (subject, view or star disc), and the
  view's galaxy/star split, from values `sky::expose` already computes.
  Photometry lives in `space_simulation::light`;
  exposure, the pools and the HDR camera in `crates/client/src/render.rs`,
  with the shaders in `body.wgsl` and `point.wgsl`.
- Bodies are plain spheres under the wireframes. `scene::draw` gathers `Lights`
  while drawing gizmos; `render::sync_bodies`/`sync_points` place pooled mesh
  entities in the camera's cell and write `GlobalTransform` directly, because
  they run after propagation.
- Body colours reach the GPU pre-exposed on the CPU. Never send raw radiance:
  star surfaces reach 1e10 W m⁻² sr⁻¹ and HDR targets are half-float. The
  galactic point shader instead computes bounded display light from luminosity
  and camera distance.
- Star colours are blackbody spectra white-balanced to the Sun. True colours make
  red dwarfs orange; assisted exposure saturates them (`light::saturate`) so they
  read red. Brightness is visual, not bolometric.
- Assisted exposure compresses contrast around the planets, exposes the star
  field on its own and floors planet brightness. Physical exposure (default) is
  one linear exposure per frame for bodies, galactic points and glow alike, at
  `Sky::key`; it stops down as a star disc fills the view (`Exposure::adapt`),
  so a close star shows its surface, and beside a sunlit planet the galaxy
  falls below display range, as in a real camera. Bevy's tone mapping is the
  frame's one tone curve.
- Bodies below `POINT_PIXELS` on screen are camera-facing point sprites, added
  over the frame (`AlphaMode::Add` needs zero alpha from the shader). A point's
  spread integrates to its whole light, exposed as if spread over a point-sized
  disc (`Exposure::point`), so it matches the sphere it replaces and physical
  stays linear. Point planets dim with phase (`render::phase`). Assisted floors
  apply to points, not to dim spheres.
- Close star discs have per-channel limb darkening and procedural granulation
  in `body.wgsl`, fixed to the star's spin. Granule size follows the pressure
  scale height (`render::granules`); patterns fade out before their cells reach
  a pixel, and hot stars have none. Assisted exposure eases large discs down to
  `DISC_KEY` so the surface survives tone mapping. Coronas are additive sprites
  (`Frame::coronas`, `point.wgsl`) at a millionth of the disc when physical;
  assisted, capped relative to `DISC_KEY`. Coronas come first in the sprite
  pool so they keep their entity.
- Cameras sharing the window must agree on `Hdr`: the egui overlay camera switches
  with the 3D camera (without tone mapping), or it presents a stale frame.
- The 3D camera carries `NoIndirectDrawing`: GPU-driven culling drops every mesh
  with its 1e22 far plane. WebGL 2 never draws indirectly anyway.

## Current limitations

- No N-body simulation, moons or task integrations (ClickUp was removed).
- Galaxy travel, mapped list page stars and visits to generated systems work,
  but dust, lensing and galaxy rotation are not implemented. Diffuse galaxy glow
  is a smooth prototype, not yet a detailed or dust-attenuated galaxy renderer.
- The client draws the active system procedurally with gizmos. Per-body ECS
  entities and GPU batching can be introduced when dataset size warrants them.
