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

## Craft

`ns ship` drives live craft through the same `POST /command` path and reads
them from `/state/craft` (see [craft](../simulation/craft.md#in-the-client)).

```sh
./scripts/ns ship spawn --body g78040000.2.813/2 --altitude 400
./scripts/ns ship spawn --altitude 400 --speed 9      # prograde, km/s
./scripts/ns ship spawn --altitude 400 --velocity 0,7.5,0
./scripts/ns ship state craft-1
./scripts/ns ship thrust 0,0,1 --id craft-1           # m/s², system axes
./scripts/ns ship follow craft-1
./scripts/ns ship despawn                             # every craft
```

- `spawn` defaults to the selected planet or star, else the visited system,
  and prints the new ship's state once a later snapshot shows it.
- `state` gives position and velocity in the system frame, the body it is
  `around` with altitude and relative speed, its input, and whether craft
  `held_clock` this frame. `step_seconds` is the fixed physics step.
- The commands are `spawn_ship`, `despawn_craft`, `set_thrust`,
  `select_craft` and `fly_to_craft`; vectors in typed form are JSON lists
  without spaces, such as `set_thrust [0,0,1]`.

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
  it; `set_search threads=<n> work_cap_millions=<m>` adjusts debug settings
  (threads are native-only).

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
