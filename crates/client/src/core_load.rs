//! Core load test (debug window, `tools` tab, native): jumps beside the
//! central black hole with no stars loaded, as at startup, and times loading
//! every cell in reach, for before-and-after comparisons of star generation.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use crate::Navigation;
use crate::presentation::PresentationMode;
use crate::scene::CameraRig;
use crate::stream::{Starfield, StreamRestart};
use bevy::{math::DVec3, platform::time::Instant, prelude::*};
use space_simulation::galaxy::{GalaxyModel, PC_METRES};

/// The camera's distance from the black hole, towards home.
const FROM_CENTRE_PC: f64 = 1.0;

#[derive(Resource, Default)]
pub enum CoreLoad {
    #[default]
    Idle,
    Requested,
    Running(Instant),
    Done {
        cells: usize,
        stars: usize,
        seconds: f64,
    },
}

/// Places the camera `FROM_CENTRE_PC` from the black hole, facing it.
fn at_core(nav: &mut Navigation) {
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let eye = DVec3::X * FROM_CENTRE_PC;
    nav.visit = None;
    nav.page = 0;
    nav.selection = None;
    nav.following = false;
    nav.presentation_mode = PresentationMode::Simulation;
    nav.distance = FROM_CENTRE_PC * PC_METRES;
    nav.target = -home * PC_METRES;
    nav.rotation = Transform::default()
        .looking_to(Vec3::NEG_X, Vec3::Y)
        .rotation;
    nav.eye = Some((eye - home) * PC_METRES);
    nav.fly = false;
}

/// Before `scene::place_camera`, so the stream restarts at the new eye in the
/// same frame.
pub fn run(
    mut load: ResMut<CoreLoad>,
    mut nav: ResMut<Navigation>,
    mut rig: ResMut<CameraRig>,
    mut restart: ResMut<StreamRestart>,
    field: Res<Starfield>,
) {
    match *load {
        CoreLoad::Requested => {
            at_core(&mut nav);
            rig.land();
            restart.0 = true;
            *load = CoreLoad::Running(Instant::now());
        }
        CoreLoad::Running(started) if field.pending == 0 && !field.cells.is_empty() => {
            let (cells, stars) = (field.cells.len(), field.stars);
            let seconds = started.elapsed().as_secs_f64();
            info!("Core load test: {cells} cells, {stars} stars in {seconds:.2} s");
            *load = CoreLoad::Done {
                cells,
                stars,
                seconds,
            };
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_eye_is_beside_the_black_hole_facing_it() {
        let mut nav = Navigation::default();
        at_core(&mut nav);
        let home = DVec3::from_array(GalaxyModel::milky_way().home_pc) * PC_METRES;
        let eye = nav.eye.unwrap() + home;
        assert!((eye.length() / PC_METRES - FROM_CENTRE_PC).abs() < 1e-6);
        let ahead = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        // The camera looks along an f32 rotation: allow a millionth of a parsec.
        assert!((ahead - (nav.target + home)).length() < 1e-6 * PC_METRES);
        assert!(ahead.length() < 1e-6 * PC_METRES);
    }
}
