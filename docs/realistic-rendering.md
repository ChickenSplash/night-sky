# Realistic rendering

Design notes for body lighting, manual exposure, compression and HDR sprites.
Paths starting with `crates/` are repository-relative; other client paths are
relative to `crates/client/src/`. Shaders live beside
their modules in `render/` or `stars/`. See [agent rules](../AGENTS.md) and
[galaxy model](galaxy-model.md) for streaming, point rendering and diffuse glow.

## Pipeline and light inputs

- On by default (`Navigation::realistic`); the wireframe bodies must stay
  unchanged when it is off (galactic stars appear in both modes). The smoke
  test turns it off for its wireframe captures.
  `exposure::facts` (debug window, `render` tab) shows only manual exposure
  in stops and compression as a percentage, with expandable explanations.
  No physical/assisted checkbox, metering, target key or adaptation remains.
  Photometry lives in `crates/simulation/src/light.rs`;
  light inputs and cached photometry in `render/lights.rs`, per-body exposure in
  `render/resolve.rs`, the pools and the HDR camera in `render/bodies.rs`,
  with the plugin and materials in `render/mod.rs` and the shaders in
  `render/body.wgsl` and `render/point.wgsl`.
- Every star of a multiple system lights its planets: `PlanetLight::light`
  holds up to `MAX_LIGHTS` (4) stars' direction, flux and temperature,
  brightest first, and `body.wgsl` adds each in its own colour, so a
  binary's planet has a fainter second day side towards the far star. The
  brightest sets the exposure key; each lit side gets a blurred limb (streaks
  from the brightest only), and a point planet sums every star's light by
  its own phase. The table shows only the primary, one row of planets per
  host (`TableLayout`), lit from the primary's side.
- Bodies are plain spheres under the wireframes. `scene/draw.rs` gathers `Lights`
  while drawing gizmos; `render::bodies::sync_bodies`/`sync_points` place pooled mesh
  entities in the camera's cell and write `GlobalTransform` directly, because
  they run after propagation.

## HDR, point spread and sprites

- Body colours reach the GPU pre-exposed on the CPU, in f32 uniforms (only an
  arithmetic safety limit, `MAX_VALUE` 1e30). Never send raw radiance: star
  surfaces reach 1e10 W m⁻² sr⁻¹. Every realistic shader that writes the HDR
  target (bodies, sprites, galactic points on both paths, glow composite)
  stores light above `KNEE` (4) logarithmically (`hdr.wgsl` `encode`, slope
  one at the knee, `GLARE` 2). The display clips at white either way, so this
  only shapes bloom: glare grows with stops over white, not raw light. A star
  disc 2^30 over white stores about 43, so its glow stays compact instead of
  swallowing the inner orbits; galactic stars a few stops over keep most of
  their bloom. There is no decoding and no half-float cap: never reinstate one.
- One point spread for everything: `render::PSF_SIGMA_PX` (0.55 physical px)
  is the galactic stars' Gaussian. A body point sprite uses it widened by its
  disc (variance plus a quarter of the squared disc radius), and every disc
  gets a rim sprite (`Frame::rims`, `Point::rim`) with its limb blurred by the
  same spread (erfc; a planet's only on its lit side), so a point and the disc
  replacing it are white over the same radius (tested). This fixed overexposed
  stars shrinking suddenly where points became discs, 100 to 200 AU from a
  giant.
- Eye-like streaks (`point.wgsl` `streaks`, `render::resolve::streaks`) mark light too
  bright for the camera: many fine rays of uneven length and strength, fixed
  to the screen, from points and lit limbs more than `STREAK_START` (3) stops
  over white, longer per stop up to `STREAK_MAX_PX`, whiter than the source.
  Not diffraction spikes. `Lights::streaks` fades them in from zero where the
  camera arrives (where galactic points become bodies) to full at half that
  distance, so they never pop. Galactic points have none.
- Coronas follow Baumbach's K and F profile (in millionths of the disc,
  `CORONA_SHARE`): helmet streamers near the star's equator (from its spin
  axis, narrowing outwards; a ring when seen pole-on), polar plumes and faint
  rays, all seeded per star and turning with it. `CORONA_*_MAX` bound the
  structure for sprite sizing; keep them in step with the shader.
- Sprites size themselves until their light falls below `SPRITE_FLOOR`
  (1/1024), capped at `SPRITE_MAX_PIXELS`, so the fade-out at the sprite edge
  is never visible. Close to a star at galaxy exposure and 0% compression its
  corona is far over white and fills the view; that is the camera model, not
  a bug. Compression balances it with its star.
- Star colours are blackbody spectra white-balanced to the Sun. Red dwarfs
  look orange; there is no assisted saturation. Brightness is visual, not bolometric.

## Exposure and compression

- One manual linear exposure applies to bodies, galactic points and glow.
  No subject metering, brightness floors, artificial night-side ambient,
  adaptation or display curve. Dynamic Range Compression is per-body
  balance (`render::resolve::balance`): each star (with its corona) and
  planet is moved from the shared exposure towards its own, by that share of
  the stops between them. A body's key is how strongly it is lit, never phase
  or screen coverage: a planet's sub-stellar radiance (shown at `PLANET_KEY`,
  0.8) and a star's surface (`STAR_KEY`, 2.0). Points use their light spread
  over a point-sized disc, which matches the disc at the switch. Bodies are
  only dimmed, never brightened beyond physical. 0% is one physical
  exposure; 100% exposes every body for itself, so they look equally bright.
  Shading within a body is untouched, so terminators stay soft. Galactic
  points and glow never balance: Exposure alone sets the galaxy. A former
  scene-wide highlight curve (`af68625`) could not do this: squeezing ~24
  stops into the display washed out and kept terminators hard. Do not bring
  one back. `exposure.wgsl` only clips at white and applies Bevy's deband
  dither (Bevy's tone-mapping pass is disabled).
  The visited star balances only while it is a body; leaving `body_range`
  it becomes an unbalanced galactic point, a jump at high compression.
  `set_exposure <stops>` and `set_compression <0..1>` use the same path as
  the HUD. Future auto-exposure should supply stops, separately from rendering.
  `node scripts/test-exposure.mjs` checks WebGL shaders, that compression
  never changes the galaxy, planet shading at 100% and immediate HUD text drags
  headlessly, after a web build with a server running (`SPACE_URL` for
  another port; it waits for streaming to finish before comparing).

## Bodies and cameras

- Bodies below `POINT_PIXELS` on screen are camera-facing point sprites, added
  over the frame (`AlphaMode::Add` needs zero alpha from the shader). A point's
  spread integrates to its whole light, exposed as if spread over a point-sized
  disc (`Exposure::point`), so it matches the sphere it replaces, and is
  balanced like it. Point planets dim with phase (`render::phase`, in `render/lights.rs`).
- Close star discs have per-channel limb darkening and procedural granulation
  in `body.wgsl`, fixed to the star's spin. Granule size follows the pressure
  scale height (`render::granules`, in `render/lights.rs`); patterns fade out before their cells reach
  a pixel, and hot stars have none. Manual exposure never changes with apparent
  disc size. Rims (blurred limbs, coronas, streaks) are additive sprites
  (`Frame::rims`, `point.wgsl`) and come first in the sprite pool so they
  keep their entity.
- Cameras sharing the window must agree on `Hdr`: the egui overlay camera switches
  with the 3D camera (without tone mapping), or it presents a stale frame.
- The 3D camera carries `NoIndirectDrawing`: GPU-driven culling drops every mesh
  with its 1e22 far plane. WebGL 2 never draws indirectly anyway.
