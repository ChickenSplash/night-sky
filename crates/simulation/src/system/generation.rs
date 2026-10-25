//! Procedural planetary systems: one per star, generated as a whole so every
//! planet's mass, size and orbit is consistent with its neighbours.
//!
//! - Planet counts vary from `MIN_PLANETS` to `MAX_PLANETS` per star or close
//!   pair, fewer where a companion's pull cuts the system short (`multiple`).
//! - Material inventories follow continuous formation-location priors.
//! - Retained mass and composition determine reference radii, then descriptions.
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

mod materials;
pub(crate) mod retention;
use super::{
    composition::Budget,
    structure::{self, Structure},
};
use crate::units::EARTH_MASS_KG;
pub use retention::RetentionSolve;

pub const EARTH_MASS_SOLAR: f64 = crate::units::EARTH_SOLAR;
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
    pub budget: Budget,
    pub structure: Structure,
    /// Proposed formation location, never redrawn after orbital spacing.
    pub formation_distance_au: f64,
    /// Conservative pre-escape spacing mass.
    pub formation_mass_earth: f64,
    pub retention_solve: RetentionSolve,
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

/// Mutual Hill radius of two neighbours, in AU.
fn mutual_hill(inner: &Body, outer_au: f64, outer_mass: f64, star: f64) -> f64 {
    let a = inner.orbit.semi_major_au;
    ((inner.mass_earth + outer_mass) * EARTH_MASS_SOLAR / (3.0 * star)).cbrt()
        * (a + outer_au)
        * 0.5
}

/// Stable neighbours: enough Hill radii apart, and the outer periapsis stays
/// clear of the inner apoapsis by more than the two-planet Hill limit (2√3).
pub(crate) fn stable(
    inner: &Body,
    outer_au: f64,
    outer_e: f64,
    outer_mass: f64,
    star: f64,
) -> bool {
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
        let budget = materials::draw(&mut rng, tentative, snow, mass_solar, compact);
        let mass = budget.retained().total_mass_kg() / EARTH_MASS_KG;
        let (e_sigma, i_sigma) = if compact {
            (0.015, 0.025)
        } else if mass < 0.02 {
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
            // Solve both Hill inequalities directly. The host-mass cap keeps
            // their denominators positive; no unbounded outward-stepping loop.
            let h = ((inner.formation_mass_earth + mass) * EARTH_MASS_SOLAR / (3.0 * mass_solar))
                .cbrt()
                * 0.5;
            let circular = a * (1.0 + MIN_HILL_SPACING * h) / (1.0 - MIN_HILL_SPACING * h);
            let clearance = 2.0 * 3_f64.sqrt() * h;
            let eccentric =
                a * (1.0 + inner.orbit.eccentricity + clearance) / (1.0 - eccentricity - clearance);
            assert!(
                circular.is_finite() && circular > 0.0 && eccentric.is_finite() && eccentric > 0.0
            );
            semi_major = semi_major.max(circular.max(eccentric) * (1.0 + 1e-12));
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
        // Provisional formation structure. Reconciled using all-star heating only
        // after assembly; not used for orbital placement or inventory draws.
        let physical = structure::derive(
            &budget,
            1361.0 * formed.star.luminosity_solar / semi_major.powi(2),
            5.0,
        )
        .expect("formation draws stay within structure domains");
        let orbit = Orbit {
            semi_major_au: semi_major,
            eccentricity,
            inclination_rad: inclination,
            ascending_node_rad: node,
            phase_rad: phase,
        };
        bodies.push(Body {
            spin: Spin::default(),
            orbit,
            class: materials::provisional_class(&physical),
            mass_earth: mass,
            radius_km: physical.radius_m() / 1000.0,
            resonance: resonance.map(|(r, _)| r),
            budget,
            structure: physical,
            formation_mass_earth: mass,
            formation_distance_au: tentative,
            retention_solve: RetentionSolve::NoEnvelope,
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
fn planet_spin(rng: &mut Rng, body: &Body, star_mass: f64) -> Spin {
    let orbit = &body.orbit;
    let gas = body.structure.h_he_mass_fraction();
    let envelope = (gas / 0.5).clamp(0.0, 1.0);
    let small = ((0.02 - body.mass_earth) / 0.02).clamp(0.0, 1.0);
    let orbital_hours = period_seconds(orbit, star_mass) / 3_600.0;
    let a = orbit.semi_major_au;
    let reach = star_mass.cbrt();
    let lock = (0.3 - 0.2 * envelope) * (1.0 - small) * reach;
    let isotropic = |rng: &mut Rng| (1.0 - 2.0 * rng.unit()).acos();
    let (period_hours, tilt, locked) = if a < lock {
        (orbital_hours, rng.rayleigh(0.5_f64.to_radians()), true)
    } else if gas < 0.1 && a < 2.5 * lock {
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
        let hours = rng.log_range(
            10.0 - envelope - 6.0 * small,
            60.0 - 45.0 * envelope + 340.0 * small,
        );
        let tilt = if rng.chance((0.75 + 0.1 * envelope) * (1.0 - small)) {
            rng.rayleigh((22.0 - 4.0 * envelope).to_radians())
        } else {
            isotropic(rng)
        };
        let fastest = crate::system::shape::fastest_planet_hours(
            body.structure.inertia_factor(),
            body.mass_earth,
            body.radius_km,
        );
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
