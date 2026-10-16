# Craft and their physics

Craft are man-made objects: ships now, later stations, rovers and probes
(`crates/simulation/src/craft.rs`). Unlike stars and planets they are not
generated from seeds. They are live state that changes every step, with IDs
of their own (`craft-1`), and they never shift any generated object. See the
[flight simulator plan](../plans/flight-sim.md) for where this is heading.

## Model

| Part | Meaning |
| --- | --- |
| `kind` | `ship`, `station`, `rover` or `probe`; only ships exist yet |
| `frame` | What the position is measured from: a star system, by its page star's ID |
| `motion` | How it moves; only `Free` (integrated) exists yet |
| `seconds` | The simulation time its state describes |
| `input` | Persistent throttle, translation and rotation fractions; legacy frame-axis thrust remains for the current CLI |
| `orientation` | Unit quaternion mapping body axes to system axes |
| `spin` | Body-local angular velocity in rad/s, without passive damping |
| `thrusters` | Per-ship main, reverse, fine and angular acceleration limits |

A system frame uses the same origin and axes as the bodies' orbits: metres
from the page's outermost barycentre, in its plane axes (see
[orbits](orbits.md)). Positions are f64 metres and velocities m/s.

The model is serde-serializable and engine independent, so a server can run
the same code. A craft changes only through its input, applied at fixed
steps, never by editing its state, apart from placing it when it is built.

### Body-axis control (simulation)

Body axes are +X right, +Y up and -Z forward. Rotation inputs are right-handed
about X (pitch), Y (yaw) and Z (roll). Each input is a finite fraction in
[-1, 1], validated atomically by `control`. Negative throttle uses the reverse
drive and is refused when that drive is disabled. Fine translation remains
available independently. Main and fine thrust add together.

Default main and reverse drives deliver 9.80665 m/s²; fine translation delivers
0.980665 m/s² per axis. Turning delivers 0.1 rad/s² per axis, an independent
angular limit, not a linear acceleration in g. There is no inertia or fuel
model yet. With no rotation input, spin persists. A midpoint angular step
updates and normalizes the quaternion; thrust in the two Verlet half-kicks
uses the starting and ending attitudes.

