use super::*;
use crate::format::speed_label;
use crate::navigation::flight::Pose;
use crate::navigation::presentation::PresentationMode;
use crate::{Clock, Navigation, Universe};
use bevy::math::{DQuat, DVec3};
use space_shared::{Orbit, Spin, StarSystem};
use space_simulation::AU_METRES;
use space_simulation::craft::Field;
use space_simulation::{
    LY_METRES, SUN_RADIUS_METRES,
    galaxy::{GalaxyModel, PC_METRES},
    position,
};

/// A star near home streamed into the field, with its ID and its position
/// in parsecs from the galactic centre.
fn streamed_star() -> (crate::stars::stream::Starfield, String, [f64; 3]) {
    use space_simulation::{
        galaxy::cells::{Cell, Sky, parse_cell_star},
        galaxy::stream::{StarCell, StarPoint},
    };
    let home = GalaxyModel::milky_way().home_pc;
    let cell = Cell::containing(10, home).unwrap();
    let mut sky = Sky::default();
    let id = sky.cell_star_at(cell, 0).unwrap().id;
    let star = sky.star(&id).unwrap();
    let mut field = crate::stars::stream::Starfield::default();
    field.cells.insert(
        cell,
        StarCell::new(
            cell,
            vec![StarPoint {
                position_pc: star.position_pc,
                luminosity_solar: star.star.luminosity_solar as f32,
                temperature_k: star.star.temperature_k as f32,
                radius_solar: star.star.radius_solar as f32,
                separation_au: 0.0,
                component: parse_cell_star(&id).unwrap().1,
                index: parse_cell_star(&id).unwrap().2 as u32,
            }],
        ),
    );
    (field, id, star.position_pc)
}

