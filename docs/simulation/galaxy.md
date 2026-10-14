# Galaxy population and cells

The galaxy is procedural: its density model determines populations, and stable
cell codes and seeds make the same stars available without storing the whole
galaxy. See [stars](stars.md) for stellar properties and companions,
[streaming](../client/streaming.md) for loading visible cells, and
[galactic rendering](../client/galactic-rendering.md) for their light.

## Population model

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

## Cells and brightness tiers

- Galactic stars are generated from cells (`space_simulation::cells`), never
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
