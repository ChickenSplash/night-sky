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
| `frame` | A star system by its page star's ID, or a fixed galactocentric anchor with local metres in galactic axes |
| `motion` | How it moves; only `Free` (integrated) exists yet |
| `tick` | The world step its state describes; its time is `tick / 20` seconds |
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

Every change a pilot asks for is an `Order` (`craft/order.rs`): drive
inputs, pointing, thruster limits, legacy thrust and warp. `Order::apply`
validates and applies it in one step, so any client or server refuses the
same orders and nothing else edits a craft's input. `configure` changes the
ship itself; a server should take it only from an administrator. The client
sends orders through `steer_craft` and `ns ship`; see the
[CLI guide](../client/developer-api.md#craft) for input persistence, command
grouping and examples.

`Voyage::order(tick, order)` is the step-keyed submission path. An order at
the current `craft.tick` changes inputs now and affects the next uncomputed
interval, from tick N to N+1, never motion already integrated for display.
Future orders are queued, in submission order within each tick, and applied
as soon as the craft reaches that boundary. Past ticks are rejected, not
silently retimed. Future orders are validated against the drive state and
frame at application, so a queued disengage followed by thrust works. A
refusal is returned with that step, consumed once, rather than accumulated
inside the simulation. Waiting for system data consumes neither steps nor
queued orders.

Each voyage accepts at most 1,024 pending orders, at most 300 simulated
seconds ahead of its current tick. Value checks (finite numbers, ranges and
nonzero directions) run at submission; drive interlocks and body checks run
at application. Long replay logs must be fed incrementally as ticks advance.

`Voyage::timed_order(tick, end, order)` gives an order for a time: the order
at `tick` and its `Order::off` at `end`, queued together or refused together.
The off order puts the order's controls at rest and keeps motion and spin:
zero throttle, translation, rotation or legacy thrust (`point` ends as a zero
`rotate`), and disengage for warp engage. Other orders have no rest state and
cannot be timed. The pending end is tagged with its start step and dropped if
its start is refused, or once a later order on the same controls (throttle,
translation, turning, thrust or warp engagement; `cut` covers all but warp)
applies after that start, so an earlier burn's end never cuts a newer one.
An end due at the same step as another order applies first, so the other
order wins. A timed order uses two of the 1,024 pending slots.

`ScheduledOrder` is a serializable tick/order record for replay, with
`ends_from` set on timed orders' ends; `pending_orders()` supplies the
remaining queue to save alongside a craft snapshot. Reconstruct its field
and frame origin, then resubmit the records through `Voyage::schedule` to
restore it. JSON parsing uses
round-trip f64 precision so a snapshot does not lose physics bits. This is
replay infrastructure, not a multiplayer protocol: ownership, administrator
permissions, player rate limits, acknowledgements and rollback/correction
still belong to future server work. Cross-target correction must still
tolerate the numerical differences noted below.

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
  (`REFERENCE_STEPS`), which costs more than a step, and stores its ID in
  the serialized craft so a server or saved craft chooses the same way.

These are display and guidance heuristics: being "around" a body does not
mean the orbit is bound or stable. The reference's GM is everything inside
it (a pair's stars together), for the two-body readouts below.

## Integration

- **Fixed steps** of `STEP_SECONDS` (1/20 s) of simulation time, shared by
  every client and a future server, whatever the frame rate.
- **One world step grid.** A craft stores the number of the step it is at
  (`tick`), counted from simulation time zero, and derives its time from it
  (`tick_seconds`). A craft placed at any time starts on the first boundary
  at or after it (`tick_at`), so every craft steps on the same instants, a
  step number names the same moment on any machine, and orders can be keyed
  by it. Time never accumulates rounding from repeated additions.
- **Velocity Verlet** (kick, drift, kick). It is symplectic, so orbital energy
  oscillates slightly rather than drifting: around a fixed Earth, an
  eccentric orbit from 400 km keeps its energy to better than $10^{-8}$ over
  100 laps (11 million steps). Thrust is applied in both half kicks.
- Gravity at the end of each step is kept for the start of the next, so a
  step costs one field evaluation (about 1 µs for a star with ten
  planets). It is not serialized: a loaded craft recomputes the same value
  and steps identically.
- `advance` steps until the craft reaches a time, with a cap on steps so a
  long stall cannot freeze the frame.
- Different CPUs or libm builds may differ in the last digits of trigonometry
  and powers, so a future server's correction must tolerate small differences.

## Coast prediction

Coast forecasts are cleared during warp and in the galactic frame, rather than
showing the departure orbit as a forecast of warp travel. A new forecast starts
after dropout in a system.

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
  the reference's reach (its Hill sphere or stable zone) at its hyperbolic
  excess speed $\sqrt{2E}$, the speed it keeps once far away. It is always
  faster than that, so even a slow escape is predicted past the edge. At most 100 years, and at most 6,000 steps, each kept as a sample.
- **How it ended:** the planned horizon, `Truncated` when the step budget ran
  out first, or `Impact` at a body's surface. There are no collisions, so
  the real craft would pass through; the forecast stops there instead.
- **Events:** sampled periapses and apoapses, where the radial speed changes
  sign, placed on the cubic between samples; and predicted reference
  changes, using the same selection and switching margin as the real craft.
  The path is not re-referenced after one.

`Predictor` does this in slices of steps, for spreading over frames; slices
give exactly the same path as one go. Each step costs about 4 µs,
so a forecast of about 1,750 steps costs about 7 ms in total.

## In the client

The client keeps live craft in `craft::Fleet` (`crates/client/src/craft.rs`),
in memory only: nothing is saved or shared yet.

- **Stepping:** `craft::step` runs after the clock advances each frame and
  steps every craft until it reaches the clock, at most
  `MAX_STEPS_PER_FRAME` (1,000, about 50 s of simulation time) per frame.
  Craft step in lockstep (`Fleet::catch_up`): each round steps those
  furthest behind, every one given the others as they were as its step
  began (`Voyage::traffic`, a `Sighting` per craft), so craft that target
  each other fly the same paths whatever order they are stepped in. A craft
  waiting for system data stops the whole fleet for the frame, so none
  reads another that loading has left behind.
  A craft that cannot keep up **holds the clock** back to its own time, so
  bodies and craft never disagree; the time HUD then reads `· held by craft`.
  At the default 1 day/s, time runs at roughly 3,000 times real time with a
  ship present. Pausing stops craft too.
  Drive inputs, spin, or an engaged attitude pilot cap advancement at 10×
  before physics steps, without overwriting the requested clock rate. Quiet
  manual coasting restores the requested rate. A craft whose next step
  [waits for system data](#frames-and-arrival) holds the clock the same way
  until a background worker supplies it. The HUD's `held by craft` covers
  the manoeuvre cap and either hold.
  Pending sublight or pointing orders also conservatively apply the manoeuvre
  cap, so a catch-up frame cannot run a newly scheduled burn at the high
  coasting rate. Pending warp-only orders do not apply that cap, and neither
  does steering the bubble of an engaged warp.
- **Forecasts:** each craft keeps its last finished
  [coast prediction](#coast-prediction) (`craft::Forecast`) on show while a
  newer one is worked on. `craft::forecast` runs after stepping and shares
  250 prediction steps a frame evenly between craft,
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
  Forward and top are distinguishable without model assets. Craft render and
  can be followed across system and galactic frames, independently of the
  visited system.
- **Predicted path:** the selected craft's shown prediction is drawn from
  the craft onwards (`craft::predicted_path`), dimmed once it is out of date.
  A current prediction's whole revolution is joined back to the craft, so a
  bound orbit is one closed loop, but only when it ends within 1% of the
  craft's distance from the reference (`Prediction::closing_lap`). A
  revolution that another body's pull has really bent is left open,
  showing the changed orbit rather than a join on no path.
  The real craft drifts from an older prediction by metres to hundreds of
  metres, and a revolution ends up to 1% off where it began. Rather than
  kink at the craft when seen close up, both differences are spread
  smoothly along the drawn path, so it starts at the craft and a closed
  loop ends there too.
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
  the system's axes. Speeds are relative to the body. The system frame is
  anchored where seeds put its star (`voyage::frame_origin_pc`), the same
  place arrival uses, never where the camera has streamed it, so leaving and
  re-entering the spawn system cannot shift the ship. A legacy list page with
  no cell star is anchored where its list places it. The first spawn works
  out the galaxy's coarse cells on the frame, about 150 ms once.
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
thrust and frame handoffs) stays craft-based and never depends on the camera
or on streamed stars.

## Warp core

Warp is bubble transport along the ship's nose, commanded through the CLI/API,
and steered by turning the nose (see [warp steering](#warp-steering)), by
hand or by the [route pilot](#plotted-routes). There is no turn-limit,
collision or approach assist, fuel or energy budget yet. Explicit matched dropout is described
below. Manual warp is deliberately unsafe:
there are no body collision checks, atmosphere exclusions or missing-data stops.
It can engage inside a body and pass through stars and planets, with no crash
or damage response. Arrival detection is separate and does not stop the drive.

- **Two motions:** each 0.05 s step adds ordinary drift and bubble displacement.
  Bubble motion never becomes ordinary velocity on dropout.
- **Orders take effect from the next step:** every order, warp included, changes
  the craft's input or drive state and acts from its next fixed step (its
  `tick`), so a server can key inputs by step number. Physics brackets the clock with a step that may
  end up to 0.05 s ahead of the displayed pose, so that step still plays out
  after a command. After a disengage the ship keeps travelling for at most one
  more step (up to 0.5 ly at 10,000 ly/s). Ordered while paused, that last step
  is seen once time resumes.
- **Drive interlock:** all sublight thrust must be zero before engaging warp.
  Nonzero throttle and fine/legacy translation orders are refused during warp,
  not silently cleared. Disengage before using them. Turning is not
  interlocked: during warp it steers the bubble.
- **Gravity policy:** `active` by default, evaluated at the current travelled
  positions and integrated into ordinary velocity. The developer alternative
  `suspended` retains ordinary velocity but not position. At and above light
  speed (`GRAVITY_CUTOFF_MS`, judged by each step's average bubble speed)
  gravity is ignored under either policy. Each 0.05 s step then spans at
  least 15,000 km, and sampling gravity only at its ends could land deep in a
  dense body and give a large, arbitrary kick, while a real pass that fast
  changes velocity by at most metres per second for a main-sequence star.
  Below light speed, a step ending close to a white dwarf can still give an
  oversized kick. Gravity resumes on dropout; changing policy requires
  disengagement. Neither policy matches a
  destination. This is a game rule, not a relativistic spacetime solution.
- **Speed:** 1 m/s to 10,000 ly/s, matching the free camera's maximum,
  with each up/down multiplying/dividing by
  √10. A held sweep (-1 to 1, piloting's Shift/Ctrl) moves it each step at up
  to 2 factors of ten per second, so every decade takes as long; it needs warp
  engaged, ends at dropout and takes over a route. Requested speed persists;
  actual speed approaches it with a 0.3 simulated
  second exponential response. Displacement integrates that ramp. Pause stops
  motion; warp alone does not activate the sublight manoeuvre time cap. Normal
  fixed-step catch-up limits still apply.
- **Gravity locality:** the current system supplies ordinary-flight gravity
  during warp under the active policy. In a galactic frame no local-system
  gravity is applied, and distant systems are never summed. This is an
  explicit alpha approximation, not a complete galactic gravitational field.
- **One step for both drives:** `Craft::step` integrates ordinary flight or,
  while warp is engaged, bubble transport, so anything stepping a craft
  (client, tests, a future server) moves it the same way.

### Warp steering

The drive turns the bubble and the ship inside it together, without the
turning thrusters. This is a game rule, in keeping with steering above light
speed (see the [plan](../plans/flight-sim.md#warp-drive-rules)): in the
Alcubierre picture the ship floats freely in flat space inside the bubble, so
rotating it with the bubble resembles frame dragging, not anything the metric
predicts. The crew would feel no turning force.

- **Rate, not acceleration:** the same rotation fractions and attitude pilot
  as ordinary flight set a target turn rate in body axes. Manual input is the
  fraction times `turn_rate_dps`, a per-ship setting from 1 to 360 °/s
  (default 30) that can change engaged or not. Several axes share it: the
  total rate never exceeds it.
- **Ramp:** the bubble's rate (`spin`) approaches the target with the same
  0.3 s exponential response as speed, integrated exactly over each step.
  Releasing steering ends the turn with no residual spin.
- **The path bends with the nose:** each step's bubble displacement runs along
  the step's midpoint attitude, so a steady turn traces a circle of radius
  speed over turn rate. Roll turns the ship about its direction of travel
  without changing the path. Ordinary velocity is never rotated.
- **Pointing:** `point` drives the bubble's rate in proportion to the
  attitude error, at a gain that critically damps the rate ramp with an
  allowance for each step's held target. Turns from rest settle without
  overshoot in the regression tests, never above the turn rate. `stop` and
  `cut` target zero rate.
- **Engaging and dropout:** engagement takes over any spin and eases it to
  the steering target. Disengaging or a matched dropout leaves the ship with
  no spin; a held rotation input or pointing pilot then passes back to the
  turning thrusters, unchanged.

### Plotted routes

`ns ship warp to <id> [--match | --orbit] [--time <s>]` plots a trip to a
planet, star, barycentre or system key (its star or outermost barycentre),
in the ship's current system or another, engages warp and flies it
(`craft::route`). The
route pilot is ship state beside the attitude pilot. Each step, before the
ship moves, it gives the orders a player would: pointing at a direction,
the requested speed and, at the standoff, dropout. It never moves the ship
itself, so replays and servers run it from the same inputs.

- **Target facts:** each step the pilot asks the field for the target's
  state, size, keep-clear distance, orbit reach, GM and system (`Target`),
  and never checks what kind of object it is. A body's size is its radius;
  a barycentre's is its pair's separation, and it keeps clear of its stars.
- **Standoff:** four sizes from the centre, at least the keep-clear distance
  (1.5 radii, or a barycentre's farthest star plus 1.5 of its radii) and at
  most 90% of the orbit reach, so `--orbit` works on arrival. A star of a
  tight pair whose stable zone barely clears its surface has none, and the
  order is refused.
- **Speed profile:** requested speed is the distance to the target's centre
  over a time constant τ, so every second covers the same share of what is
  left, reaching the standoff after τ × ln(distance ÷ standoff). The request
  is lowered by (1 − 0.3 s ÷ τ) so the lagging speed ramp still flies the
  profile; τ is at least 0.6 s. Speed is 1 m/s while the nose is more than
  10° off, ramping up to full by 2°.
- **Trip time:** 10 to 3,600 s, or by default 20 s plus 10 s for each tenfold
  distance beyond 1 AU, up to 60 s. It includes the first turn, estimated
  from the turn rate and pointing's easing. τ is fitted to it at plotting,
  again once lined up, and once a second until the approach (the last ten
  standoffs), so a trip whose turn ran long catches up. A time that cannot
  be met (a 10 s trip facing away, or a tight detour) is accepted and shows
  as `late_s`, with its cause in `slowed_by`: the first turn, the costliest
  bend, or the top warp speed.
- **Bodies in the way:** every star and planet but the target has a
  keep-clear sphere of 1.5 radii. Each step the pilot checks the line to
  the standoff against them; when one is in the way it aims along the
  tangent to the nearest such sphere, on the side the target lies, and
  checks that tangent against the others in turn. Worked out afresh every
  step, it curves the ship round like a string pulled tight, so moving
  bodies and several in a row need nothing more. A ship starting inside a
  sphere, such as low over a planet, aims along its horizon and climbs out.
  - **Bend speed:** rounding a body, speed is capped so the aim turns at no
    more than half the turn rate, leaving the rest for pointing to follow.
    The lead then uses the tangent's own turn rate, not the line of sight's.
  - **Impact slowdown:** heading into a sphere it is outside of, speed is
    capped to the gap over 1.7 s (pointing's response plus the speed ramp),
    so a late turn never cuts into it.
  - **Timing:** at plotting and once a second, the trip is measured along
    the wrapped path (tangents and arcs round spheres frozen where they are
    now), and τ is fitted to the time left after the bends. A bend of turn θ
    allows (1 − cos θ) over half the turn rate up to a quarter turn, the
    rest at that rate, plus pointing's 1.4 s settling scaled by θ up to a
    quarter turn: a barely hidden target costs almost nothing.
  - **Cost:** one line-to-sphere test per body per step, over the bodies
    gravity already sums; the path is measured only once a second.
- **Aim:** the bubble's wanted velocity closes along the line of sight and
  cancels the drift between the ship's ordinary velocity and the target's,
  so moving targets are met. Flying off the line turns the line of sight
  faster than pointing (a 1.4 s response) can follow once τ is short, which
  would diverge; the pilot aims ahead by the line of sight's turn over that
  response (at most 45°), which keeps the nose on it.
- **Arrival:** at the standoff, `keep` disengages with ordinary velocity,
  `match` takes the target's velocity and `orbit` adds rough-orbit speed, as
  [matched dropout](#matched-dropout) does. The attitude pilot returns to
  manual, with no spin. The ship also drops out when the coming step's
  travel (the speed ramp's integral plus drift) would reach the standoff,
  so it stops at most one step early and can never pass through. That
  matters when τ is pinned at its minimum by too short a time: the ramp
  then cannot slow the ship as fast as the profile asks, and a 2 ly trip
  asked to take 10 s drops out at about 1.8 standoffs, still faster than
  light and so in galactic space, entering the system the next step.
- **Takeover:** `warp cancel`, any turning order (`rotate`, `point`, `stop`,
  `release`, `cut`), a warp speed order, `disengage` or `match` ends the
  route. Warp is left as it was, so nothing jumps; turning returns to manual.
- **Another system:** a target whose system key is not the ship's own
  system, or any target from galactic space, is accepted in the `plotting`
  stage without engaging warp. Each step, `Voyage` first needs that
  system's location (`Wait::Star`, from seeds, or from the lists for a
  legacy page) and then its bodies (`Wait::System`), waiting like any other
  data, so the client holds the clock while they load and a slow machine
  flies the same path. The first step that has them plots the route as an
  in-system order would be, from the same step; a refusal then ends it in
  `released` with the reason, warp still off. While the ship is outside
  the destination, the pilot sees its target and bodies converted into the
  ship's frame (`Rebase` from the destination's frame, system velocities
  included), for aiming and wrapping alike; gravity stays the local
  system's. Leaving, re-anchoring and entering are the usual frame rules:
  the profile only drops below light speed about c × τ from the target, so
  the ship crosses most of the destination's range in galactic space and
  enters on the slowdown, typically a few hundred thousand kilometres out.
  A 2 ly trip in 40 s peaks near 0.6 ly/s.
- **Other craft:** a craft ID targets that craft, in any frame. The pilot
  sees it as it was at the start of the step, converted into the ship's
  frame, and aims along its whole motion, warp bubble included, correcting
  every step; a target pulling away shows a growing ETA. The standoff is
  1 km (`CRAFT_STANDOFF_M`), `--match` gives formation flying beside it,
  matching its ordinary velocity only, never its bubble, and `--orbit` is
  refused. A target that despawns releases the route as cancelling does,
  with "`craft-2` is gone", warp left engaged.
- **Targets in low orbit:** a body whose keep-clear sphere holds the target,
  as a planet's does a ship 400 km up, shrinks to pass through it, so the
  route skims round at the target's height and never lower.
- **Refused when given, leaving the ship unchanged:** an unknown ID in the
  ship's own system, a craft ID with no craft, the ship itself, `--orbit` on
  a craft, no safe standoff, already within the standoff, sublight thrust
  still on, or a trip time out of range.
- **Readout:** `route` in the ship state holds the target and its system
  (empty for a craft),
  arrival mode, stage (`plotting`, `align`, `cruise`, `approach`, `arrived`,
  `released`), trip time, τ,
  standoff, distance, ETA, lateness and its cause, the detour beyond the
  straight line, the body being steered round, whether the top speed
  stretches it and how it ended. It is kept after the route ends.

Below the gravity cutoff, active warp retains its final gravity sample for
the next step, including ordinary flight after dropout. Samples use the
world tick's exact represented time, so snapshot reloads recompute the same
value. Suspended or superluminal steps, placement, frame changes and field
replacement discard the sample.

## Frames and arrival

`craft::voyage` decides which frame a craft flies in, from its own state and
seeded or shared system data alone (`Voyage`). Each fixed step first applies
any frame change due at the current state (`settle`), then moves. The same
inputs give the same path on any machine, wherever the camera is.

- **Leaving:** past twice its system's entry range a craft switches to a
  galactic frame anchored where it is. It needs no data, at any speed.
- **Re-anchoring:** galactic offsets beyond 0.125 ly move the anchor, keeping
  a residual in metres. Only precision; it decides nothing else.
- **Entering:** in galactic space, below `ARRIVAL_MAX_SPEED` (light speed),
  a craft enters the first system whose exact entry range holds it. Faster
  warp stays in galactic space and needs no system data: crossing a system's
  range takes a few steps there, and gravity would barely change the path.
- **Entry ranges** use the camera's convention: four times a system's extent,
  at least 0.02 ly.
- **Which systems:** space is split into 1 pc `Region`s.
  `neighbourhood::candidates` lists every generated system whose entry range
  could reach into one, from seeds alone, with an upper bound on that range
  from its stars' separation plus the 1,000 AU planet limit. A candidate in
  reach then needs its bodies for its exact range and gravity. Charted list
  pages take the place of the generated systems they are assigned to; legacy
  pages with no cell star are added as candidates of their own.
- **Waiting, not guessing:** when a step needs a region, a system's bodies or
  a route destination's location that its `Atlas` does not have yet, it
  returns `Wait` and changes nothing.
  `Library` answers synchronously for tests and offline replay. The client asks its
  background worker and holds the clock until the answer arrives, so a slow
  machine arrives at the same step, only later in real time.
- **Cost:** every star of every tier is placed to know whether it is near, and
  bright tiers hold millions of stars per coarse cell. A region takes about
  10 ms near home, 50 ms in the inner disc and 90 ms in the bulge with
  20 threads (300 ms and 700 ms on one thread), plus about 200 ms the first
  time while coarse cells are integrated. Workers keep their `Sky` warm. A
  region is reused while cached, and below light speed a craft takes years of
  simulated time to cross one. The figures above are historical query timings,
  not timings for the bounded live worker pool described below.
- **Transformations:** position, attitude and ordinary velocity transform
  together (`Rebase`), and the client applies the same change to its
  interpolation states. System velocities are added/subtracted when converting
  ordinary momentum, never matched implicitly. Stars' origins remain static,
  the existing galaxy-model approximation: assigned galactic velocities are
  momentum offsets, not moving stellar ephemerides, so drift relative to
  stars in galactic space is approximate.
  This approximation is retained for phase 5: matching is scoped to bodies
  in the craft's current system, not remote stars in galactic space. A ship
  that leaves a system retains its galactic velocity and can drift relative
  to the static star positions. Subtracting local rotation would not fix
  individual stellar scatter and would change the velocity model, so it is
  not used as a workaround. Consistent matching outside systems requires
  moving stellar ephemerides and corresponding spatial lookup work.
  Persistent frame-axis thrust, fixed pointing directions, the pointing up
  vector and the pilot's retained attitude rotate with a frame handoff;
  body-local inputs and spin do not. Queued orders describe axes at their
  application tick, so unapplied orders are not rotated during handoffs.
- **Shared atlas rules:** the simulation owns list-page lookup, legacy
  candidate inclusion, ID ordering and replacement of generated bodies by
  charted pages. `Library::new(lists)` and the client's asynchronous atlas
  use the same `Cache`, given the same list snapshot. A generated worker
  reply cannot overwrite a charted page. `Voyage::step` also returns the
  pre-motion state, attitude and rebase for interpolation, so the client
  does not duplicate frame-change and stepping logic.
- **Cache lifetime:** the cache normally retains 8 regions and 64 systems,
  evicting least recently used entries. Entries read or supplied in this or
  the previous round are protected, allowing a larger active fleet without
  eviction/reload churn. These are idle-cache targets, not hard memory limits:
  the active working set may exceed them, then shrinks after it goes idle.
  A client round is a frame; the synchronous library uses world ticks and
  should step the fleet together on that grid, not run one ship far ahead.
  Dropping data changes loading cost, never physics decisions.
  The warm generation `Sky` also drops fine cells after a request if its
  estimated cache storage exceeds 32 MiB, retaining coarse cells through
  level 3. This bounds retained fine-cell growth on long voyages, not the
  temporary memory needed to answer one query or allocator overhead.
- **In the client:** `Fleet::charts` uses the shared background loader below.
  Worker failure reports `system_loading_error` and leaves the dependent step
  waiting for a retry. There is no synchronous generation fallback in live
  updates. Spawn-origin lookup remains synchronous, as documented above.

### Matched dropout

`ns ship warp disengage` preserves ordinary momentum. The explicit alternative,
`ns ship warp match <body-id> [--orbit]`, ends warp and replaces ordinary velocity
at the command's application tick. It resolves the same stable planet, star or
barycentre IDs used by inspection, selection and orbit references, against the
craft's current system, or a system key as its star or outermost barycentre.
Galactic-space and remote matching are refused, except of another craft:
`warp match <craft-id>` takes its ordinary velocity as it was at the start of
the step, from any frame, for formation flying (`--orbit` refused). A
plotted route's `--match`
and `--orbit` arrivals use the same rules, except that a route to another
system sees its target converted into the ship's frame, so it can match on
arrival before the ship has entered the system. There is no separate target picker
or persistent matching setting; in-app selection-to-flight actions remain
later work.

Without `--orbit`, the result is the body's analytic velocity at that tick.
This alone does not support the ship against gravity. With `--orbit`, the drive
adds tangential speed `sqrt(GM/r)` at the existing position. Direction follows
the tangential part of the ship's ordinary velocity relative to the body;
if absent or under about 0.06° off radial, it uses the projected ship-forward
direction, then ship-up. Heading is unchanged, and no teleport or thrust burn
occurs. A position inside/on the reference's surface, at a barycentre's centre,
or outside the reference's reach (a planet's Hill sphere, a star's or
barycentre's stable zone) cannot request a rough orbit, as another body
dominates there. Velocity-only matching has no reach limit.
This is only an instantaneous two-body circular estimate: perturbations, nearby
bodies and unstable regions can spoil it. It does not certify a safe orbit or
replace later circularisation control.

Matching requires engaged warp and a valid reference. Refusal changes nothing,
including the engaged drive. Successful matching is recorded in
`warp.last_match` with body ID, orbit flag, tick and system-frame velocity at
exit; the inspector shows its body and mode. This is a historical receipt,
not the current velocity or a standing target. A new engagement clears it.
Both gravity policies use this explicit replacement; unmatched exits retain
their existing ordinary momentum rules. Position, heading, tick and inputs
are unchanged, and ordinary gravity resumes on the following step.

### Background preparation and future server hosting

`craft::voyage::loading::Loader` shares the atlas cache, request deduplication,
priority and prefetch rules between the client and a future server. Generation never runs in its polling or stepping methods.

- At most 128 requests are queued and two are in flight. Required step data
  displaces speculative queued work and dispatches first. Queued interests
  expire each host round and are refreshed by the fleet, so turns, speed changes
  and despawns discard obsolete work. Already running work finishes and may be
  cached; it cannot be preempted, but is bounded to two jobs.
- Generation uses two warm workers with one generation thread each,
  rather than an all-core lookup per ship. Failures are
  retried without running expensive work on the caller.
- Prefetch considers the current region and a five-simulated-second projected
  endpoint, then only candidate bodies whose upper entry range intersects
  that short segment. It requests at most eight dependencies per ship per round.
  Ships well inside a system need no regional census. Sustained FTL cruise does
  none either. On a requested slowdown below light speed, the exponential ramp
  predicts one endpoint five seconds after crossing below light speed, not an
  arbitrarily long chain of regions. Turns and abrupt dropout can still miss.
- A server calls `begin_round` once, `catch_up` for each voyage towards the
  host's target tick, then `dispatch` once. A missing dependency returns that
  ship's `waiting` state without advancing its tick or consuming its orders.
  Other ships continue. Later updates catch up the delayed ship within a step
  budget, preserving tick-keyed input order and reporting any refused orders.
  The host must also enforce an aggregate fleet step budget and network limits.
- This is a server-ready hosting path, not a running authoritative backend.
  The standalone client intentionally retains its shared offline-clock hold
  while data is missing, keeping its existing time controls and presentation.
  Future multiplayer collision/interaction rules must handle a temporarily
  delayed ship explicitly; this loader does not pretend it is at the host tick.

Prefetch improves smoothness only. Cache warmth, completion order and prediction
accuracy never authorize a different frame handoff or skipping gravity.

`ns ship state` and the inspector expose requested/actual speed, gravity policy,
`warp_mode: "unsafe_manual"`, `arrival` (whether it can enter systems, or
what its next step waits for) and stop reason. Waiting holds time, never the
drive. Phase 4 has joint live acceptance.

The fleet-level `system_loading` readout reports queued/running jobs and cached
region/system counts; `system_loading_error` retains the last transport failure.
