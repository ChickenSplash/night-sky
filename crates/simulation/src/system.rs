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
use crate::multiple::{Bodies, assemble, designation};
use crate::star::{Formation, census, galactic};
use crate::{period_seconds, stable_hash};
use space_shared::{Galaxy, MAX_PLANETS, MIN_PLANETS, Orbit, Planet, Spin, StarSystem, Task};
use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
};

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
const MAX_SYSTEM_APOAPSIS_AU: f64 = 1_000.0;
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

/// SplitMix64: small, fast and identical on every platform.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(key: &str) -> Self {
        Self(stable_hash(key))
    }

    pub(crate) fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(crate) fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    pub(crate) fn log_range(&mut self, low: f64, high: f64) -> f64 {
        (low.ln() + (high.ln() - low.ln()) * self.unit()).exp()
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// Eccentricities and inclinations of real systems are close to Rayleigh.
    pub(crate) fn rayleigh(&mut self, sigma: f64) -> f64 {
        sigma * (-2.0 * (1.0 - self.unit()).ln()).sqrt()
    }

    /// A standard normal draw (Box–Muller), using two uniforms.
    pub(crate) fn normal(&mut self) -> f64 {
        let radius = (-2.0 * (1.0 - self.unit()).ln()).sqrt();
        radius * (std::f64::consts::TAU * self.unit()).cos()
    }
}

/// The SplitMix64 finaliser: spreads every input bit over the output.
pub(crate) fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// ID of a list page's star, which seeds the star and its planets.
pub fn star_id(list_id: &str, page: u32) -> String {
    format!("{list_id}:page-{page}")
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

/// Number of planets, and so of task slots, in a page's system.
pub fn page_capacity(list_id: &str, page: u32) -> u32 {
    planet_count(&star_id(list_id, page))
}

fn assigned_star(system: &StarSystem, page: u32) -> String {
    system
        .page_star(page)
        .map_or_else(|| star_id(&system.id, page), |s| s.id.clone())
}

/// Each page's generated bodies, worked out once while placing a list.
type Generated = BTreeMap<u32, Bodies>;

fn generated<'a>(system: &StarSystem, cache: &'a mut Generated, page: u32) -> &'a Bodies {
    cache
        .entry(page)
        .or_insert_with(|| bodies(&assigned_star(system, page)))
}

fn slot_page(system: &StarSystem, cache: &mut Generated, slot: u32) -> (u32, u32) {
    let (mut page, mut first) = (0, 0);
    loop {
        let count = generated(system, cache, page).planets.len() as u32;
        if slot < first + count {
            return (page, slot - first);
        }
        first += count;
        page += 1;
    }
}

/// An append-only slot mapped through this list's actual assigned stars.
pub fn system_page_of_slot(system: &StarSystem, slot: u32) -> (u32, u32) {
    slot_page(system, &mut Generated::new(), slot)
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
    crate::cells::cell_bodies(key).unwrap_or_else(|| {
        let (formed, origin, mut rng) = census(key);
        assemble(key, formed, origin, &mut rng, None)
    })
}

/// The stars of a star's system, primary first, and its pairs, without its
/// planets.
pub fn system_stars(key: &str) -> (Vec<space_shared::Star>, Vec<space_shared::Barycentre>) {
    crate::cells::parse_cell_star(key).map_or_else(
        || {
            let (formed, origin, mut rng) = census(key);
            crate::multiple::stars_only(key, formed, origin, &mut rng, None)
        },
        |(cell, component, _)| crate::population::tiered_stars(key, component, cell.level()),
    )
}

/// A star system's age in billions of years: its companions' age for a
/// multiple, otherwise drawn from its own `{key}:age` seed when its star was
/// drawn without one.
pub fn system_age_gyr(key: &str) -> f64 {
    let (formed, origin, mut draws) = match crate::cells::parse_cell_star(key) {
        Some((cell, component, _)) => {
            let (formed, origin, draws, _) =
                crate::population::tiered(key, component, cell.level());
            (formed, origin, draws)
        }
        None => census(key),
    };
    let primary = crate::multiple::Primary::of(&formed, origin);
    crate::multiple::system_age(&mut draws, &primary, &mut Rng::new(&format!("{key}:age")))
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
            spin: planet_spin(&mut spins, class, &orbit, mass_solar),
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
fn planet_spin(rng: &mut Rng, class: Class, orbit: &Orbit, star_mass: f64) -> Spin {
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
        (hours, tilt, false)
    };
    Spin {
        period_hours,
        tilt_rad: tilt.min(PI),
        azimuth_rad: rng.range(0.0, TAU),
        phase_rad: rng.range(0.0, TAU),
        locked,
    }
}

