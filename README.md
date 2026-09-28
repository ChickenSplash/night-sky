# Task Space

A Rust mission-control view of your tasks, available natively and in a browser. Each list is a constellation of systems with at most ten tasks each;
each task is a wireframe planet. Deep blue space, physical distances, Keplerian
orbits, and a practical task inspector.

## Run

### Browser on the homelab

Open **http://192.168.1.90:4321/** from your home network.

The Bevy client runs as WebAssembly with WebGL 2 in the browser. Your homelab
serves the files and runs the Rust API; the viewing device does the rendering.
Use a current desktop browser with hardware acceleration enabled. WebGL 2 works
over the LAN HTTP URL, so this deployment does not require WebGPU or HTTPS.

Deployment files, credentials and the database live at
`/home/ecorreia/homelab/task-space` on the homelab. Docker restarts the
`task-space` container automatically after a reboot. The service is published
on the homelab's LAN address, with no public tunnel route configured.

To rebuild and redeploy from this project:

```sh
./scripts/deploy-homelab.sh
```

This builds on the development PC, transfers the browser bundle and backend over
SSH, and restarts the container. By default the homelab serves demo data only:
the script removes any `.env` and task cache there, so visitors never see real
tasks. `./scripts/deploy-homelab.sh --with-clickup` publishes your credentials
and, on first deployment, the local SQLite cache. Credentials stay outside the
image and web directory.

After SSHing into the homelab, view logs with:

```sh
docker logs --tail 50 task-space
```

### Native desktop

```sh
./scripts/run.sh
```

The first build downloads dependencies and compiles Bevy. Subsequent launches
reuse `target/`. Closing the window also stops the backend started by this script.
No ClickUp credentials are needed for the demo. Its six fictional lists contain
473 stable tasks, forming multi-star constellations without any live data.

Requirements: Rust via rustup, clang, LLD, pkgconf, Wayland/X11 development
libraries and a Vulkan-capable graphics driver. These are already installed on
the development machine. The pinned toolchain is declared in `rust-toolchain.toml`.

### Controls

| Action | Control |
| --- | --- |
| Orbit the camera target | Right mouse drag |
| Zoom | Mouse wheel |
| Translate camera and target | W/A/S/D |
| Move down / up | Q / E |
| Move faster | Left Shift |
| Pause/resume orbital motion | Space |
| Clear selection and stop tracking | Escape |
| Inspect task | Click planet/marker or task index |
| Track planet at close range | **Follow this planet** |
| Return to orbital view | **Orbit** or **Frame orbital system** |
| Align ten task-planets in a top-down table | **Task table** |
| Frame the list's systems, click one to enter | **Constellation** |
| Browse system pages | **‹ / ›** in the inspector |
| Jump between systems | Click list in sidebar or a star marker |
| View light-year distances | **Interstellar atlas** |

Search covers all systems. The simulation clock starts at an arbitrary epoch;
task deadlines always use wall-clock time. Changing simulation speed never
rewinds or jumps the orbit phase.

Table view aligns planets beneath their sun, with task labels alongside. Planets
and the sun keep their physical radii, so a sun-sized star shows only its limb
along the top edge. Row spacing is schematic, scaled to the page's largest planet.
Both presets automatically frame the available viewing area. Switch to **Orbit**
to restore physical positions at the current simulation time; camera movement is
available in Orbit.

Every jump, page change, preset and focus is a short camera flight (0.25 to
0.75 s) along van Wijk and Nuij's zoom-and-pan path, so long trips zoom out first
and keep the destination in view. Planets slide between their orbits and the
table rows during the flight. Camera input resumes when it lands. Changing page
flies between two distinct page stars using that same path and camera rotation.
Every star-to-star flight, whether to another page or another list, takes
0.65 s regardless of distance, so both have the same rhythm; only the peak
zoom-out grows with the distance. Close up, distant stars sit on a shell at
four camera ranges and join their true positions as the zoom reaches them,
so nothing jumps mid-flight. All page stars of every list are always drawn.
Search selects a task's page automatically. Pages use permanent slots, so edits,
API reordering and task removals never move surviving tasks between systems.
Partially filled pages remain; wholly empty pages are omitted from navigation.

## Connect ClickUp (optional)

1. Copy `.env.example` to `.env` inside this folder.
2. Set a personal API token in `CLICKUP_TOKEN`.
3. Set one or more comma-separated numeric list IDs in `CLICKUP_LIST_IDS`.
4. Restart `./scripts/run.sh`.

