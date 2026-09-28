//! Opt-in, reproducible browser star-stream benchmark (`?benchmark=star-flight`).
//! A fresh page has an empty loader and starfield. Drive the camera from home
//! toward the galactic centre, independent of keyboard focus or frame rate.

use crate::Navigation;
use bevy::{math::DVec3, prelude::*};
use space_simulation::LY_METRES;

pub const SECONDS: f64 = 15.0;
const SPEED: f64 = 1_000.0 * LY_METRES;

#[derive(Resource, Default)]
pub struct BenchmarkFlight {
    pub elapsed: f64,
}

pub fn requested() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| {
            query
                .split('&')
                .any(|part| part == "benchmark=star-flight" || part == "?benchmark=star-flight")
        })
}

pub fn at_home(nav: &mut Navigation) {
    nav.visit = None;
    nav.eye = Some(DVec3::ZERO);
    nav.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    nav.move_speed = SPEED;
    nav.target = -DVec3::X * nav.distance;
}

pub fn advance(
    time: Res<Time>,
    mut flight: Option<ResMut<BenchmarkFlight>>,
    mut nav: ResMut<Navigation>,
) {
    let Some(flight) = flight.as_deref_mut() else {
        return;
    };
    flight.elapsed = (flight.elapsed + time.delta_secs_f64()).min(SECONDS);
    nav.visit = None;
    nav.following = false;
    nav.fly = false;
    nav.view = crate::views::View::Orbit;
    nav.move_speed = SPEED;
    let eye = -DVec3::X * (SPEED * flight.elapsed);
    nav.eye = Some(eye);
    nav.target = eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_route_starts_at_home_and_ends_fifteen_thousand_ly_toward_centre() {
        let mut nav = Navigation::default();
        at_home(&mut nav);
        assert_eq!(nav.eye, Some(DVec3::ZERO));
        assert_eq!(nav.move_speed, SPEED);
        assert_eq!(
            -DVec3::X * (SPEED * SECONDS) / LY_METRES,
            -DVec3::X * 15_000.0
        );
    }
}
