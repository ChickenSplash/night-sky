//! Bodies flattened by their spin. A body round under its own gravity settles
//! into an oblate spheroid, widest at the equator, whose surface is one of
//! equal gravity and spin potential: water at height zero lies flat on it.
//!
//! - Planets follow the Darwin-Radau relation, which allows for mass
//!   concentrated towards the centre through the moment of inertia factor.
//! - Stars follow the Roche model, all their mass at the centre: the equator
//!   widens up to 1.5 times the pole at break-up.
//! - A body's generated radius stays its mean radius, the sphere of the same
//!   volume, so its density is unchanged.
//! - Generation keeps spins slower than `MAX_FLATTENING` allows
//!   (`fastest_planet_hours`, `fastest_star_hours`).

use crate::{Class, EARTH_SOLAR, SUN_GM, SUN_RADIUS_METRES};
use glam::DVec3;
use space_shared::{Planet, Spin, Star};
use std::f64::consts::TAU;

/// The flattest a generated body may be. A star this flat spins at about 0.93
/// of its break-up speed; a fluid planet flatter than this, like the dwarf
/// planet Haumea, no longer keeps a spheroid's shape.
pub const MAX_FLATTENING: f64 = 0.2;

/// An oblate spheroid, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spheroid {
    pub equatorial: f64,
    pub polar: f64,
}

impl Spheroid {
    /// `(equatorial - polar) / equatorial`: zero for a sphere.
    pub fn flattening(&self) -> f64 {
        1.0 - self.polar / self.equatorial
    }

    /// The point of the surface in a unit `direction` from the centre, in the
    /// body's axes: the pole is +Y.
    pub fn along(&self, direction: DVec3) -> DVec3 {
        let (a, c) = (self.equatorial, self.polar);
        let across = direction.x * direction.x + direction.z * direction.z;
        direction / (across / (a * a) + direction.y * direction.y / (c * c)).sqrt()
    }

    /// The outward normal at a point of the surface: "up" there.
    pub fn normal(&self, point: DVec3) -> DVec3 {
        let (a2, c2) = (self.equatorial.powi(2), self.polar.powi(2));
        DVec3::new(point.x / a2, point.y / c2, point.z / a2).normalize()
    }

    /// The spheroid of this mean radius and flattening.
    pub fn of(mean_radius: f64, flattening: f64) -> Self {
        let equatorial = mean_radius * (1.0 - flattening).powf(-1.0 / 3.0);
        Self {
            equatorial,
            polar: equatorial * (1.0 - flattening),
        }
    }
}

impl Class {
    /// From `Class::name`, as a planet carries it.
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Class::Dwarf,
            Class::Rocky,
            Class::SubNeptune,
            Class::IceGiant,
            Class::GasGiant,
        ]
        .into_iter()
        .find(|class| class.name() == name)
    }

    /// Moment of inertia over $Ma^2$: 0.4 for uniform density, less the more
    /// the mass gathers at the centre. Representative values (Earth 0.331,
    /// Jupiter 0.254, Uranus 0.23). Legacy records only: new generation supplies
    /// its composition-derived factor explicitly.
    fn inertia(self) -> f64 {
        match self {
            Class::Dwarf => 0.35,
            Class::Rocky => 0.33,
            Class::SubNeptune => 0.26,
            Class::IceGiant => 0.23,
            Class::GasGiant => 0.25,
        }
    }
}

fn response(inertia: f64) -> f64 {
    2.5 / (1.0 + 6.25 * (1.0 - 1.5 * inertia).powi(2))
}

/// Spin rate squared, in rad² s⁻², or zero for a body that does not turn.
fn spin_rate_squared(period_hours: f64) -> f64 {
    if period_hours > 0.0 {
        (TAU / (period_hours * 3_600.0)).powi(2)
    } else {
        0.0
    }
}