#[test]
fn drifting_into_a_streamed_star_preserves_the_eye_and_does_not_fly() {
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let (field, id, position_pc) = streamed_star();
    let position_ly = (DVec3::from_array(position_pc) - home) * (PC_METRES / LY_METRES);
    let mut nav = Navigation {
        rotation: Quat::IDENTITY,
        distance: 40_000.0 * LY_METRES,
        ..default()
    };
    let eye = position_ly * LY_METRES + DVec3::X * (APPROACH_METRES * 0.5);
    nav.target = eye - DVec3::Z * nav.distance;
    let pose = CameraPose {
        eye,
        distance: nav.distance,
        ..default()
    };
    let rig = CameraRig {
        shown: Some(Pose {
            target: nav.target,
            distance: nav.distance,
            rotation: nav.rotation,
            layout: 0.0,
        }),
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(field)
        .init_resource::<Clock>()
        .insert_resource(Universe(space_simulation::demo_galaxy()))
        .insert_resource(rig)
        .insert_resource(pose)
        .insert_resource(nav)
        .init_resource::<crate::stars::stream::StreamMetrics>()
        .init_resource::<crate::craft::Fleet>()
        .add_systems(Update, (locate, locate_body).chain());
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(nav.arrived);
    assert_eq!(
        nav.system(&app.world().resource::<Universe>().0)
            .unwrap()
            .page_star(0)
            .unwrap()
            .id,
        id
    );
    assert!(!nav.fly);
    let rig = app.world().resource::<CameraRig>();
    let system = nav.system(&app.world().resource::<Universe>().0).unwrap();
    let actual_eye = page_origin_ly(system, 0) * LY_METRES
        + page_frame(system, 0) * (nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance);
    assert!((actual_eye - eye).length() < 100_000.0);
    assert_eq!(rig.frame_ly, DVec3::ZERO);
    let tilt = page_frame(system, 0);
    let range = system_range(system, 0);

    // Still there just beyond the arrival range, so the boundary cannot flicker.
    let frame = position_ly;
    app.world_mut().resource_mut::<CameraRig>().frame_ly = frame;
    app.world_mut().resource_mut::<CameraPose>().eye = DVec3::X * (range * 1.5);
    app.update();
    assert!(app.world().resource::<Navigation>().arrived);

    // Leaving the system restores open space without moving the eye.
    let departing_eye = DVec3::X * (LEAVE_FACTOR * range * 1.1);
    app.world_mut().resource_mut::<CameraPose>().eye = departing_eye;
    {
        let mut nav = app.world_mut().resource_mut::<Navigation>();
        nav.target =
            tilt.inverse() * departing_eye - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
        nav.eye = Some(tilt.inverse() * departing_eye);
    }
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(nav.visit.is_none() && !nav.arrived);
    // Open space keeps the system's frame, where the eye is still precise.
    assert_eq!(nav.galactic_origin_ly, frame);
    let restored_eye = nav.eye.unwrap();
    assert!((restored_eye - departing_eye).length() < 100_000.0);
}

/// A visited list's star, with the camera placed `eye` from it in `presentation_mode`.
fn visited_list(eye: DVec3, presentation_mode: PresentationMode) -> App {
    let page = space_simulation::demo_galaxy().systems[0].first_page();
    visited_page(page, eye, presentation_mode)
}

/// `visited_list` at one of its pages.
fn visited_page(page: u32, eye: DVec3, presentation_mode: PresentationMode) -> App {
    let galaxy = space_simulation::demo_galaxy();
    let frame_ly = page_origin_ly(&galaxy.systems[0], page);
    let nav = Navigation {
        visit: Some(crate::Visit::List(0)),
        page,
        presentation_mode,
        eye: Some(eye),
        ..default()
    };
    let rig = CameraRig {
        shown: Some(Pose {
            target: DVec3::ZERO,
            distance: nav.distance,
            rotation: nav.rotation,
            layout: 0.0,
        }),
        frame_ly,
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(crate::stars::stream::Starfield::default())
        .init_resource::<Clock>()
        .insert_resource(Universe(galaxy))
        .insert_resource(rig)
        .insert_resource(CameraPose {
            eye,
            distance: nav.distance,
            page,
            ..default()
        })
        .insert_resource(nav)
        .init_resource::<crate::stars::stream::StreamMetrics>()
        .init_resource::<crate::craft::Fleet>()
        .add_systems(Update, (locate, locate_body).chain());
    app
}

/// How near the camera must be to be at a planet: its Hill sphere, never
/// under `PLANET_MIN_RADII`.
fn camera_reach(system: &StarSystem, planet: &space_shared::Planet) -> f64 {
    space_simulation::craft::planet_reach_m(system, planet)
        .max(planet.radius_km * 1000.0 * PLANET_MIN_RADII)
}

#[test]
fn a_planets_location_has_hysteresis_and_uses_physical_orbits() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap();
    let field = Field::new(system.clone(), page);
    let id = Some(planet.id.clone());
    for seconds in [0.0, 1e7] {
        let centre = DVec3::from_array(space_simulation::planet_position(system, planet, seconds));
        let reach = camera_reach(system, planet);
        let check = |share, previous, mode| {
            nearby_body(
                &field,
                centre + DVec3::Y * (reach * share),
                seconds,
                previous,
                mode,
            )
        };
        let simulation = PresentationMode::Simulation;
        assert_eq!(check(0.95, None, simulation), id);
        assert_ne!(check(1.05, None, simulation), id);
        assert_eq!(check(1.05, id.as_deref(), simulation), id);
        assert_ne!(check(1.15, id.as_deref(), simulation), id);
        assert_eq!(check(0.0, None, PresentationMode::Table), None);
        assert_eq!(check(0.0, None, PresentationMode::Constellation), None);
    }
}

#[test]
fn the_camera_is_at_a_close_in_planet_within_its_minimum_but_a_ship_is_not() {
    let mut system = space_simulation::demo_galaxy().systems[0].clone();
    let page = system.first_page();
    // A hot Jupiter, whose Hill sphere is smaller than the following distance.
    let planet = system
        .planets
        .iter_mut()
        .find(|p| p.page() == page)
        .unwrap();
    planet.mass_earth = 318.0;
    planet.radius_km = 71_492.0;
    planet.orbit.semi_major_au = 0.02;
    planet.orbit.eccentricity = 0.0;
    let planet = planet.clone();
    let radius = planet.radius_km * 1000.0;
    assert!(space_simulation::craft::planet_reach_m(&system, &planet) < 12.0 * radius);
    let field = Field::new(system.clone(), page);
    let centre = DVec3::from_array(space_simulation::planet_position(&system, &planet, 0.0));
    let at = |radii: f64| centre + DVec3::Z * (radius * radii);
    let camera = |eye: DVec3| nearby_body(&field, eye, 0.0, None, PresentationMode::Simulation);
    let id = Some(planet.id.clone());
    assert_eq!(camera(at(20.0)), id);
    assert_ne!(camera(at(31.0)), id);
    let ship = field.select_reference(None, at(20.0).to_array(), 0.0);
    assert_ne!(ship.map(|r| r.id), id);
}

/// The demo's multiple system: its page, and a star of an inner pair with
/// its stable zone in metres.
fn binary_star() -> (StarSystem, u32, String, f64) {
    let system = space_simulation::demo_galaxy().systems[0].clone();
    let page = system.pages()[1];
    let root = system.root(page).unwrap().to_owned();
    let star = system
        .stars_on(page)
        .find(|star| star.id != root && system.pair_of(page, &star.id).is_some())
        .expect("the demo's second system is multiple")
        .id
        .clone();
    let zone = Field::new(system.clone(), page)
        .reference_by_id(&star, 0.0)
        .unwrap()
        .reach_m;
    assert!(zone.is_finite());
    (system, page, star, zone)
}

#[test]
fn a_star_of_a_multiple_system_is_a_location_within_its_stable_zone() {
    let (system, page, star, zone) = binary_star();
    let field = Field::new(system.clone(), page);
    let id = Some(star.clone());
    for seconds in [0.0, 1e7] {
        let centre = space_simulation::member_state(&system, page, &star, seconds).position;
        let check = |share, previous| {
            let eye = DVec3::from_array(centre) + DVec3::Z * (zone * share);
            nearby_body(&field, eye, seconds, previous, PresentationMode::Simulation)
        };
        assert_eq!(check(0.5, None), id);
        assert_eq!(check(0.95, None), id);
        assert_ne!(check(1.05, None), id);
        assert_eq!(check(1.05, id.as_deref()), id);
        assert_ne!(check(1.15, id.as_deref()), id);
    }
}

#[test]
fn drifting_near_a_planet_sets_location_without_selection_or_following() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap();
    let id = planet.id.clone();
    let local = DVec3::from_array(space_simulation::planet_position(system, planet, 0.0))
        + DVec3::Y * (planet.radius_km * 1000.0 * 12.0);
    let mut app = visited_list(
        page_frame(system, page) * local,
        PresentationMode::Simulation,
    );
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert_eq!(nav.location_body, Some(id));
    assert!(nav.selection.is_none() && !nav.following && !nav.fly);
}

#[test]
fn leaving_a_visited_list_star_returns_to_open_space_and_closes_the_list() {
    let mut app = visited_list(DVec3::X * (5.0 * AU_METRES), PresentationMode::Simulation);
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(nav.arrived && nav.list() == Some(0));
    assert!(
        nav.location(&app.world().resource::<Universe>().0)
            .is_some()
    );

    let far = DVec3::X * (LEAVE_FACTOR * WIDE_METRES * 50.0);
    let selection = crate::Selection::planet(&app.world().resource::<Universe>().0.systems[0], 0);
    app.world_mut()
        .resource_mut::<Navigation>()
        .select(Some(selection.clone()));
    app.world_mut().resource_mut::<CameraPose>().eye = far;
    app.world_mut().resource_mut::<Navigation>().eye = Some(far);
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(!nav.arrived && nav.visit.is_none() && nav.list().is_none());
    assert_eq!(nav.selection, Some(selection));
}

#[test]
fn a_constellation_keeps_its_list_open_away_from_every_star() {
    let mut app = visited_list(
        DVec3::X * (5.0 * LY_METRES),
        PresentationMode::Constellation,
    );
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(!nav.arrived && nav.list() == Some(0));
    assert!(
        nav.location(&app.world().resource::<Universe>().0)
            .is_none()
    );
}

#[test]
fn a_flight_is_not_at_its_destination_until_it_arrives() {
    let mut app = visited_list(DVec3::X * (5.0 * LY_METRES), PresentationMode::Simulation);
    app.world_mut().resource_mut::<Navigation>().fly = true;
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(!nav.arrived && nav.list() == Some(0));
}

