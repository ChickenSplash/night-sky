# Planetary system generation

Systems are generated as a whole from stable star IDs, with planets around
component stars or close pairs as their stability zones allow. A single star
typically has 7 to 14 planets; multiple systems can have more, fewer or none.
See [stars](stars.md) for companion stability zones, [orbits](orbits.md) for
motion, and [the information overlay](../shared/information-overlay.md) for
how optional tasks attach to planets.

## Bodies, composition and spacing

- Composition follows the snow line: rocky worlds and sub-Neptunes inside,
  giants farther out and dwarf planets beyond. Radii range from Pluto-like to
  just beyond Jupiter.
- Neighbours stay Hill-stable without crossing orbits; some pairs have
  resonant period ratios (2:1, 3:2, 4:3, 5:4, 5:3, 3:1).
- Planet mass is capped at 0.4% of the star's, or Hill spacing cannot converge.
- Spins have independent seeds and tilted axes. Close-in planets can be tidally
  locked; others draw spin-orbit states, slow spins or large tilts. Spin does
  not move an orbit.

## Generation, from the inside out

The model generates plausible final systems; it does not simulate a gas disc
accreting into planets. For each permitted host (a star or close pair):

1. Establish the inner boundary, snow line and companion-imposed outer limit.
2. Choose a target count of 7 to 14 planets, then propose orbits inside-out.
3. Draw a class and mass from the proposed distance relative to the snow line.
4. Draw eccentricity, inclination, orbital orientation and phase.
5. Move the proposed orbit outward until neighbour-spacing conditions hold;
   optionally place it at an allowed resonant period ratio.
6. Stop if the orbit's farthest point exceeds the host's stable zone. Otherwise
   assign its radius and separately seeded spin, then continue outward.

The count is a starting target, not a guarantee: a companion may leave fewer
planets or none. Different hosts in one multiple system each contribute planets.
If an ordinary generated system extends beyond 1,000 AU, the model retries
with a compact, lower-mass population, rather than letting crowded systems
grow to interstellar scales.

### The snow line and planet classes

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

| Region | Favoured generated bodies |
| --- | --- |
| Inside the snow line | Rocky planets and sub-Neptunes |
| One to four snow-line distances | Gas giants, ice giants and rocky planets |
| Beyond four snow-line distances | Mostly dwarf planets, with some giants |

These are probability mixtures, not rigid boundaries. A **sub-Neptune** is a
small volatile-envelope world, an **ice giant** an intermediate-mass world
with a larger volatile contribution, and a **gas giant** a massive gas-rich
world. They are model classes, not composition resolved by chemistry. Gas
giants are less likely around lower-mass stars. The compact retry uses rocky
and dwarf bodies instead of the ordinary class mixture.

Composition is chosen at the proposed orbit before stability adjustments,
so moving an orbit outward does not redraw its class. The snow line is a
formation heuristic, not a strict present-day classification boundary.

### Mass and radius are linked

Rocky/dwarf radii scale approximately as $R \propto M^{0.27}$; volatile-envelope
worlds grow more steeply up to about 130 Earth masses, then giant radii change
little with increasing mass. With mass/radius in Earth units, the fits are:

$$
R = \begin{cases}
M^{0.27} & \text{rocky or dwarf} \\
1.2(M/2)^{0.55} & \text{other classes, }M < 130 \\
11.9(M/130)^{-0.04} & \text{other classes, }M \ge 130
\end{cases}
$$

A small multiplicative scatter (0.92 to 1.10) is applied to radius. A ten-Earth-mass
envelope world has a nominal radius near 2.9 Earth radii before scatter;
doubling a gas giant's mass does not double its radius. This is an empirical
size approximation, not a solved interior or atmospheric model.

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
The locking-distance prescription
depends on class and scales with the cube root of host mass. Just beyond that
region, eccentric rocky/sub-Neptune worlds can receive 3:2 spin-orbit states;
less eccentric ones can spin slowly, sometimes retrograde. Farther-out worlds
draw class-dependent periods and tilts, including occasional large tilts.

These outcomes are assigned during generation, not obtained by integrating
tidal torques. System planes have isotropically distributed poles; planets
within a system generally have small inclinations, with a broader distribution
for dwarf planets. Obliquity is the spin-axis tilt, not orbital inclination.

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

## Presentation and limitations

Planets retain physical radii in every presentation mode. Table layout is
presentation-only; leaving it restores current simulated positions. Constellation
view never moves stars. Rendering and camera flights are explained in the
[client guides](../README.md#client).

There is no N-body simulation, moons or collisions. Orbital motion is analytical,
not the result of integrating forces between generated bodies.

## Scientific basis and approximations

Mass–radius fits are inspired by Chen & Kipping (2017), giant occurrence by
the host-mass trend in Johnson et al. (2010), and neighbour-spacing choices by
Chambers et al. (1996) and Pu & Wu (2015). Tidal-spin prescriptions draw on
Solar System examples and the trends discussed by Gladman et al. (1996).
These are simplified fits/heuristics implemented in `crates/simulation/src/system.rs`,
not exact reproductions of the cited models.

The model does not compute atmospheric chemistry, surface climate, habitability,
formation histories, migration, tidal evolution or secular orbital perturbations.
A class such as "rocky" describes the generated model, not a prediction that
the world is Earth-like or habitable.
