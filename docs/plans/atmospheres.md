# Planetary atmospheres · plan

A proposal, not a description of the current application. The rendering
guide's atmospheres section ([rendering](../client/rendering.md)) describes
what is built.

Atmospheres come before moons and the galactic dust work: flight-sim phase
11's autopilot standoffs and assists wait for each body's atmosphere extent
(see [flight sim](flight-sim.md#paused-until-atmospheres)), and moons later
reuse the same model as just another body.

## Goal

- Every planet gets a generated atmosphere, or none: whether it keeps one,
  its composition, surface pressure, scale height and extent.
- Climate: equilibrium and greenhouse surface temperature, the equator to pole
  difference and a tidally locked world's day to night difference.
- Atmospheres are seen: a scattering shell coloured by composition, a lit
  limb, a twilight band and the sky from inside.
- The atmosphere's extent feeds flight-sim standoffs and clearances.

## Order

1. ✅ **Rendering prototype** (stage 1).
2. **Rendering, finished** (stage 2, below): multiple scattering, then every
   planet with air. **4c done, checked live and accepted by the user.** It
   builds on the shape and ground chunks from [planet surfaces](planet-surfaces.md).
3. **Simulation** (stage 3, now planned together with integration in
   checkpoint 5): physical data model first, then retention and generation,
   optical conversion and per-planet rendering. More detailed climate follows
   later. The sliders stay as a debug override.
4. **Search and the information overlay:** the new fields.

Not planned yet: clouds, gas giant banding, aurorae, rings.

## Rules for rendering work

- One change per checkpoint, behind a `set_debug` switch to compare with
  what came before, checked live by the user before it is committed.
- A change meant to be faster must look the same with its switch flipped.
- Measure against the Monte Carlo reference before judging by eye.

## Built so far (branch `atmospheres`)

- A shell sphere picks pixels; `render/atmosphere/atmosphere.wgsl` marches single
  scattering (Rayleigh, Cornette-Shanks Mie, a tent absorbing layer) in
  planet radii, energy-conserving steps (Hillaire 2015), bunched near a
  camera low in the air. Premultiplied output, grey dimming of what lies
  behind; ground lighting now includes filtered sunlight and skylight (4c).
  Air is on the selected planet only, set by the `atmosphere` debug tab, `set_atmosphere` or
  `ns atmosphere`, with Earth, Venus, Mars, Titan and hydrogen giant presets.
- **Jitter** (`atmosphere_jitter`, on by default; the user checks with it
  off and may want it off by default).
- **Transmittance table** (`atmosphere::tables`, `render/atmosphere/tables.rs`):
  256 x 64 optical depth, Bruneton's parametrisation, built on the CPU in
  the background per air. Looks identical; 5.6 → 2.4 ms in one view.
- **Stop where opaque** (`atmosphere_stop_opaque`): each ray ends where the
  air in front reaches optical depth 10 in its least dimmed channel, found
  from the table; what lies behind is still dimmed by the whole ray. This
  ended all banding and seams: Venus is a smooth ball, Earth unchanged.
- View steps default to 16; even 4 look smooth now. Settle a lower default
  with the user.
- GPU baseline (before the table, 16 / 24 view steps): the air adds 0.4 /
  0.75 ms from orbit and 0.8 to 1.8 ms from inside.

## Checkpoint 4 · Multiple scattering (complete)

Original problem: Venus's surface was black and Titan far too dark with
single scattering only. Addressed by 4a to 4c below.

### 4a ✅ Reference (commit `499c367`)

`render/atmosphere/reference.rs`, tests only: Monte Carlo with delta
tracking through the same air, any number of scatterings off the air and a
grey ground (albedo 0.3), starlight at each event from the transmittance
table. A fast test checks scattering once matches a fine march. Measurements
(release, `-- --ignored --nocapture`):

- `measure_multiple_scattering` (20 s): all orders over scattering once.
  Earth 1.4 to 2; Mars and hydrogen giant 1.2 to 4.4; Titan 10 to over 1000.
- `measure_venus_haze_absorption` (8 s): Venus's haze must barely absorb.
  At Earth's 0.1 absorbed per event its ground stays black even with every
  order. Ground light (planet mean, green) and reflectance from orbit:
  0.01 → 1.3%, 0.82; **0.005 → 3.0%, 0.89**; 0.001 → 6.6%, 0.97. Real
  Venus: about 2.5 to 3% and 0.75.

### 4b ✅ Multiple scattering table (commit `e0d024d`)

**Pick up here (2026-10-24):**

- 4b checked live by the user and accepted. Stage 2 paused here for
  [planet surfaces](planet-surfaces.md); the shape and chunked ground are now
  built, and 4c uses their shared surface shader. Landing controls remain
  separate work in that plan.
- **Pop-in, deferred by the user:** the multiple scattering arrives seconds
  after the rest, so the look changes suddenly. Proposed (not yet agreed):
  build it in parallel across cores (rows are independent; the solve is
  about 0.5 s), build tables before a planet is seen (on entering its
  system, nearest first, once air is per planet in checkpoint 5), and fade
  the extra light in over a moment as a safety net.
- GPU run at `cbb6b17` (planet `g276dff.4.5146/7`, 2548 x 1388, every switch
  on, multiple scattering on only, so no on/off comparison): from low down
  the air adds 1.1 to 1.8 ms. The orbit and high views are noisy (the air-off
  frame swings 1.1 to 3 ms, frames at 16.7 ms), probably tables building in
  the background; rerun with the air settled if the cost matters.
- Test build: `/tmp/ns-jitter-test.sh` (API port 4330, log
  `data/logs/jitter-test.log`; recreate it if `/tmp` was cleared: it runs
  `target/debug/space-client` with `NIGHT_SKY_API=1 NIGHT_SKY_API_PORT=4330`),
  launched with `hyprctl dispatch "hl.exec_cmd('/tmp/ns-jitter-test.sh',
  { workspace = '8 silent' })"`.

Three tables per air now, each its own job: the transmittance (the view's
dimming, the opaque stop, the ground's dimming), the thinned transmittance
(the starlight in scattering) and the multiple scattering table (the light
scattered more than once, and its flow). The first is built alone (about
0.1 s) so slider drags stay quick; the other two follow together.

Built:

- New setting `haze_absorbed` (share of the light the haze stops that it
  absorbs): slider "Haze absorbs", `set_atmosphere haze_absorbed=...`.
  Earth and others 0.1 (unchanged look); Venus preset 0.005.
- `Air.ground`: the ground's albedo per channel (planet surface tint times
  albedo, set in `render/resolve.rs`; 0.3 default), part of the table key.
- `render/atmosphere/multiple.rs`: the table. 32 x 32 sky texels (the
  star's cosine, -1 to 1, by height, rows evenly in $1 - e^{-h/2H}$) plus 32
  ground texels, per unit starlight: the mean radiance of light scattered
  at least once, and its net upward flow. Every order is solved at once as
  a linear system (Gaussian elimination, 2 to 5 s per air in release, 3 to
  7 s in a dev build), with the ground in the system. `seen()` is the CPU
  model of what the shader draws. Wired into the shader, switch
  `atmosphere_multiple` (debug tab "Multiple scattering"), on by default.
- Test `measure_the_multiple_scattering_table` (about 25 s) prints table
  over reference per channel for sky views, from orbit and the sky light on
  the ground.

How the table works, and why (each found by measuring, order by order,
with a black ground):

- **Isotropic past the first scattering** (Hillaire 2020), with the haze's
  forward peak taken as no scattering at all (`thinned`: Mie scattering
  times $1 - g$). This is done consistently: inside the table the starlight
  is dimmed by the thinned air, and the haze's remaining scattering is
  isotropic.
- **The shader's single scattering dims starlight by the thinned air too**
  (a second transmittance table), so starlight scattered slightly forwards
  still reaches deep into haze. The view's own dimming stays the real air's.
- **The net upward flow** is stored with the mean radiance. The view adds
  the haze's forward scattering of it, $\sigma_M \frac{3g}{4\pi} F \cdot
  (-d)$. Under thick cloud light flows downwards, and forward-scattering
  haze sends it on down, so the sky seen from below is up to twice
  "isotropic". This was the largest single error for Venus and Titan.
- **Gather steps at most 0.3 optical depths**: light deep in cloud
  scatters again within a fraction of a long step, and placing it at the
  step's middle carried it too far (too much light reached Venus's ground).
- **Rows blended on a parabola through three rows** when gathering, not a
  straight line between two, for the same reason.
- **Rows on twice the tallest scale height**: on one, everything above
  55 km on Venus fell into one cell, right where its cloud top reflects.
- Twice the gather directions no longer changes anything (converged).

Results, table over reference (1.00 is exact; deep twilight, near black,
aside):

| Air | Result |
| --- | --- |
| Earth | 0.96 to 1.07 |
| Venus | 0.83 to 1.11; from orbit 1.01 to 1.11 |
| Titan | 0.79 to 1.23 (from orbit at a slant too bright in red) |
| Mars | 0.76 to 1.35: towards the sun too dark, away too bright |
| Hydrogen giant | 0.91 to 1.10 |

At the start of the session: Venus red 0.25, Titan 0.2, Mars 0.5 to 2.3.

Next, in order:

1. Wired into the shader, switch `atmosphere_multiple`, on by
   default: the thinned transmittance table for starlight, and the table
   (mean radiance and upward flow, one lookup each per view step, light per
   star, also in its shadow). Built after the transmittance table, in a dev
   build 2.9 s for Earth and 6.6 s for Venus. ✅ Checked live by the user.
2. Only if Mars looks wrong: store the flow's horizontal part towards the
   star as well (the remaining error there is light flowing away from the
   sun, scattered on forwards).

### 4c ✅ Light on the ground

Implemented, checked live and accepted by the user:

- The surface shader reads row 32 of the multiple-scattering sky image, by
  each star's cosine, and multiplies its reflected radiance by that star's
  exposed flux. The table already includes surface albedo, thinned direct
  sunlight and skylight, so these are not applied twice.
- Both ground chunks and the fallback sphere share this lighting. A common
  readiness decision keeps the old surface and shell behaviour until the
  current air's table is available; then the shell skips its old `sunlit`
  dimming, but still dims the view path.
- `set_debug atmosphere_ground false` compares the old grey dimming. On by
  default, needs atmospheres and tables, independent of sky multiple scattering.
- Visual check: Earth, Venus and Titan from orbit and just above the ground,
  towards and away from the star and around twilight. Let the tables settle
  after each preset; compare the switch, then `ground false` for the sphere.
  Check for double-darkening, a bright rim, seams and stale light on changing
  planets. The point-spread rim sprites still use airless photometry.

Automated checks passed: workspace tests, strict Clippy and headless shader
validation (body, chunk-level tinting, and shell with/without tables and
multiple scattering). The focused `measure_ground_lighting` measurement
compares reflected surface radiance against all-order Monte Carlo paths,
40,000 per channel at star zenith angles 30° and 85°, with grey albedo 0.3:

| Preset | Table / reference, across RGB and both angles |
| --- | --- |
| Earth | 0.998 to 1.033 |
| Venus | 0.869 to 0.967 |
| Mars | 0.977 to 1.111 |
| Titan | 0.823 to 0.983 |

These check the existing table's ground row, not GPU screenshots. Titan's
largest deficit is at low sun; the scattering solver is unchanged. Repeat
with `cargo test --locked --workspace measure_ground_lighting -- --ignored --nocapture`.

## Checkpoint 5 · Every planet with air

Agreed direction: scientifically grounded, simple first, with explicit
approximations that can be improved independently. Generate a plausible
present-day state, not a full formation or atmospheric-evolution simulation.
Keep physical state, optical conversion and rendering separate.

### 5a ✅ Physical data model

Implemented foundation in `system/atmosphere.rs`; see the
[physical atmosphere guide](../simulation/atmospheres.md) for current behaviour
and limits. Not attached to generated planets or rendering. Seven focused
tests and `./scripts/check.sh` passed (workspace tests, formatting and strict
Clippy). Next: 5b, with generation rules agreed before implementation.

- Simulation-owned atmosphere or explicit absence; no Bevy or shader settings
  in the model. Gas mole fractions (N₂, CO₂, H₂, He, H₂O, CH₄, O₂, Ar),
  reference pressure and temperature, and a separate simple aerosol description.
  Supporting oxygen does not imply generating Earth-like oxygen routinely.
- Reference pressure means pressure at the planet's reference radius: the
  reference ground surface on rocky/dwarf worlds, a chosen 1 bar level on
  envelope worlds. The latter is not solid or landable ground.
- Begin with an isothermal column. Distinguish its reference temperature from
  equilibrium temperature and any later surface-climate estimate. Derive mean
  molecular mass, density, gravity and scale height ($H = kT/\bar m g$).
- Define an operational extent, initially 12.5 scale heights or the aerosol
  extent if taller, not a physical edge/exobase. Flag columns outside the
  thin-atmosphere approximation rather than silently treating them as valid.
- Validate mixtures, pressure and temperature; check Earth-, Mars- and
  hydrogen-rich example columns. Use SI internally. Leave generated planets
  and rendering unchanged: this step builds only the tested foundation.

### 5b · Retention and generation

- Deterministic per-planet composition and pressure, informed by mass/radius,
  irradiation, stellar age/activity and available volatiles. Use smooth,
  tunable heuristics, with the cosmic shoreline as an empirical guide rather
  than a universal law; account approximately for loss near active stars.
- Correlated atmospheric families, not independently random sliders. Allow
  airless worlds, heavy secondary air, cold volatile air and hydrogen-rich
  envelopes. Ice giants also have hydrogen/helium outer atmospheres.
- Start with approximate thermal/condensation rules, not a full climate model.
  Centralise uncertain prescriptions and seed air independently of orbits/spin.

### 5c · Physical state to optics

- Derive gas scattering, density and vertical profiles from the physical state;
  approximate visible absorbers and aerosols separately. CO₂ does not imply an
  orange sky, and greenhouse absorption is not the shader's visible absorption.
- Check against the existing reference worlds. Keep assumptions explicit and
  the conversion replaceable without changing the renderer or generation.

### 5d · Per-planet rendering

- Feed generated air to every planet; `AtmosphereTuning` becomes a selected
  planet's debug override. Define an opaque lower-cloud approximation for
  giants without introducing landable ground.
- Tables cached by air with a memory limit; shells sorted together, with no
  pop at the point/disc hand-over. Table-build pop-in remains a separate,
  previously deferred issue to agree before changing its behaviour.

### 5e · Validation

- Inspect population distributions and physical invariants across many systems,
  not only hand-picked worlds. Check several different atmospheres together,
  airless worlds and giants; rerun the atmosphere test and user visual checks.

## Checkpoint 6 · Docs

The rendering guide's atmospheres section no longer "prototype", benchmarks
updated, and the documentation index's limits updated. Document implemented
generation and approximations; leave richer climate as later work.

Later, only if wanted: colour-dependent dimming of what lies behind
(dual-source blending), a sky-view table if views from inside cost too much.

## Working notes

- **Code:** `render/atmosphere.rs` (settings, presets, material, sync),
  `render/atmosphere/atmosphere.wgsl`, `render/atmosphere/tables.rs`,
  `render/atmosphere/multiple.rs`, `render/atmosphere/reference.rs`,
  `render/debug.rs` (switches), `devtools/tabs/atmosphere.rs`,
  `devtools/atmosphere_test.rs` (benchmark), `scene/draw.rs`.
- **Live checks are the user's.** Test builds open on workspace 8 without
  focus, the developer API on its own port, driven with
  `./scripts/ns --port <port>`. WGSL compiles only at run time: check the
  build's log for errors. Don't take screenshots unless asked.
- The user runs the atmosphere GPU test; read
  `data/logs/atmosphere-test.jsonl`.
- **Test planets:** `gf008000017.2.23/5`, `g1e01000002.2.162/0`.
- The rejected first stage 2 attempt is in the stash "stage 2 checkpoints
  1-3 (tables, two-stream scattering, dual-source), rejected look";
  reference only.
