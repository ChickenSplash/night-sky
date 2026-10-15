//! Presentation layouts never modify orbital elements or simulation time.
use crate::{Navigation, Selection};
use bevy::math::{DVec3, Quat};
use space_shared::{Galaxy, Planet, StarSystem};
use space_simulation::{AU_METRES, LY_METRES, SUN_RADIUS_METRES};

/// Scene layout and preset framing, independent of camera tracking or free flight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationMode {
    #[default]
    Simulation,
    Table,
    Constellation,
}

/// Camera range that frames a page's stars and planets in simulation mode.
pub fn fit_distance(system: &StarSystem, page: u32) -> f64 {
    space_simulation::system_extent_au(system, page).max(3.0) * 3.1 * AU_METRES
}

/// A row of planets to the right of their star, all at physical radii, one row
/// per star or pair they orbit. Spacing scales with the page's largest planet;
/// a large star shows only its right limb. Only the primary is shown.
pub struct TableLayout {
    pub unit: f64,
    first: f64,
    pitch: f64,
    pub left: f64,
    pub right: f64,
    /// Planet hosts, one per row, top to bottom.
    hosts: Vec<String>,
}

impl TableLayout {
    pub fn new(system: &StarSystem, page: u32) -> Self {
        let unit = system
            .on_page(page)
            .map(|p| p.radius_km * 1000.0)
            .fold(1.0e6, f64::max);
        let last = system
            .on_page(page)
            .map(|p| p.orbit_index)
            .max()
            .unwrap_or(0);
        let star = system
            .star(page)
            .map_or(SUN_RADIUS_METRES, |s| s.radius_solar * SUN_RADIUS_METRES);
        let pitch = 3.2 * unit;
        let first = star + 3.0 * unit;
        let right = first + f64::from(last) * pitch + 2.0 * unit;
        let left = if star < 5.0 * unit {
            -star - unit
        } else {
            star - unit
        };
        Self {
            unit,
            first,
            pitch,
            left,
            right,
            hosts: system.hosts(page).into_iter().map(str::to_owned).collect(),
        }
    }

    pub fn position(&self, planet: &Planet) -> DVec3 {
        let column = f64::from(planet.orbit_index);
        let row = self
            .hosts
            .iter()
            .position(|h| *h == planet.host)
            .unwrap_or(0);
        DVec3::new(
            self.first + column * self.pitch,
            0.0,
            row as f64 * self.row_pitch(),
        )
    }

    /// Distance between rows, leaving room for the labels below each.
    fn row_pitch(&self) -> f64 {
        5.0 * self.unit
    }

    /// Distance from the first row to the last.
    pub fn depth(&self) -> f64 {
        self.hosts.len().saturating_sub(1) as f64 * self.row_pitch()
    }
}

impl Navigation {
    /// Constellation pages are selections within one shared camera frame.
    pub fn frame_page(&self, system: &StarSystem) -> u32 {
        if self.presentation_mode == PresentationMode::Constellation {
            system.first_page()
        } else {
            self.page
        }
    }

    pub fn focus_selected(&mut self, lists: &Galaxy) {
        let Some((page, radius_km)) = self
            .system(lists)
            .and_then(|s| s.planets.get(self.selected_planet(lists)?))
            .map(|planet| (planet.page(), planet.radius_km))
        else {
            return;
        };
        self.presentation_mode = PresentationMode::Simulation;
        // A marker clicked mid page swap may belong to the page being wound away.
        self.page = page;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.following = true;
        self.fly = true;
        self.distance = radius_km * 1000.0 * 12.0;
        self.move_speed =
            (self.distance * 0.35).clamp(crate::scene::MIN_SPEED, crate::scene::MAX_SPEED);
    }

    /// Follow the selected craft from the closest zoom, in simulation mode.
    /// It must be in the visited system (`craft::followed`).
    pub fn follow_craft(&mut self) {
        self.presentation_mode = PresentationMode::Simulation;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.following = true;
        self.fly = true;
        self.distance = crate::scene::MIN_DISTANCE;
        self.move_speed =
            (self.distance * 0.35).clamp(crate::scene::MIN_SPEED, crate::scene::MAX_SPEED);
    }

