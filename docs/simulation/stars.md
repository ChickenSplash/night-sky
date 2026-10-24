# Stars and multiple systems

Stellar properties come from a birth mass and age, while companion generation
builds a stable hierarchy of stars and barycentres. See [the galaxy model](galaxy.md)
for regional populations and [planetary systems](planetary-systems.md) for planets
inside the hierarchy's stable zones.

## Reading a star's type

A spectral class describes temperature, not age. The main-sequence order is
**O, B, A, F, G, K, M**, hottest to coolest. Night Sky uses these approximate
temperature bands to assign labels:

| Class | Temperature | Typical description |
| --- | --- | --- |
| O | 30,000 to 60,000 K | Blue, very hot and short-lived |
| B | 10,000 to 30,000 K | Blue-white |
| A | 7,500 to 10,000 K | White |
| F | 6,000 to 7,500 K | Yellow-white |
| G | 5,200 to 6,000 K | Sun-like, traditionally called yellow |
| K | 3,700 to 5,200 K | Orange |
| M | 2,600 to 3,700 K | Red dwarf, orange in realistic rendering |

Within a class, a digit runs from 0 at the hot end to 9 at the cool end.
The trailing luminosity class separates **V** (main sequence), **III** (giant)
and **I** (supergiant). For example, `G2 V` means a relatively hot G-class
main-sequence star, not a giant. These are temperature-based approximations,
not classifications computed from simulated absorption spectra.

White dwarfs use a different label, `DA` plus a temperature index. In the model
the index is roughly `50,400 / temperature_K`, rounded and limited to 1 to 9.
This is a simplified designation, not an atmosphere or spectral-line simulation.
A white dwarf can be hot but faint because its emitting surface is small;
a cool giant can be bright because its surface is enormous.

## From birth mass and age to a star

Galactic generation follows this chain:

```text
galactic component -> birth mass and age -> evolutionary stage
                   -> present mass, radius, temperature and luminosity
                   -> companion hierarchy and planet-forming conditions
```

The Kroupa initial mass function makes low-mass births much more frequent than
high-mass ones: the number per mass interval scales as $M^{-1.3}$ below
0.5 solar masses and $M^{-2.3}$ above it, continuously joined. The sampled
range is 0.08 to 100 solar masses. A region's formation history supplies the
age, which is why old halo populations and young arms look different.

For main-sequence stars, mass determines radius and luminosity, then temperature
follows the Stefan–Boltzmann relation. In solar units, the implemented fits are:

$$
R = M^{0.8}, \qquad
L = \begin{cases}
0.43^{1.7} M^{2.3} & M < 0.43 \\
M^4 & 0.43 \le M < 2 \\
\sqrt{2} M^{3.5} & M \ge 2
\end{cases}
$$

$$
T = 5772\,\mathrm{K}\,(L/R^2)^{1/4}
$$

A one-solar-mass star therefore has one solar radius, one solar luminosity and
5,772 K in these fits. At half a solar mass, the radius is about 0.57 solar
radii, luminosity about 0.063 solar luminosities, and temperature about 3,800 K:
less massive stars are not merely smaller copies with the same brightness.

The approximate main-sequence lifetime is $10 M^{-2.5}$ billion years, with a
minimum of 0.003 billion years. Ages below that lifetime produce a main-sequence
star; the next 10% of the lifetime is assigned a giant/supergiant phase. Older
births below 8 solar masses leave white dwarfs. More massive expired births
would leave neutron stars or black holes and are redrawn rather than represented.
Giant radii/temperatures are drawn from simplified branch ranges; white dwarfs
use an approximate remnant mass, mass-dependent radius and cooling law.

**This is evolution evaluated during generation, not evolution driven by the
simulation clock.** Advancing time moves orbits and spins; it does not age stars,
turn them into giants, trigger supernovae or cool white dwarfs during a session.

## How an evolved star changes its planets

Planet-forming conditions remember the progenitor, not just the star's current
brightness. A giant's inner boundary excludes planets within three current
stellar radii, approximating engulfment. Its formation snow line still comes
from the main-sequence luminosity of its birth mass.

For a white dwarf, the model widens surviving orbital scales by the ratio of
progenitor mass to remnant mass. It also removes the inner region associated
with the giant phase. Thus a now-faint white dwarf need not have planets packed
near the snow line implied by its current light. These are generation rules,
not time-integrated mass loss or a replay of actual planet formation.

