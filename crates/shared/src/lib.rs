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
    pub mass_solar: f64,
    pub tasks: Vec<TaskPlanet>,
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
            1.0,
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
            0.82,
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
        systems: lists.into_iter().map(|(id, name, position_ly, mass_solar, names)| {
            StarSystem {
                id: id.into(), name: name.into(), position_ly, mass_solar,
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
                        page: 0, orbit_index: 0, mass_earth: 0.0, class: String::new(), resonance: None,
                    }
                }).collect(),
            }
        }).collect(),
    }
}
