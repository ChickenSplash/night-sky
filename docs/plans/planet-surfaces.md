# Planet surfaces · plan

A proposal, not a description of the current application.

Comes before the rest of [atmospheres](atmospheres.md) stage 2: lighting the
ground from the sky (atmospheres 4c) belongs in a ground shader this plan
replaces, and geology and water depend on the simulated air (atmospheres
stage 3). Flight-sim phase 11's clearances
([paused work](flight-sim.md#paused-until-atmospheres)) use this plan's shape
and heights as well as the air's extent.

## Goal

- Every body round under its own gravity has its real shape: flattened at the
  poles and widened at the equator by its spin. The air, the oceans and the
  ground follow that shape.
- The camera can fly down to the ground and land on it, and near the ground it
  moves with the turning surface.
- Later: realistic, feasible geological features and hydrospheres; on bodies
  with no surface (giants), sinking deeper into the air until it is black.

## Why the ground clips through the camera today

- **Nothing stops the camera:** free flight adds the movement to the eye
  (`scene/controls.rs`), with no height check.
- **The drawn ground is not at the radius:** a 192 x 96 UV sphere, whose flat
  facets sit up to about 0.06% of the radius inside the true sphere (a few km
  on an Earth-sized world). At a shallow angle a facet passes up through the
  camera over seconds. The atmosphere shader uses the exact sphere, so air
  and ground disagree near the surface.
- **The near plane follows the zoom, not the height:** 0.01 render units at a
  render scale from the zoom distance (`scene/placement.rs`,
  `CameraPose::render_scale`), up to about 200 m, so close ground is cut away.
- **The spin does not carry the camera:** the anchor carries the planet's
  orbit but not its rotation, so the surface slides beneath (about 465 m/s at
  Earth's equator) and a landed camera would not stay put.

## Order

1. **Shape** (checkpoint 1, done):
   oblateness in the simulation, bodies and air drawn as spheroids.
2. **Landing** (checkpoints 2 to 5): a smooth spheroid ground that matches the
   shape exactly, the surface frame, the near plane by height, and the floor.
3. **Light on the ground:** [atmospheres 4c](atmospheres.md#4c-light-on-the-ground),
   written into the new ground shader. Then atmospheres checkpoint 5 (every
   planet with air).
4. **Geology, water and climate,** planned with the user together with
   atmospheres stage 3: temperature and pressure decide whether there are
   oceans, ice or erosion.
5. **Giants:** no floor; sink through the air until it is black, down to a
   pressure where the descent ends.

Not planned yet: moons (they reuse all of this as another body), tidally
stretched three-axis shapes, rings, clouds.

## Rules

As for atmospheres: one change per checkpoint, behind a `set_debug` switch
where there is something to compare, checked live by the user before it is
committed. Simulation changes come with tests; new generation may freely
regenerate bodies (alpha).

## Checkpoint 1 · Shape (oblateness)

**Done (2026-10-24), checked live by the user.** The canonical
description is now in [planetary systems](../simulation/planetary-systems.md#shape-flattening-by-spin)
and [rendering](../client/rendering.md#flattened-bodies). What differs from the
proposal below:

- One limit for every body, `MAX_FLATTENING` 0.2 (a star at about 0.93 of
  break-up). 76 of 4,378 test planets are slowed to it (67 dwarf planets, 9 gas
  giants); 7 of 20,000 test stars (hot B stars). 265 planets are flatter than
  0.05, mostly dwarfs and gas giants.
- Stars come out in closed form: with the mean radius kept, the Roche
  flattening is $W = \omega^2R^3/2GM$ itself. Planets too:
  $f(1-f) = K\omega^2R^3/GM$.
- Rim sprites stretch along the pole's projection so the limb blur and the
  corona follow the ellipse.
- **Craft keep the mean radius for now.** Moving craft clearances and spawn
  heights to the equatorial radius tripped
  `a_predicted_path_is_anchored_where_its_planet_is_now`, which turned out
  fragile on its own: moving the spawn 10 m swings its drift from 211 m to
  4.3 km against a 1 km bound. Left for flight-sim phase 11, with that test
  looked at then.
- Live check: a giant such as `g1e01000002.2.162/5` (flattening 0.025, the
  hydrogen giant preset), `set_debug oblateness false` to compare; the
  inspector shows "Flattening". Look at the limb from the equator and the
  pole, the rim's blur on the polar limb, the air's limb and twilight band,
  and a fast hot star's corona.

A spinning body settles into an oblate spheroid: equatorial radius $a$, polar
radius $c$, flattening $f = (a - c)/a$. Every input is already generated: mass,
radius, spin period (`Spin`), class. Computed in the simulation crate,
deterministically, so client, collision and later gravity agree.

- **Spin parameter:** $m = \omega^2 a^3 / GM$, centrifugal over gravitational
  acceleration at the equator.
- **Planets, Darwin-Radau** (hydrostatic, centrally condensed), with $k$ the
  moment of inertia factor $C/Ma^2$:

$$
f = \frac{\tfrac{5}{2}m}{1 + \tfrac{25}{4}\left(1 - \tfrac{3}{2}k\right)^2}
$$

  Checked by hand: Earth ($k$ 0.331, $m$ 0.00345) gives 1/300 (real 1/298),
  Jupiter (0.254, 0.089) 0.066 (real 0.065), Saturn (0.21, 0.155) 0.099 (real
  0.098). $k$ by class: rocky 0.33, dwarf 0.35, sub-Neptune 0.26, ice giant
  0.23, gas giant 0.25 (a prescription, until interiors are modelled).
- **Stars, Roche model** (mass at the centre): the polar radius barely
  changes, the equator widens up to 1.5 times it at break-up. Fast hot stars
  are visibly flattened (Altair about 0.2); the Sun is not.
- **Radius stays the mean radius:** `radius_km` and `radius_solar` keep their
  meaning (same volume, so density is unchanged): $a = R(1-f)^{-1/3}$,
  $c = a(1-f)$. $m$ uses $a$, so solve by a few fixed-point steps.
- **Break-up limits:** some generated spins are too fast. A 4 hour icy dwarf
  is past where a spinning fluid body stays a spheroid, and a hot B star at
  half a day is near break-up. Generation clamps periods so flattening stays
  under a limit per kind (stars below about 0.9 of break-up speed, others
  below about $f$ 0.3), with a test over many systems.
- **Sea level is the spheroid:** a hydrostatic shape is a surface of equal
  gravity plus spin potential, so water at height 0 lies flat on it. All later
  heights are measured from the spheroid along its normal (as Earth's are from
  WGS84), and "up" is that normal.
- **Later, if wanted:** $J_2 \approx (2f - m)/3$ for craft gravity (orbits
  precess around flattened bodies), and gravity darkening on fast stars (hotter,
  brighter poles: von Zeipel, Espinosa Lara & Rieutord 2011).

Rendering:

- Bodies scaled to $(a, c, a)$ about the spin axis, with spheroid normals in
  `body.wgsl`.
- **The air in stretched space:** squash the ray along the spin axis by $a/c$
  so the spheroid becomes a sphere of radius $a$, then trace as today. Ground,
  top, limb and the planet's shadow stay exact, and the tables stay spherical.
  Layer heights are off by up to the flattening (0.3% on Earth, 10% on Saturn),
  accepted for now.
- Clearances, markers and picking use the equatorial radius.
- Switch `set_debug oblateness false` draws spheres to compare; the
  information panel shows flattening.
- Docs: [planetary systems](../simulation/planetary-systems.md),
  [stars](../simulation/stars.md), [rendering](../client/rendering.md).

## Checkpoint 2 · Ground that matches the shape

The UV sphere gives way, near the camera, to a cube-sphere of chunks refined by
distance (a quadtree per cube face), each chunk placed relative to the camera
so vertices keep metre precision at any distance. A point on a chunk is the
spheroid along its direction plus `height(body, direction)` along the normal:
one deterministic function in the simulation crate, 0 for now and the terrain
later, also used for collision. The distant disc may keep the single mesh.

Checks: no seams or cracks between chunk levels, no popping as they refine,
the atmosphere's ground and the drawn ground meet from orbit to low altitude.

## Checkpoint 3 · Surface frame

Below a set height (probably the air's top, or a few radii for airless
bodies), the camera's anchor carries the body's spin as well as its orbit, so
the ground holds still under it and controls move it relative to the surface.
Above it, the inertial frame as today. Hand-over without a jump in position or
view, and a readout of which frame is in use.

## Checkpoint 4 · Near plane by height

Render scale follows the camera's height above the ground (from the height
function) as well as the zoom, so the near plane shrinks to centimetres when
landed. Check depth precision on distant bodies and the starfield at that
scale.

## Checkpoint 5 · Landing

- A floor at eye height above the ground: descending stops there, sliding
  along the surface rather than sticking.
- Speed eases down as the ground gets close, so 10 km/s towards the ground
  arrives gently.
- Free camera only; the craft keeps its current rules.
- Check on a slow, a fast and a flattened planet, at a pole and the equator.

## Open questions

- Near the ground: ease the speed automatically, or keep the set speed and only
  stop at the floor? (Proposed: ease.)
- Eye height when landed: about 1.7 m standing, or a higher hover?
- Does the craft land too, now or with flight-sim's later "crashing into
  bodies"?
- Giants: at what pressure does the descent end?

## Working notes

- **Code today:** `render/bodies.rs` (pooled body meshes, the UV sphere),
  `render/body.wgsl`, `render/atmosphere.wgsl`, `scene/controls.rs` (free
  flight), `scene/placement.rs` (anchor carry, near plane, render scale),
  `scene/camera.rs`, `crates/simulation/src/system.rs` (`planet_spin`),
  `crates/simulation/src/star.rs` (star spins), `space_shared::Spin`.
- **Live checks are the user's;** test builds as in the atmospheres plan.
