pub mod light;
mod star;
mod system;

use space_shared::Orbit;
pub use star::{luminosity, spectral_colour, spin_angle, star};
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
    let mut hash = text.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
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

/// Each page is its own star system, with its own orbital plane.
pub fn page_tilt(list_id: &str, page: u32) -> [f64; 3] {
    plane_tilt(&format!("{list_id}:page-{page}"))
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
        assert_eq!(page_tilt("list", 3), page_tilt("list", 3));
        assert_ne!(page_tilt("list", 0), page_tilt("list", 1));
        // Pages of one list are as independent as separate lists.
        let tilts: Vec<_> = (0..4000).map(|page| page_tilt("list", page)).collect();
        assert!(tilts.iter().all(|t| (0.0..=PI).contains(&t[0])));
        // Uniform poles have cos(inclination) uniform on [-1, 1].
        let mean = tilts.iter().map(|t| t[0].cos()).sum::<f64>() / 4000.0;
        let square = tilts.iter().map(|t| t[0].cos().powi(2)).sum::<f64>() / 4000.0;
        assert!(mean.abs() < 0.05 && (square - 1.0 / 3.0).abs() < 0.03);
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
