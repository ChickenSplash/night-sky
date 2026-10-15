# Flight simulator · alpha plan

A proposal, not a description of the current application. Camera flights today
are navigation aids, not physical spacecraft (see [navigation](../client/navigation.md)).

## Goal

- Walk around inside a ship as a character and use its consoles, starting with
  navigation.
- Realistic physics, but easy for a new player to pick up and explore straight away.
- **Core loop:** spawn in deep space, warp to a system in about a minute,
  warp to a planet and settle into orbit, then warp on to the next. Plotted
  trips aim for roughly 20 to 60 seconds, whatever their length. Manual
  exploration has no fixed trip duration: the player chooses direction and
  speed, with optional help managing approaches and arrival velocity.
- **Free exploration:** manually cross from day side to night side in
  seconds, fly to another planet or out of the system, without choosing an
  autopilot destination. Planet-to-moon exploration uses the same controls
  once moons exist; adding moons is not a prerequisite for these phases.

## Principles

- **Plausible physics.** Real gravity from every nearby body, so the ship can
  orbit anything. No invented forces beyond the warp drive's documented rules.
- **Two drives:**
  - **Warp drive (Alcubierre-inspired):** manual exploration and plotted trips,
    in-system or interstellar, under explicitly fictional drive rules.
  - **Sublight drive (fusion torch, about 1 g):** only for fine-tuning and
    settling into orbit.
- **Layered control.** Autopilot is enough for a new player. Assists and full
  manual control are optional depth.
- **Time warp is optional.** The loop must never need fast-forward.
- Existing invariants still apply: f64 physics, deterministic generation,
  stable body identities and both client targets.

## Warp drive rules

The Alcubierre metric motivates the bubble, not a proven engineering design.
There is no established steerable drive, obligatory nose alignment, safe
altitude or velocity-matching mechanism. The following are deliberate game
rules. Ordinary flight retains summed gravity and Newtonian thrusters; warp
is a separate motion mode, not an enormous ordinary acceleration.

- **Two ways to fly one drive.** Manual warp follows the ship's nose; a
  plotted autopilot steers and regulates the same drive towards a destination.
  The old fixed-course/no-steering restriction is removed. Looking sideways
  moves the camera, not the ship's heading.
- **Nose-directed steering.** Pitch/yaw curve the bubble's path; roll changes
  the ship's orientation, not the travel direction by itself. Turning is
  limited at high speeds; tight turns require slowing down. No aerodynamic
  lift, bank-to-turn force, or atmosphere is implied. Engagement must handle
  existing spin safely, not instantly redirect a fast bubble.
- **Wide-range speed control.** Manual W/S (or equivalent future bindings)
  changes desired warp speed, with a nonlinear/logarithmic range rather than
  delicate linear increments spanning metres to interplanetary distances.
  Releasing the speed control retains cruise speed, not continued acceleration.
  Ship thrust throttle and warp speed are distinct inputs. Exact ranges,
  ramps and turn limits are tuning parameters, not settled physics.
- **Minimum clearance and swept-path safety.** No engagement or travel inside
  a body or its atmosphere/exclusion region. Orbital engagement is allowed.
  Check the path swept during a step, not just its endpoint, including moving
  bodies. This hard safety envelope remains even with the optional assists
  off. Unknown terrain/body data is not permission to fly through it: slow or
  suspend warp at a known-safe position until the required data is available.
- **Explicit disengagement.** A player can end bubble travel to turn or look
  around. This stops warp displacement, not conventional velocity or gravity.
  In a safe region, dropout need not require a long conventional braking burn.
  Prevent entry into forbidden regions rather than trapping a ship there
  with a drive it cannot disengage. Exact stop/ramp behaviour is still a
  confirmation item below.
- **Plotted trip timing.** Aim for about 20 to 60 seconds at 1× simulation
  time, using a distance-scaled cruise profile and slower approaches. Pause
  stops travel; time acceleration changes real-world duration. Manual trips
  have no fixed-duration promise.
- **Two distinct velocities.** Track ordinary physical velocity independently
  of bubble motion. Expressing a velocity in a new reference frame must not
  itself change momentum. With matching off, preserve the underlying
  conventional velocity through warp and transform it consistently at frame
  boundaries. Do not apply bubble displacement as a conventional launch
  velocity on dropout.
