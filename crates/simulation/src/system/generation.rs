//! Procedural planetary systems: one per star, generated as a whole so every
//! planet's mass, size and orbit is consistent with its neighbours.
//!
//! - Planet counts vary from `MIN_PLANETS` to `MAX_PLANETS` per star or close
//!   pair, fewer where a companion's pull cuts the system short (`multiple`).
//! - Composition follows the snow line: rocky worlds and sub-Neptunes inside it,
//!   gas and ice giants just beyond it, dwarf planets far out.
//! - Radii follow empirical mass-radius relations (after Chen & Kipping 2017),
//!   so sizes run from Pluto to slightly larger than Jupiter.
//! - Neighbours are spaced by mutual Hill radii and never cross orbits, the
//!   criterion that separates long-lived systems from ones that go unstable.
//! - Some pairs sit in mean-motion resonances, with phases that keep their
//!   conjunctions away from the outer planet's periapsis, as for Neptune and Pluto.

use crate::galaxy::Component;
use crate::seed::Rng;
use crate::star::multiple::{Bodies, assemble};
use crate::star::{Formation, census, galactic};
use crate::{period_seconds, stable_hash};
use space_shared::{MAX_PLANETS, MIN_PLANETS, Orbit, Spin};
use std::f64::consts::{PI, TAU};

pub const EARTH_MASS_SOLAR: f64 = 3.003_489e-6;
pub const EARTH_RADIUS_KM: f64 = 6_371.0;
pub const JUPITER_MASS_EARTH: f64 = 317.83;

