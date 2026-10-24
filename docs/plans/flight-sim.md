# Flight simulator · alpha plan

A proposal, not a description of the current application. Camera flights today
are navigation aids, not physical spacecraft (see [navigation](../client/navigation.md)).

## Goal

- Walk around inside a ship as a character and use its consoles, starting with
  navigation.
- Realistic physics, but easy for a new player to pick up and explore straight away.
- **Core loop:** spawn in deep space, warp to a system in about a minute,
  warp to a planet and settle into orbit, then warp on to the next. Plotted
  trips take roughly 20 to 60 seconds, whatever their length.
- **Free exploration:** fly the warp drive by hand, steering with the nose and
  choosing speed on one logarithmic scale, from crossing a planet's day side
  to its night side in seconds to crossing between star systems.

## Principles

- **Plausible physics.** Real gravity from every nearby body, so the ship can
  orbit anything. No invented forces beyond the warp drive's documented rules.
- **Two drives:**
  - **Warp drive (Alcubierre-inspired):** manual exploration and plotted
    trips, in-system or interstellar.
  - **Sublight drive (fusion torch, about 1 g):** only for fine-tuning and
    settling into orbit.
- **Layered control.** Autopilot is enough for a new player. Assists and full
  manual control are optional depth.
- **Time warp is optional.** The loop must never need fast-forward.
- Existing invariants still apply: f64 physics, deterministic generation,
  stable body identities and the native Linux client target.

## Warp drive rules

The Alcubierre metric motivates the bubble; it is not a proven engineering
design. Warp is a separate motion mode, not an enormous ordinary acceleration:
ordinary flight keeps summed gravity and Newtonian thrusters.

How the rules compare with the theory:

- **Below light speed, steering is plausible.** The ship can still reach every
  part of the bubble, and recent slower-than-light bubble models need no
  negative energy.
- **Above light speed, steering is a deliberate game rule.** The bubble's front
  edge is cut off from the ship by a horizon, so in theory a faster-than-light
  leg would be a committed straight line. The game lets the player steer
  anyway. This is the one rule bent on top of the issues built into any
  faster-than-light travel (exotic matter, vast energy, causality, stability).
- **Matched dropout is as plausible as the drive itself.** The bubble's speed
  profile can end at any velocity; stopping is one case of it. The change in
  the ship's momentum and energy is paid for by the drive.
- **Arrival burst is left out.** Theory predicts particles trapped in a
  faster-than-light bubble's front wall are released forwards on dropout.
  It is not simulated in the alpha; it could become a hazard later.

The rules:

- **Manual or plotted.** The player steers by the ship's nose, or a plotted
  autopilot steers the same drive towards a destination. Looking around moves
  the camera, not the heading.
- **Steering.** Pitch and yaw curve the bubble's path; roll turns the ship
  without changing the direction of travel. No lift, banking force or
  atmosphere is implied.
- **One logarithmic speed scale.** Speed up and down in large, even steps from
  metres per second to interstellar speeds, rather than delicate linear
  increments. Releasing the control holds the cruise speed. Warp speed and
  thrust throttle are separate inputs. Exact ranges and ramps are tuning
  parameters.
- **Unsafe manual warp.** The user superseded the initial always-on hard-safety
  requirement after live testing. Manual travel has no body/atmosphere exclusion,
  swept collision check or missing-data stop. It can engage inside and pass
  through bodies. Collision/approach protection belongs to explicitly enabled
  future assists, not an unavoidable restriction on manual warp.
- **Disengage at any time.** Ending bubble travel stops warp motion, not the
  ship's ordinary velocity or gravity.
- **Two velocities.** The ship's ordinary velocity is tracked apart from the
  bubble's motion. Ordinary movement continues during warp: bubble motion adds
  to it rather than replacing it. Changing reference frame must not change
  momentum, and bubble motion is never applied as a launch velocity on dropout.
- **Gravity during warp.** By default, gravity remains active at the ship's
  current position and continues changing its ordinary velocity, not its
  requested bubble speed. Keep this as an explicit, engine-independent
  simulation policy, separate from safety, speed control and frame handoffs.
  An alternative gravity-suspended policy preserves ordinary velocity during
  warp, while ordinary movement still continues. Both policies are tested;
  choosing between them is initially a developer setting, not a player-facing
  assist. Gravity resumes normally on dropout under either policy. This is a
  documented game rule, not a claim to simulate an Alcubierre spacetime.
- **Dropout velocity.** Unmatched, the ship keeps its current ordinary velocity,
  including changes accumulated under the active warp policy, rather than
  restoring its pre-engagement velocity. Matched, it takes a chosen body's
  velocity, optionally plus the sideways speed for a rough orbit. A realistic 1 g drive would otherwise
  need 30 to 60 minutes to match a planet's 10 to 50 km/s.
- **Plotted trip time.** About 20 to 60 seconds at 1× simulation time by
  default, using a distance-scaled cruise and slower approach; a chosen time
  from 10 seconds to an hour can replace it. Pause stops travel. Manual trips
  have no fixed duration.

### Optional warp assists

Each assist is separately switchable, with no hidden dependency between them,
and a beginner preset turns them all on. They live with the rest of the ship's
state in `ns ship` and the developer API, so they can later map onto dashboard
controls. The state shows requested and actual speed, and the reason for any
limit.

1. **Turn limit.** The turn rate falls as speed rises, so the turning circle
   grows with speed and the ship cannot loop round absurdly fast. With it off,
   turning is limited only by the steering input.
2. **Collision.** When the predicted path would hit a body or pass too close,
   slow down, or disengage when slowing is not enough. Judge by clearance
   along the path, closing speed and reaction distance, not just distance to
   the nearest body's centre, so bodies well off the path do not slow the ship.
   Speed recovers gradually once the path clears, not with a sudden jump.
3. **Approach.** With a body selected, the maximum warp speed falls as the
   ship nears it, and warp disengages at a safe standoff distance. The player
   keeps steering; turning away does not pull the ship back. An unreachable or
   unsafe target is reported, never hidden by a teleport.

Future enabled assists combine predictably: the requested speed is capped by
every active assist, and the lowest one wins. Unsafe manual warp has none.

## Craft (man-made objects)

The ship is the first of a general family of man-made objects: ships,
stations, rovers and probes. Build the shared model once, with the ship as its
only member in the alpha.

- **Live, not generated.** Natural objects come from seeds; craft are state
  that changes every step. They have stable IDs of their own and never shift
  any generated object.
- **One object type.** A single `ObjectRef::Craft { id }` with a craft kind
  (ship, station, rover, probe), rather than a variant per kind. Selection,
  inspector, breadcrumbs, actions and `ns inspect` then work for every kind.
