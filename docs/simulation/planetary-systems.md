# Planetary system generation

Systems are generated as a whole from stable star IDs, with planets around
component stars or close pairs as their stability zones allow. A single star
typically has 7 to 14 planets; multiple systems can have more, fewer or none.
See [stars](stars.md) for companion stability zones, [orbits](orbits.md) for
motion, and [the information overlay](../shared/information-overlay.md) for
how optional tasks attach to planets.

## Bodies, composition and spacing

Generation uses the [inventory and structure model](planet-composition.md).
Mass and composition drive radius and inertia; descriptive labels are derived
afterwards. Atmospheric supply and condensates share the conserved material budget.

- Water acquisition rises smoothly across the formation snow line. Heavy-core
  growth, feeding mass and host mass control gas acquisition, without class draws.
  Radii range from sub-Pluto metal-rich bodies to roughly Jupiter-sized envelopes.
- Neighbours stay Hill-stable without crossing orbits; some pairs have
  resonant period ratios (2:1, 3:2, 4:3, 5:4, 5:3, 3:1).
- Planet mass is capped at 0.4% of the star's, or Hill spacing cannot converge.
- Spins have independent seeds and tilted axes. Close-in planets can be tidally
  locked; others draw spin-orbit states, slow spins or large tilts. Spin does
  not move an orbit.