## Stellar populations

- Galactic stars (`galactic_star(id, component)`, `GalaxyModel::star(id, p)`)
  draw a Kroupa birth mass and an age from the component's formation history,
  then evolve (main sequence, giant or supergiant, white dwarf). Neutron stars
  and stellar black holes are redrawn: `stars` counts star systems only. Their
  systems come from `galactic_bodies(id, component)`.
- Legacy page stars (`star(id)`) use a fixed solar-neighbourhood census: mostly
  red dwarfs, a few giants and white dwarfs. Galactic and page stars share the
  builders in `crates/simulation/src/star/mod.rs`; census draws must keep their
  order, or every page star changes.
- Host mass sets orbital periods (`StarSystem::host_mass`); radius and
  temperature determine the drawn size and `spectral_colour`, and stellar
  properties shape planet formation.
- Stars and planets carry a `Spin`. Their spins draw from their own seeds,
  so changing spin does not move an orbit. Spin flattens a star (the Roche
  model, see [shape](planetary-systems.md#shape-flattening-by-spin)); no star
  is generated spinning so fast it would be flatter than 0.2, a few fast hot
  stars are slowed to that.

## Companions and hierarchy

- Multiple stars (`crates/simulation/src/star/multiple.rs`): every drawn star is a
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
- Initial separations are drawn up to 20,000 AU, but mass loss and stability
  adjustments can widen them further. A post-widening hierarchy with maximum
  stellar separation over 100,000 AU is rejected, retaining only its primary.
  This is an explicit alpha approximation for disrupted ultra-wide systems,
  not an integration of galactic tides. Compact light/extent generation and
  full body generation apply the same rule. Besides avoiding unbounded bound
  hierarchies, it bounds how far craft look for systems to enter (see
  [frames and arrival](craft.md#frames-and-arrival)); planets add at
  most 1,000 AU apoapsis around their host. This generation bound does not impose
  a travel boundary or collision protection on unsafe manual warp.
- Planets orbit one star inside its stable zone, or a pair closer than 1 AU
  from outside it (Holman & Wiegert); pairs under 50 AU keep planets a third
  of the time.
- `bodies(key)`/`cell_bodies(id)` build a whole system; `member_position` and
  `planet_position` place stars and planets from the outermost barycentre.
  `place` and `galaxy_nav::generated` both build from `Bodies`; a page's
  capacity is its total planets. The census and IMF describe primaries;
  companions add about a third more stars, mostly red dwarfs.

## Understanding the companion hierarchy

A barycentre is a centre of mass, not an additional luminous object. In a
binary, both stars orbit it; in a triple, an inner pair's barycentre can orbit
with a distant third star. A quadruple can be two pairs orbiting each other or
a nested chain. The primary is the brightest star, not necessarily the most
massive object at birth.

Companion masses are sampled as fractions of the birth primary's mass, with a
small excess of near-twins. Separations are broadly log-normal: typically
tighter for red dwarfs than Sun-like stars. Close companions can eliminate
planets or truncate their allowed orbits. An **S-type** planet orbits one star
inside a companion-imposed outer boundary; a **P-type** planet orbits both stars
outside a close pair's inner exclusion zone. Stability boundaries depend on
separation, mass ratio and eccentricity, not distance alone.

See [orbits](orbits.md#nested-barycentres) for how these centres of mass move,
and [planetary systems](planetary-systems.md) for generation within their zones.

## Scientific basis and approximations

The source model draws on Kroupa (2001) for birth masses, the RECONS 10 pc
solar-neighbourhood census for legacy stars, and a Mestel-inspired white-dwarf
cooling fit. Multiplicity follows observational trends discussed by Raghavan
et al. (2010), Winters et al. (2019) and Moe & Di Stefano (2017).
Hierarchy spacing uses a Mardling & Aarseth (2001)-inspired criterion;
planet stability zones use Holman & Wiegert (1999) fits, with close-binary
planet suppression inspired by Kraus et al. (2016).

These are simplified prescriptions, not full stellar structure/evolution
calculations or dynamical stability proofs. Metallicity, interacting binaries,
mass transfer, brown dwarfs and stellar neutron-star/black-hole populations are
not modelled here. The rendered central black hole is a separate galactic object.
The authoritative fits and sampling assumptions live in
`crates/simulation/src/star/`; cited models are inspirations,
not claims that every detail of those studies is reproduced.
