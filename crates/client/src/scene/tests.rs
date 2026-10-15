use super::*;
use crate::flight::Pose;
use crate::format::speed_label;
use crate::presentation::PresentationMode;
use crate::{Clock, Navigation, Universe};
use bevy::math::{DQuat, DVec3};
use space_shared::{Orbit, Spin};
use space_simulation::AU_METRES;
use space_simulation::{
    LY_METRES, SUN_RADIUS_METRES,
    galaxy::{GalaxyModel, PC_METRES},
    position,
};

#[test]
fn drifting_into_a_streamed_star_preserves_the_eye_and_does_not_fly() {
    use space_simulation::{
        cells::{Cell, Sky, parse_cell_star},
        stream::{StarCell, StarPoint},
    };
    let home = DVec3::from_array(GalaxyModel::milky_way().home_pc);
    let cell = Cell::containing(10, home.to_array()).unwrap();
    let mut sky = Sky::default();
    let id = sky.cell_star_at(cell, 0).unwrap().id;
    let star = sky.star(&id).unwrap();
    let position_ly = (DVec3::from_array(star.position_pc) - home) * (PC_METRES / LY_METRES);
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
        .add_systems(Update, (locate, locate_planet).chain());
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
    let restored_eye = nav.eye.unwrap();
    assert!((restored_eye - (frame * LY_METRES + departing_eye)).length() < 100_000.0);
}

/// A visited list's star, with the camera placed `eye` from it in `presentation_mode`.
fn visited_list(eye: DVec3, presentation_mode: PresentationMode) -> App {
    let galaxy = space_simulation::demo_galaxy();
    let page = galaxy.systems[0].first_page();
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
        .add_systems(Update, (locate, locate_planet).chain());
    app
}

#[test]
fn planet_neighbourhood_has_hysteresis_and_uses_physical_orbits() {
    let galaxy = space_simulation::demo_galaxy();
    let system = &galaxy.systems[0];
    let planet = system.on_page(system.first_page()).next().unwrap();
    let page = planet.page();
    for seconds in [0.0, 1e7] {
        let centre = DVec3::from_array(space_simulation::planet_position(system, planet, seconds));
        let radius = planet.radius_km * 1000.0;
        let check = |radii, previous| {
            nearby_planet(
                system,
                page,
                centre + DVec3::Y * (radius * radii),
                seconds,
                previous,
                PresentationMode::Simulation,
            )
        };
        assert_eq!(check(19.0, None), Some(planet.id.clone()));
        assert_eq!(check(25.0, None), None);
        assert_eq!(check(25.0, Some(&planet.id)), Some(planet.id.clone()));
        assert_eq!(check(31.0, Some(&planet.id)), None);
        assert_eq!(
            nearby_planet(system, page, centre, seconds, None, PresentationMode::Table),
            None
        );
        assert_eq!(
            nearby_planet(
                system,
                page,
                centre,
                seconds,
                None,
                PresentationMode::Constellation
            ),
            None
        );
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
    assert_eq!(nav.location_planet, Some(id));
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
        cells::Cell,
        galaxy::Component,
        stream::{StarCell, StarPoint},
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
fn orbit_motion_does_not_steer_travel_marker() {
    let mut app = App::new();
    app.insert_resource(Time::<()>::default())
        .insert_resource(CameraRig::default())
        .insert_resource(CameraPose::default())
        .init_resource::<CameraMotion>()
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
