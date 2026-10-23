# Realistic rendering

How body lighting, manual exposure, compression and HDR sprites work.
Paths starting with `crates/` are repository-relative; other client paths are
relative to `crates/client/src/`. Shaders live beside
their modules in `render/` or `stars/`. See [star streaming](streaming.md) and
[galactic rendering](galactic-rendering.md) for point rendering and diffuse glow.

## Physical light model

The conceptual path is:

```text
stellar luminosity and temperature -> emitted colour and visual power
                                  -> illumination at each planet
                                  -> reflected light and visible phase
                                  -> shared exposure and optional body compression
                                  -> disc/point spread, glare and display
```

Generated planets are illuminated spheres, not light sources. Stars contribute
their own light; every stellar member of a visited multiple system can illuminate
its planets. Changing exposure alters the display, not the generated luminosity,
planet properties or orbits.

### Luminosity, flux and radiance

These describe different things:

- **Luminosity:** total power emitted by a source. A star's generated luminosity
  is in solar units, with one solar luminosity equal to $3.828\times10^{26}$ W.
- **Flux:** power reaching a unit area at some distance, in W/m².
- **Radiance:** directional surface brightness, in W/(m²·sr), used for surfaces
  and lighting before camera exposure. Here it uses the model's visual weighting,
  rather than every emitted wavelength equally.

For visual luminosity $L_v$ and distance $d$ in metres:

$$
F(d) = \frac{L_v}{4\pi d^2}
$$

Double the distance and illumination is quartered. A one-solar-visual-luminosity
source gives about 1,361 W/m² at 1 AU. A star's surface radiance is modelled as
$F(R_\star)/\pi$. Unlike a point's received flux, a resolved surface's radiance
does not simply fall as observer distance squared: its apparent area shrinks
instead. This distinction lets discs and their distant point replacements carry
consistent total light.

### Colour and visual versus total power

Generated **bolometric luminosity** includes all wavelengths. Realistic rendering
weights a blackbody spectrum by the CIE 1931 observer over the visible range,
then converts it to linear RGB and white-balances against a 5,772 K Sun.
Thus the Sun is neutral white, cool red dwarfs appear orange, and hot stars
appear blue-white, rather than saturated category colours.

Visual efficiency is the visible-weighted share relative to the Sun:
$L_v/L_\odot=(L_{\mathrm{bol}}/L_\odot)\,\eta(T)$, with $\eta(5772\,\mathrm{K})=1$.
A cool star emits much of its power in infrared and looks dimmer than its
bolometric luminosity alone suggests. The visual units are normalised to the
Sun; they are not an unweighted visible-band watt integral or a lumen scale.
Wireframe mode instead uses an intentionally stretched spectral palette.

