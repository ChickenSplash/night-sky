//! Procedural planetary systems: one per page, generated as a whole so every
//! planet's mass, size and orbit is consistent with its neighbours.
//!
//! - Planet counts vary from `MIN_PLANETS` to `MAX_PLANETS` per system.
//! - Composition follows the snow line: rocky worlds and sub-Neptunes inside it,
//!   gas and ice giants just beyond it, dwarf planets far out.
//! - Radii follow empirical mass-radius relations (after Chen & Kipping 2017),
//!   so sizes run from Pluto to slightly larger than Jupiter.
//! - Neighbours are spaced by mutual Hill radii and never cross orbits, the
//!   criterion that separates long-lived systems from ones that go unstable.
//! - Some pairs sit in mean-motion resonances, with phases that keep their
//!   conjunctions away from the outer planet's periapsis, as for Neptune and Pluto.

use crate::stable_hash;
use space_shared::{Galaxy, MAX_PLANETS, MIN_PLANETS, Orbit, StarSystem};
use std::{collections::HashMap, f64::consts::TAU};

pub const EARTH_MASS_SOLAR: f64 = 3.003_489e-6;
pub const EARTH_RADIUS_KM: f64 = 6_371.0;
pub const JUPITER_MASS_EARTH: f64 = 317.83;

/// Adjacent planets closer than this many mutual Hill radii tend to destabilise
/// within the age of their star (Chambers et al. 1996; Pu & Wu 2015).
const MIN_HILL_SPACING: f64 = 10.0;
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
}

/// SplitMix64: small, fast and identical on every platform.
struct Rng(u64);

impl Rng {
    fn new(key: &str) -> Self {
        Self(stable_hash(key))
    }

    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    fn log_range(&mut self, low: f64, high: f64) -> f64 {
        (low.ln() + (high.ln() - low.ln()) * self.unit()).exp()
    }

    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// Eccentricities and inclinations of real systems are close to Rayleigh.
    fn rayleigh(&mut self, sigma: f64) -> f64 {
        sigma * (-2.0 * (1.0 - self.unit()).ln()).sqrt()
    }
}

/// Number of planets in a page's system.
pub fn page_capacity(list_id: &str, page: u32) -> u32 {
    MIN_PLANETS
        + (stable_hash(&format!("{list_id}:page-{page}:planets"))
            % u64::from(MAX_PLANETS - MIN_PLANETS + 1)) as u32
}

/// Page and orbit index within the page for an append-only list slot.
pub fn page_of_slot(list_id: &str, slot: u32) -> (u32, u32) {
    let (mut page, mut first) = (0, 0);
    loop {
        let capacity = page_capacity(list_id, page);
        if slot < first + capacity {
            return (page, slot - first);
        }
        first += capacity;
        page += 1;
    }
}

