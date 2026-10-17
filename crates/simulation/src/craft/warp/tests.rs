use super::*;
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
        0.0,
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
            0.0,
        );
        craft.warp.gravity = policy;
        craft.warp.set_speed(1e12).unwrap();
        craft.engage_warp().unwrap();
        for _ in 0..20 {
            let (state, speed, bubble) = craft.warp_candidate(&Constant);
            craft.commit_warp(state, speed, bubble);
        }
        let expected = if policy == GravityPolicy::Active {
            12.0
        } else {
            10.0
        };
        assert!((craft.state().velocity[0] - expected).abs() < 1e-10);
        assert_eq!(craft.state().velocity[2], 0.0);
        assert!(craft.state().position[2] < -1e11);
        let before = craft.state();
        craft.warp.stop("manual");
        assert_eq!(craft.state(), before);
        craft.step(&Constant);
        assert!(craft.state().velocity[0] > expected);
    }
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
    let before = craft.state();
    craft.step(&Constant);
    assert_eq!(craft.state(), before); // warp is advanced through its own integrator
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
    let (state, speed, _) = craft.warp_candidate(&Constant);
    assert!(speed > 0.0 && speed < 1_000.0);
    assert!(state.position[2] < 0.0 && state.position[2] > -50.0);
    let encoded = serde_json::to_string(&craft).unwrap();
    let loaded: Craft = serde_json::from_str(&encoded).unwrap();
    assert_eq!(loaded.warp, craft.warp);
}