The streaming magnitude selection still uses bolometric luminosity with a solar
magnitude reference, while realistic displayed light uses visual efficiency.
Consequently the visibility cutoff is not a full temperature-dependent visual
survey model. See [galactic points](galactic-rendering.md#point-light-and-central-black-hole).

### Planet reflectance and phase

The surface model is **Lambertian**: diffuse reflection, bright when lit head-on
and fading as the surface normal turns away from the light. For a representative
reflectance coefficient $A$ and incoming flux $F$, face-on radiance is:

$$
I = \frac{AF}{\pi}
$$

A surface tilted by angle $\theta$ to the incoming light contributes a factor
$\max(0,\cos\theta)$. Multiple stars add their contributions with their own
directions and colours. There is no artificial night-side ambient illumination.
Planet classes provide fixed representative coefficients and tints: rocky worlds
use 0.30, dwarfs 0.15, sub-Neptunes 0.35, ice giants 0.45 and gas giants 0.50.
These are appearance prescriptions, not calculated atmospheres, clouds, terrain
or climate. The model does not resolve detailed scattering or spectral reflectance.

When a planet is too small to resolve, its point light includes the illuminated
phase. Let $\alpha$ be the angle at the planet between the star and observer:
0 means full, $\pi$ means new. The Lambert phase curve, normalised to full, is:

$$
g(\alpha)=\frac{\sin\alpha+(\pi-\alpha)\cos\alpha}{\pi}
$$

The implementation uses $(2/3)g(\alpha)$ for the disc's mean radiance relative
to its face-on sub-stellar point. A full sphere therefore averages two-thirds
of that peak, a half-lit disc ($\alpha=\pi/2$) has about 32% of its full-phase light,
and a new planet contributes none from that star. The factor avoids confusing
the brightest point on a sphere with its average brightness.

### Atmospheres (prototype)

A planet drawn as a disc may have an atmosphere: a shell mesh slightly larger
than the air's top, whose pixels march single scattering along the line of
sight (`render/atmosphere.rs`, `atmosphere.wgsl`). The shader finds the ground
and the top analytically, in planet radii and measured from the ray's closest
approach to the centre, so the limb holds in f32 at real distances; the mesh
only chooses which pixels run. At each of 16 view steps (by default), light
from every star is attenuated on its way in (none where the planet is in the
way) and scattered by gas (Rayleigh phase) and haze (Cornette-Shanks phase). Densities fall exponentially with height; an
absorbing layer, such as ozone, falls linearly either side of its peak. For
extinction coefficient $\sigma_t$, scattering $\sigma_s$, phase $p(\mu)$ and
star flux $F$:

$$
L = \sum_\star \int F\,T_\star(s)\,T_\mathrm{eye}(s)\,\sigma_s(s)\,p(\mu)\,ds
$$

$F$ is exposed with the planet's gain and balance, so $L$ is on the surface's
$AF/\pi$ scale.

**Steps.** Each step adds its light in the energy-conserving form (Hillaire
2015), integrating the dimming across the step rather than taking it at the
midpoint:

$$
\Delta L = T_\mathrm{eye}\,F\,T_\star\,\sigma_s\,p(\mu)\,
\frac{1 - e^{-\sigma_t \Delta s}}{\sigma_t}
$$

so an optically thick step (Venus's lower air) keeps its light instead of
losing it. Step lengths grow with the square of the step index when the
camera is at the ground, easing to even spacing at the top of the air, so
the air close to the camera, which sets the colour in thick air, is sampled
finely. A ray that reaches the ground is spaced over its whole length to the
top of the air as if the planet were not there, and stops at the ground,
when it meets the ground at a grazing angle; the steeper the ray, the more its
spacing shrinks back to its own length. Rays either side of the horizon (or of the
ground's limb seen from above) then sample the same points and meet without
a seam. In thick air 16 steps hide the horizon; the look stops changing at
about 24. The price is that such a ray keeps only the steps that fall before
the ground: from 200 km above an Earth-sized planet, the ground near the
horizon got 4 to 9 of 24, and banded. So this spacing fades out as the camera
climbs, from full at the ground to none at the top of the air: from above,
where nearly all the ground in view is grazed, every ray spreads its steps
over its own path through the air. `set_debug atmosphere_fit_steps false`
keeps the old spacing at any height, to compare.

**Jitter.** Each step takes its sample at a point within the step that
differs from pixel to pixel (interleaved gradient noise, Jimenez 2014), not
at its midpoint. With every pixel sampling the same depths, sharp changes
along the ray, chiefly the edge of the planet's shadow at twilight, showed as
coloured bands that only many steps hid; jittered, they blur into a fine,
static grain. `set_debug atmosphere_jitter false` samples at the midpoints
again, to compare.

**Transmittance table.** The light lost on the way in from each star is
read from a table rather than marched (`render/atmosphere_tables.rs`): the
optical depth from any height, in any direction, to the top of the air,
256 x 64 texels in Bruneton's parametrisation (by the distance to the top
of the air between its shortest and the horizon's, and by height as
$\sqrt{r^2 - 1}$), so the horizon, where the depth changes fastest, gets the
most texels. It holds RGB optical depth rather than transmittance, as depth
interpolates smoothly between texels, in `Rgba16Float`. Rays towards the
ground read the horizon's value; the planet's shadow stays the analytic test.
The table depends only on the air in planet radii, so it is built on the CPU
in a background task (about 0.1 s, 512 steps per texel) when the air
changes and cached by air; until it is ready, and with
`set_debug atmosphere_tables false`, the shader marches towards each star
instead (the light steps). A test compares the table with a 20,000-step
march: within 1% of the light that gets through, for every preset, from the
ground to the top.

**Output.** Output is premultiplied: $L$ is added and whatever lies behind is
dimmed by the line of sight's transmittance, averaged to grey. The surface
shader lights the ground with undimmed starlight, so where the ray reaches
the ground the shader also dims it by the transmittance from that ground
point to each star, weighted by each star's share of its light (flux times
the cosine of its angle from the zenith); this reddens nothing yet, as the
dimming is grey, but darkens the ground under thick air to match the sky
beside it. From inside the air the sky is this same scattered light, and the
stars and galactic glow behind it fade by its transmittance and under its
brightness; there is no separate sky pass.

**Draw order.** What lies behind must draw first: the galactic glow and star
points sort as the background (`BACKGROUND_DISTANCE`), and the shell sorts at
its near side, after its planet's limb sprites. Seen from inside, or close
enough above that the near plane would cut its near side, the far side of
the shell is drawn over everything instead.

**Settings.** For now only the selected planet has air, set by hand
(`AtmosphereTuning`) from the debug window's `atmosphere` tab, `set_atmosphere`
or `ns atmosphere`; `set_debug atmospheres false` hides it. Earth's preset uses
Hillaire's (2020) coefficients. The sliders are physical: gas scattering is
Earth's air per bar scaled by pressure and a gas factor (the gas and its
number density per bar), haze is Earth's continental haze times an amount,
coloured per channel, and the top sits 12.5 scale heights up. The Venus, Mars,
Titan and hydrogen giant presets use measured pressures and scale heights,
with haze tuned by eye.