#[test]
fn arriving_and_leaving_have_a_margin_between_them() {
    let range = WIDE_METRES;
    assert!(within(false, 0.9 * range, range));
    assert!(!within(false, 1.1 * range, range));
    assert!(within(true, 1.1 * range, range));
    assert!(!within(true, LEAVE_FACTOR * range * 1.01, range));
}

#[test]
fn close_body_is_visible_without_zooming_camera_range() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let pose = CameraPose {
        distance: 120.0 * LY_METRES,
        eye: DVec3::X * (APPROACH_METRES * 0.5),
        scale_distance: APPROACH_METRES * 0.5,
        ..default()
    };
    let nav = Navigation {
        arrived: true,
        ..default()
    };
    assert!(body_range(&nav, system, &pose).is_some());
    assert!(body_range(&Navigation::default(), system, &pose).is_none());
    assert!(pose.render_scale() < pose.distance / 1000.0);
}

#[test]
fn star_sizes_are_physical_except_in_wide_views() {
    let physical = SUN_RADIUS_METRES;
    let away = APPROACH_METRES * 0.5;
    assert_eq!(
        star_display_radius(physical, away, (false, false)),
        physical
    );
    assert_eq!(star_display_radius(physical, away, (true, true)), physical);
    assert_eq!(
        star_display_radius(physical, away, (false, true)),
        (away * 0.004).max(physical)
    );
}

#[test]
fn streamed_star_tightens_render_scale_before_system_handoff_at_max_range() {
    use space_simulation::{
        galaxy::Component,
        galaxy::cells::Cell,
        galaxy::stream::{StarCell, StarPoint},
    };
    let star_pc = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let cell = Cell::containing(10, star_pc.to_array()).unwrap();
    let mut field = crate::stars::stream::Starfield::default();
    field.cells.insert(
        cell,
        StarCell::new(
            cell,
            vec![StarPoint {
                position_pc: star_pc.to_array(),
                luminosity_solar: 1.0,
                temperature_k: 5772.0,
                radius_solar: 1.0,
                separation_au: 0.0,
                component: Component::ThinDisc,
                index: 0,
            }],
        ),
    );
    let maximum = 40_000.0 * LY_METRES;
    let eye = star_pc + DVec3::X * (6.0 * LY_METRES / PC_METRES);
    let mut metrics = crate::stars::stream::StreamMetrics::default();
    let mut scale = |eye| proximity_scale_distance(&field, eye, maximum, &mut metrics);
    assert!((scale(eye) / LY_METRES - 6.0).abs() < 1e-9);
    assert_eq!(
        scale(eye + DVec3::X * 30.0 * LY_METRES / PC_METRES),
        maximum
    );
}

#[test]
fn render_scale_comes_down_with_a_close_zoom() {
    // What is zoomed on sits well past the 0.01-unit near plane.
    let pose = CameraPose {
        distance: MIN_DISTANCE,
        scale_distance: MIN_DISTANCE,
        ..default()
    };
    assert!(MIN_DISTANCE / pose.render_scale() > 100.0);
    // From 20,000 km out, the scale is as it always was.
    let wide = CameraPose {
        distance: 1e9,
        ..default()
    };
    assert_eq!(wide.render_scale(), 20_000.0);
}

#[test]
fn speed_scroll_is_independent_of_range_and_bounded() {
    let mut nav = Navigation::default();
    let range = nav.distance;
    nav.move_speed = scroll_speed(nav.move_speed, 3.0);
    assert!(nav.move_speed > 42.0 * LY_METRES);
    assert_eq!(nav.distance, range);
    nav.move_speed = scroll_speed(nav.move_speed, -3.0);
    assert!((nav.move_speed / (42.0 * LY_METRES) - 1.0).abs() < 1e-6);
    assert_eq!(scroll_speed(MIN_SPEED, -1000.0), MIN_SPEED);
    assert_eq!(scroll_speed(MAX_SPEED, 1000.0), MAX_SPEED);
}

#[test]
fn speed_units_follow_the_distance_ladder() {
    assert_eq!(speed_label(42.0), "42.00 m/s");
    assert_eq!(speed_label(42_000.0), "42.00 km/s");
    assert_eq!(speed_label(2.0 * AU_METRES), "2.00 AU/s");
    assert_eq!(speed_label(2.0 * LY_METRES), "2.00 ly/s");
}

#[test]
fn camera_range_climbs_from_metres_to_light_years() {
    use crate::format::distance_label;
    assert_eq!(distance_label(50.0), "50.0 m");
    assert_eq!(distance_label(20_000_000.0), "20,000 km");
    assert_eq!(distance_label(20.0 * AU_METRES), "20.000 AU");
    assert_eq!(distance_label(2.0 * LY_METRES), "2.000 ly");
}

#[test]
fn free_look_holds_the_eye_still_and_turns_past_both_poles() {
    let mut nav = Navigation {
        target: DVec3::new(2.0, 3.0, -5.0) * AU_METRES,
        distance: 10.0 * AU_METRES,
        rotation: Quat::from_rotation_x(-1.4),
        ..Navigation::default()
    };
    let eye = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    drag_look(&mut nav, Vec2::new(80.0, 400.0), None);
    let expected =
        Quat::from_rotation_x(-1.4) * Quat::from_rotation_y(-0.4) * Quat::from_rotation_x(-2.0);
    assert!(nav.rotation.angle_between(expected) < 1e-5);
    let after = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    assert!(eye.distance(after) < nav.distance * 1e-7);
    drag_look(&mut nav, Vec2::new(-80.0, 200.0), None);
    let after = nav.target + (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    assert!(eye.distance(after) < nav.distance * 1e-7);
}

#[test]
fn distant_range_free_look_keeps_precise_eye_through_repeated_turns() {
    let eye = DVec3::new(13_000_000.0, -24_000_000.0, 51_000_000.0);
    let mut nav = Navigation {
        distance: 120.0 * LY_METRES,
        rotation: Quat::IDENTITY,
        target: eye - DVec3::Z * (120.0 * LY_METRES),
        ..default()
    };
    for _ in 0..50 {
        drag_look(&mut nav, Vec2::new(2.0, -1.0), Some(eye));
        assert_eq!(nav.eye, Some(eye));
    }
    // The target is only an orbit/flight compatibility representation;
    // subtracting the large offset from it loses precision near the star.
    assert_eq!(nav.eye.unwrap(), eye);
}

#[test]
fn turning_in_place_has_no_measured_velocity() {
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(CameraRig::default())
        .insert_resource(CameraPose {
            eye: DVec3::new(13_000_000.0, -24_000_000.0, 51_000_000.0),
            distance: 120.0 * LY_METRES,
            ..default()
        })
        .init_resource::<CameraMotion>()
        .init_resource::<Clock>()
        .init_resource::<CameraVelocity>()
        .add_systems(Update, measure_velocity);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_millis(16));
    app.update();
    // A rotation updates the camera orientation but not its precise eye.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_millis(16));
    app.update();
    assert_eq!(
        app.world().resource::<CameraVelocity>().metres_per_second,
        DVec3::ZERO
    );
}

