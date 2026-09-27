//! Galaxy travel and on-demand systems, independent of ClickUp list pages.
use crate::{
    Navigation,
    scene::{CameraPose, CameraRig},
    stream::Starfield,
};
use bevy::{math::DVec3, prelude::*};
use space_shared::{Planet, StarSystem};
use space_simulation::{
    AU_METRES, LY_METRES,
    cells::cell_planetary_system,
    cells::cell_star,
    galaxy::{GalaxyModel, PC_METRES},
};

/// A picked streamed star, identified by its stable cell ID, never a mesh index.
#[derive(Clone, Debug)]
pub struct PickedStar {
    pub id: String,
    pub position_pc: [f64; 3],
}

#[derive(Resource, Default)]
pub struct Galactic {
    pub picked: Option<PickedStar>,
    pub system: Option<StarSystem>,
}

fn relative_ly(position: [f64; 3]) -> DVec3 {
    (DVec3::from_array(position) - DVec3::from_array(GalaxyModel::milky_way().home_pc))
        * (PC_METRES / LY_METRES)
}

/// Select the closest star to the middle of the camera view. Only loaded
/// points are eligible, and selection never depends on their upload order.
pub fn pick_centre(
    field: &Starfield,
    rig: &CameraRig,
    pose: &CameraPose,
    rotation: Quat,
) -> Option<PickedStar> {
    let eye = rig.galactic_ly(pose);
    let forward = (rotation * -Vec3::Z).as_dvec3();
    let mut best: Option<(f64, PickedStar)> = None;
    for (&cell, stars) in &field.cells {
        for star in &stars.stars {
            let delta = relative_ly(star.position_pc) - eye;
            let distance = delta.length();
            if distance < 1e-8 {
                continue;
            }
            let pc = distance * LY_METRES / PC_METRES;
            let magnitude = 4.83 - 2.5 * f64::from(star.luminosity_solar).max(1e-30).log10()
                + 5.0 * pc.log10()
                - 5.0;
            if magnitude > crate::stream::MAGNITUDE {
                continue;
            }
            let alignment = delta.dot(forward) / distance;
            // Approximately a one-degree crosshair. Prefer angular proximity;
            // a stable ID resolves equal scores regardless of HashMap ordering.
            if alignment < 0.99985 {
                continue;
            }
            let score = 1.0 - alignment;
            let picked = PickedStar {
                id: star.id(cell),
                position_pc: star.position_pc,
            };
            if best.as_ref().is_none_or(|(old, previous)| {
                score < *old || (score == *old && picked.id < previous.id)
            }) {
                best = Some((score, picked));
            }
        }
    }
    best.map(|(_, star)| star)
}

pub fn overview(nav: &mut Navigation, galactic: &mut Galactic, centre: DVec3) {
    nav.galactic = true;
    galactic.system = None;
    nav.atlas = false;
    nav.following = false;
    nav.selected = None;
    nav.view = crate::views::View::Orbit;
    nav.target = centre * LY_METRES;
    nav.distance = 120.0 * LY_METRES;
    nav.fly = true;
}

pub fn enter(nav: &mut Navigation, galactic: &mut Galactic) -> bool {
    let Some(picked) = &galactic.picked else {
        return false;
    };
    let Some((_, mut star)) = cell_star(&picked.id) else {
        return false;
    };
    let Some(bodies) = cell_planetary_system(&picked.id) else {
        return false;
    };
    star.page = 0;
    let planets = bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| Planet {
            id: format!("{}:{index}", picked.id),
            designation: format!("{} {}", picked.id, index + 1),
            page: 0,
            orbit_index: index as u32,
            orbit: body.orbit,
            radius_km: body.radius_km,
            mass_earth: body.mass_earth,
            class: body.class.name().into(),
            resonance: body.resonance,
            spin: body.spin,
            task: None,
        })
        .collect::<Vec<_>>();
    let extent = planets
        .iter()
        .map(|p| p.orbit.semi_major_au * (1.0 + p.orbit.eccentricity))
        .fold(3.0, f64::max);
    galactic.system = Some(StarSystem {
        id: picked.id.clone(),
        name: picked.id.clone(),
        position_ly: relative_ly(picked.position_pc).to_array(),
        planets,
        stars: vec![star],
    });
    nav.galactic = true;
    nav.page = 0;
    nav.view = crate::views::View::Orbit;
    nav.target = DVec3::ZERO;
    nav.distance = extent * 3.1 * AU_METRES;
    nav.following = false;
    nav.selected = None;
    nav.fly = true;
    true
}

pub fn exit(nav: &mut Navigation, galactic: &mut Galactic) {
    let centre = galactic
        .picked
        .as_ref()
        .map_or(DVec3::ZERO, |p| relative_ly(p.position_pc));
    overview(nav, galactic, centre);
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_simulation::{
        cells::{Cell, Sky},
        galaxy::Component,
        stream::{Loader, StarCell, StarPoint},
    };

    #[test]
    fn centre_pick_ignores_stars_below_the_visible_limit() {
        let home = GalaxyModel::milky_way().home_pc;
        let point = |distance: f64, index| StarPoint {
            position_pc: [home[0], home[1], home[2] - distance],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: Component::ThinDisc,
            index,
        };
        let cell = Cell::from_coords(0, [0, 0, 0]).unwrap();
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            StarCell {
                cell,
                stars: vec![point(100.0, 0), point(0.01, 1)],
            },
        );
        let rig = CameraRig::default();
        let pose = CameraPose::default();
        let picked = pick_centre(&field, &rig, &pose, Quat::IDENTITY).unwrap();
        assert_eq!(picked.id, point(0.01, 1).id(cell));
        field.cells.get_mut(&cell).unwrap().stars.pop();
        assert!(pick_centre(&field, &rig, &pose, Quat::IDENTITY).is_none());
    }

    #[test]
    fn generated_system_is_stable_and_has_no_tasks() {
        let home = GalaxyModel::milky_way().home_pc;
        let mut loader = Loader::new(Sky::default(), 6.5);
        loader.aim(home);
        let events = loader.step(&mut || true);
        let (cell, star) = events
            .into_iter()
            .find_map(|event| match event {
                space_simulation::stream::Event::Loaded(c) => {
                    c.stars.first().copied().map(|s| (c.cell, s))
                }
                _ => None,
            })
            .expect("nearby star");
        let picked = PickedStar {
            id: star.id(cell),
            position_pc: star.position_pc,
        };
        let mut galactic = Galactic {
            picked: Some(picked.clone()),
            system: None,
        };
        let mut nav = Navigation::default();
        assert!(enter(&mut nav, &mut galactic));
        let first = galactic.system.clone().unwrap();
        assert!(!first.planets.is_empty());
        assert!(first.planets.iter().all(|p| p.task.is_none()));
        exit(&mut nav, &mut galactic);
        assert!(enter(&mut nav, &mut galactic));
        assert_eq!(galactic.system.as_ref(), Some(&first));
        assert_eq!(galactic.picked.as_ref().unwrap().id, picked.id);
    }
}