/// Places tasks by slot and fills every page they reach with its star's whole
/// planetary system. Deterministic, so repeating it on an already shaped system
/// changes nothing.
pub fn shape(system: &mut StarSystem) {
    let tasks = std::mem::take(&mut system.planets)
        .into_iter()
        .filter_map(|p| p.task)
        .collect();
    place(system, tasks);
}

/// Replaces a system's planets, stars and barycentres with those of the pages
/// the tasks' slots fall on, or of page 0 when there are no tasks.
pub fn place(system: &mut StarSystem, tasks: Vec<Task>) {
    let mut cache = Generated::new();
    let mut pages: BTreeMap<u32, Vec<Option<Task>>> = BTreeMap::new();
    let empty =
        |cache: &mut Generated, page| vec![None; generated(system, cache, page).planets.len()];
    for task in tasks {
        let (page, index) = slot_page(system, &mut cache, task.slot);
        pages.entry(page).or_insert_with(|| empty(&mut cache, page))[index as usize] = Some(task);
    }
    // A reserved page is still a real star and has its whole planetary system,
    // even when its last task disappears from the latest snapshot.
    for page in system.page_stars.iter().map(|s| s.page) {
        pages.entry(page).or_insert_with(|| empty(&mut cache, page));
    }
    if pages.is_empty() {
        pages.insert(0, empty(&mut cache, 0));
    }
    let mut planets = Vec::new();
    let mut stars = Vec::new();
    let mut barycentres = Vec::new();
    for (page, tasks) in pages {
        let key = assigned_star(system, page);
        let generated = cache.remove(&page).unwrap_or_else(|| bodies(&key));
        let cell = system.page_star(page).is_some();
        for (hosted, task) in generated.planets.into_iter().zip(tasks) {
            // Exoplanet style: the innermost planet is b.
            let designation = if cell {
                crate::cells::planet_designation(&hosted.host, hosted.index)
            } else {
                let name = format!("{} {}", system.name, page + 1);
                designation(&name, "", &hosted.host_letters, hosted.index)
            };
            planets.push(Planet {
                id: hosted.id(),
                designation,
                page,
                orbit_index: hosted.index as u32,
                host: hosted.host,
                orbit: hosted.body.orbit,
                radius_km: hosted.body.radius_km,
                mass_earth: hosted.body.mass_earth,
                class: hosted.body.class.name().into(),
                resonance: hosted.body.resonance,
                spin: hosted.body.spin,
                task,
            });
        }
        stars.extend(
            generated
                .stars
                .into_iter()
                .map(|star| space_shared::Star { page, ..star }),
        );
        barycentres.extend(
            generated
                .barycentres
                .into_iter()
                .map(|pair| space_shared::Barycentre { page, ..pair }),
        );
    }
    system.planets = planets;
    system.stars = stars;
    system.barycentres = barycentres;
}

pub fn shape_galaxy(galaxy: &mut Galaxy) {
    galaxy.systems.iter_mut().for_each(shape);
}