- **Assisted dropout.** With reference matching enabled, the drive explicitly
  changes ordinary velocity to the selected body's velocity plus a bounded
  local exit velocity. This is a fictional momentum/energy-changing capability,
  not merely a coordinate conversion. A planned orbital arrival also includes
  the sideways velocity for a rough orbit; matching a planet's translation
  alone would leave the ship falling towards it. Unassisted/manual dropout
  does not silently insert a circular orbit.

### Three independently optional warp assists

The recommended beginner preset enables all three; players can toggle each
through the CLI first and later through the piloting UI. Show requested and
actual speed, active reference, target and the reason for any speed limit.
Hard exclusion/unknown-data safeguards are separate from these assists.

1. **Safe-speed assist, no destination required.** Reduce permitted speed near
   surfaces and along predicted close approaches, then smoothly raise it as
   clearance grows. Use swept-path clearance, closing speed and steering/
   reaction distance, not just the nearest body's centre distance. Recognise
   small bodies ahead without unnecessarily crawling past a body safely off
   the flight path. Do not suddenly reaccelerate to an old high setting after
   passing an obstacle without a predictable ramp and clear feedback.
2. **Approach assist, selected destination.** Regulate closing speed and final
   standoff relative to a chosen moving planet/star (later a moon), while the
   player continues steering. It is not heading control or full autopilot;
   if the player turns away, do not pull them towards the target. Share the
   speed profile with plotted arrivals. A target's unsafe/unreachable state
   must be reported rather than concealed by a teleport or silent retarget.
3. **Reference/velocity-matching assist.** Automatically choose a planet, star
   or nested barycentre using phase 3's hierarchy and switching margins, with
   an explicit player override. Smooth the control/display reference changes
   without jumping the ship's position or silently altering physical velocity.
   Apply the documented assisted velocity handoff on dropout. Selecting a
   new reference must not automatically change heading or engage autopilot.
   Warn about unsafe exit speed when matching is disabled. Being matched to
   a body is not a promise of a stable orbit.

The assists are independent, but their limits compose predictably: manual
requested speed is constrained by the active speed/approach assists and the
hard safety envelope. Reference matching supplies a velocity frame, not a
competing steering controller. Removing an assist does not silently enable
another one; disengaging plotted autopilot does not secretly leave its
steering active.

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

### 3 · Trajectory prediction

- A predicted trajectory line ahead of the ship. This is the most important
  usability feature: it makes orbital mechanics readable.
- Predicted from the ship's current state as if it coasts, with bodies on
  their Kepler paths, using the same gravity and integrator at a coarser step
  within a per-frame budget. Redone when the inputs change and live during a
  burn.
- Drawn relative to the body the ship orbits, so an orbit shows as a closed
  near-ellipse rather than a spiral smeared along the planet's own path.
  Perturbations from other bodies remain visible; closure is not guaranteed.
  In multiple systems the reference can be a component star or a nested
  barycentre, not simply the strongest-pulling star. Selection follows the
  stellar hierarchy with conservative influence/stability boundaries and a
  switching margin. These are display/reference heuristics, not changes to
  the summed gravitational forces, and do not certify a stable orbit.
- Keep the whole prediction relative to its current reference, subtracting
  that reference's future position at each sample and anchoring the result
  at its present position. Mark predicted reference exits/encounters; switch
  the displayed reference when the ship actually crosses the boundary.
  Do not splice differently referenced segments in the first version.
- Apoapsis and periapsis in the inspector and `ns ship state`.
- **Done when:** a burn from the CLI visibly reshapes the predicted orbit.

Checkpoints, each committed and checked on both targets:

1. **Hierarchical orbit references** (`crates/simulation`): planets, individual
   stars and nested barycentres, with position, velocity and effective mass.
   Use the existing system hierarchy and stellar stability/influence zones
   to choose a useful local reference, expanding to enclosing pairs outside
   component regions; retain the current reference through a switching
   margin. Planet selection starts with the existing Hill-sphere heuristic.
   Keep reference selection separate from gravity sources: a barycentre is
   not an extra gravitating body. Test circumstellar, circumbinary and nested
   triple cases, plus boundary hysteresis. Share the reference with craft
   readouts and orbital pointing assists so they describe the same orbit.
