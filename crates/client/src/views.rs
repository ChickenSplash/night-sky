//! Presentation layouts never modify orbital elements or simulation time.
use crate::Navigation;
use bevy::math::DVec3;
use space_shared::{Galaxy, StarSystem, TaskPlanet};
use space_simulation::{LY_METRES, SUN_RADIUS_METRES, stable_hash};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Orbit,
    Table,
    Constellation,
}

/// Stars per cubic light year in the solar neighbourhood (about 0.14 per cubic
/// parsec), so neighbouring page stars are typically 3 to 5 ly apart.
pub const STAR_DENSITY_PER_LY3: f64 = 0.004;

/// Page star position relative to its list anchor (metres), scattered at random
/// like real stars. Page `n` lies uniformly within the spherical shell that holds
/// the `n`th star at the local density, in a random direction, so a list of any
/// size has an even, natural spread with no grid, cluster or plane. It depends
/// only on the list and page, never on how many pages are occupied.
pub fn page_position(list_id: &str, page: u32) -> DVec3 {
    if page == 0 {
        return DVec3::ZERO;
    }
    let unit = |axis: &str| {
        (stable_hash(&format!("{list_id}:page-star:{page}:{axis}")) % 1_000_001) as f64
            / 1_000_000.0
    };
    let volume = (f64::from(page) - unit("r")) / STAR_DENSITY_PER_LY3;
    let radius = (volume * 3.0 / (4.0 * std::f64::consts::PI)).cbrt();
    let z = 2.0 * unit("z") - 1.0;
    let phi = std::f64::consts::TAU * unit("phi");
    let ring = (1.0 - z * z).sqrt();
    DVec3::new(ring * phi.cos(), z, ring * phi.sin()) * radius * LY_METRES
}

/// Rows of planets below their star, all at physical radii. Spacing scales with the
/// page's largest planet, so a sun-sized star shows only its limb along the top edge.
pub struct TableLayout {
    pub unit: f64,
    first: f64,
    pitch: f64,
    pub top: f64,
    pub bottom: f64,
}

impl TableLayout {
    pub fn new(system: &StarSystem, page: u32) -> Self {
        let unit = system
            .on_page(page)
            .map(|p| p.radius_km * 1000.0)
            .fold(1.0e6, f64::max);
        let rows = system
            .on_page(page)
            .map(|p| p.orbit_index)
            .max()
            .unwrap_or(0);
        let pitch = 3.2 * unit;
        let first = SUN_RADIUS_METRES + 3.0 * unit;
        let bottom = first + f64::from(rows) * pitch + 2.0 * unit;
        let top = if SUN_RADIUS_METRES < 5.0 * unit {
            -SUN_RADIUS_METRES - unit
        } else {
            SUN_RADIUS_METRES - 2.0 * unit
        };
        Self {
            unit,
            first,
            pitch,
            top,
            bottom,
        }
    }

    pub fn position(&self, task: &TaskPlanet) -> DVec3 {
        let row = f64::from(task.orbit_index);
        DVec3::new(0.0, 0.0, self.first + row * self.pitch)
    }
}

impl Navigation {
    pub fn focus_selected(&mut self, galaxy: &Galaxy) {
        let Some(planet) = galaxy
            .systems
            .get(self.system)
            .and_then(|s| self.selected.and_then(|i| s.tasks.get(i)))
        else {
            return;
        };
        self.view = View::Orbit;
        // A marker clicked mid page swap may belong to the page being wound away.
        self.page = planet.page();
        self.yaw = 0.3;
        self.pitch = 0.85;
        self.atlas = false;
        self.following = true;
        self.fly = true;
        self.distance = planet.radius_km * 1000.0 * 12.0;
    }

    pub fn show_page(&mut self, page: u32, galaxy: &Galaxy) {
        let view = self.view;
        self.page = page;
        self.jump(self.system, galaxy);
        self.view = if view == View::Constellation {
            View::Table
        } else {
            view
        };
    }

