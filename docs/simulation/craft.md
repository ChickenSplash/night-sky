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
dominant body, radial in/out from relative position, and normal/anti-normal
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

The **dominant body** (shown as the craft's parent) is the nearest planet
whose Hill sphere holds the craft, $r_H = q\,\sqrt[3]{m/3M}$ at the planet's
periapsis $q$, otherwise the star pulling hardest. It is a label: gravity
still comes from every body.

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
- **Drawing:** a craft is drawn between its last two steps, blended to the
  clock's time, with shortest-arc quaternion interpolation for attitude.
  The placeholder is a 30 m long wireframe cuboid with a pyramid nose (-Z)
  and two wings rooted on its bottom edge (-Y), plus a marker and label.
  Forward and top are distinguishable without model assets. Only craft in
  the visited system, on the page their frame names,
  are drawn, picked or followed.
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
  which ignore every other body), legacy thrust, throttle, spin and attitude
  pilot. These change every frame, so
  craft facts are never cached.

What is seen (streaming, render precision, the visited system) stays
camera-based even while following; what the ship does (gravity, altitude,
thrust) stays craft-based. A ship cannot leave its system yet.