/// Adjacent planets closer than this many mutual Hill radii tend to destabilise
/// within the age of their star (Chambers et al. 1996; Pu & Wu 2015).
const MIN_HILL_SPACING: f64 = 10.0;
/// Heaviest planet as a share of its star. Keeps a pair's mutual Hill radius small
/// enough that the spacing above can always be met further out.
const MAX_MASS_RATIO: f64 = 0.004;
/// Generously beyond the known planets, but short of a loosely bound Oort-cloud
/// object. A full page should not become an interstellar-scale planetary system.
pub(crate) const MAX_SYSTEM_APOAPSIS_AU: f64 = 1_000.0;
/// Period ratios offered for resonant pairs, outer:inner.
const RESONANCES: [[u32; 2]; 6] = [[2, 1], [3, 2], [4, 3], [5, 4], [5, 3], [3, 1]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Dwarf,
    Rocky,
    SubNeptune,
    IceGiant,
    GasGiant,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Dwarf => "Dwarf planet",
            Class::Rocky => "Rocky planet",
            Class::SubNeptune => "Sub-Neptune",
            Class::IceGiant => "Ice giant",
            Class::GasGiant => "Gas giant",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Body {
    pub orbit: Orbit,
    pub class: Class,
    pub mass_earth: f64,
    pub radius_km: f64,
    /// Period ratio with the next planet in, outer:inner.
    pub resonance: Option<[u32; 2]>,
    pub spin: Spin,
}

/// Number of planets that form around a star or close pair, before any
/// companion cuts the system short.
fn host_planet_count(host: &str) -> u32 {
    MIN_PLANETS
        + (stable_hash(&format!("{host}:planets")) % u64::from(MAX_PLANETS - MIN_PLANETS + 1))
            as u32
}

/// Number of planets in a star's whole system, around every star and pair.
pub fn planet_count(star: &str) -> u32 {
    bodies(star).planets.len() as u32
}

/// Empirical radius for a mass in Earth masses, in Earth radii.
fn radius_earth(mass_earth: f64, class: Class) -> f64 {
    match class {
        Class::Dwarf | Class::Rocky => mass_earth.powf(0.27),
        // Volatile envelopes inflate radius steeply with mass up to Saturn's range,
        // then degeneracy pressure keeps giants near Jupiter's size.
        _ if mass_earth < 130.0 => 1.2 * (mass_earth / 2.0).powf(0.55),
        _ => 11.9 * (mass_earth / 130.0).powf(-0.04),
    }
}

/// Giant planets are rarer around lighter stars, roughly in proportion to their
/// mass (Johnson et al. 2010), and no planet exceeds `MAX_MASS_RATIO` of its star.
fn draw_body(rng: &mut Rng, distance_au: f64, snow_au: f64, star_mass: f64) -> (Class, f64) {
    let (class, mass) = draw_class(rng, distance_au, snow_au);
    let (class, mass) = if class == Class::GasGiant && !rng.chance(star_mass.min(1.0)) {
        (Class::IceGiant, rng.log_range(8.0, 25.0))
    } else {
        (class, mass)
    };
    (
        class,
        mass.min(MAX_MASS_RATIO * star_mass / EARTH_MASS_SOLAR),
    )
}

fn draw_class(rng: &mut Rng, distance_au: f64, snow_au: f64) -> (Class, f64) {
    let x = distance_au / snow_au;
    let roll = rng.unit();
    if x < 1.0 {
        if roll < 0.35 {
            (Class::SubNeptune, rng.log_range(2.5, 15.0))
        } else {
            (Class::Rocky, rng.log_range(0.02, 5.0))
        }
    } else if x < 4.0 {
        if roll < 0.4 {
            (
                Class::GasGiant,
                rng.log_range(40.0, 3.2 * JUPITER_MASS_EARTH),
            )
        } else if roll < 0.75 {
            (Class::IceGiant, rng.log_range(8.0, 30.0))
        } else {
            (Class::Rocky, rng.log_range(0.05, 3.0))
        }
    } else if roll < 0.08 {
        (
            Class::GasGiant,
            rng.log_range(40.0, 2.0 * JUPITER_MASS_EARTH),
        )
    } else if roll < 0.38 {
        (Class::IceGiant, rng.log_range(8.0, 25.0))
    } else {
        (Class::Dwarf, rng.log_range(0.0022, 0.01))
    }
}

/// Mutual Hill radius of two neighbours, in AU.
fn mutual_hill(inner: &Body, outer_au: f64, outer_mass: f64, star: f64) -> f64 {
    let a = inner.orbit.semi_major_au;
    ((inner.mass_earth + outer_mass) * EARTH_MASS_SOLAR / (3.0 * star)).cbrt()
        * (a + outer_au)
        * 0.5
}

/// Stable neighbours: enough Hill radii apart, and the outer periapsis stays
/// clear of the inner apoapsis by more than the two-planet Hill limit (2√3).
fn stable(inner: &Body, outer_au: f64, outer_e: f64, outer_mass: f64, star: f64) -> bool {
    let a = inner.orbit.semi_major_au;
    let hill = mutual_hill(inner, outer_au, outer_mass, star);
    outer_au - a >= MIN_HILL_SPACING * hill
        && outer_au * (1.0 - outer_e) - a * (1.0 + inner.orbit.eccentricity)
            >= 2.0 * 3f64.sqrt() * hill
}

/// Everything a star's key generates, for a galactic cell star or a legacy
/// page star.
pub fn bodies(key: &str) -> Bodies {
    crate::galaxy::cells::cell_bodies(key).unwrap_or_else(|| {
        let (formed, origin, mut rng) = census(key);
        assemble(key, formed, origin, &mut rng, None)
    })
}

/// The stars of a star's system, primary first, and its pairs, without its
/// planets.
pub fn system_stars(key: &str) -> (Vec<space_shared::Star>, Vec<space_shared::Barycentre>) {
    crate::galaxy::cells::parse_cell_star(key).map_or_else(
        || {
            let (formed, origin, mut rng) = census(key);
            crate::star::multiple::stars_only(key, formed, origin, &mut rng, None)
        },
        |(cell, component, _)| {
            crate::galaxy::population::tiered_stars(key, component, cell.level())
        },
    )
}

/// A star system's age in billions of years: its companions' age for a
/// multiple, otherwise drawn from its own `{key}:age` seed when its star was
/// drawn without one.
pub fn system_age_gyr(key: &str) -> f64 {
    let (formed, origin, mut draws) = match crate::galaxy::cells::parse_cell_star(key) {
        Some((cell, component, _)) => {
            let (formed, origin, draws, _) =
                crate::galaxy::population::tiered(key, component, cell.level());
            (formed, origin, draws)
        }
        None => census(key),
    };
    let primary = crate::star::multiple::Primary::of(&formed, origin);
    crate::star::multiple::system_age(&mut draws, &primary, &mut Rng::new(&format!("{key}:age")))
}

/// Every planet of one page star's system, by host and innermost first.
pub fn planetary_system(star: &str) -> Vec<Body> {
    bodies(star).planets.into_iter().map(|p| p.body).collect()
}

/// Everything a galactic star of this component generates.
pub fn galactic_bodies(star: &str, component: Component) -> Bodies {
    let (formed, origin, mut rng) = galactic(star, component);
    assemble(star, formed, origin, &mut rng, None)
}

/// Every planet of a galactic star's system, by host and innermost first.
pub fn galactic_planetary_system(star: &str, component: Component) -> Vec<Body> {
    galactic_bodies(star, component)
        .planets
        .into_iter()
        .map(|p| p.body)
        .collect()
}

/// Planets around a star or close pair, innermost first, none reaching past
/// `limit_au` from it.
pub(crate) fn system_within(star: &str, formed: &Formation, limit_au: f64) -> Vec<Body> {
    let ordinary = generate_system(star, formed, false, limit_au);
    if ordinary.last().is_none_or(|body| {
        body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity) <= MAX_SYSTEM_APOAPSIS_AU
    }) {
        ordinary
    } else {
        // A crowded page around an evolved star can otherwise push its outer
        // bodies thousands of AU out. Form a compact, low-mass system instead;
        // reducing mass makes Hill-stable spacing possible within the disc.
        generate_system(star, formed, true, limit_au)
    }
}

