use super::*;

struct Empty;
impl Gravity for Empty {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

fn ship() -> Craft {
    Craft::new(
        "test".into(),
        "Test".into(),
        CraftKind::Ship,
        Frame::System {
            star: "test".into(),
        },
        0,
    )
}

fn pointing_error(craft: &Craft, direction: DVec3) -> f64 {
    (DQuat::from_array(craft.orientation) * -DVec3::Z).angle_between(direction)
}

#[test]
fn pointing_settles_from_awkward_attitudes_with_real_thrusters() {
    for direction in [DVec3::Z, DVec3::X, DVec3::new(0.2, -0.8, 0.3).normalize()] {
        for spin in [[0.0; 3], [0.1, -0.2, 0.3]] {
            let mut craft = ship();
            craft.orientation = DQuat::from_euler(glam::EulerRot::XYZ, 0.7, -1.2, 2.1).to_array();
            craft.spin = spin;
            craft
                .point(PointTarget::Vector(direction.to_array()), None)
                .unwrap();
            for _ in 0..2400 {
                let before = DVec3::from_array(craft.spin);
                craft.step(&Empty);
                let delta = (DVec3::from_array(craft.spin) - before).abs();
                assert!(delta.max_element() <= 0.1 * STEP_SECONDS + 1e-12);
                assert!((DQuat::from_array(craft.orientation).length() - 1.0).abs() < 1e-12);
            }
            assert!(pointing_error(&craft, direction) < 1e-6, "{:?}", craft.spin);
            assert!(DVec3::from_array(craft.spin).length() < 1e-7);
        }
    }
}

#[test]
fn rest_to_target_turns_do_not_overshoot_and_weak_thrusters_take_longer() {
    let mut times = Vec::new();
    for limit in [0.1, 0.01] {
        let mut craft = ship();
        craft.thrusters.angular = [limit; 3];
        craft
            .point(PointTarget::Vector(DVec3::X.to_array()), None)
            .unwrap();
        let mut last = pointing_error(&craft, DVec3::X);
        let mut settled = None;
        for step in 0..4000 {
            craft.step(&Empty);
            let error = pointing_error(&craft, DVec3::X);
            assert!(error <= last + 1e-7, "{limit}: {last} -> {error}");
            last = error;
            if error < 1e-4 && settled.is_none() {
                settled = Some(step);
            }
        }
        times.push(settled.unwrap());
    }
    assert!(times[1] > times[0]);
}

#[test]
fn stopping_brakes_without_reversing_and_release_restores_manual_control() {
    let mut craft = ship();
    craft.spin = [0.233, -0.321, 0.014];
    craft.input.controls.throttle = 0.5;
    craft.stop_rotation();
    for _ in 0..300 {
        let before = craft.spin;
        craft.step(&Empty);
        for (old, new) in before.into_iter().zip(craft.spin) {
            assert!(old * new >= 0.0);
            assert!(new.abs() <= old.abs());
        }
    }
    assert!(DVec3::from_array(craft.spin).length() < 1e-10);
    assert_eq!(craft.input.controls.throttle, 0.5);
    craft.release();
    assert_eq!(craft.autopilot, AttitudePilot::Manual);
    craft.point(PointTarget::Prograde, None).unwrap();
    craft
        .control(Controls {
            rotation: [0.1, 0.0, 0.0],
            ..Controls::default()
        })
        .unwrap();
    assert_eq!(craft.autopilot, AttitudePilot::Manual);
}

struct Reference(State);
impl Gravity for Reference {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
    fn reference(&self, id: Option<&str>, _: [f64; 3], _: f64) -> Option<State> {
        if id.is_none() || id == Some("body") {
            Some(self.0)
        } else {
            None
        }
    }
}

#[test]
fn every_target_uses_relative_position_and_velocity() {
    let field = Reference(State {
        position: [10.0, 20.0, 30.0],
        velocity: [100.0, 200.0, 300.0],
    });
    let state = State {
        position: [11.0, 20.0, 30.0],
        velocity: [100.0, 202.0, 300.0],
    };
    for (target, expected) in [
        (PointTarget::Prograde, DVec3::Y),
        (PointTarget::Retrograde, -DVec3::Y),
        (PointTarget::Normal, DVec3::Z),
        (PointTarget::Antinormal, -DVec3::Z),
        (PointTarget::RadialOut, DVec3::X),
        (PointTarget::RadialIn, -DVec3::X),
        (PointTarget::Body("body".into()), -DVec3::X),
        (PointTarget::Vector([0.0, 0.0, -100.0]), -DVec3::Z),
    ] {
        assert!(
            target
                .direction(state, 0.0, &field, None)
                .unwrap()
                .distance(expected)
                < 1e-12
        );
    }
    assert!(
        PointTarget::Body("missing".into())
            .direction(state, 0.0, &field, None)
            .is_none()
    );
}

#[test]
fn invalid_or_undefined_targets_are_safe_and_roll_can_be_specified() {
    let mut craft = ship();
    for v in [[0.0; 3], [f64::NAN, 0.0, 0.0], [f64::INFINITY; 3]] {
        assert!(craft.point(PointTarget::Vector(v), None).is_err());
        assert!(craft.point(PointTarget::Prograde, Some(v)).is_err());
    }
    for target in [
        PointTarget::Prograde,
        PointTarget::Normal,
        PointTarget::Body("missing".into()),
    ] {
        craft.spin = [0.1; 3];
        craft.point(target, None).unwrap();
        for _ in 0..300 {
            craft.step(&Empty);
        }
        assert!(DVec3::from_array(craft.spin).length() < 1e-10);
    }
    craft = ship();
    craft.orientation = DQuat::from_rotation_z(0.8).to_array();
    craft
        .point(PointTarget::Vector([0.0, 0.0, -1.0]), None)
        .unwrap();
    let start = craft.orientation;
    for _ in 0..100 {
        craft.step(&Empty);
    }
    assert!(
        DQuat::from_array(start)
            .dot(DQuat::from_array(craft.orientation))
            .abs()
            > 1.0 - 1e-12
    );
    craft
        .point(PointTarget::Vector([0.0, 0.0, -1.0]), Some([0.0, 1.0, 0.0]))
        .unwrap();
    for _ in 0..1200 {
        craft.step(&Empty);
    }
    assert!((DQuat::from_array(craft.orientation) * DVec3::Y).distance(DVec3::Y) < 1e-6);
    // Parallel up is a pole, not a NaN.
    craft
        .point(
            PointTarget::Vector([0.0, 0.0, -1.0]),
            Some([0.0, 0.0, -1.0]),
        )
        .unwrap();
    craft.step(&Empty);
    assert!(DQuat::from_array(craft.orientation).is_finite());
}

struct Orbit;
impl Gravity for Orbit {
    fn acceleration(&self, p: [f64; 3], _: f64) -> [f64; 3] {
        let p = DVec3::from_array(p);
        (-p * 1000.0 / p.length().powi(3)).to_array()
    }
    fn reference(&self, _: Option<&str>, _: [f64; 3], _: f64) -> Option<State> {
        Some(State::default())
    }
}

#[test]
fn prograde_tracks_an_entire_orbit_and_saved_guidance_resumes_within_precision() {
    let mut craft = ship();
    craft.place(
        State {
            position: [1000.0, 0.0, 0.0],
            velocity: [0.0, 0.0, -1.0],
        },
        0,
    );
    craft.point(PointTarget::Prograde, None).unwrap();
    let steps = (std::f64::consts::TAU * 1000.0 / STEP_SECONDS).ceil() as usize;
    for step in 0..steps {
        craft.step(&Orbit);
        if step > 1000 {
            let direction = DVec3::from_array(craft.state().velocity).normalize();
            assert!(pointing_error(&craft, direction) < 0.003);
        }
    }
    let mut loaded: Craft = serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    for _ in 0..200 {
        craft.step(&Orbit);
        loaded.step(&Orbit);
    }
    // JSON's default float parser can lose a final digit. Guidance must not
    // amplify that tiny load-time difference into a different manoeuvre.
    assert!(
        DVec3::from_array(craft.state().position)
            .distance(DVec3::from_array(loaded.state().position))
            < 1e-9
    );
    assert!(DVec3::from_array(craft.spin).distance(DVec3::from_array(loaded.spin)) < 1e-12);
    assert!(
        DQuat::from_array(craft.orientation)
            .dot(DQuat::from_array(loaded.orientation))
            .abs()
            > 1.0 - 1e-12
    );
    assert_eq!(craft.tick, loaded.tick);
}

#[test]
fn exact_half_turn_and_disabled_turning_axes_remain_finite() {
    let mut craft = ship();
    craft
        .point(PointTarget::Vector([0.0, 0.0, 1.0]), None)
        .unwrap();
    for _ in 0..2400 {
        craft.step(&Empty);
    }
    assert!(pointing_error(&craft, DVec3::Z) < 1e-6);
    craft.thrusters.angular = [0.0; 3];
    craft.spin = [0.1, -0.2, 0.3];
    craft.stop_rotation();
    for _ in 0..100 {
        craft.step(&Empty);
    }
    assert_eq!(craft.spin, [0.1, -0.2, 0.3]);
    assert!(DQuat::from_array(craft.orientation).is_finite());
}
