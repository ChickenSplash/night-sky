# Task Space

## Scope and architecture

- Native Linux and browser/WASM clients. Rust workspace: `client` (Bevy/egui), `server` (Axum),
  `shared` (wire models), `simulation` (engine-independent orbital maths).
- Lists are constellations of system pages (7 to 14 slots each); tasks are planets. Deep navy background and line-based
  wireframe bodies. Preserve real distances, with explicit visual size exaggeration.
- Keep ClickUp access read-only until a write workflow is explicitly requested.
- Credentials belong only in the backend. Do not expose them in API models or logs.
- All runtime files belong in project-local `data/`; compiled output in `target/`.

## Development

- `./scripts/run.sh`: build and start both processes, clean up backend on exit.
- `./scripts/check.sh`: formatting, unit tests and strict Clippy checks.
- `./scripts/run.sh --smoke-test`: desktop rendering check with sixteen screenshots, including a 50-task pagination fixture, mid-flight captures of a list jump and a page jump, and five realistic-rendering captures (assisted, physical, lit planet close-up, constellation, star close-up).
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
- Task pages are filled in order from append-only slots; each page's capacity is
  `page_capacity(list_id, page)`. A page's whole planetary system is generated at
  once (`space_simulation::planetary_system`): masses, radii and orbits follow the
  snow line, neighbours stay Hill-stable and some pairs are resonant. The slot's
  index within its page picks its body. `shape` applies this on the server and
  again on the client, so fixtures need only slots. Table layout is
  presentation-only; leaving it restores current simulated positions. The table
  keeps physical radii.
- Each page has its own star (`space_simulation::star(list_id, page)`), drawn in
  solar-neighbourhood proportions (mostly red dwarfs, a few giants and white
  dwarfs). Its mass sets orbital periods (`StarSystem::mass(page)`), its radius
  and temperature colour (`spectral_colour`) are drawn, and it shapes planet formation. Stars and
  planets carry a `Spin`; planets and star spins draw from their own seeds, so
  they never move an orbit. Planet mass is capped at 0.4% of the star's, or
  Hill spacing cannot converge.
- Constellation view never moves stars: it keeps the current page's frame, fits
  all page stars from the current angle and draws links between them.
- Each page has its own deterministic orbital-plane tilt (`page_tilt(list_id, page)`),
  including pages in one constellation. Page stars are scattered at random at
  real local star density (`views::page_position(list_id, page)`); never lay them
  on a grid, cluster or plane.
- Explain demo, live, cached and failed-sync states in the interface.

## Realistic rendering

- Optional (`Navigation::realistic`); the wireframe look is the default and must
  stay unchanged when it is off. Photometry lives in `space_simulation::light`;
  exposure, the pools and the HDR camera in `crates/client/src/render.rs`,
  with the shaders in `body.wgsl` and `point.wgsl`.
- Bodies are plain spheres under the wireframes. `scene::draw` gathers `Lights`
  while drawing gizmos; `render::sync_bodies`/`sync_points` place pooled mesh
  entities in the camera's cell and write `GlobalTransform` directly, because
  they run after propagation.
- Colours reach the GPU pre-exposed on the CPU. Never send raw radiance: star
  surfaces reach 1e10 W m⁻² sr⁻¹ and HDR targets are half-float.
- Star colours are blackbody spectra white-balanced to the Sun. True colours make
  red dwarfs orange; assisted exposure saturates them (`light::saturate`) so they
  read red. Brightness is visual, not bolometric.
- Assisted exposure (default) compresses contrast around the planets, exposes the
  star field on its own and floors planet brightness. Physical exposure is one
  linear exposure per frame.
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
  (`Frame::coronas`, `point.wgsl`) at a millionth of the disc when physical.
- Cameras sharing the window must agree on `Hdr`: the egui overlay camera switches
  with the 3D camera (without tone mapping), or it presents a stale frame.
- The 3D camera carries `NoIndirectDrawing`: GPU-driven culling drops every mesh
  with its 1e22 far plane. WebGL 2 never draws indirectly anyway.

## Current limitations

- No N-body simulation, moons, workspace discovery or OAuth.
- The client draws the active system procedurally with gizmos. Per-body ECS
  entities and GPU batching can be introduced when dataset size warrants them.
- Live ClickUp access needs the user's own `.env`; demo data is the default.