fn generate_system(star: &str, formed: &Formation, compact: bool, limit_au: f64) -> Vec<Body> {
    let suffix = if compact { ":compact" } else { "" };
    let mut rng = Rng::new(&format!("{star}:system{suffix}"));
    // Spins draw from their own sequence, so they never move an orbit.
    let mut spins = Rng::new(&format!("{star}:spins"));
    let (mass_solar, snow) = (formed.star.mass_solar, formed.snow_au);
    let light = snow / 2.7;
    // A few systems form tight resonant chains, like TRAPPIST-1 or Kepler-223.
    let chain = !compact && rng.chance(0.12);
    let mut bodies: Vec<Body> = Vec::new();
    for _ in 0..host_planet_count(star) {
        let previous = bodies.last();
        let tentative = match previous {
            Some(p) if compact => p.orbit.semi_major_au * rng.log_range(1.12, 1.22),
            Some(p) => p.orbit.semi_major_au * rng.log_range(1.2, 1.9),
            None if compact => formed.inner_au.max(0.04) * rng.range(1.0, 1.2),
            None => (rng.log_range(0.04, 0.4) * light.max(0.1))
                .max(formed.inner_au * rng.range(1.0, 1.4)),
        };
        let (class, mass) = if compact && tentative < snow {
            (Class::Rocky, rng.log_range(0.02, 0.2))
        } else if compact {
            (Class::Dwarf, rng.log_range(0.0022, 0.01))
        } else {
            draw_body(&mut rng, tentative, snow, mass_solar)
        };
        let (e_sigma, i_sigma) = if compact {
            (0.015, 0.025)
        } else if class == Class::Dwarf {
            (0.12, 0.1)
        } else {
            (0.035, 0.025)
        };
        let eccentricity = rng.rayleigh(e_sigma).min(0.3);
        let inclination = rng.rayleigh(i_sigma).min(0.35);
        let node = rng.range(0.0, TAU);
        let mut phase = rng.range(0.0, TAU);
        let mut semi_major = tentative;
        let mut resonance = None;
        if let Some(inner) = previous {
            let a = inner.orbit.semi_major_au;
            while !stable(inner, semi_major, eccentricity, mass, mass_solar) {
                semi_major *= 1.01;
            }
            let minimum = semi_major / a;
            let wanted = tentative.max(semi_major) / a;
            if chain || rng.chance(0.2) {
                // The resonance closest to the spacing drawn, among those far
                // enough out to be stable.
                resonance = RESONANCES
                    .into_iter()
                    .map(|r| (r, (f64::from(r[0]) / f64::from(r[1])).powf(2.0 / 3.0)))
                    .filter(|(_, ratio)| *ratio >= minimum && *ratio <= wanted * 1.3)
                    .min_by(|x, y| (x.1 - wanted).abs().total_cmp(&(y.1 - wanted).abs()));
                if let Some((_, ratio)) = resonance {
                    semi_major = a * ratio;
                }
            }
            if let Some(([p, q], _)) = resonance {
                // Resonant angle p·λout − q·λin − (p−q)·ϖout held at 180°, so
                // conjunctions fall at the outer apoapsis. With periapsis on the
                // node line, λ = Ω + M and ϖ = Ω.
                let (p, q) = (f64::from(p), f64::from(q));
                let inner_longitude = inner.orbit.ascending_node_rad + inner.orbit.phase_rad;
                phase = ((std::f64::consts::PI + q * inner_longitude + (p - q) * node) / p - node)
                    .rem_euclid(TAU);
            }
        }
        // A companion's pull clears everything beyond its stable zone.
        if semi_major * (1.0 + eccentricity) > limit_au {
            break;
        }
        let radius = radius_earth(mass, class) * rng.range(0.92, 1.1);
        let orbit = Orbit {
            semi_major_au: semi_major,
            eccentricity,
            inclination_rad: inclination,
            ascending_node_rad: node,
            phase_rad: phase,
        };
        bodies.push(Body {
            spin: planet_spin(
                &mut spins,
                (class, mass, radius * EARTH_RADIUS_KM),
                &orbit,
                mass_solar,
            ),
            orbit,
            class,
            mass_earth: mass,
            radius_km: radius * EARTH_RADIUS_KM,
            resonance: resonance.map(|(r, _)| r),
        });
    }
    bodies
}