2. **Bounded coast predictor** (`crates/simulation`): copy the current
   translational state and predict with all propulsion off, under every
   actual star and planet on its analytic path. Share the force model and
   Verlet integration, with a prediction-only timestep policy. Bound the
   horizon, work and sample count; use about one estimated period for bound
   paths and a finite horizon for escapes/undefined periods. Check accuracy
   against fine fixed-step runs, especially at periapsis and encounters.
   Return the achieved horizon and any truncation, never imply a partial
   calculation is a complete orbit. Do not modify the real ship or clock.
3. **Client prediction scheduling:** budget work across frames on native and
   WASM, refresh from current state during flight and burns, and invalidate
   on orders, despawn or reference changes. Discard obsolete work, retain a
   clearly aged last result while updating, and prevent refreshes from
   starving completion. Test pause, high time rates and multiple craft.
4. **Trajectory and events:** draw the selected ship's path camera-relatively
   in f64 before GPU conversion, in the fixed reference chosen above. Show
   reference name, predicted influence exits/encounters and prediction age /
   horizon. Distinguish sampled predicted apsides from existing instantaneous
   two-body estimates, including in `ns ship state` and the inspector.
   Test planet and barycentre anchoring, switching and close/far render scales.
5. **End-to-end verification and docs:** from the CLI, make a burn and verify
   that the coast path reshapes, then check a planetary escape, a
   circumbinary orbit and reference switching. Run the full native/WASM
   checks, update craft/rendering/developer API docs and mark the phase done.

### 4 · Shared warp mechanics and frame handoff

- Engine-independent warp state and fixed-step input commands, with separate
  conventional velocity, bubble motion, reference and drive lifecycle.
- Explicit pause/time-rate handling: warp speeds describe simulation-time
  motion. Do not accidentally inherit the phase 2 ordinary-flight 10×
  manoeuvre cap simply because a warp steering controller is active. Report
  any work-budget hold separately from a drive's speed/safety limit.
- Engage, coast and disengage from the CLI; implement explicit matched and
  unmatched dropout rules rather than inheriting camera-flight behaviour.
- Swept-path exclusion checks, orbital engagement, safe interruption and
  fail-closed handling of missing body data. No fuel budget or realistic
  bubble energy requirement in the alpha; that limitation is explicit.
- In-system handoff first, then galactic-to-system and system-to-galactic
  craft frames. Position and conventional velocity transform consistently.
  Use existing stellar kinematic velocities while stars remain spatially
  fixed, documenting that approximation rather than adding galactic motion.
- Following finds a ship across frames and pages. Streaming, floating-origin
  precision and the visited system remain camera-based, even in free camera;
  the craft independently requests the data needed to travel safely. Do not
  make the camera's visited system the ship's authoritative physics frame.
- Define phase 3 prediction behaviour in warp: distinguish a bubble travel
  preview from a conventional coast-after-dropout prediction, and never label
  the old pre-warp orbit as the current forecast.
- **Done when:** CLI tests cover in-system and interstellar frame transitions,
  safe interruption, velocity-preserving unmatched exits and explicit matched
  exits, with no position jumps or body tunnelling, on both targets.

### 5 · Manual nose-directed warp, CLI first

- Manual heading/turn inputs and a persistent desired warp speed, separate
  from ordinary thrust controls. The nose determines bubble travel direction;
  this is a warp-mode handling rule, not a change to ordinary ship dynamics.
- Smooth speed changes over many orders of magnitude, speed-dependent turn
  limits, and reliable stop/disengage commands. Camera look is independent.
- Travel without a destination: around a planet, towards another planet and
  into open space. Extend through the same galactic frame handoffs used by
  plotted interstellar travel. Manual FTL availability awaits confirmation.
- Keep all inputs, drive state and actual/requested speed discoverable in
  `ns ship`, its help, the developer API and the inspector. No mouse/stick or
  keyboard bindings are required until phase 9.
- **Done when:** using the CLI alone, steer around a planet to its night side,
  change direction and cruise speed, leave the system, and stop bubble travel
  without autopilot, teleporting, or confusing warp speed with ordinary speed.

### 6 · Optional warp exploration assists

- Implement safe-speed, approach and reference/velocity matching as the three
  separately switchable assists described above. Beginner preset on; explicit
  state and overrides through the CLI/API, with no hidden heading autopilot.
