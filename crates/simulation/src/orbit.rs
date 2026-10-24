//! Analytic two-body orbits and nested barycentric motion, in SI units.

use crate::units::{AU_METRES, YEAR_SECONDS};
use space_shared::{Orbit, Planet, StarSystem};
use std::f64::consts::TAU;

pub fn period_seconds(orbit: &Orbit, mass_solar: f64) -> f64 {
    YEAR_SECONDS * (orbit.semi_major_au.powi(3) / mass_solar).sqrt()
}

/// Position in metres and velocity in metres per second.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
}

impl State {
    pub(crate) fn offset(self, relative: State, share: f64) -> State {
        State {
            position: std::array::from_fn(|i| self.position[i] + relative.position[i] * share),
            velocity: std::array::from_fn(|i| self.velocity[i] + relative.velocity[i] * share),
        }
    }
}

/// Analytic two-body orbit, returned in metres relative to its star.
/// Orbital elements are generated in the valid elliptic domain (0 <= e < 0.8).
pub fn position(orbit: &Orbit, mass_solar: f64, seconds: f64) -> [f64; 3] {
    position_at_eccentric_anomaly(orbit, eccentric_anomaly(orbit, mass_solar, seconds))
}

/// `position` with its time derivative, in metres per second.
pub fn orbit_state(orbit: &Orbit, mass_solar: f64, seconds: f64) -> State {
    let period = period_seconds(orbit, mass_solar);
    let anomaly = eccentric_anomaly(orbit, mass_solar, seconds);
    let (sin, cos) = anomaly.sin_cos();
    // dE/dt from Kepler's equation, E - e sin E = M.
    let rate = TAU / period / (1.0 - orbit.eccentricity * cos);
    let a = orbit.semi_major_au * AU_METRES;
    let minor = a * (1.0 - orbit.eccentricity.powi(2)).sqrt();
    State {
        position: rotate(orbit, a * (cos - orbit.eccentricity), minor * sin),
        velocity: rotate(orbit, -a * sin * rate, minor * cos * rate),
    }
}

fn eccentric_anomaly(orbit: &Orbit, mass_solar: f64, seconds: f64) -> f64 {
    let mean =
        (orbit.phase_rad + TAU * (seconds / period_seconds(orbit, mass_solar))).rem_euclid(TAU);
    let mut eccentric = mean;
    for _ in 0..20 {
        let delta = (eccentric - orbit.eccentricity * eccentric.sin() - mean)
            / (1.0 - orbit.eccentricity * eccentric.cos());
        eccentric -= delta;
        if delta.abs() < 1e-13 {
            break;
        }
    }
    eccentric
}

pub fn position_at_eccentric_anomaly(orbit: &Orbit, anomaly: f64) -> [f64; 3] {
    let a = orbit.semi_major_au * AU_METRES;
    let x = a * (anomaly.cos() - orbit.eccentricity);
    let z = a * (1.0 - orbit.eccentricity.powi(2)).sqrt() * anomaly.sin();
    rotate(orbit, x, z)
}

/// From the orbit's own plane (periapsis along x) into the system's axes.
fn rotate(orbit: &Orbit, x: f64, z: f64) -> [f64; 3] {
    let y = z * orbit.inclination_rad.sin();
    let z = z * orbit.inclination_rad.cos();
    let (s, c) = orbit.ascending_node_rad.sin_cos();
    [c * x + s * z, y, -s * x + c * z]
}

/// Where a star or barycentre is, in metres from its page's outermost
/// barycentre: each pair's members sit on opposite sides of their own
/// barycentre, at distances inverse to their masses. A single star is at the
/// origin.
pub fn member_position(system: &StarSystem, page: u32, id: &str, seconds: f64) -> [f64; 3] {
    let Some(pair) = system.pair_of(page, id) else {
        return [0.0; 3];
    };
    let base = member_position(system, page, &pair.id, seconds);
    let masses = pair.members.each_ref().map(|m| system.member_mass(page, m));
    let total = masses[0] + masses[1];
    let relative = position(&pair.orbit, total, seconds);
    let share = if pair.members[0] == id {
        -masses[1] / total
    } else {
        masses[0] / total
    };
    std::array::from_fn(|axis| base[axis] + relative[axis] * share)
}

