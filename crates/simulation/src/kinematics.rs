//! How star systems move around the galaxy: a shared rotation curve, and per
//! component a mean rotation and random scatter (the velocity ellipsoid).
//!
//! - Velocities are galactocentric, in km/s, in the galaxy's axes. The disc
//!   turns clockwise seen from the north pole (+Y), as the Milky Way does, so
//!   home moves toward +Z and the spiral arms trail.
//! - Nothing moves yet: velocities are drawn from each system's own
//!   `{key}:velocity` seed and never shift a position or another draw.

use crate::galaxy::{Component, GalaxyModel};
use crate::system::Rng;

/// G in pc (km/s)² per solar mass.
const G: f64 = 4.300_917e-3;
/// Asymmetric drift: a disc population lags circular rotation by σ_R²/k
/// (Dehnen & Binney 1998).
const DRIFT_KMS: f64 = 80.0;
/// The Sun's motion relative to circular rotation (Schönrich et al. 2010):
/// toward the centre, along rotation and north.
const SOLAR_PECULIAR_KMS: [f64; 3] = [11.1, 12.24, 7.25];

/// The galaxy's circular speed from a spherical mass model: the black hole,
/// Plummer spheres for the nuclear cluster and disc, a Hernquist bar and
/// bulge, and a cored logarithmic term for the disc and dark halo together.
/// It sets the rotation, not the star counts, which come from the density
/// profiles.
#[derive(Clone, Debug, PartialEq)]
pub struct RotationCurve {
    pub nucleus_solar: f64,
    pub nuclear_disc_solar: f64,
    pub nuclear_disc_pc: f64,
    pub bulge_solar: f64,
    pub bulge_pc: f64,
    /// Flat speed far out, and the radius it is approached over.
    pub flat_kms: f64,
    pub core_pc: f64,
}

impl RotationCurve {
    pub fn milky_way() -> Self {
        Self {
            nucleus_solar: 2.5e7,
            nuclear_disc_solar: 1.0e9,
            nuclear_disc_pc: 100.0,
            bulge_solar: 1.5e10,
            bulge_pc: 600.0,
            flat_kms: 230.0,
            core_pc: 3_000.0,
        }
    }
}

/// How a component's mean rotation follows the circular speed.
#[derive(Clone, Copy, Debug)]
enum Rotation {
    /// A disc: circular rotation less its asymmetric drift.
    Drift,
    /// A spheroid: this share of the circular speed.
    Share(f64),
}

/// Mean rotation and dispersions (radial, along rotation, vertical) in km/s,
/// after Bland-Hawthorn & Gerhard (2016) and the references there.
fn motion(component: Component) -> (Rotation, [f64; 3]) {
    match component {
        Component::Nucleus => (Rotation::Share(0.3), [100.0, 100.0, 100.0]),
        Component::Bulge => (Rotation::Share(0.35), [115.0, 100.0, 95.0]),
        // At 10 billion years; younger thin-disc stars are calmer.
        Component::ThinDisc => (Rotation::Drift, [45.0, 30.0, 27.0]),
        Component::Arms => (Rotation::Drift, [12.0, 8.0, 6.0]),
        Component::ThickDisc => (Rotation::Drift, [60.0, 45.0, 40.0]),
        Component::Halo => (Rotation::Share(0.1), [140.0, 80.0, 95.0]),
        Component::NuclearDisc => (Rotation::Drift, [75.0, 70.0, 60.0]),
    }
}

/// Thin-disc heating: dispersions grow with age as t^β, faster vertically
/// (Aumer & Binney 2009).
const HEATING: [f64; 3] = [0.31, 0.31, 0.43];

impl GalaxyModel {
    /// Speed of a circular orbit this far from the axis, in km/s.
    pub fn circular_speed_kms(&self, radius_pc: f64) -> f64 {
        let c = &self.rotation;
        let r = radius_pc.max(1e-3);
        let plummer = |mass: f64, a: f64| mass * r * r / (r * r + a * a).powf(1.5);
        let gravity = self.black_hole_solar / r
            + plummer(c.nucleus_solar, self.nucleus_pc)
            + plummer(c.nuclear_disc_solar, c.nuclear_disc_pc)
            + c.bulge_solar * r / (r + c.bulge_pc).powi(2);
        let halo = c.flat_kms * c.flat_kms * r * r / (r * r + c.core_pc * c.core_pc);
        (G * gravity + halo).sqrt()
    }