- Every planet and star is flattened by its spin into an oblate spheroid
  ([shape](#shape-flattening-by-spin)).

## Generation, from the inside out

The model generates plausible final systems; it does not simulate a gas disc
accreting into planets. For each permitted host (a star or close pair):

1. Establish the inner boundary, snow line and companion-imposed outer limit.
2. Choose a target count of 7 to 14 planets, then propose orbits inside-out.
3. Draw heavy-material mass/fractions and gas acquisition at the proposed orbit.
   Scale all allocations together to respect the host-mass cap.
4. Draw eccentricity, inclination, orbital orientation and phase.
5. Solve the two neighbour-spacing inequalities using formation masses;
   optionally place it at an allowed resonant period ratio.
6. Stop if the orbit's farthest point exceeds the host's stable zone. Otherwise
    store its formation inventory and continue outward.
    Provisional structure is a validated fallback result, with a temporary
    compatibility view of the same facts rather than a legacy structure solve.
7. Once the entire hierarchy and final orbits exist, share all-star heating
   samples with the atmosphere model. Account for accessible secondary volatile
    escape, solve H/He survival and structure together, then assign independently
    seeded spin and descriptions. Partition retained gas and condensates.
    Retention trials now use fast fallback facts and independent forcing inputs;
    the final live geometry/atmosphere still uses a legacy compatibility projection
    while P3 consumer migration is in progress.
    The retained fallback candidate now has its own gas/condensate partition,
    pressure, temperature and updated inertia at fixed proxy geometry. It is kept
    on the body and exported separately as `Planet::initial_result`; its presence
    does not switch live geometry or attach the legacy atmosphere to that result.
8. Recheck stability using final retained masses. Escape only reduces masses,
   so the formation-mass spacing remains conservative.

The count is a starting target, not a guarantee: a companion may leave fewer
planets or none. Different hosts in one multiple system each contribute planets.
If an ordinary generated system extends beyond 1,000 AU, the model retries
with a compact, lower-mass population, rather than letting crowded systems
grow to interstellar scales.

### The formation snow line

The snow line is the approximate distance where water could freeze in the
planet-forming disc. Ice beyond it provides additional solid material, helping
cores grow large enough to retain substantial gas. For a main-sequence host,
the model uses:

$$
a_{\mathrm{snow}} = 2.7\sqrt{L/L_\odot}\;\mathrm{AU}
$$

A Sun-like host has a 2.7 AU snow line; one at 0.01 solar luminosities has a
0.27 AU line. Giants and white dwarfs retain progenitor-based formation scales;
see [evolved stars](stars.md#how-an-evolved-star-changes-its-planets).

The water fraction varies smoothly with proposed orbit divided by the snow line.
Additional feeding material near/beyond it favours heavier cores, while distant
feeding tapers away. Compact retries reduce feeding mass rather than switching
to rock/dwarf class templates. Composition is chosen before spacing adjustments;
moving an orbit outward never redraws its formation material. Formation and
current irradiation are deliberately different inputs.

### Mass and radius are linked

Radius is derived from the retained constituent inventory, using compressed
heavy-material fits, mixture interpolation and a bounded light-envelope/giant
surrogate. There is no independent radius scatter or class-based radius lookup.
Escaped H/He and secondary volatiles are tracked separately and excluded from gravitational mass.
See the [composition guide](planet-composition.md) for exact formation priors,
retention convergence bounds, reservoir partition, radius conventions and limitations.

### Hill spacing and resonances

The mutual Hill radius estimates the scale over which two neighbouring planets'
gravity competes with their host. For masses $m_1,m_2$, host mass $M_\star$,
and semi-major axes $a_1,a_2$:

$$
R_H = \left(\frac{m_1+m_2}{3M_\star}\right)^{1/3}\frac{a_1+a_2}{2}
$$

All masses must use the same units. The generator requires both
$a_2-a_1 \ge 10R_H$ and a gap of at least $2\sqrt{3}R_H$ between the inner
planet's apoapsis and outer planet's periapsis. Heavier planets therefore
need more space; eccentric orbits need clearance at their closest approach,
not just a difference between average orbital sizes.

A **mean-motion resonance** is a simple ratio of orbital periods. A 2:1 pair
has its outer planet complete one orbit for every two inner orbits. From
Kepler's law, that means a semi-major-axis ratio of $2^{2/3}$, about 1.59,
not 2. The generator selects resonances only when they meet its spacing
conditions and sets phases to keep conjunctions at the outer planet's apoapsis.
Some systems receive resonant chains.

This is imposed commensurability and phase alignment, not simulated capture,
migration or libration. The spacing conditions are conservative generation
checks, not a guarantee established by evolving an interacting N-body system.

### Rotation and tidal locking

Spin is distinct from orbital motion. Close-in planets receive synchronous
rotation: the assigned spin period equals the orbital period. On an aligned
circular orbit this keeps the same face toward the host; on an eccentric orbit,
uniform spin does not track the changing instantaneous orbital angular speed.
The locking-distance prescription depends on retained envelope fraction and mass
and scales with the cube root of host mass. Just beyond that region, eccentric
low-envelope worlds can receive 3:2 spin-orbit states; less eccentric ones can
spin slowly, sometimes retrograde. Farther-out worlds draw continuously blended
mass/envelope-dependent periods and tilts, including occasional large tilts.

These outcomes are assigned during generation, not obtained by integrating
tidal torques. System planes have isotropically distributed poles; planets
within a system generally have small inclinations, with a broader distribution
for bodies below 0.02 Earth masses. Obliquity is the spin-axis tilt, not orbital inclination.
Free spins are slowed where needed so no planet is flatter than
`MAX_FLATTENING` (below). Spins use per-body `{host}/{index}:spin:v2` seeds.

### Shape: flattening by spin

A body round under its own gravity and spinning settles into an oblate
spheroid, its equatorial radius $a$ longer than its polar radius $c$; the
flattening is $f = (a - c)/a$. Its surface is one of equal gravity plus spin
potential, so it is also sea level: water at height zero lies flat on it, and
later heights are measured from it along its normal: `space_simulation::system::surface`
gives the ground as the spheroid plus `height(planet, direction)`, zero until
there is terrain, for drawing and collision alike. `space_simulation::system::shape`
computes the spheroid from mass, radius, inertia factor and spin period (`planet_shape`,
`star_shape`), deterministically, for every crate that needs it.

- **Planets, Darwin-Radau.** With spin parameter $m = \omega^2 a^3 / GM$
  (centrifugal over gravitational acceleration at the equator) and moment of
  inertia factor $k = C/Ma^2$ (0.4 for uniform density, less the more mass
  gathers at the centre):

$$
f = \frac{\tfrac{5}{2}m}{1 + \tfrac{25}{4}\left(1 - \tfrac{3}{2}k\right)^2}
$$

  Generated $k$ uses an approximate composition/envelope central-concentration
  prescription, [documented here](planet-composition.md#inertia-and-spin).
  It is carried on wire records so clients and spin limits agree. Only older
  legacy records without composition or a valid physical factor use an isolated
  class-based fallback; malformed new records never use their class as physics.
- **Stars, Roche model** (mass at the centre). The equator sits where
  $GM/R_e + \omega^2 R_e^2/2 = GM/R_p$, up to $1.5R_p$ at break-up. With the
  mean radius kept, $R_e/R_p = 1/(1 - W)$ for $W = \omega^2 R^3 / 2GM$, so
  $f = W$. The Sun comes out at about $10^{-5}$, Altair at 0.18 (0.20).
- **Mean radius kept.** `radius_km` and `radius_solar` stay the radius of the
  sphere of equal volume, so densities are unchanged: $a = R(1-f)^{-1/3}$,
  $c = a(1-f)$. For planets $f(1 - f) = K\omega^2R^3/GM$ then solves in
  closed form, $K$ being Darwin-Radau's factor.
- **Limit.** `MAX_FLATTENING` is 0.2: a star that flat spins at about 0.93
  of its break-up speed, and a fluid planet much flatter stops being a
  spheroid (Haumea is a three-axis ellipsoid). Generation slows faster spins
  to it (`fastest_planet_hours`, `fastest_star_hours`); a tidally locked
  planet keeps its orbit's period, and its shape is capped instead.

Not modelled: three-axis shapes of very fast or tidally stretched bodies,
non-hydrostatic shape (Mars's is partly held up by Tharsis), and the bulge's
pull on orbits ($J_2$).

## Stable identities

- Generation is keyed by star ID. `space_simulation::star(id)`,
  `planetary_system(id)`, `planet_count(id)` and `plane_tilt(id)` take any
  star ID. Changing a key moves every body it seeds.
- A `Planet` has a stable ID made from its host's ID and orbit index:
  `key/n` around a single star, `key#B/n` around one member of a multiple,
  `key#AB/n` around a close pair (`Planet::host`).
- Body properties and orbital elements are determined by generation, not by
  optional information attached to a body. Overlay-specific identity and slot
  rules belong in [the information-overlay guide](../shared/information-overlay.md).
- Each system has its own orbital-plane tilt (`plane_tilt`), including pages
  belonging to one constellation.

## Atmospheric foundation

The [composition calibration census](planet-composition.md#calibration-census-and-regression-scope)
records reproducible reference structures, population distributions and known
artefacts, including the imposed lower-mass pile-up and approximate giant turnover.
It is not an observational occurrence fit or proof of resolved interior physics.

After the full stellar hierarchy and stable planetary orbits are known,
the [physical atmosphere model](atmospheres.md) assigns each planet an approximate
environment: all-star mean heating, atmospheric retention, gas mixtures,
reference pressure and isothermal column estimates. Separate seeds preserve
orbital generation. Atmospheric supply and cold condensates use the same conserved
material budget as bulk structure, never a class prior. The inspector
shows generated air, and per-planet optical conversion supplies rendering.
Selected-planet debug air remains an optional override.

## Presentation and limitations

Planets retain physical radii in every presentation mode. Table layout is
presentation-only; leaving it restores current simulated positions. Constellation
view never moves stars. Rendering and camera flights are explained in the
[client guides](../README.md#client).

There is no N-body simulation, moons or collisions. Orbital motion is analytical,
not the result of integrating forces between generated bodies.

## Scientific basis and approximations

Composition-based radii and retention are documented with their sources in the
[composition guide](planet-composition.md). Host-mass acquisition loosely preserves
the trend in Johnson et al. (2010); neighbour-spacing choices are inspired by
Chambers et al. (1996) and Pu & Wu (2015). Tidal-spin prescriptions draw on
Solar System examples and the trends discussed by Gladman et al. (1996).
These are simplified fits/heuristics implemented under simulation `system/generation`,
not exact reproductions of the cited models.

The model does not solve atmospheric chemistry, surface climate, habitability,
formation histories, migration, tidal evolution or secular orbital perturbations.
A class such as "rocky" describes the generated model, not a prediction that
the world is Earth-like or habitable.