/// `member_position` with the member's velocity.
pub fn member_state(system: &StarSystem, page: u32, id: &str, seconds: f64) -> State {
    let Some(pair) = system.pair_of(page, id) else {
        return State::default();
    };
    let base = member_state(system, page, &pair.id, seconds);
    let masses = pair.members.each_ref().map(|m| system.member_mass(page, m));
    let total = masses[0] + masses[1];
    let share = if pair.members[0] == id {
        -masses[1] / total
    } else {
        masses[0] / total
    };
    base.offset(orbit_state(&pair.orbit, total, seconds), share)
}

/// Every star of a page, in `stars_on` order, where `member_position` puts
/// it, solving each pair's orbit once rather than once per star below it.
pub fn star_positions(system: &StarSystem, page: u32, seconds: f64) -> Vec<[f64; 3]> {
    let placed = member_positions(system, page, seconds);
    system
        .stars_on(page)
        .map(|star| find(&placed, &star.id).unwrap_or([0.0; 3]))
        .collect()
}

/// Every star and barycentre of a page with its state, outermost pairs'
/// members first. A single star is alone at rest at the origin.
pub fn member_states(system: &StarSystem, page: u32, seconds: f64) -> Vec<(&str, State)> {
    let mut placed = place_members(
        system,
        page,
        State::default(),
        |orbit, mass| orbit_state(orbit, mass, seconds),
        State::offset,
    );
    if let Some(root) = system.root(page) {
        placed.push((root, State::default()));
    }
    placed
}

/// `member_states` without velocities.
pub(crate) fn member_positions(
    system: &StarSystem,
    page: u32,
    seconds: f64,
) -> Vec<(&str, [f64; 3])> {
    let mut placed = place_members(
        system,
        page,
        [0.0; 3],
        |orbit, mass| position(orbit, mass, seconds),
        |base, relative, share| std::array::from_fn(|i| base[i] + relative[i] * share),
    );
    if let Some(root) = system.root(page) {
        placed.push((root, [0.0; 3]));
    }
    placed
}

pub(crate) fn find<T: Copy>(placed: &[(&str, T)], id: &str) -> Option<T> {
    placed
        .iter()
        .find(|(member, _)| *member == id)
        .map(|&(_, at)| at)
}

/// Each pair's members around their pair, outermost first, so each pair's own
/// place is known before its members'.
fn place_members<T: Copy>(
    system: &StarSystem,
    page: u32,
    origin: T,
    solve: impl Fn(&Orbit, f64) -> T,
    offset: impl Fn(T, T, f64) -> T,
) -> Vec<(&str, T)> {
    let mut pending: Vec<_> = system.barycentres_on(page).collect();
    pending.reverse();
    let mut placed: Vec<(&str, T)> = Vec::with_capacity(2 * pending.len());
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|pair| {
            let base = match find(&placed, &pair.id) {
                Some(at) => at,
                None if system.pair_of(page, &pair.id).is_none() => origin,
                None => return true,
            };
            let masses = pair.members.each_ref().map(|m| system.member_mass(page, m));
            let total = masses[0] + masses[1];
            let relative = solve(&pair.orbit, total);
            for (member, share) in pair
                .members
                .iter()
                .zip([-masses[1] / total, masses[0] / total])
            {
                placed.push((member, offset(base, relative, share)));
            }
            false
        });
        if pending.len() == before {
            break;
        }
    }
    placed
}

/// Where a planet is, in metres from its page's outermost barycentre.
pub fn planet_position(system: &StarSystem, planet: &Planet, seconds: f64) -> [f64; 3] {
    let host = member_position(system, planet.page, &planet.host, seconds);
    let local = position(&planet.orbit, system.host_mass(planet), seconds);
    std::array::from_fn(|axis| host[axis] + local[axis])
}

