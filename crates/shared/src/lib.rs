use serde::{Deserialize, Serialize};

mod atmosphere;
pub use atmosphere::{
    AerosolData, AerosolKind, AtmosphereData, AtmosphereReference, GasFraction, PlanetEnvironment,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Galaxy {
    pub source: String,
    pub systems: Vec<StarSystem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StarSystem {
    pub id: String,
    pub name: String,
    pub position_ly: [f64; 3],
    /// Every planet of each page with tasks, by host and then innermost first;
    /// some carry a task.
    pub planets: Vec<Planet>,
    /// Every star of each page, derived from the list and page. A page's
    /// primary, its brightest star, comes first.
    #[serde(default)]
    pub stars: Vec<Star>,
    /// The pairs of each multiple-star page, innermost first. A single star
    /// has none.
    #[serde(default)]
    pub barycentres: Vec<Barycentre>,
    /// Assignments from this list's pages to generated galactic stars.
    /// Empty for legacy systems.
    #[serde(default)]
    pub page_stars: Vec<PageStar>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageStar {
    pub page: u32,
    pub id: String,
    /// Position relative to the galactic home, in light years.
    pub position_ly: [f64; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Star {
    /// Stable key that seeds the star and its planetary system.
    pub id: String,
    pub page: u32,
    /// Spectral type, such as "G2 V".
    pub spectral: String,
    /// Plain name, such as "Yellow dwarf".
    pub kind: String,
    pub mass_solar: f64,
    pub radius_solar: f64,
    pub luminosity_solar: f64,
    pub temperature_k: f64,
    /// Blackbody colour of the photosphere.
    pub colour: [u8; 3],
    pub spin: Spin,
    /// Letter within a multiple system, such as "B"; empty for a single star.
    #[serde(default)]
    pub component: String,
}

/// Two members of a multiple system orbiting their shared centre of mass. A
/// member is a star or another pair, so triples and quadruples nest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Barycentre {
    /// The system's key, `#`, then the letters of its stars, such as "AB".
    pub id: String,
    pub page: u32,
    /// Letters of the stars it holds, such as "AB".
    pub component: String,
    /// Star or barycentre IDs, heavier first.
    pub members: [String; 2],
    /// Orbit of the second member relative to the first.
    pub orbit: Orbit,
}

/// Rotation about an axis tilted from the orbital plane's pole. A tilt past 90°
/// is retrograde rotation, as for Venus and Uranus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spin {
    pub period_hours: f64,
    pub tilt_rad: f64,
    /// Direction the axis leans towards, around the orbital pole.
    pub azimuth_rad: f64,
    pub phase_rad: f64,
    /// Rotation synchronised with the orbit: one face always towards the star.
    #[serde(default)]
    pub locked: bool,
}

/// A generated planet, with the task mapped onto it, if any.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Planet {
    /// Stable body ID: its host's ID and orbit index.
    pub id: String,
    /// Catalogue name, such as "Flight Operations 2c".
    pub designation: String,
    /// Page (star system) and orbit index around its host.
    pub page: u32,
    pub orbit_index: u32,
    /// The star or barycentre it orbits.
    #[serde(default)]
    pub host: String,
    pub orbit: Orbit,
    pub radius_km: f64,
    pub mass_earth: f64,
    pub class: String,
    /// Generated physical environment; absent in older or hand-built records.
    #[serde(default)]
    pub environment: Option<PlanetEnvironment>,
    /// C/(M a²), composition-based shape input; absent in legacy records.
    #[serde(default)]
    pub inertia_factor: Option<f64>,
    /// Period ratio with the next planet in, outer:inner.
    pub resonance: Option<[u32; 2]>,
    pub spin: Spin,
    pub task: Option<Task>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Append-only list slot; it picks the task's page and planet.
    pub slot: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    /// Status group: "open", "custom", "done" or "closed".
    pub status_type: String,
    /// The status's position on the list's board.
    pub status_order: u32,
    pub colour: [u8; 3],
    pub assignees: Vec<String>,
    pub due_date_ms: Option<i64>,
    pub url: Option<String>,
}

pub const MIN_PLANETS: u32 = 7;
pub const MAX_PLANETS: u32 = 14;

/// Colour of a planet without a task.
pub const UNTASKED_COLOUR: [u8; 3] = [150, 160, 180];

impl Planet {
    pub fn page(&self) -> u32 {
        self.page
    }

    /// The task's name, else the designation.
    pub fn label(&self) -> &str {
        self.task.as_ref().map_or(&self.designation, |t| &t.name)
    }

    pub fn colour(&self) -> [u8; 3] {
        self.task.as_ref().map_or(UNTASKED_COLOUR, |t| t.colour)
    }
}

impl StarSystem {
    pub fn page_star(&self, page: u32) -> Option<&PageStar> {
        self.page_stars.iter().find(|s| s.page == page)
    }
    pub fn pages(&self) -> Vec<u32> {
        let mut pages: Vec<_> = self.planets.iter().map(Planet::page).collect();
        pages.sort_unstable();
        pages.dedup();
        if pages.is_empty() {
            pages.push(0);
        }
        pages
    }

    /// The page shown on arrival, without collecting every page.
    pub fn first_page(&self) -> u32 {
        self.planets.iter().map(Planet::page).min().unwrap_or(0)
    }

    /// The page's primary: its brightest star, and the largest.
    pub fn star(&self, page: u32) -> Option<&Star> {
        self.stars.iter().find(|s| s.page == page)
    }

    pub fn stars_on(&self, page: u32) -> impl Iterator<Item = &Star> {
        self.stars.iter().filter(move |s| s.page == page)
    }

    pub fn barycentres_on(&self, page: u32) -> impl Iterator<Item = &Barycentre> {
        self.barycentres.iter().filter(move |b| b.page == page)
    }

    pub fn barycentre(&self, id: &str) -> Option<&Barycentre> {
        self.barycentres.iter().find(|b| b.id == id)
    }

    /// The pair a star or barycentre belongs to, if any.
    pub fn pair_of(&self, page: u32, member: &str) -> Option<&Barycentre> {
        self.barycentres_on(page)
            .find(|b| b.members.iter().any(|m| m == member))
    }

    /// The page's outermost barycentre, or its star when it is single: the
    /// system as a whole.
    pub fn root(&self, page: u32) -> Option<&str> {
        match self.barycentres_on(page).last() {
            Some(root) => Some(&root.id),
            None => self.star(page).map(|s| s.id.as_str()),
        }
    }

    /// Mass of a star or barycentre (all its stars), in solar masses.
    pub fn member_mass(&self, page: u32, id: &str) -> f64 {
        if let Some(pair) = self.barycentres_on(page).find(|b| b.id == id) {
            return pair.members.iter().map(|m| self.member_mass(page, m)).sum();
        }
        self.stars_on(page)
            .find(|s| s.id == id)
            .or_else(|| self.star(page))
            .map_or(1.0, |s| s.mass_solar)
    }

    /// Mass a planet orbits, in solar masses.
    pub fn host_mass(&self, planet: &Planet) -> f64 {
        self.member_mass(planet.page, &planet.host)
    }

    /// Stars and barycentres from the system as a whole down to this member,
    /// outermost first, the member included.
    pub fn lineage(&self, page: u32, member: &str) -> Vec<String> {
        let mut chain = vec![member.to_owned()];
        while let Some(pair) = self.pair_of(page, chain.last().unwrap()) {
            chain.push(pair.id.clone());
        }
        chain.reverse();
        chain
    }

    /// Hosts with planets on a page, in planet order.
    pub fn hosts(&self, page: u32) -> Vec<&str> {
        let mut hosts: Vec<&str> = Vec::new();
        for planet in self.on_page(page) {
            if !hosts.contains(&planet.host.as_str()) {
                hosts.push(&planet.host);
            }
        }
        hosts
    }

    pub fn on_page(&self, page: u32) -> impl Iterator<Item = &Planet> {
        self.planets.iter().filter(move |p| p.page() == page)
    }

    /// Tasks with their planets' indices.
    pub fn tasks(&self) -> impl Iterator<Item = (usize, &Task)> {
        self.planets
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i, p.task.as_ref()?)))
    }

    /// Index of the planet carrying a task.
    pub fn task_planet(&self, task_id: &str) -> Option<usize> {
        self.tasks().find(|(_, t)| t.id == task_id).map(|(i, _)| i)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Orbit {
    pub semi_major_au: f64,
    pub eccentricity: f64,
    pub inclination_rad: f64,
    pub ascending_node_rad: f64,
    pub phase_rad: f64,
}