#[test]
fn followed_hull_centre_does_not_round_trip_through_large_positions() {
    let offset = DVec3::new(123.456789, -65.4321, 143.210987);
    let mut pose = CameraPose {
        followed: Some(camera::FollowPose {
            id: "craft-1".into(),
            eye_offset: offset,
        }),
        ..default()
    };
    for magnitude in [1e9, 1e15, 1e18, 1e21] {
        pose.eye = DVec3::splat(magnitude) + offset;
        assert_eq!(pose.craft_relative("craft-1"), Some(-offset));
        assert_eq!(pose.craft_relative("another"), None);
    }
    pose.followed = None;
    assert_eq!(pose.craft_relative("craft-1"), None);
}

#[test]
fn followed_velocity_and_marker_survive_every_frame_reanchoring() {
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let body = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = crate::craft::Fleet::default();
    let id = fleet
        .spawn(
            system,
            page,
            &body,
            400_000.0,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(fleet)
        .insert_resource(CameraRig::default())
        .insert_resource(CameraPose {
            followed: Some(camera::FollowPose {
                id: id.clone(),
                eye_offset: DVec3::Z * 200.0,
            }),
            ..default()
        })
        .init_resource::<CameraMotion>()
        .init_resource::<Clock>()
        .init_resource::<CameraVelocity>()
        .add_systems(Update, measure_velocity);
    let dt = 0.006;
    let speed = 10.0 * LY_METRES;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_millis(6));
    app.update();
    for i in 1..=100 {
        app.world_mut().resource_mut::<CameraRig>().frame_ly = DVec3::X * i as f64 * 0.5;
        app.world_mut()
            .resource_mut::<crate::craft::Fleet>()
            .get_mut(&id)
            .unwrap()
            .display_delta_m = DVec3::X * speed * dt;
        app.update();
        let velocity = app.world().resource::<CameraVelocity>();
        assert!((velocity.metres_per_second.x / speed - 1.0).abs() < 1e-12);
        assert_eq!(
            velocity.marker_metres_per_second,
            velocity.metres_per_second
        );
    }
    app.world_mut()
        .resource_mut::<crate::craft::Fleet>()
        .get_mut(&id)
        .unwrap()
        .display_delta_m = DVec3::ZERO;
    app.update();
    assert_eq!(
        app.world().resource::<CameraVelocity>().metres_per_second,
        DVec3::ZERO
    );
}

#[test]
fn orbit_motion_does_not_steer_travel_marker() {
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(CameraRig::default())
        .insert_resource(CameraPose::default())
        .init_resource::<CameraMotion>()
        .init_resource::<Clock>()
        .init_resource::<CameraVelocity>()
        .add_systems(Update, measure_velocity);
    let frame = std::time::Duration::from_millis(16);
    app.world_mut().resource_mut::<Time>().advance_by(frame);
    app.update();

    // The eye really moves during right-drag, but the direction marker
    // should represent keyboard travel, not motion around the orbit pivot.
    app.world_mut().resource_mut::<CameraPose>().eye = DVec3::X * 1e9;
    let travel = DVec3::Y * 160.0;
    *app.world_mut().resource_mut::<CameraMotion>() = CameraMotion {
        orbiting: true,
        travel_metres: travel,
        ..default()
    };
    app.world_mut().resource_mut::<Time>().advance_by(frame);
    app.update();
    let velocity = app.world().resource::<CameraVelocity>();
    assert_eq!(velocity.metres_per_second, DVec3::X * 1e9 / 0.016);
    assert_eq!(velocity.marker_metres_per_second, travel / 0.016);

    // With no keyboard travel, an orbiting camera has no travel marker.
    app.world_mut().resource_mut::<CameraPose>().eye = DVec3::X * 2e9;
    app.world_mut().resource_mut::<CameraMotion>().travel_metres = DVec3::ZERO;
    app.world_mut().resource_mut::<Time>().advance_by(frame);
    app.update();
    assert_eq!(
        app.world()
            .resource::<CameraVelocity>()
            .marker_metres_per_second,
        DVec3::ZERO
    );
}

#[test]
fn keyboard_controls_ease_in_and_out_within_about_a_fifth_of_a_second() {
    let frame = 1.0 / 60.0;
    let run = |mut thrust: Vec4, wanted: Vec4, seconds: f32| {
        for _ in 0..(seconds / frame).round() as usize {
            thrust = ease_thrust(thrust, wanted, frame);
        }
        thrust
    };
    // W/S, A/D, R/F travel and Q/E roll all follow the same ease.
    for control in [Vec4::X, Vec4::Y, Vec4::Z, Vec4::W] {
        for key in [control, -control] {
            let started = run(Vec4::ZERO, key, 0.05);
            assert!(started.length() > 0.5 && started.length() < 0.8);
            assert!(run(Vec4::ZERO, key, 0.2).distance(key) < 0.02);
            assert!(run(key, Vec4::ZERO, 0.2).length() < 0.02);
            // The tail settles exactly, so a released key leaves no drift.
            assert_eq!(run(Vec4::ZERO, key, 0.4), key);
            assert_eq!(run(key, Vec4::ZERO, 0.4), Vec4::ZERO);
            // Frame rate does not change the ease.
            let coarse = ease_thrust(Vec4::ZERO, key, 0.1);
            assert!(coarse.distance(run(Vec4::ZERO, key, 0.1)) < 1e-4);
        }
    }
    // Adding A bends the path towards the diagonal rather than snapping.
    let forward = -Vec4::Z;
    let diagonal = Vec3::new(-1.0, 0.0, -1.0).normalize().extend(0.0);
    let turning = run(forward, diagonal, 0.05);
    assert!(turning.x < 0.0 && turning.x > diagonal.x);
    // Rolling while travelling leaves the travel alone.
    let rolling = run(forward, forward + Vec4::W, 0.05);
    assert_eq!(rolling.truncate(), forward.truncate());
    assert!(rolling.w > 0.5 && rolling.w < 0.8);
}

