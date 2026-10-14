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
