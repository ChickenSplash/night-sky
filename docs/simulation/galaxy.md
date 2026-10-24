# Galaxy population and cells

The galaxy is procedural: its density model determines populations, and stable
cell codes and seeds make the same stars available without storing the whole
galaxy. See [stars](stars.md) for stellar properties and companions,
[streaming](../client/streaming.md) for loading visible cells, and
[galactic rendering](../client/galactic-rendering.md) for their light.

## Population model

The model adds seven overlapping populations. A component is not a disjoint
region: a location near the centre can contain nuclear-disc, bulge and other
stars. Each contributes its own density and formation-age distribution.

| Component | Spatial shape | Generated ages |
| --- | --- | --- |
| Nuclear star cluster | Compact spherical concentration around the central black hole, with a Plummer profile | Mostly 5 to 12 billion years, plus a younger population |
| Bar and bulge | Rotated, elongated three-axis exponential concentration | Mostly 8 to 12 billion years, with a smaller 1 to 5 billion-year population |
| Thin disc | Broad exponential disc, with weak spiral modulation | 0.2 to 10 billion years |
| Young arm stars | Thin, strongly spiral-modulated disc with a depleted inner region | Up to 200 million years |
| Thick disc | More vertically extended exponential disc, without arm modulation | 8 to 12 billion years |
| Halo | Flattened, cored outer population, fading roughly as distance to the power of -4 | 11 to 13 billion years |
| Nuclear stellar disc | Dense, flattened exponential disc near the centre, separate from the spherical cluster | Mostly 8 to 12 billion years, with intermediate and recent bursts |

Those ages are sampled at generation, not advanced by the simulation clock.
Young arms can contain massive hot stars whose short lives would already have
ended in the old halo. Low-mass stars survive in both, while white dwarfs
represent part of the older population. See [stellar generation](stars.md).

### Density shapes

For an ordinary exponential disc, density decreases both outward and above or
below the galactic plane:

$$
\rho(R,y) \propto e^{-R/l}\,e^{-|y|/h}
$$

$R$ is distance from the centre in the XZ plane, $l$ the radial scale length,
and $h$ the vertical scale height. Each scale is an e-folding distance: move
one scale farther and that factor becomes about 37% of its previous value.
For example, the thin disc has $l=2.6$ kpc and $h=300$ pc; the thick disc's
larger 900 pc height spreads its stars much farther from the plane.

The young-arm disc instead multiplies the radial exponential by $(R/l)^2$,
so it is depleted at the centre and peaks at $R=2l$ before spiral modulation.
The bar uses an exponential in ellipsoidal distance, rather than cylindrical
radius. The nuclear cluster uses the Plummer shape
$\rho(r)\propto[1+(r/a)^2]^{-5/2}$, with a 4 pc scale; the halo has a core
and flattened-distance shape $\rho(s)\propto(s^2+c^2)^{-2}$, not a dark-matter
or gravitational-potential model.