    pub fn select_task(&mut self, system: usize, task: usize, galaxy: &Galaxy) {
        let Some(planet) = galaxy.systems.get(system).and_then(|s| s.tasks.get(task)) else {
            return;
        };
        let view = self.view;
        self.jump(system, galaxy);
        self.show_page(planet.page(), galaxy);
        self.view = if view == View::Constellation {
            View::Table
        } else {
            view
        };
        self.selected = Some(task);
    }

    pub fn preset(&mut self, view: View, galaxy: &Galaxy) {
        if view == View::Orbit {
            self.jump(self.system, galaxy);
        } else {
            self.atlas = false;
            self.following = false;
            self.fly = true;
            self.view = view;
        }
    }

    pub fn fit_preset(&mut self, system: &StarSystem) {
        let (low, high) = match self.view {
            View::Orbit => return,
            View::Table => {
                // Planets sit left of centre, leaving room for their labels.
                let layout = TableLayout::new(system, self.page);
                (
                    DVec3::new(-4.0 * layout.unit, 0.0, layout.top),
                    DVec3::new(20.0 * layout.unit, 0.0, layout.bottom),
                )
            }
            View::Constellation => {
                // Stars stay where they are: frame all page stars from the current
                // camera angle. Bounds are in the shown page's plane, like Orbit.
                let into_page = crate::scene::page_frame(system, self.page).inverse();
                let here = crate::scene::page_offset(system, self.page);
                let stars: Vec<_> = system
                    .pages()
                    .iter()
                    .map(|&page| into_page * (crate::scene::page_offset(system, page) - here))
                    .collect();
                let (low, high) = stars.iter().fold(
                    (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN)),
                    |(low, high), &p| (low.min(p), high.max(p)),
                );
                self.target = (low + high) * 0.5;
                let radius = stars
                    .iter()
                    .map(|p| (*p - self.target).length())
                    .fold(0.0, f64::max)
                    + LY_METRES;
                self.distance = radius / 22.0_f64.to_radians().sin();
                return;
            }
        };
        let aspect = self
            .viewport
            .as_ref()
            .map(|v| v.physical_size.x as f64 / v.physical_size.y.max(1) as f64)
            .unwrap_or(1.0);
        self.target = (low + high) * 0.5;
        let size = high - low;
        self.distance = size.z.max(size.x / aspect) * 0.5 / 25.0_f64.to_radians().tan();
        self.yaw = 0.0;
        self.pitch = std::f32::consts::FRAC_PI_2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::{page_capacity, page_of_slot};

    fn large_galaxy() -> Galaxy {
        let mut galaxy = space_simulation::demo_galaxy();
        let template = galaxy.systems[0].tasks[0].clone();
        galaxy.systems[0].tasks = (0..50)
            .map(|slot| {
                let mut task = template.clone();
                task.id = format!("task-{slot}");
                task.slot = Some(slot);
                task
            })
            .collect();
        space_simulation::shape(&mut galaxy.systems[0]);
        galaxy
    }

    #[test]
    fn fifty_tasks_have_stable_pages_after_edits_and_removals() {
        let mut galaxy = large_galaxy();
        let system = &mut galaxy.systems[0];
        let last = page_of_slot(&system.id, 49).0;
        assert_eq!(system.pages(), (0..=last).collect::<Vec<_>>());
        for page in 0..last {
            assert_eq!(
                system.on_page(page).count() as u32,
                page_capacity(&system.id, page)
            );
        }
        let before = system.tasks.clone();
        system.tasks.reverse();
        system.tasks.retain(|p| p.stable_slot() != 12);
        for task in &mut system.tasks {
            task.status = "complete".into();
            task.due_date_ms = Some(123);
        }
        space_simulation::shape(system);
        for task in &mut system.tasks {
            let original = before.iter().find(|p| p.id == task.id).unwrap();
            assert_eq!((task.orbit, task.page), (original.orbit, original.page));
            let slot: u32 = task.id.strip_prefix("task-").unwrap().parse().unwrap();
            assert_eq!(task.page(), page_of_slot("demo-flight", slot).0);
            // Snapshots older than slots used whole-list orbital radii.
            task.slot = None;
            task.orbit.semi_major_au = 0.55 + f64::from(slot) * 0.65;
            assert_eq!(task.stable_slot(), slot);
        }
    }