/// Main-sequence luminosity in solar units.
pub fn luminosity(mass_solar: f64) -> f64 {
    if mass_solar < 0.43 {
        0.23 * mass_solar.powf(2.3)
    } else {
        mass_solar.powi(4)
    }
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

fn draw_body(rng: &mut Rng, distance_au: f64, snow_au: f64) -> (Class, f64) {
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

/// The whole planetary system of one page, innermost first.
pub fn planetary_system(list_id: &str, page: u32, mass_solar: f64) -> Vec<Body> {
    let mut rng = Rng::new(&format!("{list_id}:page-{page}:system"));
    let light = luminosity(mass_solar).sqrt();
    let snow = 2.7 * light;
    // A few systems form tight resonant chains, like TRAPPIST-1 or Kepler-223.
    let chain = rng.chance(0.12);
    let mut bodies: Vec<Body> = Vec::new();
    for _ in 0..page_capacity(list_id, page) {
        let previous = bodies.last();
        let tentative = match previous {
            Some(p) => p.orbit.semi_major_au * rng.log_range(1.2, 1.9),
            None => rng.log_range(0.04, 0.4) * light.max(0.1),
        };
        let (class, mass) = draw_body(&mut rng, tentative, snow);
        let (e_sigma, i_sigma) = if class == Class::Dwarf {
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
        let radius = radius_earth(mass, class) * rng.range(0.92, 1.1);
        bodies.push(Body {
            orbit: Orbit {
                semi_major_au: semi_major,
                eccentricity,
                inclination_rad: inclination,
                ascending_node_rad: node,
                phase_rad: phase,
            },
            class,
            mass_earth: mass,
            radius_km: radius * EARTH_RADIUS_KM,
            resonance: resonance.map(|(r, _)| r),
        });
    }
    bodies
}

/// Gives every task the page, orbit and physical body of its slot. Deterministic,
/// so repeating it on an already shaped system changes nothing.
pub fn shape(system: &mut StarSystem) {
    let mut systems: HashMap<u32, Vec<Body>> = HashMap::new();
    for task in &mut system.tasks {
        let slot = task.stable_slot();
        let (page, index) = page_of_slot(&system.id, slot);
        let body = &systems
            .entry(page)
            .or_insert_with(|| planetary_system(&system.id, page, system.mass_solar))
            [index as usize];
        task.slot = Some(slot);
        task.page = page;
        task.orbit_index = index;
        task.orbit = body.orbit;
        task.radius_km = body.radius_km;
        task.mass_earth = body.mass_earth;
        task.class = body.class.name().into();
        task.resonance = body.resonance;
    }
}

pub fn shape_galaxy(galaxy: &mut Galaxy) {
    galaxy.systems.iter_mut().for_each(shape);
}

pub fn demo_galaxy() -> Galaxy {
    let mut galaxy = space_shared::demo_fixture();
    shape_galaxy(&mut galaxy);
    galaxy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::period_seconds;

    fn systems() -> impl Iterator<Item = Vec<Body>> {
        (0..400).map(|page| planetary_system("list", page, 1.0))
    }

    #[test]
    fn planet_counts_vary_within_bounds() {
        let counts: Vec<_> = (0..400).map(|page| page_capacity("list", page)).collect();
        assert!(
            counts
                .iter()
                .all(|c| (MIN_PLANETS..=MAX_PLANETS).contains(c))
        );
        for n in MIN_PLANETS..=MAX_PLANETS {
            assert!(counts.contains(&n));
        }
    }

    #[test]
    fn slots_fill_pages_in_order() {
        let mut expected = (0, 0);
        for slot in 0..500 {
            assert_eq!(page_of_slot("list", slot), expected);
            expected.1 += 1;
            if expected.1 == page_capacity("list", expected.0) {
                expected = (expected.0 + 1, 0);
            }
        }
    }

    #[test]
    fn sizes_run_from_pluto_to_just_past_jupiter() {
        let radii: Vec<_> = systems().flatten().map(|b| b.radius_km).collect();
        let (low, high) = radii
            .iter()
            .fold((f64::MAX, 0.0f64), |(l, h), r| (l.min(*r), h.max(*r)));
        assert!((1_000.0..1_400.0).contains(&low), "smallest {low} km");
        assert!((71_492.0..84_000.0).contains(&high), "largest {high} km");
    }

    #[test]
    fn neighbours_are_hill_stable_and_never_cross() {
        for bodies in systems() {
            for pair in bodies.windows(2) {
                let (inner, outer) = (&pair[0], &pair[1]);
                assert!(outer.orbit.semi_major_au > inner.orbit.semi_major_au);
                assert!(stable(
                    inner,
                    outer.orbit.semi_major_au,
                    outer.orbit.eccentricity,
                    outer.mass_earth,
                    1.0
                ));
            }
        }
    }

    #[test]
    fn resonant_pairs_have_exact_periods_and_protected_conjunctions() {
        let mut found = 0;
        for bodies in systems() {
            for pair in bodies.windows(2) {
                let Some([p, q]) = pair[1].resonance else {
                    continue;
                };
                found += 1;
                let (inner, outer) = (&pair[0].orbit, &pair[1].orbit);
                let ratio = period_seconds(outer, 1.0) / period_seconds(inner, 1.0);
                assert!((ratio - f64::from(p) / f64::from(q)).abs() < 1e-9);
                let (p, q) = (f64::from(p), f64::from(q));
                let angle = p * (outer.ascending_node_rad + outer.phase_rad)
                    - q * (inner.ascending_node_rad + inner.phase_rad)
                    - (p - q) * outer.ascending_node_rad;
                assert!((angle.rem_euclid(TAU) - std::f64::consts::PI).abs() < 1e-6);
            }
        }
        assert!(found > 100, "only {found} resonant pairs");
    }

    #[test]
    fn composition_follows_the_snow_line() {
        for body in systems().flatten() {
            if body.orbit.semi_major_au < 2.0 {
                assert!(matches!(body.class, Class::Rocky | Class::SubNeptune));
            }
        }
    }
}
