# Night Sky

Formerly Task Space. The repo is `ChickenSplash/night-sky`.

## Scope and architecture

- Native Linux and browser/WASM clients. Rust workspace: `client` (Bevy/egui), `server` (Axum),
  `shared` (wire models), `simulation` (engine-independent orbital maths).
- Lists are constellations of system pages (7 to 14 slots per star or close
  pair, more in a multiple system, fewer where a companion cuts it short);
  tasks are planets. Deep navy background (black with realistic lighting) and line-based
  wireframe bodies. Preserve real distances and physical planet radii.
- A space simulator first. Lists, pages and tasks stay as a general way of
  tracking information; the ClickUp integration was removed and will be
  re-approached later. The backend serves only the built-in demo galaxy.
- Any future credentials belong only in the backend, never in API models or logs.
- All runtime files belong in project-local `data/`; compiled output in `target/`.

## Design references

Read the relevant notes before changing these systems, rather than loading all
design notes into every session:

- [Galaxy model](docs/galaxy-model.md): population, deterministic generation,
  multiple systems, streaming, spatial queries, benchmarks, galactic points,
  diffuse glow and navigation.
- [Realistic rendering](docs/realistic-rendering.md): body lighting, photometry,
  manual exposure, compression, HDR, point spread, sprites and cameras.
- [Object search plan](docs/object-search-plan.md): planned object registry,
  query model, search engine, CLI and browser. Read before starting that work.
- [Documentation index](docs/README.md): controls, development setup, backend,
  logs, project structure and checks.
