use super::*;
use crate::State;
use crate::craft::{Controls, CraftKind, Frame, PointTarget};

struct Constant;
impl Gravity for Constant {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [2.0, 0.0, 0.0]
    }
}

struct Counted {
    calls: std::cell::Cell<usize>,
}

impl Gravity for Counted {
    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3] {
        self.calls.set(self.calls.get() + 1);
        [2.0 + seconds * 0.1 + position[0] * 0.001, 0.0, 0.0]
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn warp_reuses_gravity_and_invalidates_it_across_skipped_samples() {
    let field = Counted {
        calls: std::cell::Cell::new(0),
    };
    let mut craft = ship();
    craft.engage_warp().unwrap();
    for _ in 0..10 {
        craft.step(&field);
    }
    assert_eq!(
        field.calls.get(),
        11,
        "two first samples, then one per step"
    );
    // Recomputed snapshot samples must use precisely the same tick time as
    // the retained sample, not the rounded sum of previous time and dt.
    let mut restored: Craft =
        serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    for _ in 0..10 {
        craft.step(&field);
        restored.step(&field);
        assert_eq!(craft.state(), restored.state());
    }
    craft.warp.gravity = GravityPolicy::Suspended;
    let before = field.calls.get();
    craft.step(&field);
    assert_eq!(
        field.calls.get(),
        before,
        "slow suspended warp also skips gravity"
    );
    assert!(craft.gravity.is_none());
    craft.disengage_warp("test");
    craft.step(&field);
    assert_eq!(field.calls.get(), before + 2);
    craft.engage_warp().unwrap();
    for policy in [GravityPolicy::Active, GravityPolicy::Suspended] {
        craft.warp.gravity = policy;
        craft.warp.actual_ms = MAX_SPEED;
        craft.warp.set_speed(MAX_SPEED).unwrap();
        let before = field.calls.get();
        craft.step(&field);
        assert_eq!(field.calls.get(), before, "no samples above cutoff");
        assert!(craft.gravity.is_none());
        craft.disengage_warp("test");
        craft.step(&field);
        assert_eq!(field.calls.get(), before + 2, "dropout resamples gravity");
        craft.engage_warp().unwrap();
    }
}

struct Zero;
impl Gravity for Zero {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

fn forward(craft: &Craft) -> DVec3 {
    DQuat::from_array(craft.orientation) * -DVec3::Z
}

fn steer(craft: &mut Craft, rotation: [f64; 3]) {
    craft
        .control(Controls {
            rotation,
            ..Default::default()
        })
        .unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn steady_yaw_bends_the_path_into_a_circle_of_speed_over_rate() {
    let mut craft = ship();
    let speed = 1_000.0;
    craft.warp.set_speed(speed).unwrap();
    craft.engage_warp().unwrap();
    craft.warp.actual_ms = speed;
    let rate = craft.warp.turn_rate_dps.to_radians();
    // Already turning at the full rate, so the circle starts at once.
    craft.spin = [0.0, rate, 0.0];
    steer(&mut craft, [0.0, 1.0, 0.0]);
    let seconds = std::f64::consts::FRAC_PI_2 / rate;
    let steps = (seconds / STEP_SECONDS).round() as usize;
    for _ in 0..steps {
        craft.step(&Zero);
    }
    // A right-handed yaw turns the nose from -Z towards -X.
    let radius = speed / rate;
    let expected = DVec3::new(-radius, 0.0, -radius);
    let position = DVec3::from_array(craft.state().position);
    assert!(
        position.distance(expected) < 0.5,
        "{position} vs {expected}"
    );
    assert!(forward(&craft).distance(-DVec3::X) < 1e-9);
    assert!((DVec3::from_array(craft.spin).y - rate).abs() < 1e-12);
    assert_eq!(
        craft.state().velocity,
        [0.0; 3],
        "ordinary velocity untouched"
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn steering_ramps_shares_the_turn_rate_and_stops_on_release() {
    let mut craft = ship();
    craft.engage_warp().unwrap();
    craft.warp.set_turn_rate(90.0).unwrap();
    let most = 90_f64.to_radians();
    steer(&mut craft, [1.0, 1.0, 0.0]);
    let mut previous = 0.0;
    for _ in 0..100 {
        craft.step(&Zero);
        let rate = DVec3::from_array(craft.spin).length();
        assert!(rate >= previous && rate <= most * (1.0 + 1e-12));
        previous = rate;
    }
    assert!((previous - most).abs() < 1e-6 * most, "both axes share it");
    steer(&mut craft, [0.0; 3]);
    for _ in 0..100 {
        craft.step(&Zero);
    }
    assert!(DVec3::from_array(craft.spin).length() < 1e-6 * most);
    let held = forward(&craft);
    craft.step(&Zero);
    assert!(forward(&craft).angle_between(held) < 1e-8);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn roll_turns_the_ship_without_changing_the_direction_of_travel() {
    let mut craft = ship();
    craft.warp.set_speed(1_000.0).unwrap();
    craft.engage_warp().unwrap();
    steer(&mut craft, [0.0, 0.0, 1.0]);
    for _ in 0..60 {
        craft.step(&Zero);
    }
    let [x, y, z] = craft.state().position;
    assert!(x.abs() < 1e-9 && y.abs() < 1e-9 && z < -1_000.0);
    assert!(forward(&craft).distance(-DVec3::Z) < 1e-12);
    let up = DQuat::from_array(craft.orientation) * DVec3::Y;
    assert!(up.angle_between(DVec3::Y) > 1.0, "rolled well over");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn engaging_takes_over_spin_and_eases_it_out_and_dropout_leaves_none() {
    let mut craft = ship();
    craft.spin = [0.3, -0.2, 0.1];
    craft.engage_warp().unwrap();
    let mut previous = DVec3::from_array(craft.spin);
    for _ in 0..60 {
        craft.step(&Zero);
        let spin = DVec3::from_array(craft.spin);
        // Each axis shrinks by the same smooth factor, with no jump.
        assert!(spin.length() < previous.length());
        assert!(spin.length() > 0.8 * previous.length());
        previous = spin;
    }
    assert!(previous.length() < 1e-4);
    craft.spin = [0.0, 0.1, 0.0];
    craft.disengage_warp("test");
    assert_eq!(craft.spin, [0.0; 3]);
    let held = forward(&craft);
    craft.step(&Zero);
    assert_eq!(forward(&craft), held);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pointing_in_warp_settles_without_overshoot_within_the_turn_rate() {
    for (target, rate) in [
        ([1.0, 0.0, 0.0], 30.0),
        ([0.3, -0.8, 0.4], 360.0),
        ([0.0, 0.0, 1.0], 1.0),
        ([-0.2, 0.1, 1.0], 120.0),
    ] {
        let mut craft = ship();
        craft.warp.set_speed(1e6).unwrap();
        craft.engage_warp().unwrap();
        craft.warp.set_turn_rate(rate).unwrap();
        let goal = DVec3::from_array(target).normalize();
        craft.point(PointTarget::Vector(target), None).unwrap();
        let most = rate.to_radians();
        let mut previous = forward(&craft).angle_between(goal);
        let steps = ((previous / most + 10.0) / STEP_SECONDS) as usize;
        for _ in 0..steps {
            craft.step(&Zero);
            let error = forward(&craft).angle_between(goal);
            assert!(error <= previous + 1e-9, "{target:?} at {rate} °/s");
            assert!(DVec3::from_array(craft.spin).length() <= most * (1.0 + 1e-12));
            previous = error;
        }
        assert!(previous < 1e-4, "{target:?} at {rate} °/s: {previous}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn turn_rate_is_bounded_and_older_snapshots_get_the_default() {
    let mut warp = Warp::default();
    for valid in [MIN_TURN_RATE_DPS, MAX_TURN_RATE_DPS] {
        warp.set_turn_rate(valid).unwrap();
    }
    for invalid in [0.5, 361.0, f64::NAN] {
        assert!(warp.set_turn_rate(invalid).is_err());
        assert_eq!(warp.turn_rate_dps, MAX_TURN_RATE_DPS);
    }
    let mut old = serde_json::to_value(Warp::default()).unwrap();
    old.as_object_mut().unwrap().remove("turn_rate_dps");
    let loaded: Warp = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.turn_rate_dps, DEFAULT_TURN_RATE_DPS);
}

fn ship() -> Craft {
    Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::System { star: "sun".into() },
        0,
    )
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn speed_limit_accepts_ten_thousand_light_years_per_second() {
    let mut warp = Warp::default();
    assert_eq!(MAX_SPEED, 10_000.0 * crate::LY_METRES);
    warp.set_speed(10.0 * crate::LY_METRES).unwrap();
    warp.set_speed(MAX_SPEED).unwrap();
    for invalid in [MAX_SPEED * 1.001, f64::INFINITY, f64::NAN, 0.0] {
        assert!(warp.set_speed(invalid).is_err());
        assert_eq!(warp.requested_ms, MAX_SPEED);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn ordinary_momentum_and_bubble_are_separate_under_both_policies() {
    for policy in [GravityPolicy::Active, GravityPolicy::Suspended] {
        let mut craft = ship();
        craft.place(
            State {
                position: [0.0; 3],
                velocity: [10.0, 0.0, 0.0],
            },
            0,
        );
        craft.warp.gravity = policy;
        craft.warp.set_speed(1e7).unwrap();
        craft.engage_warp().unwrap();
        for _ in 0..20 {
            craft.step(&Constant);
        }
        let expected = if policy == GravityPolicy::Active {
            12.0
        } else {
            10.0
        };
        assert!((craft.state().velocity[0] - expected).abs() < 1e-10);
        assert_eq!(craft.state().velocity[2], 0.0);
        assert!(craft.state().position[2] < -1e6);
        let before = craft.state();
        craft.disengage_warp("manual");
        assert_eq!(craft.state(), before);
        craft.step(&Constant);
        assert!(craft.state().velocity[0] > expected);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn gravity_is_ignored_at_and_above_light_speed() {
    let mut craft = ship();
    craft.engage_warp().unwrap();
    let step_at = |craft: &mut Craft, speed: f64| {
        craft.warp.set_speed(speed).unwrap();
        craft.warp.actual_ms = speed;
        craft.step(&Constant);
        craft.state().velocity[0]
    };
    assert_eq!(step_at(&mut craft, GRAVITY_CUTOFF_MS), 0.0);
    assert_eq!(step_at(&mut craft, MAX_SPEED), 0.0);
    // Pulled again as soon as a step's bubble speed falls below the cutoff.
    let slowed = step_at(&mut craft, GRAVITY_CUTOFF_MS * 0.99);
    assert!((slowed - 2.0 * STEP_SECONDS).abs() < 1e-12);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn engagement_and_control_interlocks() {
    let mut craft = ship();
    craft
        .control(Controls {
            throttle: 0.1,
            ..Default::default()
        })
        .unwrap();
    assert!(craft.engage_warp().is_err());
    craft.control(Controls::default()).unwrap();
    craft.engage_warp().unwrap();
    for refused in [
        Controls {
            throttle: 0.1,
            ..Default::default()
        },
        Controls {
            translation: [0.0, 0.1, 0.0],
            ..Default::default()
        },
    ] {
        assert!(craft.control(refused).is_err());
    }
    assert_eq!(craft.input.controls, Controls::default());
    // Stepping an engaged craft moves it by bubble transport.
    craft.warp.set_speed(1_000.0).unwrap();
    craft.warp.actual_ms = 1_000.0;
    let before = craft.state();
    craft.step(&Constant);
    assert!(craft.state().position[2] < before.position[2] - 49.0);
    assert_eq!(craft.tick, 1);
    assert_eq!(craft.seconds(), STEP_SECONDS);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn speed_validation_and_ramp() {
    let mut craft = ship();
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_SPEED * 2.0] {
        assert!(craft.warp.set_speed(invalid).is_err());
    }
    craft.warp.set_speed(1_000.0).unwrap();
    craft.engage_warp().unwrap();
    let encoded = serde_json::to_string(&craft).unwrap();
    let loaded: Craft = serde_json::from_str(&encoded).unwrap();
    assert_eq!(loaded.warp, craft.warp);
    craft.step(&Constant);
    let speed = craft.warp.actual_ms;
    assert!(speed > 0.0 && speed < 1_000.0);
    let z = craft.state().position[2];
    assert!(z < 0.0 && z > -50.0);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_held_sweep_takes_as_long_for_every_factor_of_ten_and_dropout_ends_it() {
    let mut craft = ship();
    craft.engage_warp().unwrap();
    craft.warp.sweep = 1.0;
    let steps = (1.0 / (SWEEP_DECADES_PER_S * STEP_SECONDS)).round() as usize;
    for _ in 0..steps {
        craft.step(&Constant);
    }
    assert!((craft.warp.requested_ms - 10.0).abs() < 1e-9);
    for _ in 0..steps {
        craft.step(&Constant);
    }
    assert!((craft.warp.requested_ms - 100.0).abs() < 1e-9);
    craft.warp.sweep = -1.0;
    for _ in 0..10 * steps {
        craft.step(&Constant);
    }
    assert_eq!(craft.warp.requested_ms, MIN_SPEED);
    craft.disengage_warp("test");
    assert_eq!(craft.warp.sweep, 0.0);
    // An old snapshot has no sweep.
    let mut old = serde_json::to_value(&craft.warp).unwrap();
    old.as_object_mut().unwrap().remove("sweep");
    assert_eq!(serde_json::from_value::<Warp>(old).unwrap().sweep, 0.0);
}
