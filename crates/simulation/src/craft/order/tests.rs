use super::*;
use crate::craft::{AttitudePilot, CraftKind, Frame, warp};

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
fn legacy_thrust_is_validated_like_every_other_order() {
    let mut craft = ship();
    let thrust = |thrust| Order::Thrust { thrust };
    thrust([0.0, 1.0, 0.0]).apply(&mut craft, None).unwrap();
    assert_eq!(craft.input.thrust, [0.0, 1.0, 0.0]);
    for invalid in [[0.0, 1e3, 0.0], [f64::NAN, 0.0, 0.0]] {
        assert!(thrust(invalid).apply(&mut craft, None).is_err());
        assert_eq!(craft.input.thrust, [0.0, 1.0, 0.0]);
    }
    // The body-axis drives replace legacy thrust.
    Order::Throttle { value: 0.5 }
        .apply(&mut craft, None)
        .unwrap();
    assert_eq!(craft.input.thrust, [0.0; 3]);
    Order::Throttle { value: 0.0 }
        .apply(&mut craft, None)
        .unwrap();
    let warp = |command| Order::Warp { command };
    warp(WarpOrder::Engage).apply(&mut craft, None).unwrap();
    assert!(thrust([0.0, 1.0, 0.0]).apply(&mut craft, None).is_err());
    thrust([0.0; 3]).apply(&mut craft, None).unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn warp_speed_steps_stay_on_the_scale() {
    let mut craft = ship();
    let warp = |command| Order::Warp { command };
    warp(WarpOrder::Down).apply(&mut craft, None).unwrap();
    assert_eq!(craft.warp.requested_ms, warp::MIN_SPEED);
    warp(WarpOrder::Up).apply(&mut craft, None).unwrap();
    assert_eq!(craft.warp.requested_ms, warp::SPEED_FACTOR);
    warp(WarpOrder::Set {
        speed_ms: warp::MAX_SPEED,
    })
    .apply(&mut craft, None)
    .unwrap();
    warp(WarpOrder::Up).apply(&mut craft, None).unwrap();
    assert_eq!(craft.warp.requested_ms, warp::MAX_SPEED);
    warp(WarpOrder::Engage).apply(&mut craft, None).unwrap();
    let suspend = warp(WarpOrder::Gravity {
        policy: warp::GravityPolicy::Suspended,
    });
    assert!(suspend.apply(&mut craft, None).is_err());
    warp(WarpOrder::Disengage).apply(&mut craft, None).unwrap();
    suspend.apply(&mut craft, None).unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn turning_orders_steer_the_bubble_during_warp() {
    let mut craft = ship();
    let warp = |command| Order::Warp { command };
    warp(WarpOrder::Engage).apply(&mut craft, None).unwrap();
    let rotate = Order::Rotate {
        pitch: 0.0,
        yaw: 0.5,
        roll: 0.0,
    };
    rotate.apply(&mut craft, None).unwrap();
    assert_eq!(craft.input.controls.rotation, [0.0, 0.5, 0.0]);
    Order::Point {
        target: PointTarget::Vector([1.0, 0.0, 0.0]),
        up: None,
    }
    .apply(&mut craft, None)
    .unwrap();
    assert!(matches!(craft.autopilot, AttitudePilot::Point { .. }));
    Order::Stop.apply(&mut craft, None).unwrap();
    Order::Cut.apply(&mut craft, None).unwrap();
    assert!(craft.warp.engaged, "cutting drives leaves warp engaged");
    // Disengaging hands the held input back to the turning thrusters.
    rotate.apply(&mut craft, None).unwrap();
    craft.spin = [0.0, 0.2, 0.0];
    warp(WarpOrder::Disengage).apply(&mut craft, None).unwrap();
    assert_eq!(craft.spin, [0.0; 3]);
    assert_eq!(craft.input.controls.rotation, [0.0, 0.5, 0.0]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn turn_rate_is_validated_and_changes_engaged_or_not() {
    let mut craft = ship();
    let rate = |degrees_per_second| Order::Warp {
        command: WarpOrder::TurnRate { degrees_per_second },
    };
    assert_eq!(craft.warp.turn_rate_dps, warp::DEFAULT_TURN_RATE_DPS);
    for invalid in [0.0, 0.99, 360.01, -30.0, f64::NAN, f64::INFINITY] {
        assert!(rate(invalid).validate().is_err());
        assert!(rate(invalid).apply(&mut craft, None).is_err());
    }
    assert_eq!(craft.warp.turn_rate_dps, warp::DEFAULT_TURN_RATE_DPS);
    rate(1.0).apply(&mut craft, None).unwrap();
    Order::Warp {
        command: WarpOrder::Engage,
    }
    .apply(&mut craft, None)
    .unwrap();
    rate(360.0).apply(&mut craft, None).unwrap();
    assert_eq!(craft.warp.turn_rate_dps, 360.0);
    let json = serde_json::to_value(rate(45.0)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"action": "warp", "command": {"command": "turn_rate", "degrees_per_second": 45.0}})
    );
}