The backend only performs GET requests to ClickUp. It includes closed tasks,
paginates each list, and refreshes every five minutes. The client checks the
local snapshot every two seconds; unchanged responses have no body, so large
galaxies are not downloaded and rebuilt on every poll. Lists appear incrementally during initial sync,
with progress shown in the interface. Subtasks/moons and tasks added to a list from
another home list are not included yet.

Tokens remain in the backend, never in the `/galaxy` response or browser bundle.
`.env` and `data/` are excluded from Git. Native development binds to
`127.0.0.1:4321`; Docker binds internally to `0.0.0.0:4321` and publishes only
the homelab LAN address. The app uses one configured ClickUp account; all LAN
visitors see that account's configured lists. OAuth/per-user login is not implemented.

On a list sync error the backend retains that list's previous data and continues
loading the other lists. The final status reports failed lists.
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
Cached tasks, stable orbital slots and ClickUp page-to-galactic-star assignments
are stored in `data/space.sqlite3`. Keep this database to preserve layout across
task deletions and subsequent additions. A new task is appended to the next free
slot rather than rearranging existing planets. On the first run with this mapping,
existing ClickUp pages move once from legacy list-generated stars to assigned
galactic stars; their assignments and subsequent task orbits then remain stable.
The demo retains its legacy page stars for now. No star represents a catalogued
astronomical object.

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
- The active system is drawn in detail. Other stars are directional markers at
  system scale and placed at their actual generated separation in atlas mode.
- Bevy's reverse-Z perspective depth and distance-adjusted near plane support the
  view's changing scale. This is not a logarithmic-depth-buffer implementation.
- Bodies use physical radii by default; selectable markers keep them visible.
  **Readable planet sizes** exaggerates radii in Orbit view, not positions or periods.
- Each page is a separate star system with a stable, independently random orbital
  plane (poles uniform over the sphere). The telemetry panel shows the current
  page's tilt. Live ClickUp pages occupy distinct, visible cell stars near galactic
  home, with their real generated positions and planetary systems. Demo pages
  still use legacy seeded local positions. Constellation view moves nothing: it zooms out from the current
  angle until every page star fits and joins them with lines. Orbit and Task table
  retain physical orbital distances. Camera flights roll between planes; the atlas
  uses galactic axes.
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
  natively and in a 4 ms slice of each frame in the browser, and drops cells
  left behind. Batched point meshes show stars actually bright enough to see,
  in both the wireframe and realistic views. A faint approximate glow traces
  unresolved disc and spiral-arm light; the central black hole is modelled at
  its physical horizon radius. There is no galactic navigation, dust or
  gravitational lensing yet.

- **Generated galaxy (Phase 6):** choose **Generated galaxy** in the native
  navigation panel or press **G** (the same shortcut works on the web canvas).
  Scroll out for kiloparsec-scale travel; WASD and Q/E move, Shift accelerates.
  Aim the centre of the view at a visible star and press **F** (or **Pick centre
  star**), then **Enter** to fly into its deterministic, task-free planetary
  system. **Backspace** returns to the galaxy, **G** returns to lists. Canvas
  controls are present on both browser and native. Picking uses only currently
  loaded stars; wait for streaming if the target is not yet selectable.

## Project structure

```text
crates/client/       Bevy scene, controls, web bridge and egui dev UI
crates/server/       Axum API, ClickUp adapter and SQLite storage
crates/shared/       Serializable API models and offline demo
crates/simulation/   Orbital mechanics, clock, galaxy model and deterministic layout
assets/             Reserved for project assets
scripts/            Launch and verification commands
data/               Local database, logs and smoke-test screenshots
target/             Compiled output
web/                Browser page, HTML/CSS panels (ui/) and generated dist/
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
node scripts/test-html-ui.mjs
```

The graphical smoke test requires a working desktop session. It exercises the
system view, close tracking, atlas, interstellar jumps (one captured mid-flight),
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
drives the egui panels (`?ui=egui`). `test-html-ui.mjs` drives the default HTML
panels against a local server: lists, views, pages, task search, selection,
following, display options, keyboard focus and wheel routing. It saves
`data/html-*.png`.

## Browser UI

The browser is the main client; the native window is a developer fallback that
keeps the egui panels, smoke test and bug reports. In the browser, HTML/CSS
panels in `web/ui/` surround the 3D view, and `?ui=egui` brings back the egui
panels. Run `./scripts/dev-web.sh` to serve the browser client with `web/` linked
live, so HTML and CSS edits need only a refresh.

## Next milestones

- Smooth jump/focus transitions, camera collision limits and better label placement.
- Real-data validation with your chosen lists and larger task counts.
- Workspace/list discovery UI, task filters and explicit refresh controls.
- Subtask moons and richer task details.
- Browser performance on larger lists, responsive UI and packaged native releases.
