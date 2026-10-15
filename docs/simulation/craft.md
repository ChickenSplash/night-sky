# Craft and their physics

Craft are man-made objects: ships now, later stations, rovers and probes
(`crates/simulation/src/craft.rs`). Unlike stars and planets they are not
generated from seeds. They are live state that changes every step, with IDs
of their own (`craft-1`), and they never shift any generated object. See the
[flight simulator plan](../plans/flight-sim.md) for where this is heading.

## Model

| Part | Meaning |
| --- | --- |
| `kind` | `ship`, `station`, `rover` or `probe`; only ships exist yet |
| `frame` | What the position is measured from: a star system, by its page star's ID |
| `motion` | How it moves; only `Free` (integrated) exists yet |
| `seconds` | The simulation time its state describes |
| `input` | What it is told to do, held until changed: thrust in m/s², frame axes |
| `orientation` | A unit quaternion; nothing turns it yet |

A system frame uses the same origin and axes as the bodies' orbits: metres
from the page's outermost barycentre, in its plane axes (see
[orbits](orbits.md)). Positions are f64 metres and velocities m/s.

The model is serde-serializable and engine independent, so a server can run
the same code. A craft changes only through its input, applied at fixed
steps, never by editing its state, apart from placing it when it is built.

## Gravity

A craft feels the summed pull of every star and planet of its system, each at
its analytic Kepler position at that instant (`Field`). It is a test
particle: it pulls on nothing, and the bodies stay on their fixed orbits.

$$
\mathbf a = \sum_i \frac{GM_i}{r_i^3}\,\mathbf r_i
$$

where $\mathbf r_i$ points from the craft to body $i$. $GM_\odot$ is derived
from the same year and AU as Kepler periods,
$4\pi^2\,\mathrm{AU}^3/\mathrm{year}^2 \approx 1.327\times10^{20}$ m³/s², so a
craft placed on a planet's orbit follows it exactly. Planet masses use
$M_\oplus = 3.0035\times10^{-6} M_\odot$.

Inside a body, the pull is that of a uniform sphere, $GM r/R^3$, falling to
zero at the centre. This only keeps the maths finite: there are no collisions
yet, so a craft can pass straight through a planet.

The **dominant body** (shown as the craft's parent) is the nearest planet
whose Hill sphere holds the craft, $r_H = q\,\sqrt[3]{m/3M}$ at the planet's
periapsis $q$, otherwise the star pulling hardest. It is a label: gravity
still comes from every body.

## Integration

- **Fixed steps** of `STEP_SECONDS` (1/20 s) of simulation time, shared by
  every client and a future server, whatever the frame rate.
- **Velocity Verlet** (kick, drift, kick). It is symplectic, so orbital energy
  oscillates slightly rather than drifting: around a fixed Earth, an
  eccentric orbit from 400 km keeps its energy to better than $10^{-8}$ over
  100 laps (11 million steps). Thrust is applied in both half kicks.
- Gravity at the end of each step is kept for the start of the next, so a
  step costs one field evaluation (about 1 µs natively for a star with ten
  planets). It is not serialized: a loaded craft recomputes the same value
  and steps identically.
- `advance` steps until the craft reaches a time, with a cap on steps so a
  long stall cannot freeze the frame.
- Native and WebAssembly may differ in the last digits of trigonometry and
  powers, so later client correction must tolerate small differences.

The craft tests run natively and, through `wasm-bindgen-test-runner` under
Node, as WebAssembly (`./scripts/check.sh`).