**Limits.** There is no multiple scattering, so thick air is too dark and
Venus's surface is nearly black; transmittance is grey, so colour does not
shift through thick air; points have no atmosphere. The lookup-table stage
adds multiple scattering and per-channel dimming. See
[the plan](../plans/atmospheres.md).

### Physical model versus presentation

Luminosities, reflectance and orbit geometry supply the light; exposure,
compression, bloom and point spread are camera/display choices. Per-body
compression is a readability aid, not extra illumination. The procedural
granules/corona are approximate surface/appearance models, not stellar-fluid
calculations. Table view deliberately uses comparison positions and primary-side
lighting, so it should not be interpreted as a physical snapshot of the system.

`crates/simulation/src/light.rs` implements the blackbody/photometry relations
and representative surfaces. CIE observer functions use the analytic fits of
Wyman, Sloan & Shirley (2013); the illumination and phase relations are standard
inverse-square and Lambertian approximations. These models do not include dust
extinction, relativistic light propagation or a physically evolving atmosphere.

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
  with its 1e30 far plane. The far
  plane lies past the galaxy even at the closest zoom's render scale.

## Precision, scale and wireframes

- `big_space` supplies a grid-cell floating origin. Render positions subtract
  the camera in double precision before conversion to GPU floats. Bevy's
  reverse-Z perspective depth and a distance-adjusted near plane support
  changing scale; this is not a logarithmic depth buffer.
- Divide render positions and radii by `CameraPose::render_scale()` (displayed
  camera about 1,000 units from target). Raw metres push clip-space values
  to about 1e13, making the gizmo line shader drop or fragment segments.
  The scale's floor is 20,000 km of range, or the zoom when closer
  (`scene::scale_floor`, down to 1 m). Shaders scale galactic offsets to
  parsecs before squaring them, as at small scales a squared distance in
  render units would overflow f32.
- Every gizmo line is cut to the part in front of the camera in f64 before
  Bevy sees it (`scene::draw::clip_strip`). The near plane is 0.01 units,
  and a star's rings or a planet's orbit can be 1e10 units away: the line
  shader's own f32 near-plane cut then rounds the cut end behind the camera,
  and its segment flies across the opposite side of the screen as a fan.
- Planets retain physical radii in every presentation mode. Wireframes use
  procedural latitude/longitude lines, not native polygon wireframe mode.
  Pick line density from on-screen radius: dense rings on small bodies merge
  into blobs.
- Wireframe bodies use gizmos; realistic bodies use pooled per-body mesh
  entities. Galactic stars use custom GPU batches. Further body batching
  remains future work.