#[test]
fn shift_drag_up_zooms_in_and_down_zooms_out_within_range() {
    let mut nav = Navigation::default();
    // Screen y grows downwards, so dragging up is a negative delta.
    drag_fov(&mut nav, -100.0);
    assert!(nav.fov_deg < 50.0);
    drag_fov(&mut nav, 200.0);
    assert!(nav.fov_deg > 50.0);
    drag_fov(&mut nav, -10_000.0);
    assert_eq!(nav.fov_deg, 30.0);
    drag_fov(&mut nav, 10_000.0);
    assert_eq!(nav.fov_deg, 130.0);
}

#[test]
fn orbit_turn_preserves_the_target() {
    let mut nav = Navigation::default();
    let target = nav.target;
    let before = nav.rotation;
    orbit(&mut nav, Vec2::new(-100.0, 400.0));
    assert_eq!(nav.target, target);
    assert!(before.angle_between(nav.rotation) > 1.0);
}

#[test]
fn mouse_horizontal_look_uses_the_new_direction() {
    let mut nav = Navigation {
        rotation: Quat::IDENTITY,
        ..Navigation::default()
    };
    drag_look(&mut nav, Vec2::new(20.0, 0.0), None);
    assert!((nav.rotation * -Vec3::Z).x > 0.0);
}

#[test]
fn drag_tracks_screen_axes_at_poles_upside_down_and_rolled() {
    for pitch in [0.0, 0.9, std::f32::consts::FRAC_PI_2, 3.0] {
        for roll_angle in [0.0, 0.7] {
            let mut nav = Navigation {
                rotation: Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(roll_angle),
                ..Navigation::default()
            };
            let before = nav.rotation;
            drag_look(&mut nav, Vec2::new(20.0, 0.0), None);
            let change = nav.rotation * -Vec3::Z - before * -Vec3::Z;
            assert!(change.dot(before * Vec3::X) > 0.09);
            assert!(change.dot(before * Vec3::Y).abs() < 1e-5);
            let before = nav.rotation;
            drag_look(&mut nav, Vec2::new(0.0, 20.0), None);
            let change = nav.rotation * -Vec3::Z - before * -Vec3::Z;
            assert!(change.dot(before * Vec3::Y) < -0.09);
            assert!(change.dot(before * Vec3::X).abs() < 1e-5);
        }
    }
}

#[test]
fn orbit_tracks_screen_axes_at_poles_upside_down_and_rolled() {
    for pitch in [0.0, 0.9, std::f32::consts::FRAC_PI_2, 3.0] {
        for roll_angle in [0.0, 0.7, std::f32::consts::FRAC_PI_2] {
            let mut nav = Navigation {
                rotation: Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(roll_angle),
                ..Navigation::default()
            };
            let before = nav.rotation;
            orbit(&mut nav, Vec2::new(20.0, 0.0));
            let change = nav.rotation * Vec3::Z - before * Vec3::Z;
            assert!(change.dot(before * Vec3::X) < -0.09);
            assert!(change.dot(before * Vec3::Y).abs() < 1e-5);
            let before = nav.rotation;
            orbit(&mut nav, Vec2::new(0.0, 20.0));
            let change = nav.rotation * Vec3::Z - before * Vec3::Z;
            assert!(change.dot(before * Vec3::Y) > 0.09);
            assert!(change.dot(before * Vec3::X).abs() < 1e-5);
        }
    }
}

#[test]
fn roll_rotates_the_horizon_without_turning_or_moving_at_any_pitch() {
    for pitch in [0.0, 0.9, std::f32::consts::FRAC_PI_2, 3.0] {
        let mut nav = Navigation {
            rotation: Quat::from_rotation_x(-pitch),
            distance: 10.0 * AU_METRES,
            ..Navigation::default()
        };
        let before = nav.rotation;
        let target = nav.target;
        roll(&mut nav, 0.5);
        let after = nav.rotation;
        assert_eq!(nav.target, target);
        assert!((before * Vec3::Z).distance(after * Vec3::Z) < 1e-6);
        assert!((before * Vec3::Y).distance(after * Vec3::Y) > 0.4);
    }
}

#[test]
fn synchronous_spin_keeps_one_face_towards_star() {
    let orbit = Orbit {
        semi_major_au: 0.1,
        eccentricity: 0.0,
        inclination_rad: 0.0,
        ascending_node_rad: 0.0,
        phase_rad: 0.0,
    };
    let period = space_simulation::period_seconds(&orbit, 1.0);
    let spin = Spin {
        period_hours: period / 3_600.0,
        tilt_rad: 0.0,
        azimuth_rad: 0.0,
        phase_rad: 0.0,
        locked: true,
    };
    let facing = -DVec3::X;
    for fraction in [0.0, 0.125, 0.25, 0.5, 0.75] {
        let seconds = fraction * period;
        let towards_star = -DVec3::from_array(position(&orbit, 1.0, seconds)).normalize();
        let surface = spin_basis(DQuat::IDENTITY, &spin, seconds) * facing;
        assert!(surface.dot(towards_star) > 1.0 - 1e-12);
    }
}

#[test]
fn wide_orbits_remain_visible_at_their_fitted_camera_range() {
    let mut system = space_simulation::demo_galaxy().systems.remove(0);
    let page = system.first_page();
    let normal = system_range(&system, page);
    system.planets[0].orbit.semi_major_au = 5_183.534;
    system.planets[0].orbit.eccentricity = 0.057;
    let apoapsis = 5_183.534 * 1.057 * AU_METRES;
    let wide = system_range(&system, page);
    assert!(wide > 0.02 * LY_METRES);
    assert!(wide >= 3.1 * apoapsis);
    assert!(wide > normal);
}

