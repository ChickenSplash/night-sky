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
Generated planets use a continuous composition-based **appearance proxy**, not a
class lookup. With retained water/metal/H₂-He mass fractions `w`, `m`, `h`, set
`e=clamp(h/0.5,0,1)`. Reflectance is `(0.30+0.15w-0.10m)(1-e)+0.50e`.
Tint blends `[0.60+0.10m, 0.55+0.15w, 0.50+0.30w]` towards
`[0.80,0.70,0.55]` by `e`, then normalises to unit luminance. These tunable proxies
do not assert exposed water, mineral spectra or clouds. Atmospheric optical
coefficients still come from actual gas composition. Label threshold changes
cannot change reflected light. Only legacy records without physical composition
keep the old class coefficients; invalid new records use neutral rock reflectance.

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

### Flattened bodies

Stars and planets are drawn as the spheroids their spin makes them
([shape](../simulation/planetary-systems.md#shape-flattening-by-spin)): the
sphere mesh scaled to the equatorial radius across and the polar along the
spin axis, so normals and lighting follow the shape. The wireframe rings
squash the same way. A disc's rim sprite (`point.wgsl`) draws its blurred
limb and a star's corona on the body's exact outline (`resolve::Outline`):
the cone from the eye grazing the spheroid cuts the sprite's plane, which
faces the screen through the body's centre, in an ellipse. Off the view's
axis that ellipse sits off the centre, and close up it is wider than the
body. Pixels in that plane are measured at its own depth, so the blur stays
one point spread wide on screen. Once the outline stops being an ellipse in
front of the camera (very close, with the body reaching beside it), there is
no rim; the disc's own edge is all that shows. Sizes on screen, labels and the point switch use the
equatorial radius; a point's light ignores the shape. `set_debug oblateness
false` draws every body as the sphere of its mean radius, to compare.

### Planet ground

A planet's disc is drawn as its ground (`render/ground.rs`, `ground.wgsl`,
shaded by `body.wgsl`); stars keep the sphere mesh.

- **Cube-sphere quadtree.** The six faces of a cube bent onto the spheroid,
  each a quadtree of chunks of 32 × 32 cells. The map is equi-angular (equal
  steps in face coordinates are equal angles), so cells stay near one size.
  Every vertex is `space_simulation::system::surface::ground`: the spheroid plus
  `height` along its normal, zero until there is terrain, the same function
  collision will ask.
- **Split by screen error.** A chunk's error is how far its flat facets lie
  inside the ground, the sag of a cell's diagonal under the flattest
  curvature. A chunk splits while that error would span more than
  `SPLIT_PIXELS` (half a logical pixel) at its distance, so the drawn ground
  stays where the air's analytic ground is on any planet, at any zoom and
  field of view. The distance is to the chunk's bounding sphere, never less
  than the camera's height. Standing on an Earth-sized planet: level 12
  underfoot, about 80 chunks drawn. From far away the six roots do.
- **No popping, no cracks.** Each vertex carries the offset to its parent
  level's surface (edge midpoints, and cell centres on the diagonal the
  parent's cells use). The vertex shader blends towards it from 0.7 of the
  distance where the parent takes over, measured from that parent surface, so
  a chunk is exactly its parent's shape at the switch and an edge meets its
  coarser neighbour's. Skirts hanging under every edge cover what rounding
  leaves.
- **Culling and budgets.** Chunks wholly behind the horizon (tested against
  the inscribed sphere of polar radius) are neither drawn nor refined; Bevy
  culls those out of view from each chunk's box. Chunks build on worker
  threads, at most 32 at once and 8 started per frame, nearest first; a chunk
  stays drawn until all four children are ready and spawned (a frame after
  they finish), and a new planet keeps its sphere until its roots are. 3,000 built chunks are kept,
  the least recently used dropped first, and a planet out of sight for 600
  frames loses all of them.
- **Precision.** Chunks are built in f64 as f32 offsets from their own centre,
  placed each frame from the planet's f64 position relative to the camera, so
  a chunk beside the camera keeps millimetres where the planet's centre is a
  radius away.
- `set_debug ground false` draws the old sphere mesh instead, whose facets sit
  up to 0.06% of the radius inside the true surface; `set_debug ground_levels
  true` tints chunks by level. `/perf/ground` counts planets, chunks drawn,
  hidden, cached, building and built this frame, and the deepest level.

### Atmospheres

A planet drawn as a disc may have an atmosphere: a shell mesh slightly larger
than the air's top, whose pixels march single scattering along the line of
sight (`render/atmosphere.rs`, with `atmosphere/atmosphere.wgsl`). The shader
finds the ground
and the top analytically, in planet radii and measured from the ray's closest
approach to the centre, so the limb holds in f32 at real distances; the mesh
only chooses which pixels run. A flattened planet is first made round: the
camera, the ray and the star directions are stretched along the pole by
$a/c$, which makes the ground a sphere of the equatorial radius. Ground, top,
limb and the planet's shadow stay exact and the tables stay spherical; heights
in the air and path lengths are off by at most the flattening near the poles
(0.3% on Earth, 10% on Saturn), and phase angles use the true directions. At each of 16 view steps (by default), light
from every star is attenuated on its way in (none where the planet is in the
way) and scattered by gas (Rayleigh phase) and haze (Cornette-Shanks phase). Densities fall exponentially with height; an
absorbing layer in debug presets, such as ozone, falls linearly either side of
its peak. Generated gas absorption instead follows the exponential gas profile. For
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
finely. Every ray spreads its steps over its own path through the air.

**Stopping where opaque.** A ray ends where the air between the camera and
the point reaches optical depth 10 in its least dimmed channel (less than
1/20,000 of the light gets through), whether or not ground lies beyond. The
depth between two points on a ray is the difference of their depths to the
top of the air from the transmittance table, taken forwards along the ray,
or backwards for a ray that reaches the ground, as its backward line never
meets the ground; about ten halvings, one table read each, find the point
(a test checks it lands between optical depths 9 and 13 on Venus). What lies
behind is still dimmed by the whole ray's optical depth from the table. The
ground includes its own lost sunlight in its surface lighting; while that
table is building, or with surface lighting off, the shell dims the ground's
sunlight instead. Otherwise even 1/20,000 of an undimmed surface outshone
Venus's scattered light and showed the distant ground through the haze.
In thick air (Venus) rays either side of the horizon, or of the ground's
limb from above, then end alike in the haze and meet without a seam, the
ground is hidden where it should be, and all the steps go where light is
seen. Earth's air is that thick only along the limb. Without it, rays that
reach the ground and rays that pass it are stepped over very different
lengths and the edge between them shows in thick air. It needs the table, so
does nothing while the table builds or with it off;
`set_debug atmosphere_stop_opaque false` compares.

**Jitter.** Each step takes its sample at a point within the step that
differs from pixel to pixel (interleaved gradient noise, Jimenez 2014), not
at its midpoint. With every pixel sampling the same depths, sharp changes
along the ray, chiefly the edge of the planet's shadow at twilight, showed as
coloured bands that only many steps hid; jittered, they blur into a fine,
static grain. `set_debug atmosphere_jitter false` samples at the midpoints
again, to compare.

**Transmittance table.** The light lost on the way in from each star is
read from a table rather than marched (`render/atmosphere/tables.rs`): the
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

Transmittance and multiple-scattering tables share a least-recently-used
cache: at most 32 resident atmospheres (roughly 9 MiB of image data), with
at most 8 retained when unused, plus one build of each kind in flight.
The key includes every optical coefficient, profile, radius scaling and lower
boundary albedo. Visible planets request their own air, never another planet's
table. More than 32 simultaneous distinct atmospheres can cause rebuild churn.

**Multiple scattering.** Light scattered more than once is read from a
second table (`render/atmosphere/multiple.rs`), built after the
transmittance table in the background (a few seconds) and cached with it;
`set_debug atmosphere_multiple false` leaves it out. Per unit starlight, by
height and the star's angle from the zenith (32 x 32 texels, rows evenly in
$1 - e^{-h/2H}$, $H$ the air's tallest scale height), it holds the mean
radiance of light scattered at least once and its net upward flow, plus the
ground's radiance in one more row. Light past its first scattering is taken
as the same from every direction (Hillaire 2020), and the haze's forward
peak as no scattering at all: its Mie scattering times $1 - g$. Rather than
Hillaire's sum of one more order, every order is solved at once as one
linear system with the ground in it, so light diffusing down through thick
cloud is kept. Each view step adds the stars' light times the scattering
times that mean radiance, plus the haze's forward scattering of the flow,
$\sigma_M \frac{3g}{4\pi} F \cdot (-d)$: under thick cloud light flows
downwards and the haze sends it on down, so the sky from below is up to
twice as bright as the mean alone. For consistency, the starlight in single
scattering is then dimmed by the thinned air's transmittance table: light
scattered slightly forwards still counts as starlight. Measured against the
Monte Carlo reference (`render/atmosphere/reference.rs`, tests only) the
result is within about 15% for every preset, Earth within 5%; Mars's dust
is the worst (0.76 to 1.35), as light flowing sideways away from the star
is not kept.

**Light on the ground.** The surface shader reads the last row (row 32) of
the 32 x 33 multiple-scattering sky image, at the star's cosine from the
zenith. It holds reflected ground radiance per unit stellar flux, including
the surface's RGB albedo, thinned direct sunlight and diffuse skylight. Each
star's exposed flux multiplies its own lookup, with no second albedo, cosine
or $1/\pi$ factor. Thus thick air can light the ground diffusely and colour
it, rather than merely darkening undimmed sunlight. The ground chunks and
fallback sphere use the same shader and table. On spheroids the shader
recovers the rounded radial direction from the surface normal and stretches
the star direction along the pole, matching the shell's spherical tables
and their flattening approximation.

`set_debug atmosphere_ground false` restores the old ground lighting. The
switch is on by default, requires atmospheres and lookup tables, and is
independent of `atmosphere_multiple` (scattering in the sky). Until the
multiple-scattering table for the current air is ready, both surface paths
and the shell keep the old behaviour together. Switching planets, disabling
the atmosphere or changing its air cannot reuse another air's ground light.

**Output.** Output is premultiplied: $L$ is added and whatever lies behind is
dimmed by the line of sight's transmittance, averaged to grey. With ground
table lighting active, the shell does not dim the surface's sunlight again.
Otherwise it retains the old grey dimming by the transmittance from the
ground point to each star, weighted by flux times the cosine of its zenith
angle. From inside the air the sky is this same scattered light, and the
stars and galactic glow behind it fade by its transmittance and under its
brightness; there is no separate sky pass.

**Draw order.** What lies behind must draw first: the galactic glow and star
points sort as the background (`BACKGROUND_DISTANCE`), and the shell sorts at
its near side, after its planet's limb sprites. Seen from inside, or close
enough above that the near plane would cut its near side, the far side of
the shell is drawn over everything instead.

**Generated air.** Every resolved planet with a generated atmosphere receives
its own optical shell, regardless of selection. `atmosphere/generated.rs`
revalidates wire inputs, recomputes columns using the mean reference radius,
then adapts simulation optics to the shader. It scales physical coefficients
and heights by the drawn equatorial radius, preserving the existing spheroidal
approximation. Missing, airless or invalid data gets no shell. Unsupported
prescribed submicron aerosols currently fail optical conversion rather than
being silently given micron-particle optics. See the
[physical optics assumptions](../simulation/atmospheres.md#visible-optical-conversion).

Extended columns retain the constant-gravity estimate and inspector warning,
without a radius clamp. A one-bar envelope's lower opaque sphere is a simple
cloud/deep-envelope proxy at the reference radius, shaded with the existing
composition reflectance proxy and atmosphere tables, not a generated cloud deck or a solid
surface. It has no terrain chunks. This rendering choice changes neither
collision nor flight mechanics, and does not model descent below that radius.
Supported [condensed-boundary atmospheres](../simulation/planet-composition.md#supported-approximate-condensed-boundaries)
instead draw ground at the boundary, with gas density and scattering scaled by
the full boundary pressure, so a dense column is optically thicker rather than
only relabelled. No cloud deck is inferred from that pressure.

At the point/disc hand-over the shell's scattered light and opacity and the
surface's atmospheric table lighting fade from zero to full between one and
four times `POINT_PIXELS`. Optical coefficients and cache keys do not change
with this presentation fade. Distant point photometry is still airless, not an
integral of the atmospheric disc; this avoids an abrupt new shell, not a fully
photometrically matched atmosphere point model. Table readiness pop-in remains.

**Settings.** Generated air is the default. `AtmosphereTuning` is an optional
selected-planet override from the debug window's `atmosphere` tab,
`set_atmosphere` or `ns atmosphere`. A preset or optical slider edit enables it;
quality edits do not. The **Selected planet override** toggle or
`set_atmosphere enabled=false` restores generated air. The override follows
selection, affects no other planets, and never mutates physical inspector data.
`set_debug atmospheres false` hides all shells and atmospheric surface lighting.
Earth's debug preset uses
Hillaire's (2020) coefficients. The sliders are physical: gas scattering is
Earth's air per bar scaled by pressure and a gas factor (the gas and its
number density per bar), haze is Earth's continental haze times an amount,
coloured per channel, and the top sits 12.5 scale heights up. The Venus, Mars,
Titan and hydrogen giant presets use measured pressures and scale heights,
with haze tuned by eye.

**Limits.** View-path transmittance remains grey, so colour does not shift
along the line of sight through thick air, although incoming ground lighting
is now coloured. Points and the disc's point-spread rim sprites retain the
original airless photometry; points have no atmosphere. Ground tables assume
a smooth, uniform Lambertian surface, not slopes, terrain shadows or local
albedo variations. See
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
  with the plugin and materials in `render.rs` and the shaders in
  `render/body.wgsl` and `render/point.wgsl`.
- Every star of a multiple system lights its planets: `PlanetLight::light`
  holds up to `MAX_LIGHTS` (4) stars' direction, flux and temperature,
  brightest first, and `body.wgsl` adds each in its own colour, so a
  binary's planet has a fainter second day side towards the far star. The
  brightest sets the exposure key; each lit side gets a blurred limb (streaks
  from the brightest only), and a point planet sums every star's light by
  its own phase. The table shows only the primary, one row of planets per
  host (`TableLayout`), lit from the primary's side.
- Bodies are spheroids under the wireframes, flattened by their spin
  ([flattened bodies](#flattened-bodies)). `scene/draw.rs` gathers `Lights`
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