- Share reference selection with phase 3, and speed/arrival logic with the
  later plotted autopilot. Establish handover rules when toggling an assist,
  changing targets/references, cancelling, or switching manual/autopilot.
- Surface-relative exploration should use clearance above the body, not a
  magic centre-distance slowdown. Test day/night flight and movement between
  very different distance scales; add planet/moon cases when moons exist.
- Test moving targets, close flybys, small obstacles ahead, binary/triple
  references, reference override, missing data and every assist on/off
  combination. Limit changes must remain continuous and explainable.
- **Done when:** a novice-friendly preset permits manually steered planetary
  exploration and approaches to another planet, without precision speed
  juggling; toggling matching gives the documented, observable exit velocity.

### 7 · Plotted warp autopilot

- Plot and execute routes using the same drive, safety checks, references and
  speed/approach controllers as manual flight. Autopilot adds heading and
  route management, not another transport model or a camera teleport.
- In-system first, then interstellar; account for moving destinations and
  intervening bodies, maintain clearance, and aim for the 20–60 s trip profile.
- Destination choice, progress, cancellation and manual takeover through the
  CLI. A plotted safe-orbit arrival explicitly requests matching and rough
  orbital velocity, or reports why it cannot satisfy that arrival mode.
- **Done when:** the CLI core loop runs end to end: depart orbit, warp to
  another planet/system, drop into a rough orbit, and take over manually or
  cancel safely without discontinuities.

### 8 · Orbit autopilot

- After drop-out, a short sublight burn (seconds to a minute) circularises the orbit.
- Close-range correction only; no interplanetary transfer planning.
- **Done when:** "go to planet" ends in a stable orbit with no player input.

### 9 · Layered controls and in-app piloting

- In-app piloting: a piloting state where the keys fly the ship instead of
  the camera, with a key layout for all six axes and the throttle.
- Add the manual warp interface planned above: mouse-steering while held,
  or a flight stick, and W/S-style desired-speed adjustment. Bindings, input
  sensitivity and device support details are designed here, not prerequisites
  for the CLI phases. Permit camera look to the side without steering.
- Expose the three independent warp assist toggles, reference override,
  selected approach target, actual/requested speed and limiting reason.
- **Autopilot:** pick a target; everything else is handled.
- **Assisted:** the pointing assists from phase 2, plus circularise, hold
  orbit and match target.
- **Manual:** assists off.

### 10 · Walking inside the ship

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
- Craft saved on the backend, then rudimentary multiplayer: other players'
  ships shown live through a WebSocket channel.
- Then server-authoritative multiplayer: the server runs all craft from
  players' inputs, with client prediction and correction.
- Live UI on in-world screens (render to texture).
- Thrust gravity, fuel and delta-v limits, crashing into bodies.
- Proper ship models.
- Moving stars over limited time spans (galaxy rotation stage 2): cells loaded
  by rotating back to stars' birth cells, with padding for wobble.

## Open questions

Warp choices to confirm before implementation (the preceding sections record
the design direction, not implemented functionality):

- **Manual FTL:** allow it with increasingly restrictive turn/safety limits,
  or keep manual warp sublight and reserve FTL for plotted trips? Manual
  interplanetary sublight flight alone cannot promise short trips across all
  systems. Recommendation for unrestricted exploration: allow manual FTL,
  subject to data availability and the hard safety envelope.
- **Stopping feel:** immediate bubble stop/disengage when safely clear, or
  a brief predictable ramp? Recommendation: a short normal speed ramp plus
  an explicit immediate dropout at the current safe position; neither command
  cancels conventional velocity. Speed/turn values can be tuned later.
- **Beginner defaults and exits:** confirm the recommended all-assists-on
  preset, unmatched velocity preservation with matching off, and matching-on
  manual exits with a bounded local velocity (not automatic orbit insertion).
  Only an explicit orbital arrival requests rough circularisation velocity.

Implementation tuning, not blocking product decisions: exact exclusion
altitudes, speed ceilings, speed/turn ramps, approach standoff, reference
blending and local exit-speed bounds. Prototype and test these before fixing
numbers. Warp safety is not a substitute for later ordinary-flight collisions.

- Ship size and style: small shuttle or Expanse-style tower?
- Fuel limits, or unlimited for the alpha?
- Can the ship crash?
- Thrust gravity or artificial deck gravity in the long run?