#[test]
fn constellation_pages_have_independent_planes_and_local_coordinates() {
    let system = &space_simulation::demo_galaxy().systems[0];
    let poles: Vec<_> = (0..5)
        .map(|page| page_frame(system, page) * DVec3::Y)
        .collect();
    for pole in &poles {
        assert!((pole.length() - 1.0).abs() < 1e-12);
    }
    assert!(poles.windows(2).all(|pair| pair[0].dot(pair[1]) < 0.99));
    let orbit = &system.planets[0].orbit;
    for page in 0..5 {
        let frame = page_frame(system, page);
        let point = orbit_point(orbit, 7, 48);
        // Drawing and navigation use the same f64 transform. Rotation changes
        // the plane, never the orbit's radius or the table's local ordering.
        assert!(((frame * point).length() - point.length()).abs() < 0.001);
    }
}

#[test]
fn lines_are_cut_in_front_of_the_camera_in_f64() {
    use super::draw::clip_strip;
    use std::f64::consts::TAU;
    // A star's ring 2.4e10 render units away, 87° off the view axis, wide
    // enough to straddle the plane through the camera.
    let forward = DVec3::NEG_Z;
    let centre = DVec3::new(2.4e10, 0.0, -1.2e9);
    let ring: Vec<Vec3> = (0..=96)
        .map(|i| {
            let a = i as f64 / 96.0 * TAU;
            (centre + DVec3::new(a.cos(), a.sin(), a.sin()) * 2.8e9).as_vec3()
        })
        .collect();
    let strips = clip_strip(ring.clone(), DVec3::ZERO, forward);
    assert!(!strips.is_empty());
    for strip in &strips {
        assert!(strip.len() > 1);
        for p in strip {
            let p = p.as_dvec3();
            // Well in front: no f32 near-plane clip can flip it behind.
            assert!(p.dot(forward) > 1e-5 * p.length(), "{p}");
        }
    }
    // A line wholly in front is untouched; one wholly behind is dropped.
    let ahead = vec![Vec3::new(0.0, 0.0, -5.0), Vec3::new(1.0, 2.0, -7.0)];
    assert_eq!(clip_strip(ahead.clone(), DVec3::ZERO, forward), vec![ahead]);
    let behind = [Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 2.0, 7.0)];
    assert!(clip_strip(behind, DVec3::ZERO, forward).is_empty());
}

#[test]
fn following_a_ship_in_galactic_space_arrives_as_the_camera_nears_a_star() {
    use space_simulation::{State, craft::Frame};
    let (field, id, star_pc) = streamed_star();
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let body = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = crate::craft::Fleet::default();
    let craft = fleet
        .spawn(
            system,
            page,
            &body,
            400_000.0,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    // Too fast to enter the star's system, but within the camera's reach of it.
    let fly_by = |fleet: &mut crate::craft::Fleet, from_star_m: f64| {
        let live = fleet.get_mut(&craft).unwrap();
        let tick = live.craft.tick;
        live.voyage.origin_pc = star_pc;
        live.voyage.craft.frame = Frame::Galaxy { origin_pc: star_pc };
        live.voyage.craft.place(
            State {
                position: [from_star_m, 0.0, 0.0],
                velocity: [0.0; 3],
            },
            tick,
        );
    };
    fly_by(&mut fleet, APPROACH_METRES * 0.5);
    let seconds = fleet.get(&craft).unwrap().craft.seconds();
    let frame_ly = fleet.get(&craft).unwrap().origin_ly();
    let mut nav = Navigation {
        selection: Some(crate::Selection::Craft(craft.clone())),
        following: true,
        distance: crate::craft::FOLLOW_M,
        ..default()
    };
    nav.target = DVec3::X * (APPROACH_METRES * 0.5);
    let mut app = App::new();
    app.insert_resource(field)
        .insert_resource(Time::<()>::default())
        .insert_resource(Clock(space_simulation::SimulationClock {
            seconds,
            ..default()
        }))
        .insert_resource(Universe(galaxy))
        .insert_resource(CameraRig {
            frame_ly,
            ..default()
        })
        .init_resource::<CameraPose>()
        .insert_resource(nav)
        .insert_resource(fleet)
        .init_resource::<crate::stars::stream::StreamMetrics>()
        .add_systems(Update, (locate, place_camera).chain());
    for _ in 0..3 {
        app.update();
    }
    let world = app.world();
    let nav = world.resource::<Navigation>();
    let system = nav.system(&world.resource::<Universe>().0).unwrap();
    assert!(nav.arrived && nav.following);
    assert_eq!(system.page_star(0).unwrap().id, id);
    let live = world.resource::<crate::craft::Fleet>().get(&craft).unwrap();
    assert!(
        live.star().is_empty(),
        "the ship itself stays in galactic space"
    );
    // The camera, in the system's frame, still centres on the ship.
    let rig = world.resource::<CameraRig>();
    assert_eq!(rig.frame_ly, page_origin_ly(system, 0));
    let ship = (live.origin_ly() - rig.frame_ly) * LY_METRES
        + DVec3::from_array(live.craft.state().position);
    let target = page_frame(system, 0) * nav.target;
    assert!(
        (target - ship).length() < 1.0,
        "{} m",
        (target - ship).length()
    );

    // Far beyond the system, the camera leaves it and keeps following.
    let range = system_range(system, 0);
    fly_by(
        &mut app.world_mut().resource_mut::<crate::craft::Fleet>(),
        3.0 * LEAVE_FACTOR * range,
    );
    for _ in 0..3 {
        app.update();
    }
    let world = app.world();
    let nav = world.resource::<Navigation>();
    assert!(nav.visit.is_none() && !nav.arrived && nav.following);
    let live = world.resource::<crate::craft::Fleet>().get(&craft).unwrap();
    assert_eq!(world.resource::<CameraRig>().frame_ly, live.origin_ly());
    assert_eq!(nav.target, DVec3::from_array(live.craft.state().position));
}

/// A visited list with the camera free at `local` in its first page's frame,
/// placed and anchored every update.
fn anchored_app(local: DVec3, fleet: crate::craft::Fleet) -> App {
    let page = space_simulation::demo_galaxy().systems[0].first_page();
    anchored_page(page, local, fleet)
}

/// `anchored_app` at one of the list's pages.
fn anchored_page(page: u32, local: DVec3, fleet: crate::craft::Fleet) -> App {
    let galaxy = space_simulation::demo_galaxy();
    let mut app = visited_page(
        page,
        page_frame(&galaxy.systems[0], page) * local,
        PresentationMode::Simulation,
    );
    app.world_mut().resource_mut::<Navigation>().eye = Some(local);
    app.insert_resource(Time::<()>::default())
        .insert_resource(fleet)
        // In the app's order: placed after `locate`, measured after placing.
        .add_systems(
            Update,
            (
                place_camera.after(locate).before(locate_body),
                locate_anchor.after(locate_body),
            ),
        );
    app
}

#[test]
fn free_movement_in_a_planets_neighbourhood_moves_with_it() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap();
    let at =
        |seconds| DVec3::from_array(space_simulation::planet_position(system, planet, seconds));
    let offset = DVec3::Y * (planet.radius_km * 1000.0 * 12.0);
    let mut app = anchored_app(at(0.0) + offset, default());
    app.update();
    app.update();
    let anchor = Some(crate::Anchor::Body(planet.id.clone()));
    let nav = app.world().resource::<Navigation>();
    assert_eq!(nav.anchor, anchor);
    assert_eq!(nav.location_body.as_ref(), Some(&planet.id));
    for seconds in [600.0, 86_400.0] {
        app.world_mut().resource_mut::<Clock>().0.seconds = seconds;
        app.update();
        let nav = app.world().resource::<Navigation>();
        assert!(!nav.following && nav.anchor == anchor);
        let eye = nav.eye.unwrap();
        assert!((eye - (at(seconds) + offset)).length() < 1.0, "{seconds} s");
        let pose = app.world().resource::<CameraPose>();
        assert!((pose.eye - page_frame(system, page) * eye).length() < 1.0);
    }

    // Beyond its neighbourhood the camera is no longer at the planet, and
    // stays with the system.
    let far = at(86_400.0) + DVec3::Y * (camera_reach(system, planet) * 1.5);
    app.world_mut().resource_mut::<Navigation>().eye = Some(far);
    app.update();
    let nav = app.world().resource::<Navigation>();
    assert!(nav.anchor.is_none() && nav.location_body.is_none());
    app.world_mut().resource_mut::<Clock>().0.seconds = 90_000.0;
    app.update();
    assert_eq!(app.world().resource::<Navigation>().eye, Some(far));
}

