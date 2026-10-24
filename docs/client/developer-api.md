# Native developer API and CLI

This is the native client's local developer interface, not the
[galaxy backend](../server/backend.md) on port 4321. It provides cached state,
performance diagnostics, command discovery, application commands and screenshots.
See [diagnostics](devtools.md#snapshots-and-measurement-meaning) for snapshot
timing/meaning and [object search](../simulation/search.md) for the engine.

## Starting and discovering

`./scripts/run.sh` enables the API by default; opt out with `NIGHT_SKY_API=0`.
Direct client launches require `NIGHT_SKY_API=1`. `devtools/http.rs` listens
only on `127.0.0.1:4322`; `NIGHT_SKY_API_PORT` changes the port, never the bind
address. API startup enables shared snapshots without requiring file export.
The listener stops with its resource; bind failures leave the app usable.

The transport keeps request security and limits in `devtools/http.rs`.
Under `devtools/http/`, `bridge.rs` forwards bounded jobs to the main thread,
`commands.rs` owns command discovery/submission, `search.rs` owns search routes,
and `state.rs` selects cached snapshots and history. All routes pass the same
central security checks before dispatch.

`./scripts/ns` is the Python 3 standard-library CLI, with no extra packages.
`ns help` works offline; start the app for other commands. Discover rather than
guessing commands or fields:

```sh
./scripts/ns help
./scripts/ns commands
./scripts/ns commands set_debug
./scripts/ns keys state
./scripts/ns keys perf/latest
```

- `GET /health` checks the listener.
- `GET /commands` lists the live catalogue from `command_line::USAGE` (name,
  positional fields, synopsis, description); `/commands/set_debug` selects one.
  These need no snapshot. Terminal-only `help`/`clear` are not UiCommands and
  are not listed.
- Any snapshot path accepts `?keys=1` for immediate object keys/array indices
  as strings without values. Scalars/null give an empty array. `/perf?keys=1`
  describes the performance object, not the root response envelope. Unknown
  paths return 404; `keys` cannot combine with `history` or `seconds`.

## Reading state and performance

`GET /state` and `GET /perf` return cached snapshots, published at most every
250 ms, with process ID/sample time. Root responses omit history by default;
either accepts `?history=1` for bounded per-frame history.

`/state/clock` gives simulation `seconds`, `rate` (simulated seconds per
real second, before any hold by craft) and `paused`.

Nested reads use a generic JSON-pointer resolver: `/state/camera`,
`/state/debug/split_multiples`, `/perf/latest/split/preparation_ms`.
`/perf/<path>` selects directly inside the performance object without the root
envelope. Responses contain only the selected value (including scalars/null);
unknown paths return 404. Array indices are zero-based path segments.
URL-encode special characters; `~1` escapes `/` in keys and `~0` escapes `~`.

`/perf/history?seconds=2` selects frames within two seconds of the latest sample;
the same filter works on `/state/performance/history`. Seconds must be finite,
positive and at most 10. Unsupported/duplicate query parameters return 400.
Nested reads use cached snapshots, not newly collected measurements; read root
sample time to check freshness.

```sh
./scripts/ns state camera
./scripts/ns perf latest/split/preparation_ms
./scripts/ns perf history --seconds 2
./scripts/ns perf --history
./scripts/ns watch perf latest/split --interval 0.5
```

`watch` repeatedly reads without changing app state, prints a new JSON value per
refresh without clearing the terminal, and stops with Ctrl+C. Reads default to
indented JSON; global `--json` gives compact JSON, including command discovery.
Command help defaults to readable text; subcommand `--help` describes arguments.
Global `--port` overrides `NIGHT_SKY_API_PORT` (default 4322). Host is fixed to
loopback, proxies bypassed, requests time out after five seconds, and errors
go to stderr with a nonzero exit code.

## Commands and screenshots

`POST /command` accepts typed text or UiCommand JSON. The HTTP thread parses
syntax and `UiCommand::check_values` ranges (400, logged as `cli> line: reason`),
then sends `Typed` (`Source::Api`, reply channel). `command_line::run` echoes
`cli> line` and checks current state with `UiCommand::check`:

- 409 with a reason if `actions::apply` would ignore it (unknown/unreachable
  IDs, no visited system, nothing selected).
- 202 `accepted` when applied that frame. Flights continue afterwards.
- 202 `queued` if no main-thread reply within `REPLY_WAIT` (2 s).

Keep `check` in step with `apply` conditions; a test applies every refused
command and asserts nothing changed. Terminal/stdin get the same checks as
warnings; panels/drags do not go through them and still clamp.

`POST /screenshot` (no body) schedules bug-report capture and returns canonical
`report` and `png` paths; the PNG lands a frame or two later. It waits up to
two seconds for the main-thread reply; timeout does not cancel the queued job.

```sh
./scripts/ns cmd "set_debug split_multiples false"
./scripts/ns cmd --wait "jump demo-flight"
./scripts/ns lists
./scripts/ns screenshot
```

`ns atmosphere` prints the selected-planet debug override settings, including
whether they are enabled (`/state/atmosphere`),
the switch, and the GPU frame and transparent-pass times from the snapshot's
GPU diagnostics. Arguments are sent as `set_atmosphere`: a preset, then any
`field=value` over it, such as `ns atmosphere mars haze=20`. It then waits for
a snapshot taken after the change before printing. A preset or optical edit
enables the override; `ns atmosphere enabled=false` restores generated air.
Quality-only changes do not enable it. These settings are not the generated
physical atmosphere shown in the inspector.

`cmd` takes one quoted text/JSON string; refusals exit 1 with the reason.
`--wait` polls `/state` until a snapshot sampled after the reply has no flight,
then adds `settled` (trail, presentation mode, location planet, camera).
`--timeout` defaults to 30 s. `lists` reads `/state/lists` (IDs, names, task
counts, page star IDs) for `jump`, `find_task`, `visit_star`. `screenshot` waits
up to 5 s for the PNG and prints its path; `--no-wait` returns immediately.

### Local transport constraints

Both command/capture POST routes require `X-Night-Sky-Client: cli`. Browser
Origin/Sec-Fetch-Site headers and non-loopback Host values are rejected, with
no CORS support. Local processes are trusted: this is not remote authentication.
Command bodies are limited to 1,024 bytes, require Content-Length, and cannot
use chunked transfer or Expect. The bounded queue holds 32 jobs, at most eight
forwarded per frame. Reads/final JSON encoding run on the HTTP thread;
commands/captures run on the main thread. No endpoint manipulates ECS directly.

```sh
curl http://127.0.0.1:4322/state
curl 'http://127.0.0.1:4322/perf?history=1'
curl -H 'X-Night-Sky-Client: cli' --data 'set_debug split_multiples false' http://127.0.0.1:4322/command
```

## Terminal control panel

`./scripts/ns-tui` is a Textual app over the same API, for trying commands
before they have in-app UI. It opens on a **Ship** tab built from the
[ship schema](#ship-schema): a ship picker, order buttons grouped as sublight,
turning and warp (orders without settings run when pressed, others open a
typed form with ranges and units, plus `in` and, on orders that can be
timed, `for` in seconds), and the picked ship's live state, grouped with
readable units and refreshed twice a second. Pending orders show as
"in 0.95 s: throttle value=0 (timed end)". It reads the live schema,
or the committed copy when the app is older or offline.

The **Commands** tab builds its command list and forms from
`GET /commands`, so a new `USAGE` entry appears with no TUI change. Forms send
`name field=value` text (blank fields are left out) through `POST /command`
and log the reply or refusal. A free-text line takes any command, a side panel
polls chosen `/state` paths every second, and Ctrl+S captures a screenshot.
Its fields are untyped text, as `USAGE` names fields but not their types.

Textual is the one dependency outside the standard library. The first run makes
a virtual environment in `tools/tui/` and installs it there; delete that
folder to reinstall. Agents should keep using `ns`, which has no dependencies.

## Inspector readout

`ns inspect` reads `/state/inspector`, the active top-left inspector, which
can differ from selection. `ns inspect facts`, `ns inspect actions`, or
`ns inspect actions/0/commands` selects subsets. The readout includes source/
object, title, ordered label/value facts, task description, source note,
action buttons and, for a located star or system, `velocity_kms`
(galactocentric, galaxy axes). It reuses `ui::inspector::describe`, actions and list-page
builders; facts invalidate on object/universe changes, not every publication.

Buttons include label, current/available state, group and kind. Unreachable
buttons are omitted, like the UI. Command buttons export ordered `commands`
arrays of UiCommand JSON accepted by `ns cmd` or `POST /command`. List-system
buttons also export follow-up `InspectObject` to preserve the list tab. Task
links export a URL and no app commands; CLI reads never open them. These reads
do not select or move the camera and use the normal snapshot refresh rate.

## Ship schema

Ship orders and the ship state readout are described by the Rust types
themselves. `schemars` derives JSON Schema from `Order`, the readout structs
and the simulation types inside them; doc comments become help text, range
attributes use the constants validation uses, and `x-group`/`x-unit`
extensions add display groups and units. `craft/schema.rs` flattens that into
a catalogue that clients read without knowing JSON Schema:

- `orders`: each order's words (`warp set`), group, help, JSON skeleton, the
  path its fields go under, and its fields: key path, kind (number, integer,
  boolean, text, choice, vector, variant), range, unit, default, and whether
  the CLI takes it as a positional argument or a flag; and `off`, the order
  that ends it when timed (`Order::off`), or null when it cannot be timed;
- `common`: `id`, `tick`, `in` and `for`, which orders take from
  `steer_craft` (key `in_s` and `for_s`); `for`, marked `timed`, applies only
  to orders with an `off`;
- `state`: display groups and every readout field with its group, label and
  unit, so new state fields appear in the TUI's state panel.

`GET /craft/schema` serves it, and a client test keeps a copy in
`scripts/craft-schema.json`, rewriting it and failing while it is stale:
commit the rewritten file. `ns ship` builds its order subcommands from that
copy, so it needs no app for `--help` or `ns ship tree`. Tests require every
order to have help and a group, every catalogue order to build an order the
simulation accepts, and every number to declare a unit (`""` for none,
`fraction` for [-1, 1] inputs).

Adding an order or a state field therefore needs only the Rust change, its
doc comment and attributes, and the rewritten copy. Generation rules:
required fields are positional in declaration order; defaulted fields are
flags; a nested struct becomes flags; an order holding only a tagged enum,
like `warp`, becomes a menu of subcommands. Spawning, listing, state, follow
and despawn are hand-written in `ns`.

## Craft

`ns ship` drives live craft through the same `POST /command` path and reads
them from `/state/craft` (see [craft](../simulation/craft.md#in-the-client)).

```sh
./scripts/ns ship spawn --body g78040000.2.813/2 --altitude 400
./scripts/ns ship spawn --altitude 400 --speed 9      # prograde, km/s
./scripts/ns ship spawn --altitude 400 --velocity 0,7.5,0
./scripts/ns ship state craft-1
./scripts/ns ship list
./scripts/ns ship point prograde --id craft-1
./scripts/ns ship throttle 0.25                      # persistent main throttle
./scripts/ns ship translate --up 0.1 --right -0.2    # fine thrusters
./scripts/ns ship rotate --pitch 0.1 --roll -0.2     # manual, releases pointing
./scripts/ns ship point body g78040000.2.813/2
./scripts/ns ship point vector --up 0,1,0 -- -1,0,0
./scripts/ns ship stop                              # brake spin, keep drives
./scripts/ns ship cut                               # cut all drives, brake spin
./scripts/ns ship release                           # disengage attitude pilot
./scripts/ns ship configure --forward 9.80665 --reverse 0 --fine 0.980665 --angular 0.1,0.1,0.1
./scripts/ns ship follow craft-1
./scripts/ns ship despawn                             # every craft
```

- `spawn` defaults to the selected planet or star, else the visited system,
  and prints the new ship's state once a later snapshot shows it.
- `state` gives system-frame position, velocity, quaternion and forward/up
  vectors; body-local `spin_rads` (pitch/yaw/roll); inputs, thruster limits and
  attitude pilot state. `around` names the
  [orbit reference](../simulation/craft.md#orbit-reference) and its kind,
  with altitude, relative speed and instantaneous `two_body_orbit`
  elements. `prediction` describes the shown coast prediction: its
  reference, `age_seconds` since it started, horizon, `achieved_seconds` and
  `ahead_seconds` still ahead, `end` (`horizon`, `truncated` or
  `impact:<body>`), whether it is `current` for the ship's inputs and
  reference, and whether a newer one is `updating`. Its `events` list the
  next sampled periapsis and apoapsis (`altitude_km`) and every predicted
  `exit` or `encounter` (`to`), each `in_seconds` from now.
  `held_clock` indicates physics catch-up; `step_seconds` is the fixed step,
  and each craft's `tick` is the world step its state describes.
  While pointing, `target_direction` and `pointing_error_deg` report the
  resolved direction and nose error at the latest physics step; they are
  null outside pointing mode or when its target is undefined.
- Steering defaults to the selected ship, or the only ship. Use `--id` to
  disambiguate. Throttle and axis inputs persist as fractions in [-1,1]. Each
  `translate` or `rotate` replaces its three axes, zeroing omitted axes;
  throttle, translation and rotation are otherwise independent. Negative
  throttle needs a reverse drive. Forward is -Z, up +Y, right +X; rotations
  are right-handed about X/Y/Z. See [craft](../simulation/craft.md).
- `stop` brakes spin only, not linear velocity or throttle. `cut` zeroes all
  drive inputs and engages spin braking. `release` disables the attitude
  pilot but leaves manual drive inputs unchanged. Pointing accepts orbital
  directions, body IDs within the ship's frame, or system-axis vectors.
- The API command is `steer_craft` with an `order` object, for example
  `{"type":"steer_craft","order":{"action":"throttle","value":0.25}}`.
  It also accepts `translate`, `rotate`, `point`, `stop`, `cut`, `release` and
  `configure`. `ns ship --help` and each subcommand's help describe arguments.
  Spawn, despawn, selection and following retain their existing commands.
- `steer_craft` also accepts an optional integer `tick`, for example
  `{"type":"steer_craft","tick":200,"order":{"action":"throttle","value":0.25}}`.
  Without it, the order affects the ship's next unsimulated interval as
  before. A future tick queues it; a past tick is refused. Same-tick orders
  keep submission order. Future inputs are value-checked on submission and
  limited to 300 simulated seconds ahead and 1,024 pending orders per craft.
  Queue acceptance is not execution acceptance:
  drive/frame interlocks are checked at that future boundary and refusals
  are logged. `ship state` exposes `pending_orders`, each with its `tick`,
  `order`, `in_s` (seconds until it applies) and, for a timed order's end,
  `ends_from`. See [step-keyed orders](../simulation/craft.md#body-axis-control-simulation)
  for queue limits and replay semantics.
- People and agents should give times in seconds rather than steps:
  `in_s` delays an order from the ship's next step (instead of `tick`), and
  `for_s` ends it with its off order after that long, both rounded to whole
  0.05 s steps. Throttle, translate, rotate, point, thrust and warp engage
  can be timed; their off order is zero on the order's axes (a zero `rotate`
  for point, which keeps any spin) or disengage. The app queues both halves
  at once, so a client closing mid-burn changes nothing, and a later order
  on the same controls cancels a pending end. In `ns` these are `--in` and
  `--for` on every order that takes them, and `--tick` remains for exact
  steps:
  `ns ship throttle 0.5 --for 1` is a one-second burn,
  `ns ship warp engage --in 5 --for 30` a delayed 30-second warp hop, and
  `{"type":"steer_craft","in_s":2,"for_s":1,"order":{"action":"throttle","value":0.5}}`
  the same through the API.
- Legacy `ns ship thrust x,y,z` / `set_thrust [x,y,z]` remains a system-axis
  debug acceleration, at most 100 m/s². It is the `thrust` order underneath
  (`{"action":"thrust","thrust":[x,y,z]}`), validated like the others. A
  throttle or translate command clears it, and `cut` clears it too. Prefer
  the body-axis commands for flying.
- Manoeuvres (drive inputs, spin, or an engaged attitude pilot) cap clock
  advancement at 10× without changing the requested rate or pause setting.
  Steering the warp bubble is not a manoeuvre and does not cap it.
  `/state/clock/effective_rate` reports that ceiling (zero while paused),
  before any further physics catch-up hold. Quiet manual coasting restores
  the requested rate; release the attitude pilot to allow that restoration.

### Warp commands

```sh
./scripts/ns ship throttle 0       # warp needs every sublight thrust input at zero
./scripts/ns ship state            # inspect spin, inputs and warp state
./scripts/ns ship warp set 1000000  # requested m/s; does not engage
./scripts/ns ship warp engage
./scripts/ns ship warp up          # multiply requested speed by √10
./scripts/ns ship warp down
./scripts/ns ship warp sweep 0.5 --for 2  # sweep speed up 1 decade over 2 s (1 is 2 decades/s)
# Steering while engaged: the same turning commands steer the bubble.
./scripts/ns ship rotate --yaw 0.5        # turn at half the turn rate, until changed
./scripts/ns ship rotate                  # stop turning (eases out over ~0.3 s)
./scripts/ns ship point body 'body-id'    # turn towards a body and track it
./scripts/ns ship stop                    # stop turning, ending pointing
./scripts/ns ship warp turn-rate 90       # turn rate at full steering, 1 to 360 °/s
./scripts/ns ship warp disengage
# Alternative exits while engaged, using a body in the ship's current system:
./scripts/ns ship warp match 'body-id'          # match only the body's velocity
./scripts/ns ship warp match 'body-id' --orbit  # add speed for a rough orbit
./scripts/ns ship warp match craft-1 --id craft-2  # formation: craft-1's velocity, from anywhere
./scripts/ns ship warp gravity suspended  # developer policy, only while off
./scripts/ns ship warp gravity active     # default
# Plotted routes, to a body in the ship's current system:
./scripts/ns ship warp to 'body-id'                  # fly there, keep velocity on arrival
./scripts/ns ship warp to 'body-id' --orbit          # drop into a rough orbit
./scripts/ns ship warp to 'other-system-body-id'     # loads that system first, then flies there
./scripts/ns ship warp to craft-1 --match --id craft-2  # fly to another ship, hold formation 1 km off
./scripts/ns ship warp to 'body-id' --match --time 120  # match it, in two minutes; replies with the plotted route
./scripts/ns ship state              # its route: stage, distance, ETA, lateness
./scripts/ns ship warp cancel        # end the route; warp stays engaged
```

All warp commands accept `--id`. Sublight thrust is interlocked with warp;
turning is not. While engaged, `rotate`, `point`, `stop` and `cut` steer the
bubble rather than the turning thrusters, at up to the turn rate (default
30 °/s), and engagement takes over any existing spin and eases it out. On
disengagement the ship keeps no spin, and a held rotation input or pointing
pilot passes back to the turning thrusters. `warp to` engages warp, turns,
curves round bodies in the way, slows all the way in and drops out at a
standoff, in this system or another (loaded first, clock held), or 1 km
from another ship; turning, warp speed,
`disengage` and `match` orders take over from it (see
[plotted routes](../simulation/craft.md#plotted-routes)). Manual warp is
unsafe: no collision checks, proximity slowdown, automatic arrival stop or
missing-data stop. It can pass through stars and planets without
a crash response. Ordinary motion continues. See the
[warp model](../simulation/craft.md#warp-core) for the precise rules and limits.

The API uses `steer_craft`, for example
`{"type":"steer_craft","order":{"action":"warp","command":{"command":"engage"}}}`
or `{"type":"steer_craft","order":{"action":"warp","command":{"command":"set","speed_ms":1000000}}}`.
The turn rate order is `{"action":"warp","command":{"command":"turn_rate","degrees_per_second":90}}`.
`/state/craft/craft/<index>/warp` contains `engaged`, `requested_ms`, `actual_ms`,
`gravity`, `stop_reason`, `turn_rate_dps`, the held `sweep` and the last step's `bubble_delta_m`
in frame axes. The bubble's current turn rate is `spin_rads`, body axes.
`last_match` is null or the last successful matched exit's `body`, `orbit`,
`tick` and `velocity_ms` (system-frame velocity at exit, not current velocity).
It clears on the next engagement. Matching uses existing planet, star and
barycentre IDs, not camera location or a separate flight selection. It does
not navigate to the body. The API order is
`{"action":"warp","command":{"command":"match","body":"body-id","orbit":true}}`.
Matching requires engaged warp and a reference in the current system; a refused
match leaves warp running unchanged. Rough-orbit exits require being outside
the reference body but within its reach (Hill sphere or stable zone), and are
two-body estimates, not guaranteed safe orbits. Body IDs in orders are 1 to
256 bytes.
See [matched dropout](../simulation/craft.md#matched-dropout) for direction rules.
`warp_mode` (`unsafe_manual`), `arrival` (whether it can enter systems, or
what its next step waits for while the clock is held) and galactocentric
`position_pc` are alongside it. Fleet-level `system_loading` reports `queued`,
`running`, `cached_regions` and `cached_systems`; `system_loading_error` retains
the last background loader failure. Failures retry without inline generation.
`velocity_ms`
remains ordinary velocity in the named frame, never bubble speed. During warp
or galactic flight, prediction reports unavailable rather than an old orbit.

## Object search

- `GET /catalogue` lists kinds, families, properties, relationships and syntax.
- `POST /search` takes query text or a JSON object with `query`, optional
  `origin` (`"camera"`, `"home"` or `[x,y,z]` in pc), and optional `work_cap`.
  It replies 202 with ID, or 400 with refusal reason.
- `GET /search` lists retained requests without results; `GET /search/<id>`
  gives results/progress; `POST /search/<id>/cancel` stops it. The last 32
  requests are retained. API requests never change the browser's shown query.
- Typed `search <query text>` becomes `UiCommand::Search`, opening/updating
  the browser and cancelling its prior shown request. `clear_search` clears
  it; `set_search threads=<n> work_cap_millions=<m>` adjusts debug settings.

```sh
./scripts/ns catalogue
./scripts/ns searches
./scripts/ns search kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=50
./scripts/ns search --show kind=star star_class=white_dwarf within_ly=50
./scripts/ns search kind=ship orbits.planet_class=rocky within_ly=100
```

`ns search <words>` waits and prints a table, cancelling on Ctrl+C or timeout.
Options include `--home`, `--origin x,y,z`, `--work-cap`, `--timeout`, `--json`.
`--show` sends the application `search` command and follows the game request
instead. Request logs and debug counters are described in
[the search guide](../simulation/search.md#client-requests).

`scripts/test-ns.py` tests CLI transport against an ephemeral mock server,
runs in `./scripts/check.sh`, and never drives the live app.
