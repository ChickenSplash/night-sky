# Night Sky

A Rust space simulator, available natively and in a browser, that also keeps track of lists of tasks. Each list is a constellation of systems with at most ten tasks each;
each task is a wireframe planet. Deep blue space, physical distances, Keplerian
orbits, and a practical task inspector.

## Run

### Browser on the homelab

Open **http://192.168.1.90:4321/** from your home network.

The Bevy client runs as WebAssembly with WebGL 2 in the browser. Your homelab
serves the files and runs the Rust API; the viewing device does the rendering.
Use a current desktop browser with hardware acceleration enabled. WebGL 2 works
over the LAN HTTP URL, so this deployment does not require WebGPU or HTTPS.

Deployment files live at
`/home/ecorreia/homelab/night-sky` on the homelab. Docker restarts the
`night-sky` container automatically after a reboot. The service is published
on the homelab's LAN address, and publicly at
**https://space.chickensplash.dpdns.org** through the Cloudflare tunnel on the
`edge` network.

To rebuild and redeploy from this project:

```sh
./scripts/deploy-homelab.sh
```

This builds on the development PC, transfers the browser bundle and backend over
SSH, and restarts the container. The homelab serves the built-in demo data. The
script also removes any `.env` and task cache left there by the former ClickUp
integration.

After SSHing into the homelab, view logs with:

```sh
docker logs --tail 50 night-sky
```

### Native desktop

```sh
./scripts/run.sh
```

The first build downloads dependencies and compiles Bevy. Subsequent launches
reuse `target/`. Closing the window also stops the backend started by this script.
The data is a built-in demo: six fictional lists contain 473 stable tasks,
forming multi-star constellations.

Requirements: Rust via rustup, clang, LLD, pkgconf, Wayland/X11 development
libraries and a Vulkan-capable graphics driver. These are already installed on
the development machine. The pinned toolchain is declared in `rust-toolchain.toml`.

### Controls

| Action | Control |
| --- | --- |
| Look around from the camera | Left mouse drag |
| Roll around the viewing direction | Q / E |
| Orbit the camera target | Right mouse drag |
| Zoom | Mouse wheel |
| Translate camera and target | W/A/S/D |
| Move up / down | R / F |
| Move faster | Left Shift |
| Pause/resume orbital motion | Space |
| Clear selection and stop tracking | Escape |
| Inspect task | Click planet/marker or task index |
| Track planet at close range | **Follow this planet** |
| Return to orbital view | **Orbit** or **Frame orbital system** |
| Align ten task-planets in a top-down table | **Task table** |
| Frame the list's systems | **Constellation** |
| Browse system pages | **‹ / ›** in the inspector |
| Select a star, then visit it | Click it, then click again or press Enter |
| Select the star at the view centre | C |
| Jump to a list | Click it in the sidebar |

Search covers all systems. The simulation clock starts at an arbitrary epoch;
task deadlines always use wall-clock time. Changing simulation speed never
rewinds or jumps the orbit phase.

Table view aligns planets to the right of their star, with task labels below.
Planets and the star keep their physical radii, so a large star shows only its
right tip at the left edge. Column spacing is schematic, scaled to the page's largest planet.
Both presets automatically frame the available viewing area. Switch to **Orbit**
to restore physical positions at the current simulation time; camera movement is
available in Orbit.

Every jump, page change, preset and focus is a short camera flight (0.25 to
0.75 s) along van Wijk and Nuij's zoom-and-pan path, so long trips zoom out first
and keep the destination in view. Planets slide between their orbits and the
table positions during the flight. Camera input resumes when it lands. Changing page
flies between two distinct page stars using that same path and camera rotation.
Every star-to-star flight, whether to another page or another list, takes
0.65 s regardless of distance, so both have the same rhythm; only the peak
zoom-out grows with the distance. Close up, distant stars sit on a shell at
four camera ranges and join their true positions as the zoom reaches them,
so nothing jumps mid-flight. All page stars of every list are always drawn.
Search selects a task's page automatically. Pages use permanent slots, so edits,
API reordering and task removals never move surviving tasks between systems.
Partially filled pages remain; wholly empty pages are omitted from navigation.

## Backend and logs

The backend serves the web client and the demo galaxy. The client checks the
snapshot every two seconds; unchanged responses have no body, so large galaxies
are not downloaded and rebuilt on every poll. `data/` is excluded from Git.
Native development binds to `127.0.0.1:4321`; Docker binds internally to
`0.0.0.0:4321` and publishes only the homelab LAN address. There is no task
integration for now: ClickUp support was removed, to be revisited later.