These controls are integrated through the client's `steer_craft` orders and
`ns ship` subcommands. See the [CLI guide](../client/developer-api.md#craft)
for input persistence, command grouping and examples.

### Attitude guidance (simulation)

`point` engages a persistent pointing autopilot; `stop_rotation` brakes spin
without changing linear velocity or throttle, and `release` returns to manual
rotation. A nonzero manual rotation input also releases the autopilot. Manual
throttle and translation do not cancel pointing. The autopilot is serialized
with the craft, including its roll reference.

Each fixed step resolves prograde/retrograde from velocity relative to the
[orbit reference](#orbit-reference), radial in/out from relative position, and normal/anti-normal
from their cross product. A body target follows its instantaneous position;
a vector target is fixed in system axes. `Field` supplies the same analytic
body states used elsewhere. A missing body or degenerate direction (zero
relative speed, for example) makes guidance brake spin and retry next step.

Pointing uses the shortest quaternion error, a bounded desired angular rate,
and per-axis acceleration limited by the real turning thrusters. It never
sets attitude or spin directly. A damped approach slows near the target;
rest-to-target turns settle without overshoot in the regression tests. An
arbitrary initial spin may make overshoot physically unavoidable, and a
rapidly moving target may outrun a weak ship's turning capability. Disabled
axes cannot be controlled. Moving targets are tracked with a small lag, not
by instant alignment.

Without an explicit up vector, the desired attitude is parallel transported
as the direction changes, preserving roll without a world-up pole. An
explicit up vector fixes roll by its projection perpendicular to the nose.
When that projection vanishes, guidance retains transported roll until up
becomes usable again. Linear acceleration in g does not determine rotational
performance: the independent angular limits are in rad/s².

## Gravity

A craft feels the summed pull of every star and planet of its system, each at
its analytic Kepler position at that instant (`Field`). It is a test
particle: it pulls on nothing, and the bodies stay on their fixed orbits.

$$
\mathbf a = \sum_i \frac{GM_i}{r_i^3}\,\mathbf r_i
$$

where $\mathbf r_i$ points from the craft to body $i$. $GM_\odot$ is derived
from the same year and AU as Kepler periods,
$4\pi^2\,\mathrm{AU}^3/\mathrm{year}^2 \approx 1.327\times10^{20}$ m³/s², so a
craft placed on a planet's orbit follows it exactly. Planet masses use
$M_\oplus = 3.0035\times10^{-6} M_\odot$.

Inside a body, the pull is that of a uniform sphere, $GM r/R^3$, falling to
zero at the centre. This only keeps the maths finite: there are no collisions
yet, so a craft can pass straight through a planet.

## Orbit reference

The **orbit reference** (shown as the craft's parent, "Around") is what its
orbit is described around: a planet, one star, or a pair's barycentre
(`craft/reference.rs`). It is a label for readouts, prediction and the
orbital pointing targets, so all of them describe the same orbit. Gravity
still comes from every star and planet; a barycentre never pulls.

- **Planets first:** the nearest planet whose Hill sphere holds the craft,
  $r_H = q\,\sqrt[3]{m/3M}$ at the planet's periapsis $q$.
- **Otherwise, down the stellar hierarchy.** Starting from the whole system,
  step into a pair member while the craft is inside that member's stable
  zone, the region where a planet could orbit it alone against its
  partner's pull (Holman & Wiegert 1999, the same prescription that places
  planets; see [stars](stars.md#understanding-the-companion-hierarchy)).
  Near one star of a pair the reference is that star; between or around
  both it is their barycentre; outside every zone it is the whole system.
- **Switching margin:** the current reference, and everything it lies within,
  is kept until the craft is 10% past its region (`SWITCH_MARGIN`), so the
  label does not flicker on a boundary. Entering a new region needs no
  margin.
- **Kept with the craft:** the step chooses it again once a simulated second
  (`REFERENCE_SECONDS`), which costs more than a step, and stores its ID in
  the serialized craft so a server or saved craft chooses the same way.

These are display and guidance heuristics: being "around" a body does not
mean the orbit is bound or stable. The reference's GM is everything inside
it (a pair's stars together), for the two-body readouts below.

## Integration

- **Fixed steps** of `STEP_SECONDS` (1/20 s) of simulation time, shared by
  every client and a future server, whatever the frame rate.
- **Velocity Verlet** (kick, drift, kick). It is symplectic, so orbital energy
  oscillates slightly rather than drifting: around a fixed Earth, an
  eccentric orbit from 400 km keeps its energy to better than $10^{-8}$ over
  100 laps (11 million steps). Thrust is applied in both half kicks.
- Gravity at the end of each step is kept for the start of the next, so a
  step costs one field evaluation (about 1 µs natively for a star with ten
  planets). It is not serialized: a loaded craft recomputes the same value
  and steps identically.
- `advance` steps until the craft reaches a time, with a cap on steps so a
  long stall cannot freeze the frame.
- Native and WebAssembly may differ in the last digits of trigonometry and
  powers, so later client correction must tolerate small differences.

The craft tests run natively and, through `wasm-bindgen-test-runner` under
Node, as WebAssembly (`./scripts/check.sh`).

## Coast prediction

`craft::predict` forecasts where a craft goes if it coasts from now: a copy
of its position and velocity, every propulsion input off, under the same
summed gravity and Verlet kick, drift, kick as real flight. The real craft
and clock are never touched.

- **Adaptive step:** each step is 0.5% of the shortest free-fall time
  $\sqrt{r^3/GM}$ to any star or planet, about 1,250 steps per circular
  orbit, never shorter than a real step. Steps shrink near periapsis and
  close encounters, and stretch far from everything. With varying steps the
  integration is no longer symplectic, which does not matter over one
  forecast: one lap of an eccentric low Earth orbit ends within 2 km of the
  real fixed steps.
- **Relative to one reference:** every sample is the craft's state minus the
  start reference's state at that sample's time. So an orbit around a planet
  closes relative to it instead of smearing along the planet's own path.
  Perturbations from other bodies still show, and closure is not promised.
- **Bounded horizon:** 137% of the period when the two-body orbit around
  the reference is bound: 102% so a whole revolution is predicted even when
  other bodies stretch the lap, and a 35% lead that keeps one ahead until a
  newer prediction replaces it. Only one revolution is shown: it ends where
  the path has swept 360° around the reference from the craft
  (`Prediction::lap_end`), measured in angle rather than time so it ends
  level with the craft.
  Otherwise until the craft would have gone twice
  the reference's reach (its Hill sphere or stable zone) at its current
  speed. At most 100 years, and at most 6,000 steps, each kept as a sample.
- **How it ended:** the planned horizon, `Truncated` when the step budget ran
  out first, or `Impact` at a body's surface. There are no collisions, so
  the real craft would pass through; the forecast stops there instead.
- **Events:** sampled periapses and apoapses, where the radial speed changes
  sign, placed on the cubic between samples; and predicted reference
  changes, using the same selection and switching margin as the real craft.
  The path is not re-referenced after one.

`Predictor` does this in slices of steps, for spreading over frames; slices
give exactly the same path as one go. Each step costs about 4 µs natively,
so a forecast of about 1,750 steps costs about 7 ms in total.

## In the client

The client keeps live craft in `craft::Fleet` (`crates/client/src/craft.rs`),
in memory only: nothing is saved or shared yet.

- **Stepping:** `craft::step` runs after the clock advances each frame and
  steps every craft until it reaches the clock, at most
  `MAX_STEPS_PER_FRAME` (1,000, about 50 s of simulation time) per frame.
  A craft that cannot keep up **holds the clock** back to its own time, so
  bodies and craft never disagree; the time HUD then reads `· held by craft`.
  At the default 1 day/s, time runs at roughly 3,000 times real time with a
  ship present. Pausing stops craft too.
  Drive inputs, spin, or an engaged attitude pilot cap advancement at 10×
  before physics steps, without overwriting the requested clock rate. Quiet
  manual coasting restores the requested rate. The HUD's `held by craft`
  covers either this manoeuvre cap or the catch-up hold.
- **Forecasts:** each craft keeps its last finished
  [coast prediction](#coast-prediction) (`craft::Forecast`) on show while a
  newer one is worked on. `craft::forecast` runs after stepping and shares
  250 prediction steps a frame (120 in WebAssembly) evenly between craft,
  so a lap takes several frames. A new prediction starts when there is
  none, when the inputs or orbit reference differ from the shown one's
  (dropping any work begun under the old ones), throughout a burn, and while
  coasting once the craft is a quarter of the shown period (or horizon,
  when unbound) past its start, inside the 35% lead.
  Work in progress always finishes before another starts, so a burn's
  constant refreshes never starve it. Paused, nothing is redone. `ns ship
  state` reports the shown prediction's reference, age, horizon, how far it
  reached and how it ended, and whether it is current and being updated.
- **Drawing:** a craft is drawn between its last two steps, blended to the
  clock's time, with shortest-arc quaternion interpolation for attitude.
  The placeholder is a 30 m long wireframe cuboid with a pyramid nose (-Z)
  and two wings rooted on its bottom edge (-Y), plus a marker and label.
  Forward and top are distinguishable without model assets. Only craft in
  the visited system, on the page their frame names,
  are drawn, picked or followed.
- **Predicted path:** the selected craft's shown prediction is drawn from
  the craft onwards (`craft::predicted_path`), dimmed once it is out of date.
  A current prediction's whole revolution is joined back to the craft, so a
  bound orbit is one closed loop; the join covers drift from other bodies
  since the prediction started, at most a quarter of a lap.
  Its samples are relative to the prediction's reference, so they are added
  to where that reference is now, not where it will be: an orbit around a
  moving planet or barycentre is drawn closed around it. Diamonds mark the
  predicted apsides, reference changes (amber) and an impact (red). Points
  are placed in f64 relative to the camera before conversion to render units,
  so the path holds at any zoom.
- **Spawning** (`spawn_ship`): above a planet, star or barycentre, or a system
  by its key (its star, or outermost barycentre). Its system must be loaded,
  visited or charted. The ship starts on the body's far side from the body's
  own parent, in the plane of the body's orbit and turning the same way:
  circular by default, or at a given prograde speed, or a given velocity in
  the system's axes. Speeds are relative to the body.
- **Selection and following:** `Selection::Craft` and `ObjectRef::Craft`.
  Clicking a craft's marker selects it; a second click, **Follow ship** or
  `fly_to_craft` follows it from 200 m, visiting its system first if needed.
  The camera follows a craft as it follows a planet (see
  [navigation](../client/navigation.md#camera-frames-and-controls)). The
  selection trail puts a craft below the planet it is around, else its
  system's star.
- **Search:** ships are found in the object browser and `ns search` like
  any object (`kind=ship`, `family=craft`, `altitude_km`, `speed_kms`,
  `orbits.…`, or by name); see [live objects](search.md#live-objects).
- **Inspector:** type, system, the body it is around, altitude, speed and
  two-body periapsis, apoapsis and period around that body (`Elements`,
  which ignore every other body), then the shown prediction: its reference,
  how far ahead it reaches, its age and whether it is out of date or
  updating, and the next predicted periapsis and apoapsis, exits,
  encounters and impact, each with the time until it. Predicted apsides
  come from the sampled path and include every body's pull; the two-body
  ones are instantaneous estimates. Then legacy thrust, throttle, spin and
  attitude pilot. These change every frame, so craft facts are never cached.
- **Spawning picks the reference** at once, so the first prediction is made
  under it.

What is seen (streaming, render precision, the visited system) stays
camera-based even while following; what the ship does (gravity, altitude,
thrust) stays craft-based. A ship cannot leave its system yet.
