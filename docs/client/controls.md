# Controls and interface

Both clients use the same egui interface over a full-window 3D view. Debug console
(⚙), Tasks (✔) and Object browser (🔍) open from the top right. The top-left **Location** and
**Selected** trails distinguish physical location from selection. Clicking a
breadcrumb inspects it without changing selection or moving the camera.

## Camera and time

| Action | Control |
| --- | --- |
| Look around | Left mouse drag |
| Orbit the camera target | Right mouse drag |
| Roll | Q / E |
| Move | W/A/S/D, R/F for up/down |
| Zoom | Shift + mouse wheel |
| Adjust movement speed | Mouse wheel or drag bottom-left set speed |
| Adjust field of view | Debug console's `render` tab or Shift + left drag |
| Pause/resume simulation | Space or click bottom-centre time rate |
| Adjust simulation speed | Drag time rate, real time to 100 years/s |
| Adjust exposure | Drag Exposure at bottom right, +1 stop doubles light |
| Adjust compression | Drag Dynamic Range Compression, 0 to 100% |
| Adjust visible-star magnitude limit | Drag Magnitude limit, 3 to 10.5; applies on release |

Simulation time is integrated continuously; rate changes never jump orbit phase.
Exposure and compression apply immediately, with defaults of 0 stops and 50%.
Compression balances individual bodies, not the galaxy. Neither is persisted.
See [realistic rendering](rendering.md) for the physical model.

Keys and mouse buttons are read as named actions through one binding table
(`input.rs`), shared by the free camera and piloting, so a later controls menu
can rebind both. The debug console's `controls` tab labels keys from it.

## Piloting

With a ship selected, Tab pilots it: the camera follows it by position and the
keys fly the ship. Piloting holds the clock at 1×, and leaving keeps 1×.

| Action | Control |
| --- | --- |
| Stop piloting | Tab or Esc; throttle, warp and spin carry on |
| Fine thrusters (while held) | W/A/S/D, R/F for up/down |
| Throttle lever up / down | Hold Shift / Ctrl; below zero is reverse |
| Full / cut throttle | Z / X |
| Engage warp, cutting throttle and fine thrust / drop out | G |
| Warp speed up / down (in warp) | Hold Shift / Ctrl, 2 factors of ten per second |
| Slowest warp speed (in warp) | X |
| Steer (virtual joystick) | Hold right mouse and move |
| Pitch and yaw at full deflection | Arrow keys |
| Roll | Q / E |
| Stop rotation | T |
| Look round the ship without steering | Left mouse drag |
| Camera distance | Mouse wheel |

Keys become ordinary ship orders only when they change, at the ship's next
step, so piloting uses the same order path as the CLI. Turning is the raw
`rotate` input: in sublight the spin keeps growing while held and persists when
released; in warp it steers the bubble. Fine thrusters and the throttle do
nothing in warp. Warp speed is a lever like the throttle: releasing Shift or
Ctrl holds the cruise, and warp engages at the last speed set, which the
readout shows before G, with a note when G will cut thrust.

Mouse steering is a virtual joystick. While right mouse is held, the pointer
is hidden and held still, and the mouse's motion moves a stick from the press
point: past a small dead zone (8 points) it gives pitch and yaw, reaching full
deflection at a ring about 150 points out, where it stops. A reticle shows
the ring, dead zone and stick. Holding still keeps the deflection; releasing
returns it to zero. Mouse up is nose up; the `controls` tab's Invert mouse
pitch reverses that until restart. The stick holds the same pitch and yaw
actions as the arrow keys, at part strength, so a flight stick or gamepad
axis can later bind to them the same way.

## Selection, inspection and travel

| Action | Control |
| --- | --- |
| Select a visible star | Click it |
| Visit its system | Click the selected star again or press Enter |
| Select a planet/task | Click its marker or the task index |
| Select a star or pair of the loaded system | Click its marker (a pair's is a small cross) |
| Reach a marker hidden by a crowd | Zoom in until it separates, or use the breadcrumb or search |
| Follow a planet | Click the selected planet again or use `follow` |
| Inspect without travel | Click an item in either breadcrumb trail |
| Fly to/frame a system | Inspector's Fly to system / Frame system |
| Fly to and follow a planet | Inspector's Fly to planet |
| Compare planets in a row | Inspector's Table view |
| Frame a list's systems | Inspector's Constellation view |
| Browse list systems | Previous system / Next system or a System n line |
| Visit a list | Click it in Tasks |
| Stop following and reframe | Escape while tracking |
| Look or move round a followed object | Mouse or WASD: it stops centring, but the camera keeps moving with it |
| Clear selection | Escape when not tracking |
| Move up physically | Escape with nothing selected |

Picking uses loaded, magnitude-visible stars. Wait for streaming if a target is
not selectable. Generated systems have stable catalogue names, but they are not
catalogued real astronomical objects. Lists attach fictional tasks to generated
planets; task edits and ordering do not change surviving bodies' orbital identities.

Presentation modes are separate from tracking: **Simulation** uses current orbital
positions, **Table** arranges planets for comparison while preserving physical
radii, and **Constellation** frames connected list stars without moving them.
The underlying simulation continues in every mode.

## Debug tools

Toggle the debug console with the cog, F3 or the backtick key on either client.
Toggle the object browser with the 🔍 icon or Ctrl+K. Browser shortcuts can
also be intercepted by the web browser; the icon is always available.
There is no separate Settings window. Display options and realistic lighting
live in `render`; shortcut help lives in `controls`.
Scene overlays (`set_display overlays false`) hides everything drawn over the
scene that is not in it: markers, rings, labels, the velocity marker, orbital
and predicted craft paths, wireframes over lit bodies and constellation lines.
Each of those keeps its own switch, nested beneath it. Clicks still select
bodies while overlays are hidden.
Click the left side of a checkbox row to toggle it. The square `+` / `-` button
on the right expands or collapses its explanation without changing the switch.
Only the console header (including its padding) moves the window, indicated by
a four-way arrow cursor.

The browser, Tasks and debug console use the same movable/resizable window
frame. Close glyphs align with their title and hover red. Scrollbars have
invisible tracks and subtle accent handles that brighten smoothly on hover or
drag, without changing width. Tooltips are opaque and match the owning theme.
See the [shared UI kit](ui/kit.md) for styling and interaction details.

| Tab | Contents |
| --- | --- |
| `log` | Recent client events, level filter and grep |
| `terminal` | Commands, history and output |
| `perf` | Frame times, generation, streaming and GPU metrics |
| `render` | Display options, field of view, realistic lighting, exposure facts and reference views |
| `controls` | Camera, selection, time and console shortcuts |
| `state` | Camera, navigation, selection and clock |
| `tools` | Native bug reports and performance benchmarks |

In the terminal tab, `help` lists commands. Examples: `set_presentation_mode table`,
`set_display orbits false`, `galaxy_reference face_on`. Arguments can be positional
or `field=value`; quote values with spaces. Up/down recall history; `clear` empties
the tab. The native launching terminal accepts the same commands via standard input.
