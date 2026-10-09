//! Presentation layouts never modify orbital elements or simulation time.
use crate::{Navigation, Selection};
use bevy::math::{DVec3, Quat};
use space_shared::{Galaxy, Planet, StarSystem};
use space_simulation::{AU_METRES, LY_METRES, SUN_RADIUS_METRES};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Orbit,
    Table,
    Constellation,
}

/// Camera range that frames a page's planets in orbit view.
pub fn fit_distance(system: &StarSystem, page: u32) -> f64 {
    system
        .on_page(page)
        .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
        .fold(3.0, f64::max)
        * 3.1
        * AU_METRES
}

/// A row of planets to the right of their star, all at physical radii. Spacing
/// scales with the page's largest planet; a large star shows only its right limb.
pub struct TableLayout {
    pub unit: f64,
    first: f64,
    pitch: f64,
    pub left: f64,
    pub right: f64,
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
        }
    }

    pub fn position(&self, planet: &Planet) -> DVec3 {
        let column = f64::from(planet.orbit_index);
        DVec3::new(self.first + column * self.pitch, 0.0, 0.0)
    }
}

impl Navigation {
    /// Constellation pages are selections within one shared camera frame.
    pub fn frame_page(&self, system: &StarSystem) -> u32 {
        if self.view == View::Constellation {
            system.first_page()
        } else {
            self.page
        }
    }

    pub fn focus_selected(&mut self, lists: &Galaxy) {
        let Some((page, radius_km)) = self
            .system(lists)
            .and_then(|s| s.planets.get(self.selected_planet()?))
            .map(|planet| (planet.page(), planet.radius_km))
        else {
            return;
        };
        self.view = View::Orbit;
        // A marker clicked mid page swap may belong to the page being wound away.
        self.page = page;
        self.rotation = Quat::from_rotation_y(0.3) * Quat::from_rotation_x(-0.85);
        self.following = true;
        self.fly = true;
        self.distance = radius_km * 1000.0 * 12.0;
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
        if self.view == View::Constellation {
            self.select(crate::galaxy_nav::page_pick(system, page).map(Selection::Star));
            self.page = page;
            return;
        }
        let view = self.view;
        self.page = page;
        self.frame(lists);
        self.view = view;
    }

    pub fn select_task(&mut self, list: usize, task: usize, lists: &Galaxy) {
        let Some(planet) = lists.systems.get(list).and_then(|s| s.planets.get(task)) else {
            return;
        };
        let view = self.view;
        self.arrive(crate::Visit::List(list), planet.page(), lists);
        self.view = if view == View::Constellation {
            View::Table
        } else {
            view
        };
        self.selection = Some(Selection::Planet(task));
    }

    pub fn preset(&mut self, view: View, lists: &Galaxy) {
        if self.visit.is_none() {
            return;
        }
        if view == View::Orbit {
            self.frame(lists);
        } else {
            self.following = false;
            self.fly = true;
            self.view = view;
            if view != View::Constellation && self.selected_star().is_some() {
                self.selection = None;
            }
        }
    }