/// Reproducible fictional lists. Stable IDs and slots keep every demo planet in
/// the same orbit across launches, while each list has several page stars.
pub fn demo_galaxy() -> Galaxy {
    use fake::{
        Fake,
        faker::lorem::en::Sentence,
        faker::name::en::Name,
        rand::{SeedableRng, rngs::StdRng},
    };

    let lists = [
        (
            "demo-flight",
            "Flight Operations",
            Some([0.0, 0.0, 0.0]),
            [
                "Design the navigation console",
                "Map the outer systems",
                "Calibrate orbital telemetry",
                "Test the jump drive",
                "Review the mission briefing",
                "Prepare launch checklist",
            ]
            .as_slice(),
            80,
        ),
        (
            "demo-research",
            "Deep Space Research",
            Some([3.7, 0.65, -2.1]),
            [
                "Survey a new exoplanet",
                "Analyse stellar spectra",
                "Deploy the sensor array",
                "Chart the debris field",
                "Process observatory data",
                "Publish the expedition log",
            ]
            .as_slice(),
            74,
        ),
        (
            "demo-observatory",
            "Observatory Network",
            None,
            [].as_slice(),
            84,
        ),
        (
            "demo-expedition",
            "Expedition Planning",
            None,
            [].as_slice(),
            76,
        ),
        (
            "demo-engineering",
            "Station Engineering",
            None,
            [].as_slice(),
            91,
        ),
        (
            "demo-archives",
            "Celestial Archives",
            None,
            [].as_slice(),
            68,
        ),
    ];
    let verbs = [
        "Survey",
        "Calibrate",
        "Map",
        "Review",
        "Test",
        "Deploy",
        "Monitor",
        "Catalogue",
    ];
    let subjects = [
        "sensor array",
        "navigation beacon",
        "spectral atlas",
        "orbital relay",
        "telemetry feed",
        "probe trajectory",
        "stellar catalogue",
        "landing site",
        "signal archive",
        "research module",
        "observation plan",
        "flight computer",
    ];
    let mut rng = StdRng::seed_from_u64(0x5441_534b_5350_4143);
    let mut galaxy = Galaxy {
        source: "Demo · generated fictional tasks".into(),
        systems: Vec::new(),
    };
    for (id, name, position_ly, named, count) in lists {
        let tasks = (0..count)
            .map(|slot| {
                let (status, status_type, status_order, colour) = match slot % 3 {
                    0 => ("in progress", "custom", 1, [92, 213, 242]),
                    1 => ("to do", "open", 0, [169, 154, 238]),
                    _ => ("complete", "done", 2, [100, 220, 169]),
                };
                let (name, description, assignee) = match named.get(slot) {
                    Some(name) => (
                        (*name).to_owned(),
                        "A fictional task for exploring Night Sky. Select a planet, follow its orbit, or jump to another system. Connect your own lists when ready.".to_owned(),
                        "Mission control".to_owned(),
                    ),
                    None => (
                        format!(
                            "{} the {} · sector {}",
                            verbs[(slot + id.len()) % verbs.len()],
                            subjects[(slot / verbs.len() + slot) % subjects.len()],
                            slot / 12 + 1,
                        ),
                        Sentence(8..16).fake_with_rng(&mut rng),
                        Name().fake_with_rng(&mut rng),
                    ),
                };
                Task {
                    slot: slot as u32,
                    id: format!("{id}-{slot}"),
                    name,
                    description,
                    status: status.into(),
                    status_type: status_type.into(),
                    status_order,
                    colour,
                    assignees: vec![assignee],
                    due_date_ms: None,
                    url: None,
                }
            })
            .collect();
        let mut system = StarSystem {
            id: id.into(),
            name: name.into(),
            position_ly: position_ly.unwrap_or_else(|| crate::star_position(id)),
            planets: Vec::new(),
            stars: Vec::new(),
            barycentres: Vec::new(),
            page_stars: Vec::new(),
        };
        place(&mut system, tasks);
        galaxy.systems.push(system);
    }
    // Demo tasks are overlays on the same generated population as task-free stars.
    // Assign without a database: iteration order and the immutable IDs make
    // every placement repeatable, even on an offline first launch.
    let mut sky = crate::cells::Sky::default();
    let mut occupied = std::collections::HashSet::new();
    for system in &mut galaxy.systems {
        let tasks: Vec<_> = system.tasks().map(|(_, task)| task.clone()).collect();
        let highest = tasks.iter().map(|t| t.slot).max();
        assign_page_stars(
            system,
            highest,
            &mut sky,
            |_| Ok(None),
            |_, id| Ok::<_, PageStarError>(occupied.insert(id.to_owned())),
        )
        .expect("enough visible stars near home for demo pages");
        place(system, tasks);
    }
    galaxy
}

#[derive(Debug)]
pub enum PageStarError {
    /// Every visible star in the home cell is taken.
    Exhausted(String),
    /// A recorded assignment names no cell star.
    Invalid(String),
}

impl std::fmt::Display for PageStarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exhausted(list) => write!(f, "no free galactic stars for list {list}"),
            Self::Invalid(star) => write!(f, "invalid assigned star {star}"),
        }
    }
}

impl std::error::Error for PageStarError {}

