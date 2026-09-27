use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Galaxy {
    pub source: String,
    pub systems: Vec<StarSystem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StarSystem {
    pub id: String,
    pub name: String,
    pub position_ly: [f64; 3],
    /// Every planet of each page with tasks, innermost first; some carry a task.
    pub planets: Vec<Planet>,
    /// One star per page, derived from the list and page.
    #[serde(default)]
    pub stars: Vec<Star>,
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
    /// Stable body ID: its star's ID and orbit index.
    pub id: String,
    /// Catalogue name, such as "Flight Operations 2c".
    pub designation: String,
    /// Page (star system) and orbit index within it.
    pub page: u32,
    pub orbit_index: u32,
    pub orbit: Orbit,
    pub radius_km: f64,
    pub mass_earth: f64,
    pub class: String,
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
    /// ClickUp status group: "open", "custom", "done" or "closed".
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

    pub fn star(&self, page: u32) -> Option<&Star> {
        self.stars.iter().find(|s| s.page == page)
    }

    /// Mass of the page's star, in solar masses.
    pub fn mass(&self, page: u32) -> f64 {
        self.star(page).map_or(1.0, |s| s.mass_solar)
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