    /// Keep a preset view framed on the visited system as it changes.
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
        match self.view {
            View::Orbit => None,
            View::Table => {
                let layout = TableLayout::new(system, self.page);
                let aspect = self
                    .viewport
                    .as_ref()
                    .map(|v| v.physical_size.x as f64 / v.physical_size.y.max(1) as f64)
                    .unwrap_or(1.0);
                // The width fits every planet at its true radius. Leave vertical
                // room for the row's labels without giving a giant star more space.
                let width = layout.right - layout.left;
                let height = 7.0 * layout.unit;
                let distance = height.max(width / aspect) * 0.5 / self.tan_half_fov();
                Some((
                    DVec3::new((layout.left + layout.right) * 0.5, 0.0, layout.unit),
                    distance,
                    true,
                ))
            }
            View::Constellation => {
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
mod tests {
    use super::*;
    use space_simulation::{planet_count, system_page_of_slot};

    #[test]
    fn constellation_pages_share_one_frame_without_reframing() {
        let galaxy = space_simulation::demo_galaxy();
        let system = &galaxy.systems[0];
        let pages = system.pages();
        assert!(pages.len() > 2);
        let mut nav = Navigation::default();
        nav.select_list(0, &galaxy);
        nav.fit_preset(&galaxy);
        let (target, distance, frame) = (nav.target, nav.distance, nav.frame_page(system));
        nav.rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.4);
        let rotated = nav.rotation;
        nav.fly = false;
        for &page in pages.iter().skip(1).take(2) {
            nav.show_page(page, &galaxy);
            nav.fit_preset(&galaxy);
            assert_eq!(nav.view, View::Constellation);
            assert_eq!(
                nav.selected_star(),
                crate::galaxy_nav::page_pick(system, page).as_ref()
            );
            assert_eq!(nav.frame_page(system), frame);
            assert_eq!((nav.target, nav.distance), (target, distance));
            assert_eq!(nav.rotation, rotated);
            assert!(!nav.fly);
        }
    }

    fn large_galaxy() -> Galaxy {
        let mut galaxy = space_simulation::demo_galaxy();
        let template = galaxy.systems[0].planets[0].task.clone().unwrap();
        let tasks = (0..50)
            .map(|slot| space_shared::Task {
                id: format!("task-{slot}"),
                slot,
                ..template.clone()
            })
            .collect();
        space_simulation::place(&mut galaxy.systems[0], tasks);
        galaxy
    }

    #[test]
    fn fifty_tasks_have_stable_pages_after_edits_and_removals() {
        let mut galaxy = large_galaxy();
        let system = &mut galaxy.systems[0];
        let last = system_page_of_slot(system, 49).0;
        assert!(system.pages().starts_with(&(0..=last).collect::<Vec<_>>()));
        assert_eq!(system.pages().len(), system.page_stars.len());
        for page in 0..last {
            assert_eq!(
                system.on_page(page).count() as u32,
                planet_count(&system.page_star(page).unwrap().id)
            );
        }
        let before = system.planets.clone();
        system.planets.reverse();
        system
            .planets
            .retain(|p| p.task.as_ref().is_none_or(|t| t.slot != 12));
        for task in system.planets.iter_mut().filter_map(|p| p.task.as_mut()) {
            task.status = "complete".into();
            task.due_date_ms = Some(123);
        }
        space_simulation::shape(system);
        // Every planet keeps its body; the removed task leaves a bare planet.
        assert_eq!(system.planets.len(), before.len());
        for planet in &system.planets {
            let original = before.iter().find(|p| p.id == planet.id).unwrap();
            assert_eq!(
                (planet.orbit, planet.page, planet.orbit_index),
                (original.orbit, original.page, original.orbit_index)
            );
            let Some(task) = &planet.task else {
                assert!(original.task.as_ref().is_none_or(|t| t.slot == 12));
                continue;
            };
            assert_eq!(task.id, original.task.as_ref().unwrap().id);
            let slot: u32 = task.id.strip_prefix("task-").unwrap().parse().unwrap();
            assert_eq!(planet.page(), system_page_of_slot(system, slot).0);
        }
    }

    #[test]
    fn table_uses_true_sizes_and_fits_the_star() {
        let galaxy = large_galaxy();
        let system = &galaxy.systems[0];
        let layout = TableLayout::new(system, 1);
        let columns: Vec<_> = system.on_page(1).map(|p| layout.position(p).x).collect();
        assert_eq!(
            columns.len() as u32,
            planet_count(&system.page_star(1).unwrap().id)
        );
        assert!(columns.windows(2).all(|r| r[1] - r[0] > 2.0 * layout.unit));
        assert!(system.on_page(1).all(|p| layout.position(p).z == 0.0));
        // A large star shows only its right tip; a small one fits whole.
        let star = system.star(1).unwrap().radius_solar * SUN_RADIUS_METRES;
        if star >= 5.0 * layout.unit {
            assert!(((star - layout.left) / layout.unit - 1.0).abs() < 1e-10);
        } else {
            assert!(layout.left < -star);
        }
        assert!(columns[0] - layout.unit > star);
        assert!(columns[columns.len() - 1] + layout.unit < layout.right);
        let mut nav = Navigation {
            page: 1,
            visit: Some(crate::Visit::List(0)),
            ..Navigation::default()
        };
        nav.preset(View::Table, &galaxy);
        nav.fit_preset(&galaxy);
        assert!(
            nav.distance < 3.0 * (layout.right - layout.left) && nav.distance > 3.0 * layout.unit
        );
    }

    #[test]
    fn table_frames_every_planet_for_dwarfs_and_giants() {
        let mut galaxy = large_galaxy();
        let page = 1;
        for radius_solar in [0.01, 1.0, 1000.0] {
            galaxy.systems[0]
                .stars
                .iter_mut()
                .find(|star| star.page == page)
                .unwrap()
                .radius_solar = radius_solar;
            let system = &galaxy.systems[0];
            let layout = TableLayout::new(system, page);
            let nav = Navigation {
                page,
                visit: Some(crate::Visit::List(0)),
                view: View::Table,
                ..Navigation::default()
            };
            let (target, distance, top_down) = nav.framing(system).unwrap();
            assert!(top_down);
            assert!(distance.is_finite());
            // Default framing uses a square viewport and a 50° vertical FOV.
            let half_width = distance * 25.0_f64.to_radians().tan();
            let tolerance = layout.unit * 1e-8;
            assert!(layout.left + tolerance >= target.x - half_width);
            assert!(layout.right - tolerance <= target.x + half_width);
            for planet in system.on_page(page) {
                let x = layout.position(planet).x;
                let radius = planet.radius_km * 1000.0;
                assert!(x - radius > layout.left);
                assert!(x + radius < layout.right);
            }
        }
    }

    #[test]
    fn presets_and_search_find_off_page_tasks() {
        let galaxy = large_galaxy();
        let mut nav = Navigation::default();
        nav.select_list(0, &galaxy);
        nav.select_task(0, 42, &galaxy);
        let page = galaxy.systems[0].planets[42].page;
        assert!(page > 0);
        assert_eq!(nav.page, page);
        assert_eq!(nav.selected_planet(), Some(42));
        assert!(nav.view == View::Table);
        nav.fit_preset(&galaxy);
        assert!(nav.distance.is_finite());
        nav.preset(View::Orbit, &galaxy);
        assert_eq!(nav.page, page);
        assert!(!nav.following);
    }

    #[test]
    fn one_selection_replaces_the_other_and_following_stays_with_planets() {
        let galaxy = space_simulation::demo_galaxy();
        let mut nav = Navigation::default();
        nav.select_list(0, &galaxy);
        assert!(nav.selected_star().is_some());
        nav.select(Some(Selection::Planet(0)));
        assert_eq!(nav.selected_star(), None);
        nav.following = true;
        nav.select(Some(Selection::Planet(1)));
        assert!(nav.following);
        nav.select(crate::galaxy_nav::page_pick(&galaxy.systems[0], 0).map(Selection::Star));
        assert_eq!(nav.selected_planet(), None);
        assert!(!nav.following);
    }
}
