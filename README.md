# ClickUp Space

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
`/home/ecorreia/homelab/clickup-space` on the homelab. Docker restarts the
`clickup-space` container automatically after a reboot. The service is published
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
docker logs --tail 50 clickup-space
```

### Native desktop

```sh
./scripts/run.sh
```

The first build downloads dependencies and compiles Bevy. Subsequent launches
reuse `target/`. Closing the window also stops the backend started by this script.
No ClickUp credentials are needed for the two-system demo.

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
| Browse ten-task pages | **‹ / ›** in the inspector |
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
Cached tasks and stable orbital slots
are stored in `data/space.sqlite3`. Keep this database to preserve layout across
task deletions and subsequent additions. A new task is appended to the next free
slot rather than rearranging existing planets. Stars have deterministic generated
positions, not positions of real astronomical objects.

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
  page's tilt. Page stars are scattered at random at the real star density near
  the Sun (about 0.004 stars per cubic light year, so neighbours are typically
  3 to 5 ly apart). A 140-page list spans about 40 ly, and its stars mingle
  with those of other lists, as in real space. Positions are stable per list
  and page. Constellation view moves nothing: it zooms out from the current
  angle until every page star fits and joins them with lines. Orbit and Task table
  retain physical orbital distances. Camera flights roll between planes; the atlas
  uses galactic axes.
- Each ten-slot page is its own system: a planet's orbital radius follows its slot
  within the page, so every page spans the same 0.55 to 6.4 AU.
- Wireframes use latitude/longitude line geometry, not native polygon wireframe
  mode. Decorative background points are not additional lists.

## Project structure

```text
crates/client/       Bevy scene, controls and egui mission-control UI
crates/server/       Axum API, ClickUp adapter and SQLite storage
crates/shared/       Serializable API models and offline demo
crates/simulation/   Orbital mechanics, clock and deterministic layout
assets/             Reserved for project assets
scripts/            Launch and verification commands
data/               Local database, logs and smoke-test screenshots
target/             Compiled output
web/                Browser page and generated dist/ bundle
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
```

The graphical smoke test requires a working desktop session. It exercises the
system view, close tracking, atlas, interstellar jumps (one captured mid-flight),
a 50-task table fixture, its last page, constellation framing, restored orbits
and a second orbital page (one captured mid-flight). Each capture waits for camera
flights to finish. It saves eleven screenshots under `data/`
and exits automatically. Unit tests cover physical orbit invariants, clock
continuity, persistent orbital-slot allocation, table layout and camera flights.
Set `SPACE_SMOKE_TASKS=1400` to exercise a 140-page constellation instead of
the default 50-task fixture.

The browser smoke test uses a locally installed Chromium and
`npm install --prefix tools/browser playwright-core`. It defaults to the homelab
URL; override `SPACE_URL` or `SPACE_CHROMIUM` when needed. It checks browser boot,
navigation, resize and private-file isolation, and saves `data/web-*.png`.
It intercepts the browser's galaxy response with a 50-task fixture to exercise
table pagination and constellation presets without changing backend data.

## Next milestones

- Smooth jump/focus transitions, camera collision limits and better label placement.
- Real-data validation with your chosen lists and larger task counts.
- Workspace/list discovery UI, task filters and explicit refresh controls.
- Subtask moons and richer task details.
- Browser performance on larger lists, responsive UI and packaged native releases.