- Paths in these notes are repository-relative or relative to a named crate's
  `src/` directory. Keep the notes and [project structure](docs/development.md#project-structure)
  in step with code and file moves.

## Development

- `./scripts/run.sh`: build and start both processes, clean up backend on exit.
  Enables the client's loopback developer API by default; use
  `NIGHT_SKY_API=0 ./scripts/run.sh` to disable it for a run.
- `./scripts/check.sh`: formatting, unit tests and strict Clippy checks.
- `./scripts/run.sh --smoke-test`: desktop rendering check with eighteen screenshots, including a 50-task pagination fixture, mid-flight captures of a list jump and a page jump, and seven realistic-rendering captures (0 stops: system at 100% and 0% compression, lit planet close-up, constellation, star close-up at both compression settings; then the corona's structure at -16 stops, 0%).
- Client logs live in `data/logs/client.log`, with `client.previous.log` for the last
  run and `crash-*.log` for panics. The renderer's local surface-timeout patch is
  documented in `vendor/README.md`. Keep it in step with Bevy upgrades.
- The native **Save bug report** control (debug window, `tools` tab) captures
  state, recent client log lines and a screenshot under `data/logs/bug-*` for
  visual bugs without a panic.
- `./scripts/build-web.sh`: WebGL 2 WASM build into `web/dist/`.
- The browser is the main target; native is a dev fallback. Both use the same
  egui interface (`crates/client/src/ui/mod.rs`), drawn in the same frame as the scene, which fills
  the whole window: there is no sidebar. Two free-floating halo icons at the
  top right (`ui::icons`, ⚙ above ✔, `breadcrumb::halo_glyph`, hover blue,
  white while open) toggle the debug console and Tasks window, which first open just below
  them, right-aligned. Tasks (`ui/tasks.rs`, 01 lists and 02 task
  index, with the data-source note) keeps lists and tasks apart as
  the future integration point; the rest reaches them through `UiCommand`s.
  Both start closed. The debug window opens below the icons
  (`terminal::below_icons`). Body facts, presentation modes and system paging
  are in the floating breadcrumb inspector and its actions; there is no
  telemetry section, centre-star pick or C key.
- The breadcrumb (`ui/breadcrumb.rs`) is free-floating text at the top left of
  the 3D view, under any window, with no container. A soft dark halo (two
  rings of translucent black copies, `HALO_EDGE` at 1 px and `HALO_FADE` at
  2 px) keeps it readable over star discs and the galaxy band; it vanishes
  on dark space. Reuse `halo_text`/`halo_wrapped_text` for scene text, or
  `halo_galley` for text placed by hand, such as the canvas planet labels
  (table labels sit in star glare).
  `breadcrumb::trail` is physical location: `Milky Way`, the current star's
  list if any, the star, then `Navigation::location_planet`. An open list or
  destination is not a location until arrival. `selection_trail` independently
  shows the selected object's hierarchy, even for a remote planet. List stars
  are `System n`, other stars use their catalogue names. Crumbs carry stable
  `objects::ObjectRef`s, never camera commands. Clicking either trail sends
  `InspectObject` and activates a highlighted tab in the shared, borderless
  `ui/inspector.rs` readout below both trails. Selection automatically activates
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
  `ObjectRef` (`space_simulation::objects`, re-exported by `objects`), the search registry, hierarchy builders and inspector resolution (moons do not yet
  exist). `ObjectRef::Star` by a system's key is the system as a whole: its
  star, or a multiple system's outermost barycentre, labelled with all its
  letters (`System 2 AB`). Below it, both trails show the planet's lineage
  (`StarSystem::lineage`): a component star (`ObjectRef::Star` by `key#B`)
  and, in triples and quadruples, an inner pair (`ObjectRef::Barycentre`).
  Any member's actions act on its whole system. Remote systems' crumbs use
  `objects::system_letters` (stars only, cached) and the host in the planet
  ID, never a whole generated system per frame. `UiCommand::Up` is separate keyboard navigation: reframe tracking,
  else clear selection, else move up physically; no crumb contains travel code.
- Time controls (`ui/hud.rs`) are two free-floating, centred lines at the bottom
  of the 3D viewport, using the breadcrumb font, colours and `halo_text`.
  Elapsed simulation time sits above the actual rate (two decimal places,
  seconds/minutes/hours/days/years per real second). Click the rate to toggle
  pause; drag it horizontally for continuous logarithmic adjustment from real
  time to 100 years/s (400 logical pixels covers the full range). Dragging
  preserves pause state, with `· paused` beside the retained rate. There is no
  bottom panel, preset ladder or separate speed editor.
  The bottom-left movement readout (`ui::hud::speed_readout`) uses the same halo,
  font and two-line layout: measured velocity above draggable set speed.
  Horizontal dragging adjusts speed logarithmically from `scene::MIN_SPEED`
  to `scene::MAX_SPEED` (1 m/s to 10,000 ly/s), through `SetMoveSpeed`; it
  captures input like the time rate, and a single click does nothing.
  The bottom-right **Magnitude limit** (`ui::hud::magnitude_control`, one line)
  is `Navigation::magnitude`, the faintest apparent magnitude drawn as a
  star point (default `VISIBLE_MAGNITUDE`, 6.5; `MAGNITUDE` range 3 to 10.5, in
  tenths). Dragging shows the new value; it applies on release
  (`SetMagnitude`, `set_magnitude 5.5`), because a change restarts streaming
  from nothing: `stars/stream/mod.rs` starts a new `Backend` whenever its magnitude
  differs (in the browser a new worker, `star-worker.js?magnitude=`, the old
  one terminated). The starfield's upload filter, shader cutoff and rebuild
  distance, star picking and the glow's unresolved light table all follow
  it, so fainter stars move into the glow and the total light is unchanged.
  Each magnitude fainter draws about four times the stars. Page-star
  assignment (`crates/simulation/src/system.rs`) stays at `VISIBLE_MAGNITUDE`. Not persisted.
  Above it, two matching halo-text controls adjust manual Exposure and Dynamic
  Range Compression (`ui::hud::exposure_controls`), right-aligned with the same font
  and line gap. Drag exposure at 20 pixels per stop (-64 to +16); +1 doubles
  all light, -1 halves it. Drag compression across 400 pixels for 0 to 100%.
  Both apply continuously through `SetExposure`/`SetCompression`, capture the
  full drag and do nothing on a click. Disabled in wireframe mode; not persisted.
  All text HUDs share
  `breadcrumb::HUD_PADDING` (20 logical pixels from the 3D viewport edges);
  movement, magnitude and time share `HUD_LINE_GAP` and their bottom line
  alignment.
  `ui::pointer_captured` protects the background-layer breadcrumb/inspector and draggable text and captures
  its full drag, including outside its bounds; scene camera input and canvas
  picking both use it so time interactions never also move/select the scene.
  While a mouse button is held, the HUD hit test uses its press origin, not
  hover position, so camera drags that begin outside the rate text continue
  uninterrupted when crossing it.
- Settings and debug tooling are unified in `devtools/mod.rs`: a floating,
  movable, resizable window toggled by the cog, ` or F3 on both clients (the public
  site included), with tabs `log` (an in-memory console of the last 2,000
  tracing events from `devtools::log_layers`, with level filter and grep; the
  tab shows errors and warnings logged while it was hidden), `terminal` (a
  command line and its output), `perf` (frame graph; star stream and star
  draw facts: draw calls, uploaded stars, vertices (shader runs) and vertex
  memory; cells,
  blocks (drawn of uploaded) and grid boxes in one section; natively,
  a GPU passes table of GPU/CPU time and vertex/fragment shader invocations
  per render pass from Bevy's `RenderDiagnosticsPlugin`. Every camera runs
  passes of the same names; the vendored `bevy_render` suffixes repeats on
  earlier cameras `~1`, `~2`, so the glow camera's pass no longer hides the
  main view's),
  `render` (display options, field of view, realistic lighting, manual exposure/compression
  readouts, galaxy glow, wireframes over bodies and galaxy reference views),
  `controls` (shortcut help), `state` (camera; navigation: location,
  open list, anchor, page, presentation mode, selection; anchor star; clock) and `tools` (GPU test, core load test and bug report
  natively, star-stream benchmark in the browser). New debug readouts and switches go there, not in a separate settings window or the
  canvas. Performance sections are collapsible, skipping UI work while closed;
  counts are live and timings use trailing-second averages refreshed every frame.
  Logging and benchmarks retain their independent one-second reports.
  The performance switches live in a collapsible section in the `perf` tab
  (`render::debug::RenderDebug`, not
  `Navigation`; `set_debug <switch> <on>`) are for A/B tests and reset on
  restart: freeze star streaming (the loader keeps its last aim), galactic
  star points, shade point pixels (off, points still run the vertex shader
  but are never rasterised), 4x MSAA (both window
  cameras), bloom, cull blocks outside the view (on by default), split multiple
  systems into individual stars and stars as points (native only, on by default natively).
  Each tab lives in `devtools/tabs/`; tracing capture lives in `devtools/capture.rs`.
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
- The debug window uses the terminal look (`ui/terminal.rs`): a translucent
  black, Hyprland-like window with a light grey border and text-mode
  checkboxes and buttons. Only its full padded header drags the window, with a four-way arrow cursor;
  content presses retain their hover highlight and never move the window.
  Checkbox rows are split by a vertical divider: the left toggles the setting,
  the flush square `+` / `-` button at the right expands or collapses its description.
  Each half highlights independently, with square hover edges against the divider.
  All terminal controls, headings and fact readouts share one hover/held-press
  state and blue highlight painter. Whole elements have bright edges on both
  sides; checkbox halves keep their single outer edge independently. Holds keep
  the highlight visible. Interactive controls share click-and-drag sense to
  retain egui's long presses, but never move anything on a drag. Debug scroll
  areas share wheel/scrollbar sources, never content dragging, so they cannot
  steal control presses.
  Disabled switches still allow description clicks. There is no hold gesture or shortcut tooltip.
  Headings, sections, checkboxes and facts are all
  full-width `ROW`s with text at `INSET` and the same hover light; facts put
  values flush right after a dotted leader. A fact's explanation goes behind
  its label with `terminal::facts_with_details` (`label +`, click the row to
  show it below, `label -` to hide), not as an always-visible note.
- There is no HTML UI; the HTML/CEF prototypes were abandoned (branch
  `prototype-cef-overlay`). Every panel action is an ID-based `UiCommand`
  (`actions.rs`). Panel-only choices (list/task sort, status groups) live in
  `tasks::Tasks`, not `Navigation` (`navigation.rs`).
- `devtools/automation.rs` (browser only) is a test hook, not a UI: `window.space.command`
  sends a `UiCommand` as JSON and `window.space.state()` reads a small snapshot
   (list, page, presentation_mode, display options, stream caption, benchmark results).
   `devtools/snapshot.rs` builds the shared, versioned native/browser state
   snapshot at most every 250 ms. It includes f64 camera position (parsecs
   from the galactic centre), debug switches, stream counts, performance
   averages, latest CPU timings and one-second peaks, fresh native GPU
   diagnostics with measurement ages, and per-frame history (10 seconds,
   capped at 4,096 frames). History collection runs with the UI closed and
   native export disabled. Frame intervals precede the recorded CPU work;
   GPU measurements are delayed and not matched to individual CPU frames.
   Peaks are component-wise maxima, not one correlated slow frame.
   Each frame sample also has `sync` (where `starfield::sync` spent the
   frame: uploads, draw list, precise rescan and per-frame precise sprites
   with counts, plus the render world's star extract/prepare, a frame or so
   behind) and `instrumentation` (recording, snapshot building and debug
   window layout: what measuring itself costs). The perf tab shows them in
   `star sync` and `instrumentation`. Record new per-frame measurements as
   fixed-size numbers only, no strings or allocation. Natively the snapshot
   is published without its history (an empty placeholder) plus a plain
   copy of the frames; the HTTP thread or file writer encodes the history
   only for requests that may need it (`snapshot::Published::full`).
   Native read-only access before an HTTP transport exists:
   `NIGHT_SKY_SNAPSHOT=1 ./scripts/run.sh` enables atomic JSON export to
   `data/logs/live-snapshot.json`, at most four snapshots per second. Final
   JSON encoding and disk writes use a separate thread and bounded,
   nonblocking queue. Export is off by default, contains process ID and
   publication time, and may remain stale after exit: check those before
   using it. No commands, camera movement, benchmarks or HTTP listener are
   introduced by enabling snapshots. Snapshot construction itself is CPU
   work on the main thread, so exported diagnostics are opt-in.
   Native developer HTTP API (`devtools/http.rs`): `./scripts/run.sh` enables
   it by default, with `NIGHT_SKY_API=0` to opt out. Direct client launches
   still require `NIGHT_SKY_API=1`. Listens only on `127.0.0.1:4322`;
   `NIGHT_SKY_API_PORT` may change the port, never the bind address. It
   enables shared snapshots without requiring file export. `GET /health`
   checks the listener; `GET /state` and `GET /perf` return the cached app
   snapshot (at most 250 ms publication frequency), with process ID and
   sample time. `GET /perf?history=1` includes bounded per-frame history.
   Nested reads use a generic JSON-pointer resolver: `/state/camera`,
   `/state/debug/split_multiples`, `/perf/latest/split`, or
   `/perf/latest/split/preparation_ms`. `/perf/<path>` selects directly
   inside the performance object (without the root response envelope).
   Responses contain only the selected JSON value, including scalars or
   null; unknown paths return 404. Array indices are zero-based path segments.
   URL-encode special characters; JSON-pointer `~1` escapes `/` in keys and
   `~0` escapes `~`. The full root response shapes remain compatible and
   omit history by default; either root accepts `?history=1` to include it.
   `/perf/history` returns the retained history array; append `?seconds=2`
   to select samples within two seconds of the latest captured frame.
   The same filter works on `/state/performance/history`. Seconds must be
   finite, greater than zero and at most 10 (the existing retention limit).
   Unsupported/duplicate query parameters return 400. Nested reads use the
   same cached snapshots, not newly collected measurements; root sample
   time is still needed to check freshness.
   Discovery: `GET /commands` lists the live command catalogue (name,
   positional fields, synopsis and description) from `command_line::USAGE`;
   `GET /commands/set_debug` selects one entry. These do not require a
   snapshot. Terminal-only `help` and `clear` are not UiCommands and are
   not listed. Any snapshot path accepts `?keys=1` to list its immediate
   object keys or array indices as strings, without values; scalars/null
   return an empty array. `/perf?keys=1` describes the performance object,
   not the legacy response envelope. Unknown paths still return 404;
   `keys` cannot be combined with `history` or `seconds`.
   This is the native client, not the galaxy backend on port 4321.
   `POST /command` accepts the existing typed text or UiCommand JSON. It
   parses on the HTTP thread (syntax and `UiCommand::check_values` ranges:
   400, logged as `cli> line: reason`), then sends the line as `Typed`
   (`Source::Api`, with a reply channel) so `command_line::run` echoes it
   (`cli> line`) and runs `UiCommand::check` against the current state:
   409 with the reason if `actions::apply` would ignore it (unknown or
   unreachable IDs, no visited system, nothing selected), else 202
   `accepted`, applied that frame. Flights continue afterwards. Keep
   `check` in step with `apply`'s conditions; a test applies every refused
   command and asserts nothing changed. Typed terminal and stdin commands
   get the same checks as warnings. Panels and drags never go through
   them and still clamp. With no main-thread reply in `REPLY_WAIT` (2 s)
   the reply is 202 `queued`. `POST /screenshot` (no body) runs the existing
   bug-report capture; its reply gives the canonical `report` and `png`
   paths (the PNG lands a frame or two later).
   Both POST routes require `X-Night-Sky-Client: cli`. Browser Origin and
   Sec-Fetch-Site headers and non-loopback Host values are rejected, with
   no CORS support. Local processes are trusted; this is not remote auth.
   Command bodies are limited to 1,024 bytes, require Content-Length and
   cannot use chunked transfer or Expect. The bounded queue holds 32 jobs,
   with at most eight forwarded per frame. Screenshots wait at most two
   seconds for a main-thread reply; timeout does not cancel the queued job.
   Reads and final JSON encoding run on the HTTP thread. Commands/captures
   run on the main thread; no endpoint manipulates ECS state directly.
   The listener stops with its resource; bind failures leave the app usable.
   Curl examples: `curl http://127.0.0.1:4322/state`,
   `curl 'http://127.0.0.1:4322/perf?history=1'`, and
   `curl -H 'X-Night-Sky-Client: cli' --data 'set_debug split_multiples false' http://127.0.0.1:4322/command`.
   CLI wrapper: `./scripts/ns` (Python 3 standard library, no packages).
   `./scripts/ns help` works offline; start `./scripts/run.sh` for other
   commands. Discover before guessing: `./scripts/ns commands`,
   `./scripts/ns commands set_debug`, `./scripts/ns keys state`,
   `./scripts/ns keys perf/latest`. Read with `./scripts/ns state camera`
   or `./scripts/ns perf latest/split/preparation_ms`; bounded history is
   `./scripts/ns perf history --seconds 2`, full retained history is
   `./scripts/ns perf --history`. Send one quoted string with
   `./scripts/ns cmd "set_debug split_multiples false"` (text or JSON);
   refusals exit 1 with the reason. `ns cmd --wait` (with `--timeout`,
   default 30 s) polls `/state` until a snapshot sampled after the reply
   shows no flight, and adds `settled` (trail, presentation mode, location
   planet, camera). `./scripts/ns lists` reads `/state/lists` (each list's
   ID, name, task count and system star IDs by page) as IDs for `jump`,
   `find_task` and `visit_star`. `./scripts/ns screenshot` waits up to 5 s
   for the PNG and prints its path (`--no-wait` returns at once).
   `./scripts/ns watch perf latest/split --interval 0.5` repeatedly reads,
   stopping with Ctrl+C, without changing app state. Each refresh prints a
   new JSON value; it does not clear terminal output. Global `--json` gives
   compact machine-readable JSON (including command discovery), e.g.
   `./scripts/ns --json commands`; other reads default to indented JSON.
   Global `--port` overrides `NIGHT_SKY_API_PORT` (default 4322). The host
   is fixed to loopback, proxies are bypassed, requests time out after five
   seconds and errors go to stderr with a nonzero exit code. Command help
   defaults to readable text. Subcommand `--help` documents its arguments.
   Inspector readout: `./scripts/ns inspect` reads `/state/inspector`, the
   active top-left inspector, which may differ from the selected object.
   `./scripts/ns inspect facts`, `./scripts/ns inspect actions`, or a nested
   path such as `./scripts/ns inspect actions/0/commands` selects a subset.
   The readout includes inspection source/object, title, ordered label/value
   facts, task description, source note and available action buttons. It
   reuses `ui::inspector::describe`, `actions` and list-page builders; cached
   facts invalidate on object/universe changes, not each snapshot publication.
   Buttons include label, current/available state, group and kind. Unreachable
   buttons are omitted, as in the UI. Command buttons export an ordered
   `commands` array of UiCommand JSON, each accepted by `ns cmd` or
   `POST /command`. List system buttons also export the follow-up
   InspectObject command which preserves the list tab. Task links export
   their URL and no app commands; reading the CLI never opens them. The
   inspector commands are read-only, do not select or move the camera, and
   use the existing snapshot refresh rate.
   Object search (`search.rs`, engine in `space_simulation::search`, see the
   [object search plan](docs/object-search-plan.md)): `Searches` owns a
   `native::Hub`, shared with the HTTP thread, which keeps the last 32
   requests by ID and runs them on `SearchService`'s thread (a quarter of
   the machine's threads by default; the star loader has three quarters).
   `UiCommand::Search { query }` (typed `search <ns search text>`, the rest of
   the line) searches from the camera and becomes `Searches::shown`, the
   object browser's request, cancelling the previous one; `clear_search`
   clears it; `set_search threads=<n> work_cap_millions=<m>` changes the
   debug settings. The browser refuses `search` until its search worker
   exists. `GET /catalogue` lists kinds, families, properties, relationships
   and the syntax; `POST /search` takes query text or JSON
   `{"query", "origin": "camera"|"home"|[x,y,z] pc, "work_cap"}` and replies
   202 with an ID (400 with the reason if refused); `GET /search` lists kept
   requests without results, `GET /search/<id>` gives one with results and
   progress, `POST /search/<id>/cancel` stops it. API requests never change
   `shown`. `ns search <words>` (`--home`, `--origin x,y,z`, `--work-cap`,
   `--timeout`, `--json`) waits and prints a table, cancelling on Ctrl+C or
   timeout; `ns search --show` sends `search` instead and follows the game
   request; `ns searches` and `ns catalogue` list. Requests log their start,
   refusal and end (state, counts, time) under the `search` target; the
   `perf` tab's `search` section shows each request's counters and the
   settings, the `state` tab's `search` heading the browser's request, and
   the snapshot's `search` object both.
   `scripts/test-ns.py` tests CLI transport against an ephemeral mock server
   and runs in `./scripts/check.sh`; it never drives the live app.
- `./scripts/dev-web.sh`: web build plus server with `index.html` and
  `star-worker.js` linked live. `node scripts/test-browser.mjs` clicks the egui
  panels at 1600 x 1000 and checks the results through `window.space`; update its
  coordinates when the panel layout changes.
- Keep native threading/reqwest and browser fetch/timers behind target-specific code.
- Browser requests use same-origin `/galaxy`. Never bake a LAN IP or token into WASM.
- Bevy, big_space and bevy_egui are pinned as a compatible set. Check compatibility
  before upgrading any one of them.
- No global desktop or Cargo configuration changes are needed.
- Client file paths above without a repository prefix are relative to
  `crates/client/src/`. Camera code is in `scene/`, navigation state in
  `navigation.rs`, shared text formatting in `format.rs`, and large test blocks
  are in sibling `tests.rs` files.
- The user runs GPU, core-load and star-flight benchmarks on their own focused
  window. Read results from `data/logs/`; do not launch benchmark runs yourself.
- After code changes, run `./scripts/check.sh` and
  `cargo check -p space-client --target wasm32-unknown-unknown` to cover both targets.

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
  50°), set from the debug console's `render` tab (`UiCommand::SetFov`) or Shift + left-drag (up
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
  ID (its host's ID and orbit index: `key/n` around a single star, `key#B/n`
  around one star of a multiple, `key#AB/n` around a close pair; `Planet::host`) and an optional `Task`. Select and follow
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
  planets, with tasks as an optional overlay. Assignment skips stars whose
  system has no planets (a close companion can prevent them).
- Each legacy page (with no cell star) has its own star (`space_simulation::star(&star_id(list_id, page))`),
  drawn in solar-neighbourhood proportions (mostly red dwarfs, a few giants and
  white dwarfs). Its host's mass sets orbital periods (`StarSystem::host_mass`), its
  radius and temperature colour (`spectral_colour`) are drawn, and it shapes
  planet formation. Stars and planets carry a `Spin`; planets and star spins
  draw from their own seeds, so they never move an orbit. Planet mass is capped
  at 0.4% of the star's, or Hill spacing cannot converge.
- Constellation view never moves stars: it keeps the current page's frame, fits
  all page stars from the current angle and draws links between them.
- Each page has its own orbital-plane tilt (`scene::page_tilt`, its star's
  `plane_tilt`), including pages in one constellation.
- Explain the data source (currently always the demo) in the interface.
- Preserve generation draw order and component indices. Full stars and the compact
  `Sky::try_point` path must match bit for bit, including companions and separation;
  results must not depend on traversal order, caching or loader thread count.
- Keep component populations exact and spiral modulation normalised. Build cells
  through `StarCell::new`; never mutate their stars without rebuilding the grid.
  Use spatial queries rather than per-frame or per-streaming-change scans of every star.
- `Navigation::eye` is authoritative in free flight. Arrival and body handoff follow
  physical proximity, not selection, presentation layout or camera range. Preserve
  the visited-body fallback before its cell loads.
- Keep wireframe rendering unchanged when realistic lighting is off. Share one
  manual linear exposure across bodies, galactic points and glow; compression is
  per-body and must never change the galaxy. Do not restore metering, adaptation,
  artificial ambient light, brightness floors or a scene-wide highlight curve.
- Expose colours before GPU upload, retain logarithmic HDR glare storage and the
  shared point spread, and keep CPU sprite bounds and both star shaders in step.
  Window cameras must agree on HDR; retain `NoIndirectDrawing` and never add
  `TEXTURE_BINDING` to WebGL's multisampled depth texture.

## Current limitations

- No N-body simulation, moons or task integrations (ClickUp was removed).
- Galaxy travel, mapped list page stars and visits to generated systems work,
  but dust, lensing and galaxy rotation are not implemented. Diffuse galaxy glow
  is a smooth prototype, not yet a detailed or dust-attenuated galaxy renderer.
- Wireframe bodies use gizmos; realistic bodies use pooled per-body mesh entities.
  Galactic stars use custom GPU batches. Further body batching remains future work.
