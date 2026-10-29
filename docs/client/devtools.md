# Debug tools and diagnostics

Settings and debug tooling are unified in `devtools.rs`: a floating,
movable/resizable window toggled by the cog, backtick or F3. There is no separate settings window. Client paths
are relative to `crates/client/src/`. See [controls](controls.md),
[benchmarks](benchmarks.md), [developer API/CLI](developer-api.md) and
[development logs](../development.md#logs-and-bug-reports).

## Tabs and performance switches

Each tab lives in `devtools/tabs/`; tracing capture lives in `devtools/capture.rs`.
New debug readouts/switches belong here, not in separate windows or the canvas.

| Tab | Contents |
| --- | --- |
| `log` | Last 2,000 tracing events from `devtools::log_layers`, level filter and grep; marks errors/warnings logged while hidden |
| `terminal` | Command line and output |
| `perf` | Frame graph, star generation/stream/draw metrics, cells/blocks/grid boxes, GPU passes and performance switches |
| `render` | Fullscreen, scene overlays with its nested switches (orbits, labels, markers, velocity marker, craft predicted paths, body wireframes), FOV, realistic lighting, exposure/compression facts, glow and galaxy reference views |
| `atmosphere` | All-planet air switch, optional selected-planet override, presets and sliders for debug air, global shader steps, override top/optical depths, GPU frame and transparent-pass times |
| `controls` | Shortcut help |
| `state` | Camera, location, open list, anchor/page, presentation, selection, anchor star, clock and shown search |
| `tools` | GPU/core-load tests and bug report |

Performance sections are collapsible and skip UI work while closed. Counts are
live; timings use trailing-second averages refreshed every frame. Logging and
benchmarks keep independent one-second reports. Native GPU diagnostics include
GPU/CPU time and vertex/fragment invocations per pass from Bevy's
`RenderDiagnosticsPlugin`. Every camera uses the same pass names; vendored
`bevy_render` suffixes repeats on earlier cameras `~1`, `~2`, so glow no longer
hides the main view's pass. Keep the [vendor patch](../../vendor/README.md)
in step with upgrades.

The **craft CPU** section separates system discovery (polling and requesting
the regions and bodies craft wait for) from movement plus frame handoffs.
Region lookups and body generation run off-thread and are not included in
these main-update timings.
It shows latest values and trailing-second peaks, plus fixed-step counts, even
when warp is off. These CPU elapsed timings exclude rendering and coast
forecasting. The same fields are exported as `performance.latest.craft`, each
history sample's `craft`, and `performance.craft_peak_last_second`; they remain
recorded while the console is closed. A slow update usually lengthens the next
frame interval, not the interval attached to its own work sample.

The collapsible `perf` switches use `render::debug::RenderDebug`, not `Navigation`.
They are A/B tests, reset on restart and can be set with `set_debug <switch> <on>`:
freeze star streaming (loader keeps its last aim), galactic points, shade point
pixels (off still runs vertices, never rasterises), 4x MSAA (both window cameras),
bloom, cull blocks outside the view (default on), split multiple systems, and
stars as points (default on).

## Experimental planet loading

The `perf` tab's **experimental planet loading** section enables optional
background layered calculations, off by default and not persisted
(`set_layered_planets <on>` or `ns planets on|off` also switch it). It shows
per-planet queue/running/solved/refusal states by designation, in orbit order.
The separate **CPU threads** section shows the shared star/planet budget and
star-worker activity once, rather than repeating thread counts in each loader
section. Solved results appear in the planet inspector's layered section and
`/state/planet_loading`; the live scene still uses each planet's synchronous
fallback result, not these layered results. See
[composition](../simulation/planet-composition.md#experimental-background-loading)
for target selection, caching, cancellation, the transport record and limits.

## Terminal interaction

The debug window uses `ui/terminal.rs`, a translucent black terminal skin with
light grey border and text-mode controls over the shared [UI kit](ui/kit.md).
Only its full padded header moves the window, with a four-way arrow cursor.
Content presses keep their highlight and never move it.

Checkbox rows split at a vertical divider: left toggles the setting, right
flush square `+` / `-` expands its description. Halves highlight independently
with square hover edges against the divider. Disabled switches still allow
description clicks. There is no hold gesture or shortcut tooltip.

Controls, headings and facts share blue hover/held feedback: whole elements
have bright edges on both sides; checkbox halves retain their single outer
edge. Long holds still click on release over the control (`kit::interact::held_click`),
including at slow frame rates. Scroll areas use wheel/scrollbar input, never
content dragging, so they cannot steal presses.

Headings, sections, checkboxes and facts are full-width `ROW`s with text at
`INSET`; facts align values right after a dotted leader. Explanations go behind
the label via `terminal::facts_with_details` (`label +` to show below,
`label -` to hide), not in an always-visible note.

## Typed commands

`devtools/command_line.rs` handles terminal/stdin/developer API input as `Typed` messages.
`command_line::run` parses text (`set_presentation_mode table`,
`set_display option=orbits on=false`) or `UiCommand` JSON into the same commands
as panels, before `actions::apply`. New input sources should send `Typed` too.
Every `UiCommand` needs a `command_line::USAGE` entry; tests compare it with
serde's variant list and parse each entry, naming mismatches.

`help` is built in; `clear` empties the terminal tab and is window-local.
Echoes, help and errors use `command_line::TARGET` (`console`), so they reach
the terminal tab, log file and launching terminal. Typed commands receive
value/state checks; panels and drags retain their own clamping behaviour.
See [the API command route](developer-api.md#commands-and-screenshots).

## Snapshots and measurement meaning

`devtools/snapshot.rs` builds a versioned snapshot at most
every 250 ms. It includes f64 camera position (parsecs from galactic centre),
debug switches, stream counts, performance averages, latest CPU timings and
one-second peaks, fresh GPU diagnostics with measurement ages, and
per-frame history (10 seconds, capped at 4,096 frames).

- History collects with UI closed and file export disabled.
- Frame intervals precede the recorded CPU work. GPU measurements are delayed
  and not matched to individual CPU frames.
- Peaks are component-wise maxima, not one correlated slow frame.
- Each frame has `sync`: uploads, draw list, precise rescan/sprites and counts,
  plus render-world extract/prepare (a frame or so behind).
- `instrumentation` records recording, snapshot building and window layout:
  the cost of measurement itself. The perf tab shows `star sync` and
  `instrumentation`.
- New per-frame measurements must be fixed-size numbers, no strings/allocation.
- Publication has a history placeholder plus a plain frame copy;
  HTTP/file writers encode history only when needed (`snapshot::Published::full`).

### Optional file export

`NIGHT_SKY_SNAPSHOT=1 ./scripts/run.sh` enables atomic JSON export to
`data/logs/live-snapshot.json`, at most four times a second. Final JSON encoding
and disk writes use a separate thread with a bounded nonblocking queue. Export
is off by default and includes process ID/publication time; check both because
the file can remain stale after exit.

Enabling file export does not itself introduce commands, movement, benchmarks
or an HTTP listener; the run script independently enables the API by default.
Snapshot construction is main-thread CPU work, so exported diagnostics are opt-in.
The API can enable shared snapshots without file export.