    /// Mean rotation and dispersions of a component's stars at a point.
    fn ellipsoid(&self, component: Component, p: [f64; 3], age_gyr: f64) -> (f64, [f64; 3]) {
        let (rotation, mut sigma) = motion(component);
        if component == Component::ThinDisc {
            let heated = (age_gyr.max(0.0) + 0.1) / 10.1;
            sigma = std::array::from_fn(|axis| sigma[axis] * heated.powf(HEATING[axis]));
        }
        if component == Component::Nucleus {
            // The black hole dominates within a few parsecs.
            let r = p.iter().map(|v| v * v).sum::<f64>().sqrt().max(0.01);
            let extra = G * self.black_hole_solar / (3.0 * r);
            sigma = sigma.map(|s| (s * s + extra).sqrt());
        }
        let circular = self.circular_speed_kms(p[0].hypot(p[2]));
        let mean = match rotation {
            Rotation::Drift => (circular - sigma[0] * sigma[0] / DRIFT_KMS).max(0.0),
            Rotation::Share(share) => share * circular,
        };
        (mean, sigma)
    }

    /// A star system's velocity, from its key, component, position in
    /// parsecs and age in billions of years.
    pub fn star_velocity_kms(
        &self,
        key: &str,
        component: Component,
        p: [f64; 3],
        age_gyr: f64,
    ) -> [f64; 3] {
        let (mean, sigma) = self.ellipsoid(component, p, age_gyr);
        let mut rng = Rng::new(&format!("{key}:velocity"));
        let [radial, along, vertical] = sigma.map(|s| s * rng.normal());
        compose(p, radial, mean + along, vertical)
    }

    /// Home's velocity: circular rotation plus the Sun's own motion.
    pub fn home_velocity_kms(&self) -> [f64; 3] {
        let p = self.home_pc;
        let [inward, along, north] = SOLAR_PECULIAR_KMS;
        compose(
            p,
            -inward,
            self.circular_speed_kms(p[0].hypot(p[2])) + along,
            north,
        )
    }
}

/// Radial (outward), rotational and vertical parts in the galaxy's axes.
fn compose(p: [f64; 3], radial: f64, along: f64, vertical: f64) -> [f64; 3] {
    let radius = p[0].hypot(p[2]);
    let (outward, forward) = if radius > 0.0 {
        let [x, z] = [p[0] / radius, p[2] / radius];
        ([x, z], [-z, x])
    } else {
        ([1.0, 0.0], [0.0, 1.0])
    };
    [
        radial * outward[0] + along * forward[0],
        vertical,
        radial * outward[1] + along * forward[1],
    ]
}