/// A planet's flattening from its mean radius: Darwin-Radau with $a$ from
/// the mean radius, $f(1 - f) = K\,\omega^2 R^3 / GM$.
pub(crate) fn planet_flattening(
    inertia: f64,
    mass_earth: f64,
    radius_km: f64,
    period_hours: f64,
) -> f64 {
    let gm = mass_earth * EARTH_SOLAR * SUN_GM;
    let radius = radius_km * 1_000.0;
    let spin = response(inertia) * spin_rate_squared(period_hours) * radius.powi(3) / gm;
    (0.5 * (1.0 - (1.0 - 4.0 * spin).max(0.0).sqrt())).min(MAX_FLATTENING)
}

/// A star's flattening. In the Roche model the equator sits at
/// $R_e / R_p = 1 / (1 - W)$, $W = \omega^2 R^3 / 2GM$ with $R$ the mean
/// radius, so the flattening is $W$ itself.
pub(crate) fn star_flattening(mass_solar: f64, radius_solar: f64, period_hours: f64) -> f64 {
    let radius = radius_solar * SUN_RADIUS_METRES;
    let w = spin_rate_squared(period_hours) * radius.powi(3) / (2.0 * mass_solar * SUN_GM);
    w.min(MAX_FLATTENING)
}

/// Physical inertia for generated planets; isolated legacy class fallback only
/// for records predating composition-based generation or invalid wire values.
pub fn planet_shape(planet: &Planet) -> Spheroid {
    let inertia = planet
        .inertia_factor
        .filter(|k| k.is_finite() && (0.1..=0.4).contains(k))
        .unwrap_or_else(|| {
            Class::from_name(&planet.class)
                .unwrap_or(Class::Rocky)
                .inertia()
        });
    let radius = planet.radius_km * 1_000.0;
    Spheroid::of(
        radius,
        planet_flattening(
            inertia,
            planet.mass_earth,
            planet.radius_km,
            planet.spin.period_hours,
        ),
    )
}

pub fn star_shape(star: &Star) -> Spheroid {
    Spheroid::of(
        star.radius_solar * SUN_RADIUS_METRES,
        star_flattening(star.mass_solar, star.radius_solar, star.spin.period_hours),
    )
}

/// The shortest day a planet may have: where it reaches `MAX_FLATTENING`.
pub(crate) fn fastest_planet_hours(inertia: f64, mass_earth: f64, radius_km: f64) -> f64 {
    let gm = mass_earth * EARTH_SOLAR * SUN_GM;
    let spin = MAX_FLATTENING * (1.0 - MAX_FLATTENING) / response(inertia);
    TAU / (spin * gm / (radius_km * 1_000.0).powi(3)).sqrt() / 3_600.0
}

/// The shortest day a star may have: where it reaches `MAX_FLATTENING`.
pub(crate) fn fastest_star_hours(mass_solar: f64, radius_solar: f64) -> f64 {
    let radius = radius_solar * SUN_RADIUS_METRES;
    TAU / (2.0 * MAX_FLATTENING * mass_solar * SUN_GM / radius.powi(3)).sqrt() / 3_600.0
}

