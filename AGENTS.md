# Task Space

## Scope and architecture

- Native Linux and browser/WASM clients. Rust workspace: `client` (Bevy/egui), `server` (Axum),
  `shared` (wire models), `simulation` (engine-independent orbital maths).
- Lists are constellations of system pages (7 to 14 slots each); tasks are planets. Deep navy background (black with realistic lighting) and line-based
  wireframe bodies. Preserve real distances, with explicit visual size exaggeration.
- Keep ClickUp access read-only until a write workflow is explicitly requested.
- Credentials belong only in the backend. Do not expose them in API models or logs.
- All runtime files belong in project-local `data/`; compiled output in `target/`.

## Development

- `./scripts/run.sh`: build and start both processes, clean up backend on exit.
- `./scripts/check.sh`: formatting, unit tests and strict Clippy checks.
- `./scripts/run.sh --smoke-test`: desktop rendering check with seventeen screenshots, including a 50-task pagination fixture, mid-flight captures of a list jump and a page jump, and six realistic-rendering captures (assisted, physical, lit planet close-up, constellation, star close-up in both exposures).
- Client logs live in `data/logs/client.log`, with `client.previous.log` for the last
  run and `crash-*.log` for panics. The renderer's local surface-timeout patch is
  documented in `vendor/README.md`. Keep it in step with Bevy upgrades.
- The native **Save bug report** control captures state, recent client log lines and
  a screenshot under `data/logs/bug-*` for visual bugs without a panic.
- `./scripts/build-web.sh`: WebGL 2 WASM build into `web/dist/`.
- The browser is the main target; native is a dev fallback. The browser UI is
  HTML/CSS panels (`web/ui/`); `?ui=egui` shows the egui panels instead. Native
  keeps egui. When HTML panels are active, egui draws only canvas markers, labels
  and clicks; the HTML shows captions, readouts and the loading state.
  Every panel action is an ID-based `UiCommand` (`actions.rs`), shared by egui and
  `web_bridge.rs`; Rust reports back with `space:galaxy`/`space:state`/`space:tick`
  window events. Canvas labels and clicks stay in egui on every target.
- `./scripts/dev-web.sh`: web build plus server with `web/` linked live, so
  HTML/CSS edits need only a refresh. `node scripts/test-html-ui.mjs` checks the
  HTML panels, keyboard focus and wheel routing against `SPACE_URL` (default
  local). `test-browser.mjs` covers the egui panels on the web.
- `./scripts/deploy-homelab.sh`: build locally and deploy to the homelab's
  `~/homelab/task-space`. URL: `http://192.168.1.90:4321/`. Demo data only
  by default; `--with-clickup` publishes credentials and cached tasks, so only use
  it when the user explicitly asks.
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
  `Navigation` and orbital positions use the page's plane axes;
  `scene::page_frame` rotates them into galactic axes, which `CameraPose` and star
  offsets use. Each page star has its own stable position (`views::page_position`)
  and page changes use the same `Flight` as list jumps; no page-swap animation.
  Raw metres push clip-space values to ~1e13 and the gizmo line shader then drops
  or fragments segments.
- Pick wireframe line density from on-screen radius; dense rings on small bodies
  merge into blobs.
- Integrate simulation time. Never multiply lifetime elapsed time by a mutable rate.
- Keep orbital identity stable across API reordering and edits. SQLite slots are
  append-only per list, including when tasks disappear from a snapshot.
