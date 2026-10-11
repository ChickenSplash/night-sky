pub mod cells;
pub mod galaxy;
pub mod light;
pub mod multiple;
pub mod population;
mod star;
pub mod stream;
pub mod stream_wire;
mod system;

use space_shared::{Orbit, Planet, StarSystem};
pub use star::{galactic_star, lifetime_gyr, luminosity, spectral_colour, spin_angle, star};
use std::f64::consts::TAU;
pub use system::*;

pub const AU_METRES: f64 = 149_597_870_700.0;
pub const SUN_RADIUS_METRES: f64 = 695_700_000.0;
pub const LY_METRES: f64 = 9_460_730_472_580_800.0;
pub const YEAR_SECONDS: f64 = 365.256_898_3 * 86_400.0;

pub fn period_seconds(orbit: &Orbit, mass_solar: f64) -> f64 {
    YEAR_SECONDS * (orbit.semi_major_au.powi(3) / mass_solar).sqrt()
}

/// Analytic two-body orbit, returned in metres relative to its star.
/// Orbital elements are generated in the valid elliptic domain (0 <= e < 0.8).
pub fn position(orbit: &Orbit, mass_solar: f64, seconds: f64) -> [f64; 3] {
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
    position_at_eccentric_anomaly(orbit, eccentric)
}

pub fn position_at_eccentric_anomaly(orbit: &Orbit, anomaly: f64) -> [f64; 3] {
    let a = orbit.semi_major_au * AU_METRES;
    let x = a * (anomaly.cos() - orbit.eccentricity);
    let z = a * (1.0 - orbit.eccentricity.powi(2)).sqrt() * anomaly.sin();
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

/// Where a planet is, in metres from its page's outermost barycentre.
pub fn planet_position(system: &StarSystem, planet: &Planet, seconds: f64) -> [f64; 3] {
    let host = member_position(system, planet.page, &planet.host, seconds);
    let local = position(&planet.orbit, system.host_mass(planet), seconds);
    std::array::from_fn(|axis| host[axis] + local[axis])
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

#[derive(Debug)]
pub struct SimulationClock {
    pub seconds: f64,
    pub rate: f64,
    pub paused: bool,
}

impl Default for SimulationClock {
    fn default() -> Self {
        Self {
            seconds: 0.0,
            rate: 86_400.0,
            paused: false,
        }
    }
}

impl SimulationClock {
    pub fn advance(&mut self, real_seconds: f64) {
        if !self.paused {
            self.seconds += real_seconds * self.rate;
        }
    }
}

/// FNV-1a plus an avalanche finalizer, stable across processes and Rust versions.
pub fn stable_hash(text: &str) -> u64 {
    finish_hash(hash_bytes(HASH_START, text.as_bytes()))
}

/// `stable_hash` in parts: the state before any bytes, adding bytes, and the
/// final mix, so a key can be hashed piece by piece without building a string.
pub(crate) const HASH_START: u64 = 0xcbf29ce484222325;

pub(crate) fn hash_bytes(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

pub(crate) fn finish_hash(mut hash: u64) -> u64 {
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d049bb133111eb);
    hash ^ (hash >> 31)
}

pub fn star_position(id: &str) -> [f64; 3] {
    let coord = |axis| {
        let n = stable_hash(&format!("{id}:{axis}"));
        ((n % 1_000_000) as f64 / 1_000_000.0 - 0.5) * 40.0
    };
    [coord('x'), coord('y'), coord('z')]
}

/// Orientation of a plane in galactic axes, stable for its key: (inclination, node,
/// twist) in radians, applied as Ry(node) · Rx(inclination) · Ry(twist). Poles are
/// uniform over the sphere, as real planetary systems are, so some systems orbit
/// retrograde relative to others.
pub fn plane_tilt(key: &str) -> [f64; 3] {
    let h = stable_hash(&format!("{key}:tilt"));
    let unit = |shift: u32| ((h >> shift) & 0xFFFF) as f64 / 65_535.0;
    [(1.0 - 2.0 * unit(0)).acos(), unit(16) * TAU, unit(32) * TAU]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
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
    fn system_poles_are_stable_and_isotropic() {
        let tilt = |page| plane_tilt(&star_id("list", page));
        assert_eq!(tilt(3), tilt(3));
        assert_ne!(tilt(0), tilt(1));
        // Pages of one list are as independent as separate lists.
        let tilts: Vec<_> = (0..4000).map(tilt).collect();
        assert!(tilts.iter().all(|t| (0.0..=PI).contains(&t[0])));
        // Uniform poles have cos(inclination) uniform on [-1, 1].
        let mean = tilts.iter().map(|t| t[0].cos()).sum::<f64>() / 4000.0;
        let square = tilts.iter().map(|t| t[0].cos().powi(2)).sum::<f64>() / 4000.0;
        assert!(mean.abs() < 0.05 && (square - 1.0 / 3.0).abs() < 0.03);
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

    #[test]
    fn speed_changes_and_pause_do_not_teleport() {
        let mut clock = SimulationClock::default();
        clock.advance(1.0);
        clock.rate = 1_000_000.0;
        assert_eq!(clock.seconds, 86400.0);
        clock.paused = true;
        clock.advance(10.0);
        assert_eq!(clock.seconds, 86400.0);
        clock.paused = false;
        clock.advance(1.0);
        assert_eq!(clock.seconds, 1_086_400.0);
    }
}
pub mod glow;