/// A star's spin no faster than its shape allows.
pub(crate) fn star_spin(spin: Spin, mass_solar: f64, radius_solar: f64) -> Spin {
    Spin {
        period_hours: spin
            .period_hours
            .max(fastest_star_hours(mass_solar, radius_solar)),
        ..spin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(value: f64, expected: f64, share: f64) {
        assert!(
            (value / expected - 1.0).abs() < share,
            "{value} is not within {share} of {expected}"
        );
    }

    #[test]
    fn solar_system_flattening() {
        // Mean radius, mass, sidereal day and the measured flattening.
        let earth = planet_flattening(0.33, 1.0, 6_371.0, 23.934);
        close(earth, 1.0 / 298.26, 0.03);
        let jupiter = planet_flattening(0.25, 317.83, 69_911.0, 9.925);
        close(jupiter, 0.0649, 0.05);
        let neptune = planet_flattening(0.23, 17.15, 24_622.0, 16.11);
        close(neptune, 0.0171, 0.1);
        // Saturn's mass is more concentrated (0.21) than the gas giants'
        // 0.25 allows for, so it comes out flatter than its 0.098.
        let saturn = planet_flattening(0.25, 95.16, 58_232.0, 10.656);
        close(saturn, 0.098, 0.2);
        // The Sun's 25.4 day equator: about 9e-6 is measured.
        let sun = star_flattening(1.0, 1.0, 25.4 * 24.0);
        assert!((5e-6..2e-5).contains(&sun), "{sun}");
        // Altair: 1.86 solar masses, a mean radius of 1.89 solar and 8.9
        // hours; its equator is 2.03 solar radii and its pole 1.63.
        close(star_flattening(1.86, 1.89, 8.9), 0.197, 0.15);
    }

    #[test]
    fn mean_radius_keeps_the_volume() {
        let shape = Spheroid::of(1_000.0, 0.1);
        close(shape.equatorial.powi(2) * shape.polar, 1e9, 1e-12);
        close(shape.flattening(), 0.1, 1e-12);
    }

    #[test]
    fn fastest_spins_reach_the_limit() {
        for inertia in [0.12, 0.25, 0.33, 0.35] {
            let hours = fastest_planet_hours(inertia, 2.0, 9_000.0);
            close(
                planet_flattening(inertia, 2.0, 9_000.0, hours),
                MAX_FLATTENING,
                1e-9,
            );
            assert!(planet_flattening(inertia, 2.0, 9_000.0, hours * 1.01) < MAX_FLATTENING);
        }
        let hours = fastest_star_hours(3.0, 2.5);
        close(star_flattening(3.0, 2.5, hours), MAX_FLATTENING, 1e-9);
    }

    #[test]
    fn surface_points_lie_on_the_spheroid() {
        let shape = Spheroid::of(6_371_000.0, 1.0 / 298.0);
        let (a, c) = (shape.equatorial, shape.polar);
        for direction in [DVec3::X, DVec3::Y, DVec3::new(1.0, 2.0, -3.0).normalize()] {
            let p = shape.along(direction);
            close(
                (p.x * p.x + p.z * p.z) / (a * a) + p.y * p.y / (c * c),
                1.0,
                1e-12,
            );
            assert!(p.normalize().dot(direction) > 1.0 - 1e-12);
        }
        close(shape.along(DVec3::Y).y, c, 1e-12);
        close(shape.along(DVec3::Z).z, a, 1e-12);
        // At 45 degrees of latitude the normal leans poleward of the
        // direction from the centre, as geodetic latitude does on Earth.
        let p = shape.along(DVec3::new(1.0, 1.0, 0.0).normalize());
        assert!(shape.normal(p).y > p.normalize().y);
    }

    #[test]
    fn classes_round_trip_by_name() {
        for class in [
            Class::Dwarf,
            Class::Rocky,
            Class::SubNeptune,
            Class::IceGiant,
            Class::GasGiant,
        ] {
            assert_eq!(Class::from_name(class.name()), Some(class));
        }
    }

    #[test]
    fn transported_physical_inertia_overrides_labels_and_legacy_is_unknown() {
        let mut planet = Planet {
            mass_earth: 5.0,
            radius_km: 10_000.0,
            inertia_factor: Some(0.24),
            spin: Spin {
                period_hours: 12.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let expected = planet_shape(&planet);
        for class in ["Rocky planet", "Gas giant", "unknown label"] {
            planet.class = class.into();
            assert_eq!(planet_shape(&planet), expected);
        }
        let mut wire = serde_json::to_value(&planet).unwrap();
        wire.as_object_mut().unwrap().remove("inertia_factor");
        let legacy: Planet = serde_json::from_value(wire).unwrap();
        assert_eq!(legacy.inertia_factor, None);
    }
}
