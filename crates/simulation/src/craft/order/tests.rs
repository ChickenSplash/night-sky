use super::*;
use crate::craft::{CraftKind, Frame, warp};

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
    assert!(Order::Cut.apply(&mut craft, None).is_err());
    warp(WarpOrder::Disengage).apply(&mut craft, None).unwrap();
    suspend.apply(&mut craft, None).unwrap();
}
