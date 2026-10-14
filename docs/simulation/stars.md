# Stars and multiple systems

Stellar properties come from a birth mass and age, while companion generation
builds a stable hierarchy of stars and barycentres. See [the galaxy model](galaxy.md)
for regional populations and [planetary systems](planetary-systems.md) for planets
inside the hierarchy's stable zones.

## Stellar populations

- Galactic stars (`galactic_star(id, component)`, `GalaxyModel::star(id, p)`)
  draw a Kroupa birth mass and an age from the component's formation history,
  then evolve (main sequence, giant or supergiant, white dwarf). Neutron stars
  and stellar black holes are redrawn: `stars` counts star systems only. Their
  systems come from `galactic_bodies(id, component)`.
- Legacy page stars (`star(id)`) use a fixed solar-neighbourhood census: mostly
  red dwarfs, a few giants and white dwarfs. Galactic and page stars share the
  builders in `crates/simulation/src/star.rs`; census draws must keep their
  order, or every page star changes.
- Host mass sets orbital periods (`StarSystem::host_mass`); radius and
  temperature determine the drawn size and `spectral_colour`, and stellar
  properties shape planet formation.
- Stars and planets carry a `Spin`. Their spins draw from their own seeds,
  so changing spin does not move an orbit.

## Companions and hierarchy

- Multiple stars (`crates/simulation/src/multiple.rs`): every drawn star is a
  system's primary, its brightest star, and draws its companions from the same
  sequence (no separate seed): `{id}:star` for page and galactic stars, and
  for tiered stars `{id}:tier` for what sets their light, `{id}:star` for
  the rest.
- Multiplicity rises with mass (about 26% of red dwarfs, 46% of Sun-like stars,
  most B stars), a quarter of multiples have three or four stars, and about a
  quarter of white dwarfs have companions (all tested).
- Companions share the primary's age, so some born heavier are white dwarfs;
  ones a giant phase would have engulfed are gone.
- Pairs nest hierarchically (`Barycentre`, innermost first, the last holding
  the system), each a two-body orbit widened to stay stable (Mardling & Aarseth).
  Stars are lettered down the hierarchy (a close pair AB, its distant third C).
- Planets orbit one star inside its stable zone, or a pair closer than 1 AU
  from outside it (Holman & Wiegert); pairs under 50 AU keep planets a third
  of the time.
- `bodies(key)`/`cell_bodies(id)` build a whole system; `member_position` and
  `planet_position` place stars and planets from the outermost barycentre.
  `place` and `galaxy_nav::generated` both build from `Bodies`; a page's
  capacity is its total planets. The census and IMF describe primaries;
  companions add about a third more stars, mostly red dwarfs.
