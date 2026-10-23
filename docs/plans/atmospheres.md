# Planetary atmospheres · plan

A proposal, not a description of the current application. Planets today are
plain Lambertian spheres with fixed class tints (see
[rendering](../client/rendering.md#planet-reflectance-and-phase)).

Agreed with the user on 2026-10-23. Atmospheres come before moons and the
galactic dust work: flight-sim phase 11's autopilot standoffs and assists wait
for each body's atmosphere extent (see
[flight sim](flight-sim.md#paused-until-atmospheres)), and moons later reuse
the same model as just another body.

## Goal

- Every planet gets a generated atmosphere, or none: whether it keeps one,
  its composition, surface pressure, scale height and extent.
- Climate: equilibrium and greenhouse surface temperature, the equator to pole
  difference and a tidally locked world's day to night difference, smoothed by
  thick air carrying heat round the planet.
- Atmospheres are seen: a scattering shell coloured by composition, a lit
  limb, a twilight band and the sky from inside.
- The atmosphere's extent feeds flight-sim standoffs and clearances.

## Order

1. **Rendering prototype** (this plan's first stage): the look and its GPU
   cost, judged in the real app with values set by hand, before any
   simulation work.
2. **Lookup tables and many planets** (see stage 2 below): transmittance
   and multiple scattering precomputed per air (Bruneton 2008, Hillaire
   2020), and the renderer opened to every planet with air. The direct shader stays as a
   reference toggle, to compare the look and frame time. Also colour-dependent
   dimming of what lies behind, through dual-source blending (wgpu's
   `DUAL_SOURCE_BLENDING` feature), so red light no longer leaks through
   thick air under a grey average. Multiple scattering is what lights Venus's
   surface as a dim overcast; its preset then needs nearly non-absorbing
   cloud (about 0.1% per scattering, against Earth haze's 10%).
3. **Simulation:** retention (the cosmic shoreline, Zahnle & Catling 2017,
   with stripping near flaring M dwarfs), composition, pressure and scale
   height, then climate. The sliders stay as a debug override.
4. **Search and the information overlay:** the new fields.

Not planned yet: clouds, gas giant banding, aurorae, rings.

## Rendering principles

- **A shell, not a volume.** A second, slightly larger sphere around the
  planet only chooses which pixels run the shader. The shader intersects the
  ray with the ground and the top of the atmosphere itself, in units of the
  planet's radius and relative to the closest approach, so the edge holds at
  real distances in f32.
- **Single scattering, done directly:** a fixed number of steps along each
  pixel's line of sight, each with a shorter march towards every star.
  Rayleigh scattering by gas, Mie by haze (Cornette-Shanks phase) and an
  absorbing layer such as ozone, each with exponential or tent density.
- **Physical units through the existing exposure.** Each star's flux enters
  with its colour, the shared exposure gain and the planet's balance, so
  scattered radiance sits on the same scale as the surface's $AF/\pi$.
- **Blending:** scattered light is added and whatever lies behind (the ground
  or space) is dimmed by the line of sight's transmittance. In the prototype
  that dimming is grey; colour-dependent dimming comes with the tables.
- Every star of a multiple system scatters in its own colour (`MAX_LIGHTS`).
- Planets drawn as points have no shell.

## Stage 1 · Rendering prototype

Checkpoint 1 is done and checked live by the user (about 0.2 ms of GPU time
with the switch on). The body sphere mesh went from 48 × 24 to 192 × 96
segments: the coarse facets left a dark sliver between the drawn limb and the
shader's analytic ground. The live check found a hard edge in the sky from
inside the air: rays climbing away from the planet were cut off by ground
behind the camera. Follow-up fixes after the live check:

- **Draw order:** the galactic glow and star points now sort as the
  background, before every transparent body, and the shell sorts at its near
  side. The glow flickered as its sort distance crossed the shell's, and the
  planet's limb sprites tied with it.
- **Near plane:** just above the shell mesh, the near plane cut a hole in its
  side below the camera, showing the bare ground for a few kilometres of
  descent. The shell is drawn from inside within ten near-plane distances.
- **Cost:** about 0.2 ms with the planet in view from orbit, and 1.5 to 2 ms
  with the air filling the screen from inside it, as estimated for the
  direct version. The lookup tables are expected to bring this down.

Checkpoint 2 passed the user's live check. It adds
`AtmosphereTuning`: the sliders and presets set physical
values (pressure in bar, scale heights in km, gas and haze against Earth's),
converted to coefficients per metre for the shader. The tab's GPU readout is
the frame's GPU time and the transparent pass, compared with the switch on
and off, as the shell has no pass of its own.

Checkpoints, each checked live by the user and committed:

1. ✅ **Shell and shader:** the atmosphere material and `render/atmosphere.wgsl`
   on the selected planet, with fixed Earth values, and a debug switch
   (`set_debug atmospheres`) to compare with and without.
2. ✅ **Debug tab and CLI:** an `atmosphere` tab in the debug console with
   on/off, surface pressure, thickness, gas scattering strength, haze amount
   and colour, and steps per pixel; presets for Earth, Venus, Mars, Titan
   and a hydrogen giant; a GPU frame time readout; `ns atmosphere ...` to
   do the same.
3. ✅ **Polish:** the camera inside the atmosphere (sky colour, stars fading),
   precision from AU distances down to low orbit, two-star lighting and the
   rendering guide. The user runs a GPU benchmark for a baseline before the
   lookup tables. From inside thick air (Venus), the ground shows with a hard
   edge at the horizon instead of fading into the sky. Two fixes:
   - **Sunlight at the ground:** the surface shader lights the ground with
     undimmed starlight. For pixels that reach the ground, the atmosphere
     shader also dims what lies behind by the transmittance from that ground
     point to each star. Cost: one more light march, for ground pixels only.
   - **Steps:** each step adds its light with the energy-conserving form
     (Hillaire 2015), $S\,(1 - e^{-\sigma_t \Delta s})/\sigma_t$, rather than
     $S\,e^{-\sigma_t \Delta s/2}\,\Delta s$, which loses the light of
     optically thick steps. Steps are also spaced closer near the camera, so
     rays either side of the horizon (one ending at the ground, one crossing
     the whole air) settle to the same colour. No extra samples.
   - ✅ Both fixes passed the user's live check. A faint seam remained, as
     ground rays were spaced over a far shorter length than the rays just
     past the horizon; rays grazing the ground are now spaced as if they
     crossed the whole air and stop at the ground. 16 view steps hide the
     horizon in thick air; 24 is where the look stops changing.
   - ✅ Sky colour, stars fading from inside, two-star lighting and
     precision from AU distances to the ground passed the user's live check.
     The rendering guide's atmospheres section covers the steps, the ground
     dimming and the limits.
   - ✅ **GPU baseline** (atmosphere test, commit `4081ec6`, planet
     `gf008000017.2.23/5`, 1693 x 1177, MSAA on, 8 light steps). What the
     air adds to the GPU total, in ms, at 16 and 24 view steps:

     | View | Earth 16 | Earth 24 | Venus 16 | Venus 24 |
     | --- | --- | --- | --- | --- |
     | orbit | 0.43 | 0.74 | 0.43 | 0.76 |
     | high | 1.07 | 1.70 | 1.16 | 1.82 |
     | low | 0.94 | 1.34 | 0.82 | 1.34 |

     With the air off the GPU total is about 0.8 ms; frames held at about
     6.1 ms (the display's refresh) throughout.

Known prototype limits: no multiple scattering, so thick atmospheres look too
dark and Venus's surface is black; the ground is dimmed by its sunlight
(checkpoint 3), but in grey, so the terminator is not reddened; grey
transmittance over the ground (both fixed with the lookup tables).

## Stage 2 · Lookup tables and many planets

Agreed with the user on 2026-10-23. **End goal:** optimised, finished
atmosphere rendering on any number of planets, so that all stage 3 has to do
is supply each planet's air from the simulation. Nothing in this stage
generates atmospheres: the selected planet keeps the hand-set air until
checkpoint 5 opens the renderer to every planet.

Baseline to beat: the atmosphere test in checkpoint 3 of stage 1 (the air
adds 0.4 ms from orbit and 0.8 to 1.8 ms inside it). Rerun the atmosphere
test on `gf008000017.2.23/5` after checkpoints 1, 4 and 5 and record the
table here.

Approach (Hillaire 2020, "A Scalable and Production Ready Sky and Atmosphere
Rendering Technique"): the cost today is the march towards each star, 8
samples at every view step. Tables computed once per air replace it.

- **Built on the CPU**, in a background task when the air changes, and
  uploaded as float textures (`Image` assets) bound to the atmosphere
  material. No render-graph nodes or compute passes; move to GPU compute
  only if generating many planets' tables stalls. A slider drag rebuilds at
  most a few times a second.
- **Keyed by the air**, not the planet: in units of the planet's radius,
  so identical air on two planets shares tables.
- The direct shader stays as a reference toggle (`set_debug
  atmosphere_tables`, and the atmosphere tab), to compare look and cost.

Checkpoints, each checked live by the user and committed:

1. **Transmittance table.** 256 x 64, by height and the cosine of the
   zenith angle (Bruneton's parametrisation, with the ground and top in
   planet radii), RGB optical depth or transmittance in `Rgba16Float` or
   `Rgba32Float`. Replaces `towards_star` in the view march and the ground
   dimming. Expected 3 to 5 times cheaper per pixel. Check: the look
   matches the direct shader with the toggle; rerun the atmosphere test.
2. **Multiple scattering, and light on the ground under thick air.**
   - A 32 x 32 multiple scattering table (Hillaire's isotropic
     approximation, all orders summed as a geometric series), read at each
     view step and added to the single scattering.
   - **Venus's surface:** today it is black, as the cloud (optical depth
     about 30) leaves no direct light low down. Real Venus gets 2 to 3% of
     sunlight at the surface after many scatterings, a dim orange overcast
     without sharp shadows. Venus's preset gets nearly non-absorbing cloud
     (about 0.1% lost per scattering, against Earth haze's 10%).
   - **Ground lit by the sky:** for ground pixels, add the sky's irradiance
     at that point (from the same tables) times the ground's albedo, as the
     surface shader lights the ground with direct starlight only. The
     albedo comes from the planet's class tint, passed to the material.
   - Check: Venus's surface a dim even overcast; Earth's twilight and Mars's
     and Titan's skies brighter; nothing else darker.
3. **Colour-dependent dimming** of what lies behind, through dual-source
   blending (wgpu's `DUAL_SOURCE_BLENDING`, supported on the user's RTX
   3080): the fragment writes the scattered light and a per-channel
   transmittance, blended as `src0 + dst * src1`. The ground dimming by
   its sunlight becomes per channel too, so the terminator reddens and red
   light stops leaking through thick air. Falls back to grey where the
   feature is missing. Check: sunsets and the terminator reddened.
4. **Cost inside the air, if still needed.** After 1 to 3, if the high and
   low views still cost well above the orbit view: a per-frame sky-view
   table (about 192 x 108, by azimuth and elevation round the camera) for
   sky pixels, and fewer view steps for ground pixels. Otherwise dropped.
   Settle the default view steps (16 now; 24 is where the look stopped
   changing in stage 1) with the user. Rerun the atmosphere test.
5. **Every planet with air.** The renderer takes an atmosphere per planet,
   not one tuning for the selected planet:
   - A per-planet source of `AirSettings` (a resource keyed by planet ID),
     filled for now by the sliders for the selected planet, so stage 3
     only has to fill it from the simulation. `AtmosphereTuning` becomes the
     debug override of that source.
   - Tables cached by air, built in the background, with a limit on
     memory; a planet whose tables are not ready yet draws without air
     until they are.
   - Shells for every planet drawn as a disc in the visited system, sorted
     correctly with each other (one planet's air in front of another's
     disc) and with moons later.
   - Planets drawn as points stay without a shell; check the hand-over
     from point to disc does not pop.
   - Check: several planets with different presets in one system; rerun
     the atmosphere test and a view with several shells on screen.
6. **Docs:** the rendering guide's atmospheres section rewritten for the
   tables (no longer "prototype"), benchmarks updated, the documentation
   index's limits updated, and this plan's stage 3 written in detail with
   the user.

## Where to pick up

Stage 1 is done (branch `atmospheres`). Next is stage 2, the lookup tables.

1. ✅ **GPU baseline** recorded in checkpoint 3. The lookup tables rerun
   the atmosphere test on `gf008000017.2.23/5` for the comparison.
2. **Stage 2:** start at its checkpoint 1, the transmittance table.
3. **Default view steps:** 16 now; settled in stage 2's checkpoint 4.
4. **Stage 3 · simulation:** to be planned in detail with the user. Set
   so far: the scale height comes from $H = kT/\bar m g$ once temperature
   and composition exist, with the slider kept as a debug override; the air's
   top (12.5 scale heights) gives the extent that flight-sim standoffs wait
   for; it fills the per-planet air source from stage 2's checkpoint 5.

Working notes for a fresh session:

- **Code:** `render/atmosphere.rs` (settings, presets, tuning, shell
  material and its sync), `render/atmosphere.wgsl` (the march),
  `devtools/tabs/atmosphere.rs` (the tab), `devtools/atmosphere_test.rs`
  (the benchmark), `scene/draw.rs` (`Canvas.air`), `ui/kit/slider.rs`.
- **Live checks** are the user's. Test builds open on the secondary monitor
  (workspace 8) without focus, with the developer API on its own port, and
  are driven with `./scripts/ns --port <port>`: `cmd "fly_to_planet <id>"`,
  `atmosphere <preset>`, `cmd "set_paused true"`. WGSL compiles only at
  run time: after shader changes, start a build and check its log for
  errors. The user runs the atmosphere test; read
  `data/logs/atmosphere-test.jsonl`.
- **Test planets:** `g1e01000002.2.162/0`, and the baseline's
  `gf008000017.2.23/5`.