/// `planet_position` with the planet's velocity.
pub fn planet_state(system: &StarSystem, planet: &Planet, seconds: f64) -> State {
    let host = member_state(system, planet.page, &planet.host, seconds);
    host.offset(
        orbit_state(&planet.orbit, system.host_mass(planet), seconds),
        1.0,
    )
}

/// Furthest any star or planet of a page gets from its barycentre, in AU.
pub fn system_extent_au(system: &StarSystem, page: u32) -> f64 {
    let apoapsis = |orbit: &Orbit| orbit.semi_major_au * (1.0 + orbit.eccentricity);
    let stars = system
        .barycentres_on(page)
        .map(|pair| apoapsis(&pair.orbit))
        .fold(0.0, f64::max);
    system
        .on_page(page)
        .map(|planet| {
            // A host other than the whole system is at most `stars` out.
            let host = if system.root(page) == Some(planet.host.as_str()) {
                0.0
            } else {
                stars
            };
            host + apoapsis(&planet.orbit)
        })
        .fold(stars, f64::max)
}

/// Orbital period of a planet around its host, in seconds.
pub fn planet_period(system: &StarSystem, planet: &Planet) -> f64 {
    period_seconds(&planet.orbit, system.host_mass(planet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SUN_GM, demo_galaxy};
    fn orbit() -> Orbit {
        Orbit {
            semi_major_au: 1.0,
            eccentricity: 0.0,
            inclination_rad: 0.0,
            ascending_node_rad: 0.0,
            phase_rad: 0.0,
        }
    }
    fn radius(p: [f64; 3]) -> f64 {
        p.into_iter().map(|v| v * v).sum::<f64>().sqrt()
    }

    #[test]
    fn earth_period_and_orbit_closure() {
        let o = orbit();
        let period = period_seconds(&o, 1.0);
        assert!((period / 86400.0 - 365.2569).abs() < 0.001);
        let a = position(&o, 1.0, 0.0);
        let b = position(&o, 1.0, period);
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 0.001);
        }
        assert!((position(&o, 1.0, period / 4.0)[2] - AU_METRES).abs() < 0.001);
    }

    #[test]
    fn ellipse_has_correct_apsides_even_when_tilted() {
        let o = Orbit {
            eccentricity: 0.6,
            inclination_rad: 0.7,
            ascending_node_rad: 1.2,
            ..orbit()
        };
        assert!((radius(position(&o, 1.0, 0.0)) / AU_METRES - 0.4).abs() < 1e-12);
        assert!(
            (radius(position(&o, 1.0, period_seconds(&o, 1.0) / 2.0)) / AU_METRES - 1.6).abs()
                < 1e-12
        );
        assert!(
            (period_seconds(
                &Orbit {
                    semi_major_au: 4.0,
                    ..o
                },
                1.0
            ) / YEAR_SECONDS
                - 8.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn multiple_stars_orbit_their_barycentres() {
        let galaxy = demo_galaxy();
        let mut checked = 0;
        for system in &galaxy.systems {
            for page in system.pages() {
                if system.barycentres_on(page).next().is_none() {
                    continue;
                }
                checked += 1;
                for seconds in [0.0, 1e7, 3.3e9] {
                    // The centre of mass stays at the origin.
                    let mut centre = [0.0; 3];
                    let mut mass = 0.0;
                    for star in system.stars_on(page) {
                        let p = member_position(system, page, &star.id, seconds);
                        mass += star.mass_solar;
                        (0..3).for_each(|i| centre[i] += p[i] * star.mass_solar);
                    }
                    // Solving all stars at once matches them one by one exactly.
                    let one_by_one: Vec<_> = system
                        .stars_on(page)
                        .map(|star| member_position(system, page, &star.id, seconds))
                        .collect();
                    assert_eq!(star_positions(system, page, seconds), one_by_one);
                    assert!(radius(centre) / mass < 1.0, "{centre:?}");
                    // Each pair's members follow their relative orbit.
                    for pair in system.barycentres_on(page) {
                        let [a, b] = pair
                            .members
                            .each_ref()
                            .map(|m| member_position(system, page, m, seconds));
                        let total = system.member_mass(page, &pair.id);
                        let expected = position(&pair.orbit, total, seconds);
                        let gap: [f64; 3] = std::array::from_fn(|i| b[i] - a[i] - expected[i]);
                        assert!(radius(gap) < 1e-6 * radius(expected), "{pair:?}");
                    }
                    for planet in system.on_page(page) {
                        let host = member_position(system, page, &planet.host, seconds);
                        let local = position(&planet.orbit, system.host_mass(planet), seconds);
                        let at = planet_position(system, planet, seconds);
                        let gap: [f64; 3] = std::array::from_fn(|i| at[i] - host[i] - local[i]);
                        assert!(radius(gap) < 1e-9 * radius(at).max(1.0));
                    }
                }
            }
        }
        assert!(checked > 3);
    }

    /// Central difference of a position over ±1 s, in metres per second.
    fn rate_of(at: impl Fn(f64) -> [f64; 3], seconds: f64) -> [f64; 3] {
        let [a, b] = [at(seconds - 1.0), at(seconds + 1.0)];
        std::array::from_fn(|i| (b[i] - a[i]) / 2.0)
    }

    fn close(a: [f64; 3], b: [f64; 3], tolerance: f64) -> bool {
        radius(std::array::from_fn(|i| a[i] - b[i])) <= tolerance * radius(b).max(1.0)
    }

    /// Velocities agree to a millionth, or to rounding in a difference of
    /// positions this far out.
    fn close_at(state: State, expected: [f64; 3]) -> bool {
        let gap = radius(std::array::from_fn(|i| state.velocity[i] - expected[i]));
        gap <= 1e-6 * radius(expected) + 1e-14 * radius(state.position)
    }

    #[test]
    fn orbital_velocity_is_the_rate_of_change_of_position() {
        let o = Orbit {
            eccentricity: 0.6,
            inclination_rad: 0.7,
            ascending_node_rad: 1.2,
            phase_rad: 0.4,
            ..orbit()
        };
        for seconds in [0.0, 1e6, 1.7e7, 3e9] {
            let state = orbit_state(&o, 0.8, seconds);
            assert_eq!(state.position, position(&o, 0.8, seconds));
            let expected = rate_of(|t| position(&o, 0.8, t), seconds);
            assert!(close(state.velocity, expected, 1e-6), "{seconds}");
        }
        // Earth moves at about 29.8 km/s, and vis-viva holds at periapsis.
        let earth = orbit_state(&orbit(), 1.0, 0.0);
        assert!((radius(earth.velocity) - 29_785.0).abs() < 5.0);
        let periapsis = orbit_state(&o, 1.0, 0.0);
        let r = radius(periapsis.position);
        let vis_viva = (SUN_GM * (2.0 / r - 1.0 / AU_METRES)).sqrt();
        assert!((radius(periapsis.velocity) / vis_viva - 1.0).abs() < 1e-12);
    }

    #[test]
    fn stars_barycentres_and_planets_report_their_velocities() {
        let galaxy = demo_galaxy();
        let mut multiples = 0;
        for system in &galaxy.systems {
            for page in system.pages() {
                multiples += usize::from(system.barycentres_on(page).next().is_some());
                for seconds in [0.0, 3.3e9] {
                    let states = member_states(system, page, seconds);
                    let ids = system
                        .stars_on(page)
                        .map(|s| s.id.as_str())
                        .chain(system.barycentres_on(page).map(|b| b.id.as_str()));
                    for id in ids {
                        let state = member_state(system, page, id, seconds);
                        assert_eq!(find(&states, id), Some(state), "{id}");
                        let expected = rate_of(|t| member_position(system, page, id, t), seconds);
                        assert!(close_at(state, expected), "{id}");
                    }
                    for planet in system.on_page(page) {
                        let state = planet_state(system, planet, seconds);
                        let expected = rate_of(|t| planet_position(system, planet, t), seconds);
                        assert!(close_at(state, expected), "{}", planet.id);
                    }
                }
            }
        }
        assert!(multiples > 3);
    }
}
