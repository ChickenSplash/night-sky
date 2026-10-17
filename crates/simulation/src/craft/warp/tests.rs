use super::*;
use crate::State;
use crate::craft::{Controls, CraftKind, Frame};

struct Constant;
impl Gravity for Constant {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [2.0, 0.0, 0.0]
    }
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
        craft.warp.stop("manual");
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
    craft.spin[0] = 0.01;
    assert!(craft.engage_warp().is_err());
    craft.spin = [0.0; 3];
    craft.stop_rotation();
    assert!(craft.engage_warp().is_err());
    craft.release();
    craft.engage_warp().unwrap();
    assert!(
        craft
            .control(Controls {
                throttle: 0.1,
                ..Default::default()
            })
            .is_err()
    );
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