/// Put each page of a list on a distinct visible star in the home cell, until
/// the pages hold `highest_slot`. `assigned` returns a page's recorded star;
/// `claim` records a new one, refusing stars any page already has. Candidates
/// follow a seeded order per list and page, so picks are repeatable.
pub fn assign_page_stars<E: From<PageStarError>>(
    system: &mut StarSystem,
    highest_slot: Option<u32>,
    sky: &mut crate::cells::Sky,
    mut assigned: impl FnMut(u32) -> Result<Option<String>, E>,
    mut claim: impl FnMut(u32, &str) -> Result<bool, E>,
) -> Result<(), E> {
    let home = crate::galaxy::GalaxyModel::milky_way().home_pc;
    let cell = crate::cells::Cell::containing(10, home).expect("galactic home");
    let count = sky.tier_counts(cell)[cell.level()];
    let reach = crate::population::reach_sq_per_luminosity(crate::population::VISIBLE_MAGNITUDE);
    let offset = |position_pc: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|axis| position_pc[axis] - home[axis])
    };
    system.page_stars.clear();
    let mut capacity = 0;
    let mut page = 0;
    while page == 0 || highest_slot.is_some_and(|slot| slot >= capacity) {
        let star = if let Some(id) = assigned(page)? {
            sky.star(&id).ok_or(PageStarError::Invalid(id))?
        } else {
            let seed = stable_hash(&format!("{}:{page}:galactic-star", system.id));
            let mut chosen = None;
            for attempt in 0..count {
                let candidate = sky
                    .cell_star_at(cell, seed.wrapping_add(attempt) % count)
                    .expect("ordinal within tier");
                let distance_sq: f64 = offset(candidate.position_pc).iter().map(|d| d * d).sum();
                // Assigned stars must appear in the magnitude-limited sky that
                // the client streams and can pick from the galaxy overview, and
                // have planets to hold tasks: a companion can leave none.
                if distance_sq <= candidate.star.luminosity_solar * reach
                    && planet_count(&candidate.id) > 0
                    && claim(page, &candidate.id)?
                {
                    chosen = Some(candidate);
                    break;
                }
            }
            chosen.ok_or_else(|| PageStarError::Exhausted(system.id.clone()))?
        };
        let position_ly =
            offset(star.position_pc).map(|d| d * crate::galaxy::PC_METRES / crate::LY_METRES);
        capacity += planet_count(&star.id);
        system.page_stars.push(space_shared::PageStar {
            page,
            id: star.id,
            position_ly,
        });
        page += 1;
    }
    system.position_ly = system.page_stars[0].position_ly;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::star::{formation, galactic_formation};

    #[test]
    fn demo_has_repeatable_multi_star_constellations() {
        let first = demo_galaxy();
        assert_eq!(first, demo_galaxy());
        assert_eq!(first.systems.len(), 6);
        assert_eq!(
            first
                .systems
                .iter()
                .map(|s| s.tasks().count())
                .sum::<usize>(),
            473
        );
        let mut ids = std::collections::HashSet::new();
        // Multiple-star pages, some with planets around more than one host.
        let pages = || {
            first
                .systems
                .iter()
                .flat_map(|s| s.pages().into_iter().map(move |p| (s, p)))
        };
        assert!(pages().any(|(s, p)| s.stars_on(p).count() > 2));
        assert!(pages().filter(|(s, p)| s.hosts(*p).len() > 1).count() >= 5);
        for system in &first.systems {
            assert!(system.pages().len() >= 4);
            let mut star_pages: Vec<_> = system.stars.iter().map(|s| s.page).collect();
            star_pages.dedup();
            assert_eq!(system.pages(), star_pages);
            for (slot, (_, task)) in system.tasks().enumerate() {
                assert_eq!(task.slot, slot as u32);
                assert!(ids.insert(&task.id));
                assert!(task.url.is_none());
            }
            for assigned in &system.page_stars {
                let cell_star = crate::cells::cell_star(&assigned.id).unwrap();
                let primary = system.star(assigned.page).unwrap();
                assert_eq!(crate::multiple::system_key(&primary.id), cell_star.1.id);
                assert_eq!(primary.mass_solar, cell_star.1.mass_solar);
                assert!(ids.insert(&assigned.id));
            }
        }
    }

    #[test]
    fn every_planet_of_a_tasked_page_exists_and_tasks_keep_their_bodies() {
        let mut system = demo_galaxy().systems.remove(0);
        for page in system.pages() {
            let star = &system.page_star(page).unwrap().id;
            let bodies = crate::cells::cell_bodies(star).unwrap();
            let planets: Vec<_> = system.on_page(page).collect();
            assert_eq!(planets.len(), bodies.planets.len());
            for (planet, hosted) in planets.iter().zip(&bodies.planets) {
                assert_eq!(planet.orbit, hosted.body.orbit);
                assert_eq!(planet.host, hosted.host);
                assert_eq!(planet.id, format!("{}/{}", planet.host, planet.orbit_index));
            }
            assert_eq!(system.star(page).unwrap().id, bodies.stars[0].id);
        }
        // The last page is only partly tasked; the rest are bare planets.
        assert!(system.planets.iter().any(|p| p.task.is_none()));
        let before = system.clone();
        shape(&mut system);
        assert_eq!(system, before);
        // Without tasks, allocated stars and their bare planets remain.
        system.planets.clear();
        shape(&mut system);
        assert_eq!(system.pages().len(), system.page_stars.len());
        assert_eq!(
            system.planets.len() as u32,
            system
                .page_stars
                .iter()
                .map(|s| planet_count(&s.id))
                .sum::<u32>()
        );
    }

    #[test]
    fn systems_are_keyed_by_star() {
        let star = star_id("list", 3);
        assert_eq!(star, "list:page-3");
        assert_eq!(
            planetary_system(&star).len() as u32,
            page_capacity("list", 3)
        );
        assert_eq!(crate::star(&star).id, star);
        assert_ne!(
            planetary_system("elsewhere")[0].orbit,
            planetary_system(&star)[0].orbit
        );
    }

    fn systems() -> impl Iterator<Item = Vec<Body>> {
        (0..400).map(|page| planetary_system(&star_id("list", page)))
    }

    /// Each host's planets on the first 400 legacy pages: the page, the
    /// host's mass, whether it is the primary, and its planets.
    fn hosted() -> impl Iterator<Item = (u32, f64, bool, Vec<Body>)> {
        (0..400).flat_map(|page| {
            let bodies = bodies(&star_id("list", page));
            let primary = bodies.stars[0].id.as_str();
            bodies
                .by_host()
                .into_iter()
                .map(|(host, planets)| {
                    (
                        page,
                        bodies.mass(host),
                        host == primary,
                        planets.into_iter().cloned().collect(),
                    )
                })
                .collect::<Vec<_>>()
        })
    }

    #[test]
    fn planet_counts_vary_within_bounds() {
        let (mut single, mut multiple) = (Vec::new(), Vec::new());
        for page in 0..400 {
            let bodies = bodies(&star_id("list", page));
            let count = bodies.planets.len() as u32;
            if bodies.stars.len() == 1 {
                single.push(count);
            } else {
                multiple.push(count);
            }
        }
        assert!(
            single
                .iter()
                .all(|c| (MIN_PLANETS..=MAX_PLANETS).contains(c))
        );
        for n in MIN_PLANETS..=MAX_PLANETS {
            assert!(single.contains(&n));
        }
        // Companions cut some systems short, and give others planets around
        // more than one star.
        assert!(multiple.iter().any(|&c| c < MIN_PLANETS));
        assert!(multiple.iter().any(|&c| c > MAX_PLANETS));
    }

    #[test]
    fn slots_fill_pages_in_order() {
        let mut expected = (0, 0);
        for slot in 0..500 {
            while page_capacity("list", expected.0) == expected.1 {
                expected = (expected.0 + 1, 0);
            }
            assert_eq!(page_of_slot("list", slot), expected);
            expected.1 += 1;
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
        for (_, star, _, bodies) in hosted() {
            for pair in bodies.windows(2) {
                let (inner, outer) = (&pair[0], &pair[1]);
                assert!(outer.orbit.semi_major_au > inner.orbit.semi_major_au);
                assert!(stable(
                    inner,
                    outer.orbit.semi_major_au,
                    outer.orbit.eccentricity,
                    outer.mass_earth,
                    star
                ));
            }
        }
    }

    #[test]
    fn crowded_pages_stay_within_the_planetary_system_limit() {
        let mut compact_count = 0;
        for list in ["list", "demo-flight", "demo-research"] {
            for page in 0..1_000 {
                let star = star_id(list, page);
                let formed = formation(&star);
                let ordinary = generate_system(&star, &formed, false, f64::INFINITY);
                if ordinary.last().is_some_and(|body| {
                    body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity)
                        > MAX_SYSTEM_APOAPSIS_AU
                }) {
                    compact_count += 1;
                }
                let generated = bodies(&star);
                assert_eq!(generated.planets.len(), page_capacity(list, page) as usize);
                for (host, planets) in generated.by_host() {
                    for body in &planets {
                        let apoapsis = body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity);
                        assert!(
                            apoapsis <= MAX_SYSTEM_APOAPSIS_AU,
                            "{list} page {page}: {apoapsis} AU"
                        );
                        if host == generated.stars[0].id {
                            assert!(body.orbit.semi_major_au >= formed.inner_au);
                        }
                    }
                    for pair in planets.windows(2) {
                        assert!(stable(
                            pair[0],
                            pair[1].orbit.semi_major_au,
                            pair[1].orbit.eccentricity,
                            pair[1].mass_earth,
                            generated.mass(host)
                        ));
                    }
                }
            }
        }
        assert!(compact_count > 0, "compact generation was never exercised");
    }

    #[test]
    fn galactic_stars_of_every_kind_have_stable_bounded_systems() {
        // A sample of every component, and the rare massive stars of the arms.
        let rare = (0..300_000)
            .map(|i| format!("Arms:{i}"))
            .filter(|star| {
                let kind = galactic_formation(star, Component::Arms).star.kind;
                kind.starts_with("Blue") || kind.contains("supergiant")
            })
            .map(|star| (star, Component::Arms));
        let sample = Component::ALL
            .into_iter()
            .flat_map(|c| (0..2_000).map(move |i| (format!("{c:?}:{i}"), c)));
        let mut kinds = std::collections::HashSet::new();
        for (star, component) in sample.chain(rare) {
            let formed = galactic_formation(&star, component);
            kinds.insert(formed.star.kind.clone());
            let generated = galactic_bodies(&star, component);
            for (host, planets) in generated.by_host() {
                for body in &planets {
                    let apoapsis = body.orbit.semi_major_au * (1.0 + body.orbit.eccentricity);
                    assert!(apoapsis <= MAX_SYSTEM_APOAPSIS_AU, "{star}: {apoapsis} AU");
                    if host == generated.stars[0].id {
                        assert!(body.orbit.semi_major_au >= formed.inner_au);
                    }
                }
                for pair in planets.windows(2) {
                    assert!(stable(
                        pair[0],
                        pair[1].orbit.semi_major_au,
                        pair[1].orbit.eccentricity,
                        pair[1].mass_earth,
                        generated.mass(host)
                    ));
                }
            }
        }
        for kind in [
            "Blue main-sequence star",
            "Blue supergiant",
            "Red supergiant",
            "White dwarf",
        ] {
            assert!(kinds.contains(kind), "no {kind} among {kinds:?}");
        }
    }

    #[test]
    fn resonant_pairs_have_exact_periods_and_protected_conjunctions() {
        let mut found = 0;
        for (_, _, _, bodies) in hosted() {
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
        for (page, _, primary, bodies) in hosted() {
            if !primary {
                continue;
            }
            let formed = formation(&star_id("list", page));
            for body in bodies {
                if body.orbit.semi_major_au < 0.7 * formed.snow_au {
                    assert!(matches!(body.class, Class::Rocky | Class::SubNeptune));
                }
                assert!(body.orbit.semi_major_au >= formed.inner_au);
            }
        }
    }

    #[test]
    fn spins_follow_tides_and_class() {
        let (mut locked, mut retrograde, mut total) = (0, 0, 0);
        for bodies in systems() {
            for body in bodies {
                total += 1;
                let spin = body.spin;
                assert!(spin.period_hours > 0.0 && (0.0..=PI).contains(&spin.tilt_rad));
                if spin.locked {
                    locked += 1;
                    assert!(spin.tilt_rad < 0.1);
                } else if matches!(body.class, Class::GasGiant | Class::IceGiant) {
                    assert!((9.0..=20.0).contains(&spin.period_hours));
                }
                if spin.tilt_rad > PI / 2.0 {
                    retrograde += 1;
                }
            }
        }
        assert!(
            locked > total / 20 && locked < total / 2,
            "{locked} of {total} locked"
        );
        assert!(retrograde > 0 && retrograde < total / 4);
    }
}