#[test]
fn free_movement_near_a_star_of_a_binary_moves_with_it() {
    let (system, page, star, zone) = binary_star();
    let at = |seconds| {
        DVec3::from_array(space_simulation::member_state(&system, page, &star, seconds).position)
    };
    let offset = DVec3::Z * (zone * 0.5);
    let mut app = anchored_page(page, at(0.0) + offset, default());
    app.update();
    app.update();
    let anchor = Some(crate::Anchor::Body(star.clone()));
    assert_eq!(app.world().resource::<Navigation>().anchor, anchor);
    let day = 86_400.0;
    // The star moves on its orbit round the pair, and the camera with it.
    assert!((at(30.0 * day) - at(0.0)).length() > 1_000.0);
    for seconds in [day, 30.0 * day] {
        app.world_mut().resource_mut::<Clock>().0.seconds = seconds;
        app.update();
        let nav = app.world().resource::<Navigation>();
        assert_eq!(nav.anchor, anchor);
        assert!(
            (nav.eye.unwrap() - (at(seconds) + offset)).length() < 1.0,
            "{seconds} s"
        );
        // Its exact velocity is read out, not the rounding of its position.
        let star_velocity = space_simulation::member_state(&system, page, &star, seconds).velocity;
        assert_eq!(
            app.world().resource::<CameraPose>().carried_velocity,
            Some(page_frame(&system, page) * DVec3::from_array(star_velocity))
        );
    }
}