- **A parent frame and a motion mode.** Each craft is positioned relative to a
  parent (a star system, a planet or a planet's surface), and moves in one of
  three ways:
  - **Free:** numerically integrated under gravity and thrust (a flying ship).
  - **On rails:** a fixed analytic orbit (a station, or a coasting ship under
    time warp).
  - **Surface:** latitude, longitude and heading on a rotating body (a rover).
- **Galactic position and velocity** from the parent chain, so distance,
  search scope and warp matching treat craft like any other object.
- **Fully in the object browser.** A "Craft" family in the search registry
  with kinds `ship`, `station`, `rover` and `probe`, and properties such as
  speed and parent. Existing relationships apply (`orbits.planet_class=...`).
  The search engine merges live craft from a separate source with its
  generated results, since craft are not in the procedural walk.
- **Picking:** craft are few, so clicks test their markers directly rather
  than through star picking.
- **Player-made only.** Every craft is built or placed by a player; none are
  generated from seeds.
- **Saved on the backend, ready for multiplayer.** Craft state is a
  serializable model in `crates/shared`, so client and server agree on it.
  - The backend owns which craft exist, their owners and their last saved
    state, and shares them with every client.
  - First, each client flies its own ship and reports its state a few times
    a second; other clients show it by extrapolating from the last report.
  - The goal is server-authoritative physics: clients send only their inputs,
    the server runs every ship, and clients predict their own ship and
    correct it from the server's state. Phase 1 keeps this path open:
    - Physics in `crates/simulation`, with no engine types, so the server
      can run the same code.
    - Ships move only through input commands (thrust, attitude) applied at
      fixed steps, never by direct state edits, so inputs can be sent instead.
    - A fixed step rate shared by client and server.
    - Native and WASM may differ in the last digits of sin, cos and powers,
      so correction must tolerate small differences rather than expect
      bit-identical results.
  - Live updates need a push channel (WebSockets in Axum); the two-second
    `/galaxy` poll is too slow for moving craft.
  - With no backend, the native client keeps its craft in memory, as it
    already runs alone with demo data.
  - Phase 1 keeps the ship in client memory; saving and sharing come later.

## Phases

### 0 · Star velocities ✅ Done

- Every star system has a galactocentric velocity (see
  [galaxy motion](../simulation/galaxy.md#motion)): a rotation curve plus
  per-component rotation and scatter, drawn from each system's seed. Stars do
  not move yet, so cells and streaming are unchanged.
- The spiral arms were flipped to trail the rotation as part of this.
- Shown as "Galactic velocity" in the inspector and as `velocity_kms` in
  `ns inspect`.
- Needed so interstellar warp can match the destination star's velocity.

Moons are not a prerequisite. Nested motion already exists through planets
around stars and barycentre hosts in multiple systems; moons extend it later.

### 1 · The ship as a body ✅ Done

Body velocities, the craft model with fixed-step gravity (tested on both
targets), the ship in the engine (cube, selection, inspector, breadcrumbs,
picking, "Follow ship" from 200 m, `ns ship`) and in search. See
[craft](../simulation/craft.md).

- Start here: planet and barycentre orbital velocities (the derivative of
  their Kepler positions), so any body reports position and velocity at a
  given time.
- The [craft](#craft-man-made-objects) model in `crates/simulation`, engine
  independent: ID, kind, parent frame, motion mode, f64 position, velocity and
  orientation. Only the ship and the free mode are needed now.
- Gravity summed from nearby bodies at their analytic (Kepler) positions. The
  ship is a test particle and pulls on nothing.
- Fixed-step numerical integration on the main thread, independent of frame
  rate: each frame runs as many fixed steps as real time allows, caps catch-up
  after long stalls, and blends between the last two states when rendering.
- The ship changes only through input commands applied at fixed steps, so a
  server can later run it from the same inputs.
- The ship as a selectable `ObjectRef::Craft`: inspector facts (speed, parent,
  altitude), breadcrumbs, actions such as "Follow ship", and click picking.
- The ship in the object browser and `ns search`: the Craft family and the
  live-craft source merged into search results.
- The camera follows the ship as it follows a planet, close enough to see a
  30 m hull: the zoom goes down to 1 m anywhere, and the render scale with
  it. Streaming, precision and the visited system stay
  camera-based; the ship's own facts (gravity, altitude) stay craft-based.
- Ship state and commands through the developer API and `ns`.
- **Done when:** a test orbit runs 100 laps with negligible energy drift, on
  both targets.

### 2 · Full ship control ✅ Done

The ship can be flown on every axis, entirely from the CLI. In-app controls
come later, with the layered controls.

- **Thrusters, set per ship** so later ships can differ:
  - Main drive forward at about 1 g, on a throttle from zero to full.
  - Reverse drive, on by default but optional per ship.
  - Fine thrusters at about 0.1 g, moving the ship forward and back, up and
    down, left and right, and turning it in pitch, yaw and roll.
- **Rotation in the physics.** The ship stores its spin as well as its
  orientation, turns a little each fixed step, and thrust acts along the
  ship's own axes, not the system's. Turning thrusters set how fast the spin
  changes; mass and inertia modelling can wait.
- **Inputs stay fixed-step commands:** throttle (negative for the reverse
  drive, refused on a ship without one), translation on three axes and
  rotation on three axes, each a fraction of the thruster's limit.
- **Pointing autopilot,** in `crates/simulation`, producing the same inputs a
  player would, so a server can run it too:
  - It turns towards a target direction using only the ship's own turning
    thrusters, speeding up and then braking in time to stop on target without
    overshooting from rest; an existing spin can make overshoot physically
    unavoidable within the available thruster limits.
  - Targets are worked out again each step: prograde, retrograde, normal,
    anti-normal, radial in and out (relative to the body the ship orbits), a
    body, or a fixed direction in the system's axes.
  - Roll is held as it is, or lined up with a given "up" direction.
  - "Stop rotation" is the same controller with zero spin as its target.
  - A manual rotation command releases it.
- **Time rate.** While the ship is thrusting or turning, the clock is held to
  a low rate (start at 10× real time, tune in play), so a burn is never spread
  over hours of simulation in one frame.
- **Facing is visible:** a 30 m cuboid with a pyramid nose and wings on its
  bottom edges, until proper ship models.
- **A reorganised `ns ship`,** grouped by what is being done:
  - `spawn`, `despawn`, `list`, `follow`.
  - `state`: orientation, spin, velocity, throttle, autopilot mode, orbit
    and altitude.
  - `throttle <-1..1>`, `translate --forward --up --right`,
    `rotate --pitch --yaw --roll`.
  - `point prograde | retrograde | normal | antinormal | radial-in |
    radial-out | body <id> | vector x,y,z`, `stop`, `release`.
- The inspector shows throttle, spin and autopilot mode.
- **Done when:** from the CLI alone, the ship points prograde, burns, stops
  its rotation and points at a body, settling on each target without
  overshoot from rest; autopilot tests pass on both targets.

Checkpoints, each committed and checked on both targets:

1. ✅ **Thrusters and rotation in the physics** (`crates/simulation`): per-ship
   thruster settings, spin, thrust along the ship's axes, and the three
   inputs. Tests: spin stays constant without input, thrust follows the
   orientation, a ship without a reverse drive refuses negative throttle,
   and the 100-lap energy test still passes.
2. ✅ **Pointing autopilot** (`crates/simulation`): the controller, every target,
   roll hold and "up", "stop rotation" and release. Tests: it settles without
   overshoot from rest at awkward angles, recovers from initial spin,
   tracks prograde around a whole orbit, and a
   weaker ship turns more slowly.
3. ✅ **The client and the CLI:** the client applies inputs and the autopilot at
   each fixed step, holds the time rate while thrusting or turning, and
   carries the reorganised `ns ship`, its developer API commands, the
   `ns ship state` readout and the inspector facts.
4. ✅ **Visible facing:** a 30 m long wireframe cuboid with a pyramid nose
   and wings rooted on the bottom edges, drawn with the orientation blended
   between steps like the position.
5. ✅ **Done when, run live,** then the docs (craft, developer API, limitations)
   and the phase marked done.

Verified through the live CLI: point prograde, burn, brake spin, point at the
moving planet and align a fixed direction with an up vector. Manual fine
translation and all turning axes, the manoeuvre time cap and refusal of
reverse throttle on a ship without that drive were also exercised. The
winged test hull was inspected on screen. Native/WASM checks cover the
simulation; native client regression tests cover the command-to-physics
pipeline. Browser rendering has not been visually tested for this phase.

### 3 · Trajectory prediction ✅ Done

The selected ship shows where it will go if it coasts, the feature that
makes orbital mechanics readable. See
[orbit reference and coast prediction](../simulation/craft.md#orbit-reference).

- **Orbit reference:** what the ship's orbit is described around, chosen
  from the system's hierarchy: the planet whose Hill sphere holds it, else
  one star or a pair's barycentre by their stable zones, with a switching
  margin so it does not flicker. A label for readouts, prediction and
  pointing, never an extra source of gravity.
- **Coast prediction:** the ship's state copied and coasted under every
  star and planet with the same integrator, at a coarser adaptive step.
  Bound orbits are predicted a little over one lap, escapes until past the
  edge of the sphere of influence; it stops at a surface on impact. The real
  ship and clock are never touched.
- **Spread over frames:** a fixed step budget per frame shared by all ships.
  The last result stays on show, with its age, while a newer one is worked
  on; it refreshes during burns, on new orders and every quarter lap.
- **Drawn relative to the reference,** anchored where it is now, so an
  orbit around a moving planet is one closed loop. Exactly one revolution
  is shown, joined smoothly to the ship; an orbit really bent by another
  body is left open. Diamonds mark predicted apsides, reference exits and
  encounters (amber) and impacts (red).
- **Readouts:** the inspector and `ns ship state` give the prediction's
  reference, age and reach, and the next predicted apsides, exits,
  encounters and impact, apart from the instantaneous two-body estimates.

Verified live from the CLI: eccentric orbits drawn as one closed loop at
close and far zoom and at high time rates, a prograde burn reshaping the
predicted apoapsis, a retrograde burn onto an impact, and an escape (its
exit marker, after the slow-escape horizon fix, by tests only).
Circumbinary and nested-triple references and barycentre anchoring are
covered by tests only, and browser rendering was not visually tested.

### 4 · Warp core ✅ Done

User approved phase 4 on 2026-10-18, including proceeding after the foundation
revisions below. This approval does not claim additional browser visual testing.

Complete, accepted by the user after the final joint native 10 ly/s flight.
`./scripts/check.sh` passes: formatting, CLI transport tests, native workspace
tests and strict native/WASM Clippy, with simulation craft tests under Node.
Native live outcomes and remaining browser visual coverage are recorded below.

Historical checks of the original safety-enabled implementation: outward warp at 10 km/s and 150 km/s, an inward
100,000 km/s body-safety stop, system departure and entry into a different binary
system all looked clean to the user. Eight drive interlocks refused conflicting
commands without movement. Pause held time, position, ordinary velocity and warp
state exactly, but the following movement measurement exposed an undisplayed
high-speed segment left over from manual dropout. A timestamped cutoff fixed
that at first. It was later removed for one simpler rule: every order, warp
included, takes effect from the next fixed step, matching step-keyed server
inputs. The last warp step after a disengage now plays out, at most 0.05 s.

The user subsequently requested unrestricted, unsafe manual warp. Removed both
collision sweeps and missing-route-data stops, including the inside-body
engagement restriction. Nearby discovery remains informational for gravity and
frame handoffs, never permission to travel. The speed maximum is 10,000 ly/s,
matching the camera. The final native flight was accepted in this revised mode.

At 10 ly/s the timing-only native build measured discovery at 116.8 ms,
movement/handoffs at 0.0025 ms and a 121.3 ms worst frame. Replacing the old
per-craft census removes that synchronous discovery from the frame update.
Arrival now checks loaded list systems and streamed stars; only an approached
uncharted star requests bodies in the background. One job per craft, a bounded
arrival cache and rejection of passed-system results prevent requeue churn.
Gravity uses the current system only, with no local-system gravity in galactic
space. Craft timings remain in the performance console/API. A joint native
post-restart repeat at 10 ly/s measured 5.6–6.4 ms sampled frame intervals,
6.7 ms P95 and 7.1 ms maximum trailing-second peak, versus a 5.9 ms baseline
sample. Discovery peaked at 0.089 ms and the whole craft update at 0.090 ms,
versus the previous 116.8 ms discovery stall. Warp remained engaged at
9.999998 ly/s. Measurements are saved in `data/logs/warp-discovery-after.json`.
The user confirmed smooth performance.

The subsequent live run exposed two follow-camera issues: measured camera
velocity was zeroed on every origin change (also hiding the marker), and the
small follow offset was rounded when added to a light-year-scale local pose.
Follow velocity now uses rebased previous/current displayed craft states, and
the hull/label share a separately retained camera-relative offset. Tests cover
repeated warp re-anchoring, pause, handoffs, HUD/marker continuity and offset
precision. The rebuilt-client repeat held 10 ly/s with about 6.0 ms frames,
a 7.0 ms trailing-second frame peak and 0.082 ms craft-update peak. The user
accepted the speed readout, marker, hull and label as good enough, completing
native phase 4 acceptance. Results are in `data/logs/warp-follow-retest.json`.

Automated coverage includes both gravity policies and dropout momentum, speed
validation/ramping, thrust/turning interlocks, discovery without camera streaming,
uninterrupted 10 ly/s travel without data, body crossings and inside-body
engagement, continuous system-to-galaxy-to-system
travel, frame momentum/heading preservation, local re-anchoring, following and
pause/time-rate behaviour. Simulation warp tests run natively and under Node as
WebAssembly; client integration tests run natively. Browser compilation is
checked, and a headless browser test verifies the craft-worker transport and
deterministic body generation. Browser flight visuals were not manually tested;
native visual acceptance is recorded above.

A post-acceptance review found frame handoffs depended on the client: arrival
used the camera's streamed stars and entered a system whenever a background
load happened to finish, so the same inputs could give different paths and an
unfollowed ship could miss uncharted systems. Frame decisions now live in
`craft::voyage` in the simulation, from the craft's state and seeded or list
data alone, with steps waiting (holding the clock) for missing data instead
of carrying on without it. Craft enter systems only below light speed, which
keeps the per-region seeded lookup (10 to 90 ms natively, 200 ms cold) off
high-speed flight. `Craft::step` now integrates warp too. See
[frames and arrival](../simulation/craft.md#frames-and-arrival). This
revision also passed the native CLI flight below; visual confirmation of this
revision is pending.

A multiplayer review then fixed three foundations. Ship
orders (`craft::Order`, legacy thrust included) moved from the client into the
simulation, so a server validates them the same way. Craft step on one world
grid, counted by `tick` from time zero, so orders can be keyed by step number.
A spawned ship's system is anchored at its star's seeded position, as arrival
does, rather than at a camera-streamed position that could be missing or
slightly different.

The 2026-10-17 native CLI flight exercised the revised voyage and tick code:
10 ly/s departure into galactic space, exact paused state, four refused
conflicting drive orders, return to the original uncharted seeded system below
light speed, manual dropout and ordinary coast with a fresh prediction. No
system-loading error was reported and sampled ticks matched simulation time.
The first return attempt missed its straight-line target and timed out while
still above the entry speed limit; explicitly reducing requested speed to
200,000,000 m/s allowed entry. This was a test-course correction, not automatic
arrival guidance. The client was left paused with warp off for user visual
confirmation. CLI evidence: `data/logs/warp-cli-live-review.json`.

Straight-line warp along the nose, from the CLI. See the
[warp drive rules](#warp-drive-rules).

The next foundation hardening pass shares list-page and legacy arrival rules
between the client and synchronous server-side library, adds bounded
step-keyed order queues with serializable replay inputs and an optional
`steer_craft.tick`, and replaces separate atlas caches with one working-set-aware
cache. Idle targets remain 8 regions and 64 systems; active fleets can exceed
them without evicting data another ship still needs. The client now calls the
same voyage step as a server and consumes its interpolation state, while
inspector/API readouts live in their own client module. Light-speed and spin
thresholds share constants. Automated tests cover replay after a snapshot,
order timing/refusals, cache pressure and identical client/server list arrivals.
This does not add a live authoritative server or network ownership checks.
No new live or browser visual acceptance is claimed.

A subsequent loading pass adds a shared nonblocking loader with a bounded,
deduplicated, demand-first queue, two single-thread native generation workers
and the existing browser worker transport. Local lookahead prepares region and
approached-system data, including a predicted slowdown endpoint rather than an
FTL route scan. Obsolete queued interests expire each update; in-flight work is
bounded. Transport failures never trigger synchronous generation in live updates.
The reusable server catch-up path defers only a dependent ship, retaining its
tick and orders while other ships advance. The standalone client uses the same
loader and prefetch, retaining its offline shared-clock hold. A live server and
multiplayer interaction rules for delayed ships remain later work. See
[background preparation](../simulation/craft.md#background-preparation-and-future-server-hosting).
`./scripts/check.sh` passes after this pass. Seven loader regressions run both
natively and under Node/WASM: bounded/deduplicated priority and expiry, request
promotion, transport retry, independent ship deferral and identical catch-up
with queued inputs, boundary/turn/slowdown lookahead, approached bodies only,
and no regional census for an ordinary in-system orbit. The native client
background-arrival regression also passes. No new live-flight or browser visual
acceptance is claimed.
Verified on 2026-10-18 with `./scripts/check.sh`: formatting, CLI transport,
native workspace tests, strict native/WASM Clippy and 45 craft tests under
Node. The generation worker also releases fine-cell caches above an estimated
32 MiB between requests, retaining expensive coarse integration.

Foundation review follow-up: frame handoffs also rotate persistent frame-axis
thrust and pointing state. Active sublight warp reuses its final gravity sample,
discarding it whenever gravity is skipped or the field changes. Queued orders
receive value validation on submission and have a 300 simulated second horizon;
state-dependent checks remain at execution. Regression tests cover these rules
on native and WASM. Visual acceptance of these revisions remains pending.

Phase 4 is the manually commanded drive foundation, not a destination or route
autopilot. The CLI engages it, selects speed and disengages it; steering,
assists, plotted autopilot and in-app controls are layered on in later phases.

Accepted behaviour (including the later unsafe-manual-warp revision):

- Refuse engagement while appreciably spinning or while a turning input or
  attitude controller is active. The caller must stop rotation and release
  pointing orders first; engagement never silently freezes attitude. Reject
  turning and pointing orders while phase 4's straight-line warp is engaged.
- Sublight propulsion and warp are mutually exclusive. Refuse warp engagement
  until all thrust inputs are zero (main/reverse throttle, fine translation
  and legacy frame-axis thrust), and reject nonzero thrust commands while warp
  is engaged. Do not silently clear inputs. Gravity is not a thrust input and
  remains governed by the gravity-during-warp policy.
- Normal speed changes use a short, tunable ramp. Manual disengagement cancels
  bubble motion from the next fixed step while preserving ordinary velocity.
- Unsafe manual travel does not stop for bodies, atmosphere, proximity or missing
  discovery data. No collision damage or crash response is implemented.

- Warp state in `crates/simulation`, engine independent: engaged or not,
  requested and actual speed, and the bubble's motion kept apart from the
  ship's ordinary velocity. Changed only through fixed-step input commands.
- The logarithmic speed scale, with step up, step down and set commands.
  Warp speeds are simulation-time speeds; pause stops travel, and warp does
  not inherit phase 2's manoeuvre time-rate cap.
- Engage and disengage, with ordinary movement carried through and gravity
  active by default. Isolate the gravity-during-warp policy so a developer
  setting can select gravity-suspended travel without changing the rest of the
  drive. Test both policies, including ordinary drift and unmatched dropout;
  retain the existing ordinary-flight integrator and its regression tests.
- No mandatory hard-safety limit. Warp can cross bodies and unknown space.
- Frame handoffs: in-system first, then system to galaxy and galaxy to system.
  Position and ordinary velocity transform consistently. Following finds the
  ship across frames; streaming and the visited system stay camera-based, and
  the craft uses current-system gravity and seeded system data for handoffs.
  Missing data holds the clock below light speed and never blocks the drive.
- How phase 3's prediction behaves in warp: never show the pre-warp orbit as
  the current forecast.
- A `ns ship warp` group and matching developer API commands, with warp state
  in `ns ship state` and the inspector.
- No fuel or realistic bubble energy budget in the alpha.
- Arrival uses a seeded per-region lookup and list systems, loaded in the
  background, not the camera's streamed stars or route certification. The existing generation bound remains a separate alpha
  simplification; see [stars](../simulation/stars.md).
- **Done when:** from the CLI, warp to another planet and out of the system,
  then disengage, with no position jumps or unwanted automatic stops, on both
  targets.

### 5 · Dropout velocity ✅ Done

User approved phase 5 on 2026-10-21 after a native live CLI flight they
watched. A follow-up hardening pass refuses rough orbits outside the
reference's reach, treats near-radial courses (under about 0.06° tangential)
as radial when choosing direction, bounds body IDs in orders and clears the
loader error after a successful retry. The live run, around a rocky planet in
the triple system `gd.3.378`:

- Plain match onto the planet about 97,000 km out: zero relative speed,
  unchanged position and heading, then the expected slow radial fall.
- Rough orbit onto the planet from a radial-in course: circular (eccentricity
  6e-11) at 76,900 km altitude, using the ship-up direction fallback.
- Refusals with HTTP 409 and warp left engaged and unchanged: `--orbit` outside
  the planet's Hill sphere, and a planet in another system with and without
  `--orbit`. A plain match from the same distance was accepted.
- Rough orbit around Star A at 54 AU: circular (eccentricity 2e-12), keeping
  the current orbital sense.

Browser flight visuals remain untested.

Implementation: explicit `ns ship warp match <body-id> [--orbit]`, shared
fixed-tick simulation/API order, inspector and `warp.last_match` receipt.
Existing stable body IDs and phase 3 reference resolution are reused; no
separate selection model or automatic route is introduced. Rough-orbit
direction preserves relative tangential velocity, falling back to ship-forward
then ship-up projected perpendicular to the radial direction. All checks are
atomic; an invalid match leaves warp engaged. See
[matched dropout](../simulation/craft.md#matched-dropout).
Verified on 2026-10-18 with `./scripts/check.sh`: CLI transport tests, formatting,
native workspace tests, strict native/WASM Clippy and 59 craft tests under
Node/WASM. New regressions cover planet/star/barycentre matches under both gravity
policies, tangential rough-orbit estimates and direction fallbacks, atomic
refusals, tick-keyed replay after snapshots and client API/readout integration.
In-app selection-to-command controls remain phase 11 work.

- **Frame decision:** keep the existing static-star approximation and
  galactic momentum convention. Matching in this phase uses a body in the
  craft's current system only. Remote matching in galactic space is deferred
  until stellar positions and spatial queries can advance consistently;
  subtracting a local rotation velocity alone does not solve stellar scatter.
- Unmatched dropout keeps the current ordinary velocity, including gravity's
  changes during warp under the default policy. With gravity suspended during
  warp, it preserves the pre-engagement ordinary velocity instead. Neither
  policy adds bubble speed or silently matches the destination's motion.
- Matched dropout sets it to a reference body's velocity (planet, star or
  barycentre, using phase 3's references), with an optional rough-orbit exit
  that adds the sideways speed. Matching a planet's motion alone would leave
  the ship falling towards it.
- Matching is an explicit command or setting, never silent, and selecting a
  reference does not change heading.
- **Done when:** both exits work from the CLI as documented and are visible
  in `ns ship state`, including a rough orbit after a matched exit.

### 6 · Warp steering ✅ Done

- Pitch and yaw inputs curve the warp path; roll does not change the
  direction of travel. Engaging handles existing spin safely.
- CLI first. Mouse steering arrives with in-app piloting in phase 11.
- **Done when:** from the CLI, steer round a planet to its night side, turn
  towards another planet and leave the system.
- The turn-limit assist is deferred: the user will decide whether it is
  needed once mouse steering in phase 11 gives a feel for the controls.

Decisions agreed with the user on 2026-10-21:

- **The drive turns the bubble,** with the ship inside it, not the turning
  thrusters: a game rule like frame dragging. Steering sets a turn rate, eased
  in and out over 0.3 s, so releasing it ends the turn and dropout leaves no
  spin. See [warp steering](../simulation/craft.md#warp-steering).
- **One input path:** `rotate`, `point`, `stop` and `cut` steer the bubble
  while engaged and the thrusters otherwise, so a stick or mouse later sends
  the same orders in both modes.
- **Engaging while spinning is allowed:** the bubble takes over the spin and
  eases it to the steering input, rather than refusing.
- **Pointing works during warp,** which "turn towards another planet" needs and
  phase 9's autopilot reuses.
- **Turn rate is ship state,** set with `ns ship warp turn-rate <°/s>`, from 1
  to 360 °/s (default 30), engaged or not.
- **The turn-limit assist is deferred,** until planetary atmospheres.

Checkpoints:

1. ✅ **Steering in the simulation:** rate-commanded bubble turning, midpoint
   path bending, spin takeover and spinless dropout, pointing during warp,
   the turn rate order, `ns ship warp turn-rate`, readouts and docs. Tests
   cover the turning circle, shared rate limit, release, roll, takeover and
   overshoot-free pointing at 1 to 360 °/s.
2. ✅ **Live CLI steering flight** (2026-10-21, system `gf008000016.2.34`):
   two half turns at 15 °/s round planet c at 20,000 km altitude, night side
   to day side and back; pointing turned onto planet b and flew 1.44 million
   km; rough-orbit dropout was refused outside b's sphere of influence, then
   gave eccentricity 1.4e-5 at 8,400 km; re-engaged with pointing active, left
   the system at about 2,500 AU and yawed in galactic space, with no spin
   after disengaging. The user confirmed it visually.

Deferred: **turn-limit assist** (the rate cap falling with speed, its switch
and the reason for any limit), until planetary atmospheres.

### 7 · Collision assist (deferred)

Deferred with phase 6's turn limit and phase 8, until planetary atmospheres
(see [paused work](#paused-until-atmospheres)): clearance includes the atmosphere.

- Assist 2: slow down or disengage ahead of a predicted impact or too-close
  pass, and recover speed gradually once clear.
- Test close flybys, small bodies ahead, bodies well off the path and moving
  targets.
- **Done when:** flying straight at a planet with the assist on ends safely,
  passing beside one does not slow the ship needlessly, and with it off manual
  warp has no collision protection.

### 8 · Approach assist (deferred)

Deferred with phase 6's turn limit and phase 7, until planetary atmospheres
(see [paused work](#paused-until-atmospheres)): the standoff clears the atmosphere.

- Assist 3: with a body selected, the maximum warp speed falls on approach
  and warp disengages at a safe standoff distance. Reuses phase 9's speed
  profile.
- Unsafe or unreachable targets are reported.
- **Done when:** steering towards a selected planet by hand ends at the
  standoff distance without precise speed juggling.

### 9 · Plotted warp autopilot ✅ Done

Original implementation accepted. Shape/atmosphere clearance corrections are
done and signed off under [the follow-up checkpoint](#shape-and-atmosphere-clearance-follow-up).

- Plot and fly a route with the same drive and explicitly enabled assists as
  manual flight. Autopilot only adds steering and route management.
- In-system first, then interstellar. Allows for moving destinations and
  bodies in the way, and aims for the 20 to 60 second trip profile.
- With the assists deferred, the autopilot owns its speed profile (cruise,
  then a slowing approach to a standoff distance) and keeps its own route
  clear of bodies. Phase 8 later reuses that profile for manual flight.
- Choosing a destination, progress, cancelling and manual takeover through
  the CLI. A plotted orbital arrival asks for a matched, rough-orbit exit.
- **Done when:** the core loop runs end to end from the CLI: leave orbit, warp
  to another planet or system, drop into a rough orbit, then take over or
  cancel without discontinuities.

Decisions agreed with the user on 2026-10-22:

- **A route pilot in the simulation, not a CLI script.** One step at
  interstellar speed covers about half a light year, and destinations move,
  so the aim, speed and dropout must be worked out every fixed step. A route
  pilot is ship state beside the attitude pilot, in `crates/simulation`, and
  each step gives the same orders a player would: pointing during warp
  (phase 6), requested speed (phase 4) and, at the standoff, matched dropout
  (phase 5). A server can run it from the same inputs.
- **The command:**

  ```
  ns ship warp to <id> [--match | --orbit] [--time <seconds>]
  ns ship warp cancel
  ```

  - One `WarpOrder` each, so `ns` and the TUI get them from the schema, and
    `--in` and `--tick` work as on every order.
  - `--match` drops out at the target's velocity, `--orbit` into a rough
    orbit around it (a match included). With neither, dropout keeps the
    ship's velocity.
  - `--time` sets the trip time, from 10 to 3,600 s. The minimum leaves room
    for turning round first (up to about 6 s at the default turn rate).
    Without it, the time scales with distance: about 20 s for a short
    in-system hop, up to 60 s between systems. The time includes the
    initial turn; a trip that runs late shows the delay in its progress.
- **The speed profile: always slowing.** Requested speed is the remaining
  distance divided by a time constant τ, so every second covers the same
  fraction of what is left: the destination looks twice as large every so
  often, a steady zoom like the camera's logarithmic flights. A trip takes
  about τ × ln(start distance ÷ standoff), so τ is chosen when the route is
  plotted to fit the trip time. Speed is held low until the nose is lined
  up, and the profile allows for the 0.3 s speed ramp so the ship does not
  overshoot. When the 10,000 ly/s cap stretches a trip, its progress says so.
  The profile is a standalone function for phase 8 to reuse.
- **Moving targets:** the pilot aims at where the target will be, worked out
  again every step.
- **Cancelling and taking over:** `warp cancel`, or any manual turning or
  warp speed order, releases the pilot, as turning orders release the
  attitude pilot. Warp stays as it was, so nothing jumps.
- **Progress** in `ns ship state` and the inspector: target, stage (align,
  cruise, approach, arrived), distance, ETA, trip time and arrival mode.
- **Bodies in the way: wrap round them,** like a string pulled tight from the
  ship to the target.
  - Every step the pilot checks the straight line to the target against a
     keep-clear sphere round each body, a margin larger than its surface. The
     original radius-only boundary is replaced by the shape- and
     atmosphere-aware clearance of the follow-up below. If
    the line is clear it aims at the target. If not, it aims at the edge of
    the first sphere in the way, on the shorter side, so the ship curves
    round the body and turns back to the target once it is in view.
  - Steering needs only that next edge. Because it is worked out again every
    step, moving bodies, moving targets and several bodies in the way need
    no special handling, and no waypoint goes stale.
  - The bubble's turn rate limits how tight a bend can be, so near a body
    being skirted, speed is capped for the bend to fit, then recovers. A
    target barely hidden needs little slowing.
  - Bodies in the ship's current system are checked on the way out, and the
    destination system's on the way in. Stars between systems are ignored.
  - Cost: one line-to-sphere test per body, a few multiplications, over the
    same bodies and positions gravity already sums each step. Galactic space
    has none to check until the destination system is near. The whole
    wrapped path is measured only for timing.
- **Fitting the trip time round a detour.** At plotting, the pilot measures
  the wrapped path, not the straight line, and allows for the initial turn
  and the slowdown at each bend when choosing τ. It re-measures about once a
  second for the ETA. A time that cannot be met, such as a tight detour at a
  low turn rate, is still accepted: the reply gives the best achievable time
  and its cause (such as "about 34 s, detour round planet b"), and progress
  shows the delay.
- **Interstellar, with no new frame rule.** Speed drops below light speed only
  when the remaining distance is about light speed × τ, well inside the
  destination's entry range, so the existing entry rule enters the system
  on the way in. The destination's data is requested when the route is
  plotted; until it arrives the pilot aims at the system's star, then at
  the body. Phase 5's current-system-only matching still holds, because the
  ship is in the destination system by the standoff.

#### Targets

One command for every target, and the pilot never checks what kind of object
it is flying to. Each kind supplies the same facts, every step:

- position and velocity now;
- **size**, which sets the standoff;
- **keep-clear distance**: its surface, or its companion stars;
- **orbit reach**, or none when it cannot be orbited;
- which system it is in, or galactic space.

Moons and asteroids later add a source of these facts and change nothing in
the pilot, profile, standoff rule or readout.

**Standoff rule:** a comfortable multiple of the target's size, never closer
than its keep-clear distance and never further than its orbit reach, so
`--orbit` always works on arrival. When no such distance exists (a star in a
tight binary, whose reach is barely beyond its surface), the order is refused
when given, with the reason, not on arrival.
Keep-clear distances now include oblateness and, for planets, a
maintenance-free atmosphere height; see the follow-up below.

| Target | Size | Orbit reach | Standoff |
| --- | --- | --- | --- |
| Planet | radius | sphere of influence | a few planet widths |
| Single star | radius | stable zone | clear of the surface, inside its planets |
| One star of a multiple | radius | its own stable zone | close to that star |
| Barycentre | pair separation | the pair's stable zone | outside both stars, for a circumbinary orbit |
| System, by its key | its star or outermost barycentre | as that object | as that object |
| Ship | hull length | none: `--orbit` refused | about 1 km |
| Moon (later) | radius | sphere of influence inside its planet's | as a planet |
| Asteroid (later) | radius | tiny | a minimum floor, so it does not stop metres off a rock |

- **A barycentre has no surface:** its stars are what the ship keeps clear of,
  and what the route steers round.
- **A system is fixed,** as everywhere else under the static-star
  approximation. Planets, stars in multiple systems and inner barycentres
  move.
- **Ships:**
  - They thrust and warp at will, so the pilot aims along the target's
    current velocity and corrects every step. A target pulling away shows a
    growing ETA.
  - `--match` gives formation flying alongside it. `warp match <ship-id>`
    gains the same, for manual flight.
  - A target that despawns releases the pilot as cancelling does, with the
    reason in the readout. Targeting the ship itself is refused.
  - Each ship reads other craft as they were at the start of the step, so
    ships chasing each other give the same paths whatever the update order.
- **Refused when given, leaving the ship unchanged:** an unknown ID, the ship
  itself, `--orbit` on a ship, or no valid standoff.

Checkpoints:

1. ✅ **In-system pilot:** the target facts with natural bodies as the first
   source, the speed profile and trip time, aiming at moving targets, the
   standoff rule, the three arrival modes, `warp to` and `warp cancel`,
   takeover, progress readouts and refusals. Tests for each target kind,
   both gravity policies, trip times from 10 to 3,600 s and replay from
   a snapshot.
   - Done 2026-10-22. Found while testing: with a short τ, flying even a
     few degrees off the line turns the line of sight faster than pointing
     follows, and the nose never settled. The pilot now aims ahead by the
     line of sight's turn over pointing's response, which keeps it stable.
   - τ is fitted again once lined up and once a second, so a turn that ran
     long is made up. A 10 s trip facing away runs about 4 s late: the
     turn alone takes about 9 s at 30 °/s, not the 6 s guessed above.
   - `--match` with `--orbit` is accepted as `--orbit`, which matches too.
2. ✅ **Bodies in the way:** the per-step clearance check, aiming at the edge,
   the bend speed cap, and the wrapped path in the trip time, best
   achievable time and ETA. Tests with a target directly and barely behind
   a planet, a binary companion on the line, two bodies in the way, a moving
   blocker, bodies well off the line, an interstellar departure from behind
   a planet, and a trip time too short for its detour.
   - Done 2026-10-22. The departure test flies to a far star of the same
     system from low over a planet's night side; true interstellar
     departures wait for checkpoint 3 but start the same way.
   - Found while testing: aiming at the edge alone let the nose lag into
     the keep-clear spheres. The lead now follows the edge's own turn, and
     speed is capped when heading into a sphere so there is time to turn
     away and slow.
   - Timing each bend by its full turn was far too pessimistic: speed round
     an arc recovers as the target comes into view, so a bend costs
     (1 − cos turn), plus pointing's settling scaled by the turn.
   - `ns ship warp to` now replies with the plotted route, including the
     best time and `slowed_by` when the asked time cannot be met.
3. ✅ **Interstellar routes:** requesting the destination's data at plotting,
   aiming at its star then its body, and entry on the slowdown. Tests with a
   seeded system and a list system, through the background loader.
   - Done 2026-10-22. Changed from the plan: the pilot does not aim at the
     star while the data loads. A path that changed when the data arrived
     would depend on loading speed, which voyages forbid. The order is
     accepted in a `plotting` stage, and each step first waits for the
     destination's location (a new `Wait::Star`) and bodies, holding the
     clock as entry already does; the first step that has them plots it.
   - While outside the destination, the pilot sees its target and bodies
     converted into the ship's frame, so wrapping and aiming need nothing
     new. A 2 ly trip in 40 s peaks near 0.6 ly/s and enters about
     570,000 km out, below light speed, as planned.
   - Found while testing: with too short a time, τ pins at its minimum and
     the speed ramp cannot brake as fast as the profile asks, so the ship
     covered its last million kilometres in one step and passed through the
     standoff. The pilot now drops out when the coming step would reach it.
   - Tests: a seeded system, a charted list page from galactic space,
     leaving from behind a planet, a whole system matched on arrival, an ID
     naming no system, a slow loader giving the identical path, and a trip
     through the app's command path.
4. ✅ **Ships as targets:** the craft source, start-of-step reads, formation
   matching, `warp match <ship-id>` and a despawned target. Tests with a
   coasting, a thrusting and a warping target, and two ships chasing each
   other.
   - Done 2026-10-22. Each voyage is given every craft as a `Sighting` (its
     state, warp bubble and frame) before each step and order, converted
     into its own frame. The client now steps its fleet in lockstep, the
     craft furthest behind first, and a craft waiting for data stops them
     all, so no read depends on update order or loading speed.
   - The pilot tracks a target's whole motion, bubble included, but a
     match takes its ordinary velocity: a warping ship is left behind at
     rest with where it was.
   - Found while testing: a ship in low orbit sits inside its planet's
     keep-clear sphere, so the route circled the planet for ever. A sphere
     that holds the target now shrinks to pass through it, so the route
     skims round at the target's height.
   - A warping target (1,000 km/s) pins τ at its minimum, arrives about
     5 s late and drops out a step early, at 1.3 km.
   - Tests: coasting, thrusting and warping targets, one in galactic space
     from a system, two ships chasing each other in both update orders, a
     despawned target, refusals and manual formation matching, and a chase
     through the app's command path to a ship in low orbit.
5. ✅ **Live CLI core loop:** leave orbit, fly to another system, drop into a
   rough orbit, fly to a second ship and match it, wrap round a planet in
   the way, then cancel and take over mid-route without discontinuities.
   Craft update timings in the performance console confirm the per-step
   checks stay cheap. Then the docs and the phase marked done.
   - Done 2026-10-22; the user confirmed it visually. From System 1
     (`g78040000.2.813`): round planet d into orbit round planet e, 30 s, on
     time (eccentricity 1.5e-7); 3.9 ly to `g1e010000005fe.2.0` in 60 s, on
     time, into a circular orbit 334,594 km above its star; a second ship
     matched the first across 3.9 ly in 60 s, holding 1,039 m off at under
     1 cm/s, and `warp match <ship-id>` by hand left 1 mm/s; a route to
     planet c with the star on the line skirted it about 57,000 km up and
     arrived on time into an orbit of eccentricity 8e-4; cancelling kept
     warp's speed and released pointing, and manual drive, turning and
     stop then responded. Stepping both ships peaked at 0.07 ms a frame.
   - Found while testing: a followed ship's destination system popped in
     only at dropout, because the camera took its system from the ship's
     frame. While the ship is in galactic space the camera now arrives and
     leaves by its own distance, as in free flight (see
     [navigation](../client/navigation.md)); the light-speed entry rule
     is unchanged.
   - Still open: a 20 s chase of a ship orbiting at 173 km/s ran 5.4 s
     late with no cause in the readout, like earlier chases of orbiting
     targets; and the inspector's attitude pilot row shows a raw vector
     during routes.

#### Shape and atmosphere clearance follow-up

**✅ Done, signed off by the user 2026-10-25 after a CLI test flight.**
Atmosphere generation is [complete](atmospheres.md). Update the
existing route pilot before resuming in-app autopilot and the deferred
assists. This does not reopen the accepted route foundation or add mandatory
protection to unsafe manual warp. See
[navigation clearance](../simulation/craft.md#navigation-clearance).

Implementation, 2026-10-25:

- `system::clearance` gives each body an exclusion and a preferred radius,
  cached per loaded system. Agreed with the user: the atmosphere part is a
  **maintenance-free orbit height** (at most 1 m of orbital radius lost per
  year, reference ship B = 10 kg/m², retrograde against co-rotating air), not
  the visible cutoff. `Column::density_at` extends density above that cutoff.
- The low-orbit relaxation stops at the exclusion. Unknown or invalid air,
  a start inside an exclusion and an arrival sphere crossing another
  body's exclusion are refused at plotting and rechecked every step; each
  warp step is also swept against the exclusions. A failure ends the route
  and drops out, keeping ordinary velocity. Starting inside is refused, not
  escaped: leave by hand first.
- Extended columns are reported in the route's `clearance_warning`.
- Tests: the helper (decay budget, density beyond the cutoff, oblate
  enclosure, airless/unknown/invalid/extended envelopes), the bulge and air
  widening a giant's clearance, a route round an atmosphere never entering
  it, and the refusals. The existing route tests now use explicitly airless
  planets. `./scripts/check.sh` passes.
- CLI test flight in `g13b7db6.4.24`: a start 150 km over the 8.39 bar
  rocky `/6` (clearance 284 km) was refused; from 400 km it flew to the
  sub-Neptune `/5` and arrived on time, showing its extended-air warning; a
  route from 200,000 km beyond `/5` to the star behind it steered round
  `/5` and arrived.
- Not covered: exact spheroid routing (the bound is a sphere round the
  equator), a hot thermosphere (Earth-like worlds clear at a few hundred km,
  lower than the real Earth needs), rings and drag itself.

1. **One simulation-owned clearance policy.** Separate the physical exclusion
   boundary from the pilot's preferred routing margin. Use it for obstacle
   wrapping, bend/closing-speed limits, route timing and destination standoffs;
   future collision/approach assists reuse it. No renderer or debug-preset input.
2. **Oblateness and atmosphere together.** Use the generated spheroid, not its
   volume-equivalent mean radius as a bounding radius. Keep the existing spherical
   routing algorithm initially: conservatively enclose the planet and its modelled
   atmosphere with `equatorial_radius + atmosphere_extent`, then add explicit
   ship-hull clearance and a tunable safety margin. Airless worlds use zero
   atmosphere extent. This safely covers equatorial, polar and tilted passes,
   at the cost of extra clearance near the poles. Direction-dependent spheroid
   routing can come later. Check consistency with the rendered shell geometry.
3. **Physical inputs and validity.** Recompute extent from validated generated
   atmosphere state using its mean reference radius and GM, following the
   [column conventions](../simulation/atmospheres.md#derived-column). A one-bar
   envelope level is not solid ground, but its atmosphere still needs clearance.
   The operational cutoff is a navigation policy, not a physical end of air.
   Preserve extended-column warnings and their unclamped estimates; report that
   approximation in route diagnostics. Unknown/invalid atmosphere data must not
   silently become airless: refuse or hold safe routing with an explicit reason.
4. **No safety-boundary bypass.** The existing low-orbit-target rule may relax
   a preferred margin, never shrink the physical exclusion boundary. Refuse a
   target/arrival region that cannot be reached safely, including a craft inside
   that boundary or an atmosphere leaving no valid orbital standoff. Check the
   whole arrival region, not only the target point. Starting inside the boundary
   needs a defined outward-only escape policy or an explicit refusal, not a
   teleport or a route through the planet. Recheck moving targets each step.
5. **Verify and accept.** Regressions: spherical/airless baseline; highly oblate
   equatorial, polar and tilted flybys; thick and extended atmospheres; envelope
   worlds; moving blockers; unsafe low-orbit craft targets; starts inside the
   boundary; and no safe standoff within orbit reach. Cover route plotting,
   per-step avoidance and dropout, deterministic replay and unchanged unsafe
   manual warp. Run workspace checks, then user-watched CLI flights before
   committing implementation. Update the craft guide and readouts with the
   implemented policy and limits.

**Done when:** plotted avoidance and arrivals stay outside the combined
shape/atmosphere boundary, or explicitly report why a safe route is unavailable.
Atmospheric drag, climate, exact spheroid routing and general collision response
are not prerequisites for this correction.

### 10 · Orbit autopilot (deferred)

Deferred with the assists, until planetary atmospheres. Phase 5's rough-orbit
exit, used by `warp to --orbit`, already ends near-circular (eccentricity 1e-7
to 1e-3 in the phase 9 live test), so "go to planet" needs no player input.

- After drop-out, a short sublight burn (seconds to a minute) circularises the orbit.
- Close-range correction only; no interplanetary transfer planning.
- **Done when:** "go to planet" ends in a stable orbit with no player input.

### 11 · Layered controls ⏸ Paused after checkpoint 3

Paused by the user on 2026-10-23, with the ship fully flyable in the app:
piloting, manual warp and mouse steering are done. The rest (in-app
autopilot, the layers, the assists and their readout) waits for planetary
atmospheres, so assists and route standoffs can use each body's atmosphere
as well as its size. See [paused work](#paused-until-atmospheres).

- In-app piloting: a piloting state where the keys fly the ship instead of
  the camera, with a key layout for all six axes and the throttle.
  - A key (Tab) toggles piloting while a ship is selected. The camera
    follows the ship as today's follow camera does, by position only: it
    does not turn with the ship. Tab or Esc leaves piloting.
  - Leaving changes nothing on the ship: warp continues and the throttle
    keeps burning. Fine thrusters stop, as their keys are no longer read.
  - Piloting holds the clock at 1× real time, so the controls feel direct.
    Leaving keeps 1×; the previous rate is not restored.
  - Mouse steering is a virtual joystick: while right mouse is held, the
    pointer's offset from where it was pressed gives pitch (up and down)
    and yaw (left and right) from -1 to 1, reaching full deflection at a
    ring about 150 points out, past a small dead zone. An on-screen reticle
    shows the press point, ring and stick; the pointer is hidden while
    steering, so the screen edge never stops it. Holding still keeps the
    value; releasing returns it to zero. Arrow keys give full deflection.
    Mouse up is nose up; an invert option can come with the bindings.
  - The value is the existing `rotate` input, read as it is today:
    - In warp, a fraction of the bubble's turn rate (default 30 °/s): a
      steady turn while held, stopping within about 0.3 s of release.
    - In sublight, a fraction of the turning thrusters' power: held, the
      spin keeps growing (about 5.7 °/s per second at full); released, it
      persists until countered or stopped with T.
- Manual warp in the app: mouse steering while held (a flight stick later),
  warp speed up and down on the logarithmic scale, and looking sideways
  without steering.
- **One layout, read two ways.** Most keys mean the same in both modes and
  keep their free-camera meanings; Shift/Ctrl is the one "how fast" control.
  Thrust and warp stay mutually exclusive (phase 4), so thruster keys do
  nothing in warp.

  | Control | Thrusters (sublight) | Warp engaged |
  | --- | --- | --- |
  | Hold right mouse, or arrow keys | Pitch and yaw | Steer the bubble's path |
  | Q / E | Roll | Roll, direction of travel unchanged |
  | W / S | Fine thrusters forward and back | Nothing |
  | A / D, R / F | Fine thrusters left, right, up and down | Nothing |
  | Shift / Ctrl (held) | Main drive throttle up and down; below zero is reverse | Warp speed up and down, sweeping the logarithmic scale |
  | Z / X | Full throttle / cut throttle | X: slowest warp speed; Z: nothing |
  | G | Cut throttle and fine thrust, then engage warp | Disengage |
  | T | Stop rotation | Stop rotation |
  | Left drag | Look around the ship without steering | Same |
  | Mouse wheel | Camera follow distance | Same |
  | Tab or Esc | Leave piloting | Leave piloting (warp continues) |

  - Fine thrusters fire only while held. The throttle and warp speed are
    levers: they stay where they are left, so releasing warp speed holds
    the cruise.
  - Warp speed sweeps at a steady rate on the logarithmic scale, starting at
    2 decades per second (about 10 s from 1 m/s to the top speed), so every
    factor of ten takes as long and a tap nudges it. It is a held,
    fixed-step input like the throttle (a warp speed rate from -1 to 1), not
    a stream of set commands; `warp up`, `down` and `set` stay for the CLI
    and autopilot.
  - Warp engages at the last warp speed used, shown in the readout before G.
    G cutting the throttle is shown there too, so it is never silent.
  - Assists are deferred, all of them, until the user asks for them. That
    includes a sublight rate hold (the stick setting a target spin rate,
    with releasing braking to zero), which would need a new fixed-step
    order in `crates/simulation` like stop rotation.
  - Point orders (prograde and so on), circularise and match target belong
    to the Assisted layer, on number keys or a small menu, not this layout.
- **Rebindable later.** A controls menu where the player can rebind
  everything, the free camera included, is out of scope, but the input code
  must make it easy:
  - Input is read through named actions (`PitchUp`, `ThrottleUp`,
    `EngageWarp`, `Look`...) and one binding table mapping keys, mouse
    buttons and axes to them. Systems never test a key code directly.
  - The free camera's keys move onto the same table in this phase, so there
    is one source of truth, and the controls help reads from it.
  - Axis actions take a value from -1 to 1, so a flight stick or gamepad
    axis can later bind to the same action as a pair of keys.
  - Bindings are plain serializable data with the current layout as the
    default, ready to be saved and edited by a future menu.
- Decide whether the deferred assists are needed, once mouse steering can
  be felt: turn limit (phase 6), collision (7), approach (8) and orbit (10).
- The warp assist toggles, the selected target, requested and actual
  speed and the reason for any limit.
- **Autopilot:** pick a target; everything else is handled.
- **Assisted:** the pointing assists from phase 2, plus circularise, hold
  orbit and match target.
- **Manual:** assists off.

Checkpoints, each checked live by the user and committed:

1. ✅ **Actions and piloting:** the binding table with the free camera moved
   onto it, the piloting state (Tab, Esc, 1× clock), keyboard thrusters,
   throttle and rotation, and a minimal readout.
2. ✅ **Manual warp:** G (engage and disengage, cutting throttle and fine
   thrust), the warp speed rate input, X for the slowest warp speed,
   thruster keys inert in warp, and the readout showing the warp speed G
   engages at and that G cuts the throttle. Arrow keys already steer, so
   warp can be flown before the mouse. The rate input is a held `warp
   sweep` order in `crates/simulation`.
3. ✅ **Mouse steering:** virtual-joystick steering with its reticle and the
   hidden pointer, free look while piloting, and the invert option (in the
   debug console's `controls` tab, not saved).

#### Paused until atmospheres

Atmospheres are now generated; the pause is not lifted automatically. Complete
the [clearance follow-up](#shape-and-atmosphere-clearance-follow-up), then replan
the remaining controls with the user. What phase 11 still lacks:

- **In-app autopilot:** pick a target and `warp to` it, and cancel, without
  the CLI; the route's target, distance, ETA and any slowdown in the pilot
   readout. Route standoffs must clear the oblate body and its atmosphere.
- **The layers:** how Autopilot, Assisted and Manual are chosen. Assisted
  brings `point` (prograde and so on), matched dropout, circularise, hold
  orbit and the sublight rate hold to the app.
- **The assists:** turn limit (phase 6), collision (7), approach (8) and
  orbit (10). Collision and approach clearances and speed limits take each
   body's atmosphere extent and oblateness into account, not only its mean radius.
- **The assist readout:** assist switches, requested against actual speed
  and the reason for any limit. A separate checkpoint was dropped, as there
  is nothing to show until the assists exist.
- Not yet planned anywhere: spawning and despawning ships in the app.

### 12 · Walking inside the ship

- First person in a grey-box interior, simulated in the ship's own local frame
  so it stays steady at any galactic position. The cockpit view is following
  at zero distance, turned with the ship's orientation.
- Collisions from a physics crate (`avian3d` or `bevy_rapier`), after checking
  compatibility with the pinned Bevy, big_space and bevy_egui set.
- Fixed deck gravity for the alpha.
- The nav console opens the existing navigation UI full-screen on interaction.
- **Done when:** the whole loop is playable on foot from the console. This is the alpha.

## Later

- Time warp, with coasting ships propagated analytically ("on rails") and
  numerical integration only while thrusting. Rails assume one dominant body
  (spheres of influence), a deliberate simplification of the summed gravity.
- More craft: stations on rails, rovers on planet surfaces, probes. The craft
  model and browser integration already cover them.
- Moons and asteroids as autopilot targets, each a new source of the
  phase 9 target facts.
- Craft saved on the backend, then rudimentary multiplayer: other players'
  ships shown live through a WebSocket channel.
- Then server-authoritative multiplayer: the server runs all craft from
  players' inputs, with client prediction and correction.
- Live UI on in-world screens (render to texture).
- Thrust gravity, fuel and delta-v limits, crashing into bodies.
- Proper ship models.
- Warp between galaxies, once there is more than one.
- The warp arrival burst as a hazard, scaling with faster-than-light trip
  length.
- **Atmospheric flight:** generated column density and extent now exist.
  Integrating extent with warp clearance is the follow-up above; drag on ships
  relative to rotating air remains later work, not a prerequisite for safe routes.
- Moving stars over limited time spans (galaxy rotation stage 2): cells loaded
  by rotating back to stars' birth cells, with padding for wobble.

## Open questions

- Ship size and style: small shuttle or Expanse-style tower?
- Fuel limits, or unlimited for the alpha?
- Can the ship crash?
- Thrust gravity or artificial deck gravity in the long run?
- What warp does in an atmosphere: refuse to engage, drop out, or feel drag?