/// A star system's galactocentric velocity in km/s, from its key and position
/// in parsecs. Legacy page stars, which have no component, move as thin-disc
/// stars.
pub fn system_velocity_kms(key: &str, position_pc: [f64; 3]) -> [f64; 3] {
    let component = crate::cells::parse_cell_star(key).map_or(Component::ThinDisc, |c| c.1);
    let age = crate::system_age_gyr(key);
    GalaxyModel::milky_way().star_velocity_kms(key, component, position_pc, age)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn length(v: [f64; 3]) -> f64 {
        v.iter().map(|c| c * c).sum::<f64>().sqrt()
    }

    #[test]
    fn the_rotation_curve_rises_then_stays_flat() {
        let galaxy = GalaxyModel::milky_way();
        let v = |r| galaxy.circular_speed_kms(r);
        let home = v(galaxy.home_pc[0]);
        assert!((225.0..240.0).contains(&home), "{home}");
        assert!(v(10.0) < v(100.0) && v(100.0) < v(1_000.0), "rises");
        assert!((160.0..215.0).contains(&v(1_000.0)), "{}", v(1_000.0));
        for r in [5_000.0, 15_000.0, 30_000.0] {
            assert!((v(r) - home).abs() < 15.0, "{r} pc: {}", v(r));
        }
        // Around Sgr A*, Keplerian about the black hole.
        let kepler = (G * galaxy.black_hole_solar / 0.01).sqrt();
        assert!((v(0.01) / kepler - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_spiral_arms_trail_the_rotation() {
        let galaxy = GalaxyModel::milky_way();
        let spiral = &galaxy.spiral;
        let crest = |radius: f64, from: f64, span: f64| {
            (0..4_000)
                .map(|i| from - span + 2.0 * span * i as f64 / 4_000.0)
                .max_by(|&a, &b| {
                    let m =
                        |phi: f64| spiral.modulation(1.0, radius * phi.cos(), radius * phi.sin());
                    m(a).total_cmp(&m(b))
                })
                .unwrap()
        };
        let inner = crest(8_000.0, TAU / 2.0, TAU / 2.0);
        let outer = crest(8_400.0, inner, 0.3);
        // Rotation carries stars toward larger φ = atan2(z, x).
        let p = [8_000.0 * inner.cos(), 0.0, 8_000.0 * inner.sin()];
        let forward = compose(p, 0.0, 1.0, 0.0);
        assert!(forward[2] * inner.cos() - forward[0] * inner.sin() > 0.0);
        // So the outer part of an arm must lie at smaller φ, behind.
        assert!(outer < inner, "{inner} {outer}");
    }

    #[test]
    fn home_moves_like_the_sun() {
        let galaxy = GalaxyModel::milky_way();
        let v = galaxy.home_velocity_kms();
        assert!(v[2] > 230.0 && v[0] < -10.0 && v[1] > 5.0, "{v:?}");
    }

    fn sample(galaxy: &GalaxyModel, component: Component, p: [f64; 3], age: f64) -> Vec<[f64; 3]> {
        (0..20_000)
            .map(|i| galaxy.star_velocity_kms(&format!("kin:{i}"), component, p, age))
            .collect()
    }

    fn mean(values: &[f64]) -> f64 {
        values.iter().sum::<f64>() / values.len() as f64
    }

    fn spread(values: &[f64]) -> f64 {
        let m = mean(values);
        (values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
    }

    #[test]
    fn components_rotate_and_scatter_as_observed() {
        let galaxy = GalaxyModel::milky_way();
        let p = galaxy.home_pc;
        let circular = galaxy.circular_speed_kms(p[0]);
        // At home, x is outward, z along rotation and y north.
        let check = |component, age, lag: (f64, f64), sigma: [(f64, f64); 3]| {
            let v = sample(&galaxy, component, p, age);
            let axis = |i: usize| v.iter().map(|v| v[i]).collect::<Vec<_>>();
            let behind = circular - mean(&axis(2));
            assert!(
                (lag.0..lag.1).contains(&behind),
                "{component:?} lags {behind}"
            );
            assert!(mean(&axis(0)).abs() < 3.0 && mean(&axis(1)).abs() < 3.0);
            for (i, (low, high)) in [0, 2, 1].into_iter().zip(sigma) {
                let s = spread(&axis(i));
                assert!((low..high).contains(&s), "{component:?} axis {i}: {s}");
            }
        };
        check(
            Component::Arms,
            0.1,
            (0.0, 4.0),
            [(11.0, 13.0), (7.0, 9.0), (5.5, 6.5)],
        );
        // Old thin-disc stars are hotter and lag more than young ones.
        check(
            Component::ThinDisc,
            1.0,
            (2.0, 8.0),
            [(20.0, 25.0), (13.0, 17.0), (9.0, 12.0)],
        );
        check(
            Component::ThinDisc,
            10.0,
            (22.0, 30.0),
            [(43.0, 47.0), (28.5, 31.5), (25.5, 28.5)],
        );
        check(
            Component::ThickDisc,
            10.0,
            (40.0, 50.0),
            [(57.0, 63.0), (43.0, 47.0), (38.0, 42.0)],
        );
        check(
            Component::Halo,
            12.0,
            (200.0, 215.0),
            [(133.0, 147.0), (76.0, 84.0), (90.0, 100.0)],
        );
    }

    #[test]
    fn the_centre_is_hot_and_velocities_are_repeatable() {
        let galaxy = GalaxyModel::milky_way();
        let near = sample(&galaxy, Component::Nucleus, [0.2, 0.0, 0.0], 8.0);
        let spread_y = spread(&near.iter().map(|v| v[1]).collect::<Vec<_>>());
        assert!(spread_y > 150.0, "{spread_y}");
        let a = galaxy.star_velocity_kms("g1.2.3", Component::ThinDisc, galaxy.home_pc, 4.0);
        assert_eq!(
            a,
            galaxy.star_velocity_kms("g1.2.3", Component::ThinDisc, galaxy.home_pc, 4.0)
        );
        assert_ne!(
            a,
            galaxy.star_velocity_kms("g1.2.4", Component::ThinDisc, galaxy.home_pc, 4.0)
        );
        // Nearby stars move tens of km/s relative to home, as around the Sun.
        let home = galaxy.home_velocity_kms();
        let relative: Vec<f64> = sample(&galaxy, Component::ThinDisc, galaxy.home_pc, 5.0)
            .iter()
            .map(|v| length(std::array::from_fn(|i| v[i] - home[i])))
            .collect();
        assert!(
            (30.0..70.0).contains(&mean(&relative)),
            "{}",
            mean(&relative)
        );
    }
}