/// Rotation and axial tilt, after the Solar System and exoplanet tidal theory.
///
/// - Tides lock close-in planets within a few billion years. The locking distance
///   grows as the cube root of the star's mass (Gladman et al. 1996), which is why
///   planets in red dwarfs' habitable zones are locked and Earth is not.
/// - Just beyond it, tides still slow rotation: eccentric orbits settle into a 3:2
///   spin-orbit resonance like Mercury, circular ones spin slowly either way like Venus.
/// - Free planets keep their birth spin: about a day for rocky worlds, ten to twenty
///   hours for giants. Giant impacts leave some tilted far over, like Uranus.
/// - None turns so fast that it would be flatter than `shape::MAX_FLATTENING`.
fn planet_spin(
    rng: &mut Rng,
    (class, mass_earth, radius_km): (Class, f64, f64),
    orbit: &Orbit,
    star_mass: f64,
) -> Spin {
    let orbital_hours = period_seconds(orbit, star_mass) / 3_600.0;
    let a = orbit.semi_major_au;
    let reach = star_mass.cbrt();
    let lock = match class {
        Class::GasGiant | Class::IceGiant => 0.1 * reach,
        Class::SubNeptune => 0.24 * reach,
        Class::Rocky => 0.3 * reach,
        Class::Dwarf => 0.0,
    };
    let isotropic = |rng: &mut Rng| (1.0 - 2.0 * rng.unit()).acos();
    let (period_hours, tilt, locked) = if a < lock {
        (orbital_hours, rng.rayleigh(0.5_f64.to_radians()), true)
    } else if matches!(class, Class::Rocky | Class::SubNeptune) && a < 2.5 * lock {
        if orbit.eccentricity > 0.1 {
            (
                orbital_hours * 2.0 / 3.0,
                rng.rayleigh(0.5_f64.to_radians()),
                false,
            )
        } else {
            let tilt = rng.rayleigh(3_f64.to_radians());
            let tilt = if rng.chance(0.5) { PI - tilt } else { tilt };
            (rng.log_range(1_000.0, 6_000.0), tilt, false)
        }
    } else {
        let hours = match class {
            Class::Rocky => rng.log_range(10.0, 60.0),
            Class::SubNeptune => rng.log_range(10.0, 40.0),
            Class::IceGiant => rng.log_range(12.0, 20.0),
            Class::GasGiant => rng.log_range(9.0, 15.0),
            Class::Dwarf => rng.log_range(4.0, 400.0),
        };
        let tilt = match class {
            Class::Dwarf => isotropic(rng),
            Class::GasGiant | Class::IceGiant if rng.chance(0.85) => {
                rng.rayleigh(18_f64.to_radians())
            }
            Class::Rocky | Class::SubNeptune if rng.chance(0.75) => {
                rng.rayleigh(22_f64.to_radians())
            }
            _ => isotropic(rng),
        };
        let fastest = crate::system::shape::fastest_planet_hours(class, mass_earth, radius_km);
        (hours.max(fastest), tilt, false)
    };
    Spin {
        period_hours,
        tilt_rad: tilt.min(PI),
        azimuth_rad: rng.range(0.0, TAU),
        phase_rad: rng.range(0.0, TAU),
        locked,
    }
}

#[cfg(test)]
mod tests;
