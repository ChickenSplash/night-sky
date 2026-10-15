# Orbital motion and simulation time

The simulator stores an orbit's shape and orientation, then evaluates its
position at the current simulation time. It does not advance bodies by adding
gravitational accelerations every frame. The governing model is a fixed
elliptical two-body orbit, extended to nested pairs for multiple systems.

## Orbital elements

| Element | Meaning |
| --- | --- |
| Semi-major axis $a$ | Half the ellipse's long axis, in AU; sets its overall size |
| Eccentricity $e$ | Shape: 0 is circular, values toward 1 are more elongated |
| Inclination $i$ | Orbit's tilt relative to the host system's plane |
| Ascending node $\Omega$ | Orientation of that tilt around the plane's pole |
| Phase $M_0$ | Mean anomaly at simulation time zero, locating the body along its cycle |

Inclination, node and phase are in radians in the model. The local system
plane is XZ, with Y as its pole; that whole frame can then be tilted relative
to the galaxy. A full astronomical orbit also has an argument of periapsis;
Night Sky does not store one independently. Its orientation convention fixes
periapsis along the orbit's node line, reducing the set of possible ellipses.

The nearest and farthest host distances are **periapsis** $a(1-e)$ and
**apoapsis** $a(1+e)$. A planet with $a=1$ AU and $e=0.2$ ranges from 0.8 to
1.2 AU; its semi-major axis is not a constant distance from the star.

## Period and position

The period is determined by Kepler's third law. In the units used by this model:

$$
P = \mathrm{YEAR\_SECONDS}\sqrt{\frac{a_{\mathrm{AU}}^3}{M_{\mathrm{host}}/M_\odot}}
$$

`YEAR_SECONDS` is about 365.2569 days. Planet motion uses the host's mass
(a star or the combined mass of a host pair), neglecting the planet's own mass.
A binary's relative stellar orbit uses the combined mass of both members.

At elapsed simulation time $t$, the mean anomaly advances uniformly:

$$
M = (M_0 + 2\pi t/P)\bmod 2\pi
$$

Mean anomaly is a clock angle, not the body's geometric angle about its host.
Solve Kepler's equation $E-e\sin E=M$ for **eccentric anomaly** $E$; then
construct the ellipse in its own plane:

$$
x = a(\cos E-e), \qquad z = a\sqrt{1-e^2}\sin E
$$

Inclination and node rotate this into the local system frame. Conversion from
AU to metres occurs before returning the position. Kepler's equation is solved
numerically, but the orbit being evaluated is an analytical two-body solution,
not a numerical force integration. Eccentricity makes actual motion faster
near periapsis and slower near apoapsis even though $M$ advances uniformly.

### Velocity

Velocity is the exact time derivative of that position, not a difference of
two positions. Differentiating Kepler's equation gives
$\dot E = (2\pi/P)/(1-e\cos E)$, so in the orbit's own plane:

$$
\dot x = -a\sin E\,\dot E, \qquad \dot z = a\sqrt{1-e^2}\cos E\,\dot E
$$

rotated into the system frame like the position (`orbit_state`). Barycentre
members and planets add their parents' velocities exactly as they add
positions (`member_state`, `member_states`, `planet_state`). Earth's orbit
gives about 29.8 km/s. [Craft](craft.md) need
these for gravity at moving bodies and for velocities relative to them.

### Worked example

Around a one-solar-mass host, a circular 1 AU orbit takes about one year. With
zero phase, inclination and node, it starts at $(1,0,0)$ AU and reaches
$(0,0,1)$ AU after a quarter-period. A 4 AU orbit takes $\sqrt{4^3}=8$ years;
a 1 AU orbit around 0.25 solar masses takes two years. Neither relation needs
a separate speed parameter for each planet.

## Nested barycentres

For a pair with masses $m_A,m_B$, first evaluate the relative separation vector
$\mathbf{r}=\mathbf{r}_B-\mathbf{r}_A$ using the pair's orbit and total mass.
Relative to their common centre of mass:

$$
\mathbf{r}_A = -\frac{m_B}{m_A+m_B}\mathbf{r}, \qquad
\mathbf{r}_B = \frac{m_A}{m_A+m_B}\mathbf{r}
$$

Equal masses move equally far on opposite sides; if A has twice B's mass,
A moves one-third of the separation and B two-thirds. A is not held fixed
while B goes around it.

In a triple or quadruple, add the parent barycentre's position to each member's
local position, working outward-to-inward through the hierarchy. A planet's
position is its host's position plus its own host-relative orbit. An S-type
planet therefore travels with its star; a P-type planet follows its host pair's
barycentre. See [multiple systems](stars.md#understanding-the-companion-hierarchy).

Each orbit is evaluated independently. The model does not calculate the
perturbation one orbit exerts on another or the acceleration from every star
on a planet in a multiple system.

## Simulation clock and precision

Elapsed time accumulates as $\Delta t_{\mathrm{sim}} = \mathrm{rate}\,\Delta t_{\mathrm{real}}$
while unpaused. Changing the rate changes subsequent increments, not time
already accumulated. For example, one real second at one day/s advances the
clock by one day; switching to ten days/s leaves it at that day until the next
increment. Pause stops orbital/spin advancement, not camera movement.

The clock changes orbit phases and body rotation, not generated stellar ages
or planet properties. Stellar evolution is a generation-time approximation;
see [stars](stars.md#from-birth-mass-and-age-to-a-star).

Calculations use `f64` and return positions in SI metres; the interface uses
AU and light years. Never multiply lifetime elapsed time by a mutable rate:
changing the rate or pausing must not jump orbit phase. The equations and
clock are implemented in `crates/simulation/src/lib.rs`; these are standard
Keplerian two-body relations, subject to the fixed-orbit assumptions above.

- Orbital positions use the system's plane axes. The client's
  `scene::page_frame` rotates them into galactic axes for camera and rendering
  work; see [navigation](../client/navigation.md#camera-frames-and-controls).

This is not an N-body simulation. Collisions, relativistic effects and physical
warp travel are not implemented. Camera flights are navigation aids, not
simulated spacecraft trajectories; [craft](craft.md) are integrated
separately, as test particles in these bodies' gravity.
Planet generation and stability prescriptions are described in
[planetary systems](planetary-systems.md).
