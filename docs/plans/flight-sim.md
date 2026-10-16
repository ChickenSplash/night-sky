# Flight simulator · alpha plan

A proposal, not a description of the current application. Camera flights today
are navigation aids, not physical spacecraft (see [navigation](../client/navigation.md)).

## Goal

- Walk around inside a ship as a character and use its consoles, starting with
  navigation.
- Realistic physics, but easy for a new player to pick up and explore straight away.
- **Core loop:** spawn in deep space, warp to a system in about a minute,
  warp to a planet and settle into orbit, then warp on to the next. Every
  trip takes roughly 20 to 60 seconds, whatever its length.

## Principles

- **Plausible physics.** Real gravity from every nearby body, so the ship can
  orbit anything. No invented forces beyond the warp drive's documented rules.
- **Two drives:**
  - **Warp drive (Alcubierre):** used for every trip, in-system or interstellar.
  - **Sublight drive (fusion torch, about 1 g):** only for fine-tuning and
    settling into orbit.
- **Layered control.** Autopilot is enough for a new player. Assists and full
  manual control are optional depth.
- **Time warp is optional.** The loop must never need fast-forward.
- Existing invariants still apply: f64 physics, deterministic generation,
  stable body identities and both client targets.

## Warp drive rules

These are the drive's known theoretical limits, used as game rules:

- **Plot, then engage.** The course is fixed before engaging; there is no
  steering inside the bubble. This fits "click a target, autopilot".
- **Minimum safe distance.** The bubble cannot form or collapse too close to a
  body (inside or just above its atmosphere). Engaging from orbit is allowed,
  so the ship warps straight out of one orbit and into the next.
- **Trip time is roughly constant.** Speed builds after departure and falls near
  masses, so short and long trips both take about 20 to 60 seconds.
- **Velocity matched on drop-out.** The bubble moves space, not the ship, so it
  hands the ship off at the target's velocity (planet or star), at a safe
  distance and in a rough orbit. A realistic 1 g drive would otherwise need 30
  to 60 minutes to match a planet's 10 to 50 km/s.

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

### 4 · Warp drive

- In-system first (planet to planet), then interstellar on top of the existing
  galaxy travel and streaming.
- Course plotting, minimum safe distance, trip-length speed scaling and
  velocity-matched drop-out, as in the rules above.
- The ship's frame changes as it travels (galactic in deep space, then the
  destination star's). Following tracks it across frames, and the visited
  system switches to the ship's while following.
- **Done when:** the core loop runs end to end from the CLI.

### 5 · Orbit autopilot

- After drop-out, a short sublight burn (seconds to a minute) circularises the orbit.
- Close-range correction only; no interplanetary transfer planning.
- **Done when:** "go to planet" ends in a stable orbit with no player input.

### 6 · Layered controls

- In-app piloting: a piloting state where the keys fly the ship instead of
  the camera, with a key layout for all six axes and the throttle.
- **Autopilot:** pick a target; everything else is handled.
- **Assisted:** the pointing assists from phase 2, plus circularise, hold
  orbit and match target.
- **Manual:** assists off.

### 7 · Walking inside the ship

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

- Ship size and style: small shuttle or Expanse-style tower?
- Fuel limits, or unlimited for the alpha?
- Can the ship crash?
- Thrust gravity or artificial deck gravity in the long run?
