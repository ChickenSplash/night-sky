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
    pub tasks: Vec<TaskPlanet>,
    /// One star per page, derived from the list and page.
    #[serde(default)]
    pub stars: Vec<Star>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Star {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskPlanet {
    /// Append-only list slot. Snapshots older than slots derive it from the
    /// original whole-list orbit.
    #[serde(default)]
    pub slot: Option<u32>,
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    /// ClickUp status group: "open", "custom", "done" or "closed".
    #[serde(default)]
    pub status_type: String,
    /// The status's position on the list's board.
    #[serde(default)]
    pub status_order: u32,
    pub colour: [u8; 3],
    pub assignees: Vec<String>,
    pub due_date_ms: Option<i64>,
    pub url: Option<String>,
    pub radius_km: f64,
    pub orbit: Orbit,
    /// Page (star system) and orbit index within it, derived from the slot.
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub orbit_index: u32,
    #[serde(default)]
    pub mass_earth: f64,
    #[serde(default)]
    pub class: String,
    /// Period ratio with the next planet in, outer:inner.
    #[serde(default)]
    pub resonance: Option<[u32; 2]>,
    #[serde(default)]
    pub spin: Spin,
}

pub const MIN_PLANETS: u32 = 7;
pub const MAX_PLANETS: u32 = 14;

impl TaskPlanet {
    pub fn stable_slot(&self) -> u32 {
        self.slot
            .unwrap_or_else(|| ((self.orbit.semi_major_au - 0.55) / 0.65).round().max(0.0) as u32)
    }

    pub fn page(&self) -> u32 {
        self.page
    }
}

impl StarSystem {
    pub fn pages(&self) -> Vec<u32> {
        let mut pages: Vec<_> = self.tasks.iter().map(TaskPlanet::page).collect();
        pages.sort_unstable();
        pages.dedup();
        if pages.is_empty() {
            pages.push(0);
        }
        pages
    }

    /// The page shown on arrival, without collecting every page.
    pub fn first_page(&self) -> u32 {
        self.tasks.iter().map(TaskPlanet::page).min().unwrap_or(0)
    }

    pub fn star(&self, page: u32) -> Option<&Star> {
        self.stars.iter().find(|s| s.page == page)
    }

    /// Mass of the page's star, in solar masses.
    pub fn mass(&self, page: u32) -> f64 {
        self.star(page).map_or(1.0, |s| s.mass_solar)
    }

    pub fn on_page(&self, page: u32) -> impl Iterator<Item = &TaskPlanet> {
        self.tasks.iter().filter(move |p| p.page() == page)
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

/// Demo tasks without bodies; `space_simulation::demo_galaxy` shapes them.
pub fn demo_fixture() -> Galaxy {
    let lists = [
        (
            "demo-flight",
            "Flight Operations",
            [0.0, 0.0, 0.0],
            [
                "Design the navigation console",
                "Map the outer systems",
                "Calibrate orbital telemetry",
                "Test the jump drive",
                "Review the mission briefing",
                "Prepare launch checklist",
            ],
        ),
        (
            "demo-research",
            "Deep Space Research",
            [3.7, 0.65, -2.1],
            [
                "Survey a new exoplanet",
                "Analyse stellar spectra",
                "Deploy the sensor array",
                "Chart the debris field",
                "Process observatory data",
                "Publish the expedition log",
            ],
        ),
    ];
    Galaxy {
        source: "Demo · fixture tasks".into(),
        systems: lists.into_iter().map(|(id, name, position_ly, names)| {
            StarSystem {
                id: id.into(), name: name.into(), position_ly, stars: Vec::new(),
                tasks: names.into_iter().enumerate().map(|(i, name)| {
                    let (status, status_type, status_order, colour) = match i % 3 {
                        0 => ("in progress", "custom", 1, [92, 213, 242]),
                        1 => ("to do", "open", 0, [169, 154, 238]),
                        _ => ("complete", "done", 2, [100, 220, 169]),
                    };
                    TaskPlanet {
                        slot: Some(i as u32),
                        id: format!("{id}-{i}"), name: name.into(),
                        description: "A fictional task for exploring ClickUp Space. Select a planet, follow its orbit, or jump to another system. Connect your own lists when ready.".into(),
                        status: status.into(), status_type: status_type.into(), status_order, colour, assignees: vec!["Mission control".into()],
                        due_date_ms: None, url: None, radius_km: 0.0, orbit: Orbit::default(),
                        page: 0, orbit_index: 0, mass_earth: 0.0, class: String::new(), resonance: None, spin: Spin::default(),
                    }
                }).collect(),
            }
        }).collect(),
    }
}