    pub fn show_page(&mut self, page: u32, lists: &Galaxy) {
        let Some(system) = self
            .system(lists)
            .filter(|system| system.pages().contains(&page))
        else {
            return;
        };
        if self.presentation_mode == PresentationMode::Constellation {
            self.select(crate::galaxy_nav::page_pick(system, page).map(Selection::Star));
            self.page = page;
            return;
        }
        let presentation_mode = self.presentation_mode;
        self.page = page;
        self.frame(lists);
        self.presentation_mode = presentation_mode;
    }

    pub fn select_task(&mut self, list: usize, task: usize, lists: &Galaxy) {
        let Some(planet) = lists.systems.get(list).and_then(|s| s.planets.get(task)) else {
            return;
        };
        let presentation_mode = self.presentation_mode;
        self.arrive(crate::Visit::List(list), planet.page(), lists);
        self.presentation_mode = if presentation_mode == PresentationMode::Constellation {
            PresentationMode::Table
        } else {
            presentation_mode
        };
        self.select(Some(Selection::planet(&lists.systems[list], task)));
    }

    pub fn preset(&mut self, presentation_mode: PresentationMode, lists: &Galaxy) {
        if self.visit.is_none() {
            return;
        }
        if presentation_mode == PresentationMode::Simulation {
            self.frame(lists);
        } else {
            self.following = false;
            self.fly = true;
            self.presentation_mode = presentation_mode;
            if presentation_mode != PresentationMode::Constellation
                && self.selected_star().is_some()
            {
                self.selection = None;
            }
        }
    }

    /// Keep a presentation preset framed on the visited system as it changes.
    pub fn fit_preset(&mut self, lists: &Galaxy) {
        let Some((target, distance, top_down)) =
            self.system(lists).and_then(|system| self.framing(system))
        else {
            return;
        };
        self.target = target;
        self.distance = distance;
        if top_down {
            self.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        }
    }

    /// A preset's camera target, range and whether it looks straight down.
    fn framing(&self, system: &StarSystem) -> Option<(DVec3, f64, bool)> {
        match self.presentation_mode {
            PresentationMode::Simulation => None,
            PresentationMode::Table => {
                let layout = TableLayout::new(system, self.page);
                let aspect = self
                    .viewport
                    .as_ref()
                    .map(|v| v.physical_size.x as f64 / v.physical_size.y.max(1) as f64)
                    .unwrap_or(1.0);
                // The width fits every planet at its true radius. Leave vertical
                // room for the row's labels without giving a giant star more space.
                let width = layout.right - layout.left;
                let height = 7.0 * layout.unit + layout.depth();
                let distance = height.max(width / aspect) * 0.5 / self.tan_half_fov();
                Some((
                    DVec3::new(
                        (layout.left + layout.right) * 0.5,
                        0.0,
                        layout.unit + layout.depth() * 0.5,
                    ),
                    distance,
                    true,
                ))
            }
            PresentationMode::Constellation => {
                // One fixed frame for the entire list. Changing which star is
                // selected must not re-centre or rotate the constellation.
                let frame_page = self.frame_page(system);
                let into_page = crate::scene::page_frame(system, frame_page).inverse();
                let stars: Vec<_> = crate::scene::page_offsets(system, frame_page)
                    .into_iter()
                    .map(|offset| into_page * offset)
                    .collect();
                let (low, high) = stars.iter().fold(
                    (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
                    |(low, high), &p| (low.min(p), high.max(p)),
                );
                let target = (low + high) * 0.5;
                let radius = stars
                    .iter()
                    .map(|p| (*p - target).length())
                    .fold(0.0, f64::max)
                    + LY_METRES;
                Some((target, radius / 22.0_f64.to_radians().sin(), false))
            }
        }
    }
}

#[cfg(test)]
mod tests;