Native backend errors are recorded in `data/server.log`; client events and
renderer warnings in `data/logs/client.log`, with the previous session in
`data/logs/client.previous.log`. Panics leave timestamped
`data/logs/crash-*.log` files with a backtrace, even from detached launches.
For visual bugs that do not crash, click **Save bug report** in the right panel.
It saves the current camera/list/page state and recent flight events to
`data/logs/bug-*.log`, plus a matching `.png` screenshot. Share both files when
reporting an issue. Delayed flight frames are noted in the client log.
Smoke tests write `data/logs/smoke-client.log` separately. Deployment errors
are in Docker logs. A recoverable NVIDIA/Wayland surface timeout skips a frame
and logs a rate-limited warning instead of crashing. The local renderer patch
is documented in `vendor/README.md`.
No star represents a catalogued astronomical object.

Local endpoints:

- `GET http://127.0.0.1:4321/health`
- `GET http://127.0.0.1:4321/galaxy`

The client also runs alone with `cargo run --locked -p space-client`. If no backend
is available it retains its last data, initially the clearly labelled demo.

## Physical model and rendering

- SI metres in calculations; AU and light years in the interface.
- Orbital positions are solved analytically from Kepler's equation in `f64`.
- Orbital period follows the semi-major axis and host star mass.
- No N-body interactions, collisions, relativistic effects or physical warp travel.
- `big_space` maintains the camera's grid-cell floating origin. Rendering subtracts
  the camera position in double precision before converting local vectors to GPU
  floats. Planet motion stays relative to its host star.
- The visited system is drawn in detail. Every other star, list pages included,
  is a streamed galactic point at its real position.
- Bevy's reverse-Z perspective depth and distance-adjusted near plane support the
  view's changing scale. This is not a logarithmic-depth-buffer implementation.
- Bodies use physical radii by default; selectable markers keep them visible.
  **Readable planet sizes** exaggerates radii in Orbit view, not positions or periods.
- Each page is a separate star system with a stable, independently random orbital
  plane (poles uniform over the sphere). The telemetry panel shows the current
  page's tilt. List pages occupy distinct, visible cell stars near galactic
  home, with their real generated positions and planetary systems. Constellation view moves nothing: it zooms out from the current
  angle until every page star fits and joins them with lines. Orbit and Task table
  retain physical orbital distances. Camera flights roll between planes; open
  space uses galactic axes.
- Each page is its own planetary system of 7 to 14 planets, generated as a whole
  from its star's ID. Tasks fill the planets in slot order; the rest stay bare,
  with catalogue names such as "Flight Operations 2c". Composition follows:
  rocky worlds and sub-Neptunes inside the snow line, giants beyond it, dwarf
  planets far out. Sizes run from Pluto to just past Jupiter. Neighbours sit at
  least ten mutual Hill radii apart without crossing orbits, and some pairs lock
  into resonances (2:1, 3:2, 4:3, 5:4, 5:3, 3:1).
- Demo page stars follow the solar neighbourhood census: about 72% red
  dwarfs, 12% orange, 7% yellow, 5% white dwarfs, 3% F, and rare A, B and giant
  stars. Wireframes use the star's radius and spectral colour (deep red to blue by temperature).
- Stars and planets rotate about tilted axes. Close-in planets are tidally
  locked, some near the locking distance settle into a Mercury-like 3:2
  spin-orbit state or a slow Venus-like spin, and a few are knocked far over like
  Uranus.
- Wireframes use latitude/longitude line geometry, not native polygon wireframe
  mode. Decorative background points are not additional lists.
- A Milky Way-like galaxy model (`crates/simulation/src/galaxy.rs`) holds 200
  billion stars: a nuclear cluster around a 4.3 million solar-mass black hole,
  a bar and bulge, a thin disc with four spiral arms, a thick disc and a halo.
  Near home it gives about 0.18 stars per cubic parsec. Star types follow each
  region's age: young arms have O and B stars and supergiants; the old bulge
  and halo have no hot stars left and more white dwarfs.
- Its stars are generated cell by cell (`crates/simulation/src/cells.rs`), so
  none are stored. An octree hands exactly 200 billion stars down from the root
  in seeded draws, and each brightness tier lives in cells of its own size:
  bright stars in large cells that can be loaded from far away, faint ones in
  small cells nearby. Any cell gives the same stars every time.
- The client streams the cells around the camera down to naked-eye magnitude
  (about a million stars near home, most visible first) on a worker thread
  natively and in a dedicated Web Worker in the browser (with a 4 ms per-frame
  fallback if the worker fails), and drops cells
  left behind. Batched point meshes show stars actually bright enough to see,
  in both the wireframe and realistic views. The central black hole is modelled at
  its physical horizon radius. There is no dust or gravitational lensing yet.