- Changing task status or deadline must not change orbital elements.
- Planets and tasks are separate: a `Planet` is a generated body with a stable
  ID (its star's ID and orbit index) and an optional `Task`. Select and follow
  planets by index into `StarSystem::planets`; find tasks with
  `StarSystem::tasks`/`task_planet`. Task data (status, assignees, URL) never
  lives on the body.
- Generation is keyed by star ID. A list page's star is `star_id(list_id, page)`;
  `space_simulation::star(id)`, `planetary_system(id)`, `planet_count(id)` and
  `plane_tilt(id)` take any star ID. Changing a key moves every body it seeds.
- Task pages are filled in order from append-only slots; each page's capacity is
  `page_capacity(list_id, page)`. A page's whole planetary system is generated at
  once: masses, radii and orbits follow the snow line, neighbours stay Hill-stable
  and some pairs are resonant. The slot's index within its page picks its body.
  Every page with a task gets all its planets, tasked or bare; a list without
  tasks shows page 0's. `place` builds this from tasks and `shape` repeats it
  from a system's own tasks, on the server and again on the client, so fixtures
  need only slots. Table layout is presentation-only; leaving it restores current
  simulated positions. The table keeps physical radii.
- Each page has its own star (`space_simulation::star(&star_id(list_id, page))`),
  drawn in solar-neighbourhood proportions (mostly red dwarfs, a few giants and
  white dwarfs). Its mass sets orbital periods (`StarSystem::mass(page)`), its
  radius and temperature colour (`spectral_colour`) are drawn, and it shapes
  planet formation. Stars and planets carry a `Spin`; planets and star spins
  draw from their own seeds, so they never move an orbit. Planet mass is capped
  at 0.4% of the star's, or Hill spacing cannot converge.
- Constellation view never moves stars: it keeps the current page's frame, fits
  all page stars from the current angle and draws links between them.
- Each page has its own deterministic orbital-plane tilt (`page_tilt(list_id, page)`),
  including pages in one constellation. Page stars are scattered at random at
  real local star density (`views::page_position(list_id, page)`); never lay them
  on a grid, cluster or plane.
- Explain demo, live, cached and failed-sync states in the interface.

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
  worker thread natively, a 4 ms slice per frame in the browser, into the
  `Starfield` resource. About 1.06 million stars in 175 cells near home: 2.3 s
  native, about 8 to 14 s in the browser. `space-simulation` builds at
  opt-level 3 in dev, or loading and tests crawl.
- `starfield.rs` draws one oversized triangle per visible star in mesh batches
  of at most 4,096 source stars (never an entity per star). Star positions are
  local to the owning cell; per-frame transforms subtract the camera in f64,
  then scale to render units. Filter points beyond 1.25 times their apparent
  magnitude reach before upload; the shader applies the exact cutoff. Rebuild
  retained cells after the camera travels 0.2 of the tier's reach. New entities
  start hidden for one frame, avoiding Bevy's material specialization race.
  Galactic points use apparent flux; `sky_dimming` also stops their display
  brightness down with the close-star physical exposure (as it does the glow).
- `diffuse.rs` adds an approximate, faint ray-integrated disc and spiral-arm
  glow behind the points, with no dust or extinction. The central black hole
  is an opaque sphere at its physical Schwarzschild radius; it is not visible
  from home. No accretion disc or lensing yet.
- `galaxy_nav.rs` owns the galaxy overview, selected cell-star ID and optional
  generated `StarSystem`. It never adds generated systems to the ClickUp
  `Universe`. The overview uses the home origin; entering a star changes the
  camera frame to that star's position in f64 and builds task-free planets
  from its ID. The same `Flight` shifts the displayed pose across frames.
  G enters/leaves galaxy mode, F picks a visible star at the view centre,
  Enter visits it, Backspace returns to overview. Scroll zooms as far as
  40,000 ly in the overview; movement scales with the displayed distance.

## Realistic rendering

- Optional (`Navigation::realistic`); the wireframe bodies are the default and
  must stay unchanged when it is off (galactic stars appear in both modes).
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
  one linear exposure per frame; it stops down as a star disc fills the view
  (`Exposure::adapt`), so a close star shows its surface.
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

- No N-body simulation, moons, workspace discovery or OAuth.
- Galaxy travel and visits to generated systems work, but dust, lensing,
  rotation and mapping ClickUp lists onto galactic stars are not implemented.
  The glow is approximate, not a radiative transfer simulation.
- The client draws the active system procedurally with gizmos. Per-body ECS
  entities and GPU batching can be introduced when dataset size warrants them.
- Live ClickUp access needs the user's own `.env`; demo data is the default.