Four logarithmic spiral arms redistribute stars around each radial circle.
Their contrast is strong for young arm stars and much weaker for the ordinary
thin disc. Modulation averages to one, so winding or sharpening an arm changes
where stars appear, not the component's total population. The arms wind
outward against the rotation, so they trail it as real spiral arms do. Arms
and system positions are static; see [motion](#motion) for velocities.

### Counts and local mixtures

The default model allocates **200 billion star systems**, not 200 billion
individual stellar members. Its largest shares are the thin disc (63.65%),
bar/bulge (22.5%) and thick disc (9%); the halo, young arms and nuclear
populations make up the remainder. Companions add stars without changing that
number of system primaries.

At a location, the relative contribution of a component is its density divided
by the sum of all component densities. Global shares are therefore not the
local mix everywhere. Cells use integrated population weights to allocate
systems across regions while conserving their parent counts.

The galactic model cuts off at 50 kpc from the centre; this physical model
boundary is distinct from the larger 131,072 pc-wide procedural root cube.
Smooth profile integrals set the intended populations, while discrete cell
allocation preserves exact system counts. Numerical integration tests check
the profiles against their intended shares to within 1%.

- `space_simulation::galaxy::GalaxyModel` (parameterised, `milky_way()` by
  default) gives stars per cubic parsec at any point, per `Component`: nuclear
  cluster, bar and bulge, thin disc, young arm stars, thick disc, halo and
  nuclear stellar disc (an exponential `Disc`, 75 pc by 45 pc, last in
  `Component::ALL` so the others keep their star ID indices).
- Axes are parsecs from the central black hole, disc in XZ, Y to the north
  pole; home is 8.2 kpc out on +X, between arms.
- Every profile has a closed-form integral, so components hold exactly their
  share of `stars` (2e11). Spiral modulation averages to one around every
  circle; keep it that way, or the totals drift. The test integrates the
  density numerically to within 1%.
- The census and initial mass function describe system primaries, not every
  companion. Neutron stars and stellar black holes are redrawn, so this count
  is of star systems; companions add about a third more individual stars.

## Motion

Every star system has a galactocentric velocity, but nothing moves yet:
positions, cells and streaming are unchanged. The disc turns clockwise seen
from the north pole (+Y), as the Milky Way does, so home moves toward +Z.

A system's velocity is a shared mean rotation plus random scatter sized by
its component (`crates/simulation/src/galaxy/kinematics.rs`):

- **Rotation curve.** The circular speed $v_c(R)$ at distance $R$ from the
  axis comes from a spherical mass model: the black hole, Plummer spheres for
  the nuclear cluster and nuclear disc, a Hernquist bar and bulge, and a cored
  logarithmic term for the disc and dark halo together:
  $$
  v_c^2 = G\left[\frac{M_\bullet}{R} + \sum \frac{M R^2}{(R^2+a^2)^{3/2}}
  + \frac{M_b R}{(R+c)^2}\right] + v_0^2\frac{R^2}{R^2+R_c^2}
  $$
  It rises steeply near the centre and is about 232 km/s at home, roughly flat
  beyond a few kiloparsecs. It sets motion only; star counts still come from
  the density profiles, which are not a mass model.
- **Mean rotation.** Disc components lag $v_c$ by their asymmetric drift,
  $\sigma_R^2/80$ km/s, so hotter populations rotate more slowly. The
  spheroids rotate at a fixed share of $v_c$: the nuclear cluster 0.3, the bar
  and bulge 0.35, the halo 0.1.
- **Scatter.** Gaussian draws in the radial, rotational and vertical
  directions, from the system's own `{key}:velocity` seed:

| Component | Dispersion (radial, along, vertical), km/s |
| --- | --- |
| Young arm stars | 12, 8, 6 |
| Thin disc | 45, 30, 27 at 10 billion years, scaling as age$^{0.31}$ (vertical $^{0.43}$) |
| Thick disc | 60, 45, 40 |
| Nuclear stellar disc | 75, 70, 60 |
| Bar and bulge | 115, 100, 95 |
| Nuclear star cluster | 100 each, rising as $\sqrt{GM_\bullet/3r}$ near the black hole |
| Halo | 140, 80, 95 |

- The thin disc's age-velocity relation needs the system's age
  (`system_age_gyr`): a multiple's comes from the draw its companions use, and
  a single star whose draw never needed one gets it from its `{key}:age` seed.
- Legacy page stars without a component move as thin-disc stars. Companions
  share their system's velocity; their orbits about the barycentre are separate.

## Cells and brightness tiers

- Galactic stars are generated from cells (`space_simulation::galaxy::cells`), never
  stored. `Cell` is a locational code (a leading 1, then octant bits x, y, z per
  level); the root is `ROOT_PC` (131,072 pc) across, centred on the black hole.
- `population` sorts stars into `TIERS` brightness bands, four times dimmer per
  tier (`tier_limit`). Tier t's stars belong to level-t cells, so cell size and
  reach (`reach_pc`) both halve per tier. Tiered stars are sampled directly
  within their band; together they match `galactic_star`'s mix (tested).
- `Sky` holds the root's exact counts per component and tier and splits each
  cell's finer counts among its children by a seeded multinomial, weighted by
  the model's stars in each child (`child_masses`). Children always add up to
  their parent, and the galaxy holds exactly 2e11 systems. Seeds depend only on
  cell codes, so results never depend on visiting order or the cache
  (`Sky::retain` only drops cached work).
- A cell star's ID is `g{cell hex}.{component}.{index}`. `cell_star(id)` and
  `cell_planetary_system(id)` rebuild it from the ID alone; its position needs
  `Sky::star(id)`.

## Generation consistency

Preserve generation draw order and component indices. Full stars and the compact
`Sky::try_point` path must match bit for bit, including companions and separation;
results must not depend on traversal order, caching or loader thread count.
Component populations remain exact and spiral modulation remains normalised.

## Scientific basis and approximations

The component layout is inspired by the Milky Way synthesis of Bland-Hawthorn
& Gerhard (2016), bar/bulge work by Wegg & Gerhard (2013), and nuclear-disc
work by Launhardt et al. (2002) and Sormani et al. (2022). Its nuclear-disc age
mixture draws on the burst history discussed by Nogueras-Lara et al. (2020).
Kinematics follow the same review, with asymmetric drift after Dehnen & Binney
(1998) and thin-disc heating after Aumer & Binney (2009).
Parameters and simplified profiles are implemented in
`crates/simulation/src/galaxy/mod.rs`.

This is a statistical synthetic galaxy, not a reconstruction of observed stars,
a galactic gravitational simulation or an evolving star-formation calculation.
Its central black hole has a specified mass and rendered Schwarzschild radius;
stars are not integrated under its gravity. The smooth glow renderer further
approximates these populations and averages out their spiral structure; see
[diffuse light](../client/galactic-rendering.md#diffuse-galaxy-glow).
