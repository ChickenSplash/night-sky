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
2. **Lookup tables:** transmittance and multiple scattering precomputed per
   planet (Bruneton 2008, Hillaire 2020). The direct shader stays as a
   reference toggle, to compare the look and frame time.
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

Checkpoints, each checked live by the user and committed:

1. ✅ **Shell and shader:** the atmosphere material and `render/atmosphere.wgsl`
   on the selected planet, with fixed Earth values, and a debug switch
   (`set_debug atmospheres`) to compare with and without.
2. **Debug tab and CLI:** an `atmosphere` tab in the debug console with
   on/off, surface pressure, thickness, gas scattering strength, haze amount
   and colour, and steps per pixel; presets for Earth, Venus, Mars, Titan
   and a hydrogen giant; a GPU frame time readout; `ns atmosphere ...` to
   do the same.
3. **Polish:** the camera inside the atmosphere (sky colour, stars fading),
   precision from AU distances down to low orbit, two-star lighting and the
   rendering guide. The user runs a GPU benchmark for a baseline before the
   lookup tables.

Known prototype limits: no multiple scattering, so thick atmospheres look too
dark; the ground is lit by unattenuated starlight, so the terminator is not
reddened; grey transmittance over the ground.
