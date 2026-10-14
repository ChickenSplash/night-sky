# Orbital motion and simulation time

- Calculations use SI metres and `f64`; the interface uses AU and light years.
  Orbital positions are solved analytically from Kepler's equation. Periods
  follow the semi-major axis and the host's mass, including barycentre hosts.
- Simulation time is integrated. Never multiply lifetime elapsed time by a
  mutable rate: changing the rate or pausing must not jump orbit phase.
- Task deadlines use wall-clock time independently of the simulation clock.
- Multiple systems are nested two-body orbits about barycentres; see
  [stars and multiple systems](stars.md). Planet generation and stable spacing
  are described in [planetary systems](planetary-systems.md).
- Orbital positions use the system's plane axes. The client's
  `scene::page_frame` rotates them into galactic axes for camera and rendering
  work; see [navigation](../client/navigation.md#camera-frames-and-controls).

This is not an N-body simulation. Collisions, relativistic effects and physical
warp travel are not implemented. Camera flights are navigation aids, not
simulated spacecraft trajectories.