#[test]
fn a_ship_close_by_is_the_innermost_neighbourhood() {
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = crate::craft::Fleet::default();
    let id = fleet
        .spawn(
            system.clone(),
            page,
            &planet,
            400_000.0,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    let live = fleet.get(&id).unwrap();
    let ship = super::placement::in_camera_frame(
        live,
        live.state_at(0.0),
        page_origin_ly(&system, page),
        page_frame(&system, page),
    );
    let mut app = anchored_app(ship + DVec3::X * 500_000.0, fleet);
    app.update();
    app.update();
    let anchor = |app: &App| app.world().resource::<Navigation>().anchor.clone();
    assert_eq!(anchor(&app), Some(crate::Anchor::Craft(id)));
    // Backing off hands the camera up to the planet the ship orbits.
    app.world_mut().resource_mut::<Navigation>().eye = Some(ship + DVec3::X * 2_000_000.0);
    app.update();
    assert_eq!(anchor(&app), Some(crate::Anchor::Body(planet)));
}

#[test]
fn a_ship_in_galactic_space_carries_the_camera_through_its_re_anchoring() {
    use space_simulation::{State, craft::Frame};
    let (_, _, star_pc) = streamed_star();
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let body = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = crate::craft::Fleet::default();
    let craft = fleet
        .spawn(
            system,
            page,
            &body,
            400_000.0,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    // Hold the ship at `position` metres from a galactic anchor at `origin_pc`.
    let hold = |app: &mut App, origin_pc: [f64; 3], position: [f64; 3]| {
        let mut fleet = app.world_mut().resource_mut::<crate::craft::Fleet>();
        let live = fleet.get_mut(&craft).unwrap();
        let tick = live.craft.tick;
        live.voyage.origin_pc = origin_pc;
        live.voyage.craft.frame = Frame::Galaxy { origin_pc };
        live.voyage.craft.place(
            State {
                position,
                velocity: [0.0; 3],
            },
            tick,
        );
    };
    let seconds = fleet.get(&craft).unwrap().craft.seconds();
    let nav = Navigation {
        eye: Some(DVec3::X * 500_000.0),
        distance: 1_000.0,
        ..default()
    };
    let mut app = App::new();
    app.insert_resource(crate::stars::stream::Starfield::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(Clock(space_simulation::SimulationClock {
            seconds,
            ..default()
        }))
        .insert_resource(Universe(galaxy))
        .init_resource::<CameraRig>()
        .init_resource::<CameraPose>()
        .insert_resource(nav)
        .insert_resource(fleet)
        .init_resource::<crate::stars::stream::StreamMetrics>()
        .add_systems(Update, (locate, place_camera, locate_anchor).chain());
    hold(&mut app, star_pc, [0.0; 3]);
    // Placed at home first, then the ship's frame once it anchors the camera.
    app.world_mut().resource_mut::<Navigation>().eye = None;
    let home_ly = app
        .world()
        .resource::<crate::craft::Fleet>()
        .get(&craft)
        .unwrap()
        .origin_ly();
    app.world_mut().resource_mut::<Navigation>().eye =
        Some(home_ly * LY_METRES + DVec3::X * 500_000.0);
    for _ in 0..3 {
        app.update();
    }
    let relative = |app: &App| {
        let live = app
            .world()
            .resource::<crate::craft::Fleet>()
            .get(&craft)
            .unwrap();
        let rig = app.world().resource::<CameraRig>();
        assert_eq!(rig.frame_ly, live.origin_ly());
        app.world().resource::<Navigation>().eye.unwrap()
            - DVec3::from_array(live.craft.state().position)
    };
    assert_eq!(
        app.world().resource::<Navigation>().anchor,
        Some(crate::Anchor::Craft(craft.clone()))
    );
    // Placed from home, hundreds of light years away, so only to about a km.
    let start = relative(&app);
    assert!((start - DVec3::X * 500_000.0).length() < 1_000.0);

    // The ship moves, then re-anchors its frame 0.1 pc on: the camera keeps up.
    hold(&mut app, star_pc, [1e11, 0.0, 0.0]);
    app.update();
    assert!((relative(&app) - start).length() < 1.0);
    let moved_pc = [star_pc[0] + 0.1, star_pc[1], star_pc[2]];
    hold(
        &mut app,
        moved_pc,
        [1e11 - (moved_pc[0] - star_pc[0]) * PC_METRES, 0.0, 0.0],
    );
    app.update();
    assert!((relative(&app) - start).length() < 1.0);
}

#[test]
fn moving_with_a_body_reads_its_exact_velocity_every_frame() {
    let carried = DVec3::new(1_412.8, -37.5, 6.25);
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(CameraRig::default())
        .insert_resource(CameraPose {
            carried_velocity: Some(carried),
            ..default()
        })
        .init_resource::<CameraMotion>()
        .init_resource::<Clock>()
        .init_resource::<CameraVelocity>()
        .add_systems(Update, measure_velocity);
    let velocity = |app: &App| app.world().resource::<CameraVelocity>().metres_per_second;
    app.world_mut().resource_mut::<Clock>().0.rate = 1.0;
    // Uneven frames change nothing.
    for millis in [16, 7, 9, 33] {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(millis));
        app.update();
        assert_eq!(velocity(&app), carried);
    }
    // Faster time carries the camera faster.
    app.world_mut().resource_mut::<Clock>().0.rate = 10.0;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_millis(16));
    app.update();
    assert_eq!(velocity(&app), carried * 10.0);
    // The controls' travel adds to it; pausing stops only the carrying.
    app.world_mut().resource_mut::<CameraMotion>().travel_metres = DVec3::X * 0.48;
    app.world_mut().resource_mut::<Clock>().0.paused = true;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_millis(16));
    app.update();
    assert!((velocity(&app) - DVec3::X * 30.0).length() < 1e-9);
}

#[test]
fn free_flight_far_from_home_moves_its_frame_and_keeps_every_centimetre() {
    // About 26,000 ly out, where light years from home step by tens of km.
    let far = DVec3::new(2.0e20, -0.5e20, 1.3e20);
    let mut nav = Navigation {
        eye: Some(far),
        distance: 1_000.0,
        ..default()
    };
    nav.target = far - (nav.rotation * Vec3::Z).as_dvec3() * nav.distance;
    let mut app = App::new();
    app.insert_resource(crate::stars::stream::Starfield::default())
        .insert_resource(Time::<()>::default())
        .init_resource::<Clock>()
        .insert_resource(Universe(space_simulation::demo_galaxy()))
        .init_resource::<CameraRig>()
        .init_resource::<CameraPose>()
        .insert_resource(nav)
        .init_resource::<crate::craft::Fleet>()
        .init_resource::<crate::stars::stream::StreamMetrics>()
        .add_systems(Update, place_camera);
    app.update();
    app.update();
    let nav = app.world().resource::<Navigation>();
    let eye = nav.eye.unwrap();
    assert!(eye.length() < REANCHOR_METRES);
    assert_eq!(
        app.world().resource::<CameraRig>().frame_ly,
        nav.galactic_origin_ly
    );
    // The same place, to the precision light years from home allow.
    assert!((nav.galactic_origin_ly * LY_METRES - far + eye).length() < 100_000.0);
    for _ in 0..5 {
        let before = app.world().resource::<CameraPose>().eye;
        let mut nav = app.world_mut().resource_mut::<Navigation>();
        *nav.eye.as_mut().unwrap() += DVec3::X * 0.01;
        nav.target += DVec3::X * 0.01;
        app.update();
        let moved = app.world().resource::<CameraPose>().eye - before;
        assert!((moved - DVec3::X * 0.01).length() < 1e-6, "{moved}");
    }
}

#[test]
fn a_loaded_page_marks_each_star_and_pair_and_the_table_only_its_primary() {
    use super::draw::member_markers;
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let [single, multiple] = [0, 1].map(|index| system.pages()[index]);
    assert_eq!(
        member_markers(system, single, false),
        [Marked::Star(system.star(single).unwrap().id.clone())]
    );
    let marked = member_markers(system, multiple, false);
    let stars: Vec<_> = system.stars_on(multiple).collect();
    let pairs: Vec<_> = system.barycentres_on(multiple).collect();
    assert!(stars.len() >= 2 && pairs.len() == stars.len() - 1);
    assert_eq!(marked.len(), stars.len() + pairs.len());
    for star in &stars {
        assert!(marked.contains(&Marked::Star(star.id.clone())));
    }
    // The whole system's pair included.
    for pair in &pairs {
        assert!(marked.contains(&Marked::Barycentre(pair.id.clone())));
    }
    assert_eq!(
        member_markers(system, multiple, true),
        [Marked::Star(stars[0].id.clone())]
    );
}