- **Smooth galaxy prototype:** with **Realistic lighting** (the default) on,
  use **Inside**, **Face-on** or **Edge-on** reference views. **Smooth galaxy glow
  (prototype)** switches the diffuse contribution on/off without changing stars.
  Cached, spatially batched Gaussian volume patches represent unresolved stellar
  light. Their line integrals are analytic, not full-galaxy raymarching. The
  glow renders at an eighth of the view's resolution per axis, only when the
  view changes, and is scaled up into the main HDR view. Individual stars and bodies
  remain unchanged. Under physical exposure (the default), bodies, stars and
  glow share one camera exposure in eye-weighted light. It adapts like an eye
  to the whole view, metered from the galaxy model, so the galaxy fades beside
  a sunlit planet and does not darken as stars stream in. The disc glow deliberately averages spiral modulation.
  Component populations are conserved across patch resolutions. A sampled RGB
  luminosity table excludes magnitude-visible stars statistically; finite patch
  sizes and table interpolation remain approximations, not exact per-star removal.
  Dust, small-scale structure and external-galaxy types are later milestones.
  `node scripts/test-galaxy-glow.mjs` checks the three reference
  views and saves on/off screenshots and frame timings under `data/logs/`.

- **One galaxy (Phase 6):** lists and generated stars share one view. Scroll
  out for kiloparsec-scale travel; WASD and R/F move, Shift accelerates. Click
  any visible star to select it, then click it again (or press **Enter**) to
  visit its planetary system. **C** selects the star at the view centre. A star
  that carries a list page opens that page with its tasks; any other shows its
  generated planets, with the same orbit, table and constellation views.
  Generated stars have stable `TS <cell>-<component>-<index>` catalogue names
  and lettered planets. Picking uses only currently loaded, magnitude-visible
  stars; wait for streaming if a target is not selectable.

## Project structure

```text
crates/client/       Bevy scene, controls and the egui UI
crates/server/       Axum API serving the demo galaxy and web client
crates/shared/       Serializable API models and offline demo
crates/simulation/   Orbital mechanics, clock, galaxy model and deterministic layout
assets/             Reserved for project assets
scripts/            Launch and verification commands
data/               Local database, logs and smoke-test screenshots
target/             Compiled output
web/                Browser page, star worker entry point and generated dist/
deploy/             Docker configuration and generated deployment bundle
tools/              Project-local build/test tools (ignored by Git)
vendor/             Documented Bevy renderer surface-timeout fix
```

Project files, data and compiled output stay here. Rust toolchains and Cargo's
dependency download cache use the normal user-level locations. To isolate future
downloads too, set `CARGO_HOME="$PWD/.cargo-home"` before running Cargo.

## Development checks

```sh
./scripts/check.sh
./scripts/run.sh --smoke-test
./scripts/build-web.sh
cargo clippy --locked -p space-client --target wasm32-unknown-unknown -- -D warnings
node scripts/test-browser.mjs
node scripts/test-star-worker.mjs
```

The graphical smoke test requires a working desktop session. It exercises the
system view, close tracking, a zoomed-out galaxy, interstellar jumps (one captured mid-flight),
a 50-task table fixture, its last page, constellation framing, restored orbits
and a second orbital page (one captured mid-flight), then realistic rendering in
assisted and physical exposure, a lit planet close-up, a constellation and a
star close-up in both exposures. Each capture waits for camera flights to finish. It saves seventeen
screenshots under `data/`
and exits automatically. Unit tests cover physical orbit invariants, clock
continuity, the galaxy's star count, profile and regional star mix, cell counts
and repeatable cell stars, streaming in any slices, persistent orbital-slot allocation, table layout and camera flights.
Set `SPACE_SMOKE_TASKS=1400` to exercise a 140-page constellation instead of
the default 50-task fixture.

The browser smoke test uses a locally installed Chromium and
`npm install --prefix tools/browser playwright-core`. It defaults to the homelab
URL; override `SPACE_URL` or `SPACE_CHROMIUM` when needed. It checks browser boot,
navigation, resize and private-file isolation, and saves `data/web-*.png`.
It intercepts the browser's galaxy response with a 50-task fixture to exercise
table pagination and constellation presets without changing backend data. It
clicks the egui panels and checks each result through the `window.space` test
hook.

## User interface

The browser is the main client; the native window is a developer fallback that
adds the smoke test and bug reports. Both use the same egui panels, drawn in the
same frame as the 3D view. The left sidebar holds collapsible sections: lists,
the task index (filter, sort and group by status), the visited system and the
selected planet. Time controls run along the bottom. The round ⚙ button at the
top right slides in a terminal-style settings window: display, rendering, field
of view, controls and the star-stream benchmark. Esc or `[ × ]` closes it.

## Next milestones

- Smooth jump/focus transitions, camera collision limits and better label placement.
- Real-data validation with your chosen lists and larger task counts.
- Workspace/list discovery UI, task filters and explicit refresh controls.
- Subtask moons and richer task details.
- Browser performance on larger lists, responsive UI and packaged native releases.