    #[test]
    fn page_stars_keep_separate_positions_when_other_pages_disappear() {
        let pages = 1000;
        let positions: Vec<_> = (0..pages).map(|page| page_position("list", page)).collect();
        assert_eq!(positions[0], DVec3::ZERO);
        // Real-space density: the whole list fills the sphere that holds its
        // stars at the local density, spread evenly along every axis.
        let volume = f64::from(pages) / STAR_DENSITY_PER_LY3;
        let edge = (volume * 3.0 / (4.0 * std::f64::consts::PI)).cbrt() * LY_METRES;
        assert!(positions.iter().all(|p| p.length() <= edge * 1.000_001));
        let mean = positions.iter().copied().sum::<DVec3>() / f64::from(pages);
        assert!(mean.length() < edge * 0.1);
        for axis in 0..3 {
            let spread = positions
                .iter()
                .map(|p| p[axis].powi(2))
                .sum::<f64>()
                .sqrt();
            let expected = edge * (f64::from(pages) / 5.0).sqrt();
            assert!((spread / expected - 1.0).abs() < 0.15, "axis {axis}");
        }
        // Different lists scatter differently.
        assert_ne!(page_position("list", 4), page_position("other", 4));
        let mut galaxy = large_galaxy();
        let page_four = page_position("list", 4);
        galaxy.systems[0].tasks.retain(|task| task.page() != 1);
        assert_eq!(galaxy.systems[0].pages()[..2], [0, 2]);
        assert_eq!(page_position("list", 4), page_four);
    }

    #[test]
    fn table_uses_true_sizes_and_shows_only_the_suns_limb() {
        let galaxy = large_galaxy();
        let system = &galaxy.systems[0];
        let layout = TableLayout::new(system, 1);
        let rows: Vec<_> = system.on_page(1).map(|p| layout.position(p).z).collect();
        assert_eq!(rows.len() as u32, page_capacity(&system.id, 1));
        assert!(rows.windows(2).all(|r| r[1] - r[0] > 2.0 * layout.unit));
        // Frame starts inside the star, which ends above the first planet.
        assert!(layout.top > 0.0 && layout.top < SUN_RADIUS_METRES);
        assert!(rows[0] - layout.unit > SUN_RADIUS_METRES);
        assert!(rows[rows.len() - 1] + layout.unit < layout.bottom);
        let mut nav = Navigation {
            page: 1,
            ..Navigation::default()
        };
        nav.preset(View::Table, &galaxy);
        nav.fit_preset(system);
        assert!(
            nav.distance < 3.0 * (layout.bottom - layout.top) && nav.distance > 10.0 * layout.unit
        );
    }

    #[test]
    fn presets_and_search_find_off_page_tasks() {
        let galaxy = large_galaxy();
        let mut nav = Navigation::default();
        nav.preset(View::Constellation, &galaxy);
        nav.select_task(0, 42, &galaxy);
        let page = galaxy.systems[0].tasks[42].page;
        assert!(page > 0);
        assert_eq!(nav.page, page);
        assert_eq!(nav.selected, Some(42));
        assert!(nav.view == View::Table);
        nav.fit_preset(&galaxy.systems[0]);
        assert!(nav.distance.is_finite());
        nav.preset(View::Orbit, &galaxy);
        assert_eq!(nav.page, page);
        assert!(!nav.following);
    }
}
