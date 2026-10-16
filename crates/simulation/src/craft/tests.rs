use super::*;
use std::f64::consts::TAU;

/// Earth as a fixed point mass at the origin.
struct Point(f64);

impl Gravity for Point {
    fn acceleration(&self, position: [f64; 3], _: f64) -> [f64; 3] {
        let mut sum = [0.0; 3];
        pull(&mut sum, position, [0.0; 3], self.0, 6_371_000.0);
        sum
    }
}

struct Nothing;

impl Gravity for Nothing {
    fn acceleration(&self, _: [f64; 3], _: f64) -> [f64; 3] {
        [0.0; 3]
    }
}

const EARTH_GM: f64 = EARTH_SOLAR * SUN_GM;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn body_drives_and_fine_thrusters_follow_attitude() {
    for axis in 0..3 {
        let mut craft = ship(State::default());
        craft.orientation = DQuat::from_rotation_y(0.8).to_array();
        let mut translation = [0.0; 3];
        translation[axis] = -0.4;
        craft
            .control(Controls {
                translation,
                ..Controls::default()
            })
            .unwrap();
        craft.advance(&Nothing, 2.0, 40);
        let expected = DQuat::from_array(craft.orientation)
            * DVec3::from_array(translation)
            * craft.thrusters.fine
            * craft.seconds;
        assert!(DVec3::from_array(craft.state().velocity).distance(expected) < 1e-10);
    }
    for throttle in [-1.0, 0.5, 1.0] {
        let mut craft = ship(State::default());
        craft.orientation = DQuat::from_rotation_x(0.7).to_array();
        craft
            .control(Controls {
                throttle,
                ..Controls::default()
            })
            .unwrap();
        craft.step(&Nothing);
        let expected = DQuat::from_array(craft.orientation)
            * -DVec3::Z
            * throttle
            * craft.thrusters.forward
            * STEP_SECONDS;
        assert!(DVec3::from_array(craft.state().velocity).distance(expected) < 1e-10);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn spin_persists_and_turning_uses_each_body_axis() {
    for axis in 0..3 {
        let mut craft = ship(State::default());
        let mut rotation = [0.0; 3];
        rotation[axis] = 0.5;
        craft
            .control(Controls {
                rotation,
                ..Controls::default()
            })
            .unwrap();
        for _ in 0..40 {
            craft.step(&Nothing);
        }
        let expected = DVec3::from_array(rotation) * 0.1 * 2.0;
        assert!(DVec3::from_array(craft.spin).distance(expected) < 1e-12);
        craft.control(Controls::default()).unwrap();
        let spin = craft.spin;
        let start = DQuat::from_array(craft.orientation);
        for _ in 0..2000 {
            craft.step(&Nothing);
        }
        assert_eq!(craft.spin, spin);
        let expected = start * DQuat::from_scaled_axis(DVec3::from_array(spin) * 100.0);
        assert!(DQuat::from_array(craft.orientation).dot(expected).abs() > 1.0 - 1e-12);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn commands_refuse_invalid_inputs_without_changing_state() {
    let mut craft = ship(State::default());
    craft.thrusters.reverse = 0.0;
    let original = craft.input;
    for throttle in [-1.0, 2.0, f64::NAN, f64::INFINITY] {
        assert!(
            craft
                .control(Controls {
                    throttle,
                    ..Controls::default()
                })
                .is_err()
        );
        assert_eq!(craft.input, original);
    }
    // Fine backward translation is independent of the optional reverse drive.
    craft
        .control(Controls {
            translation: [0.0, 0.0, 1.0],
            ..Controls::default()
        })
        .unwrap();
    craft.step(&Nothing);
    assert!(craft.state().velocity[2] > 0.0);
}

fn ship(state: State) -> Craft {
    let frame = Frame::System {
        star: "test".into(),
    };
    let mut craft = Craft::new("craft-1".into(), "Ship".into(), CraftKind::Ship, frame, 0.0);
    craft.place(state, 0.0);
    craft
}

fn length(v: [f64; 3]) -> f64 {
    distance(v, [0.0; 3])
}

fn energy(state: State, gm: f64) -> f64 {
    length(state.velocity).powi(2) / 2.0 - gm / length(state.position)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn an_orbit_keeps_its_energy_for_a_hundred_laps() {
    // An eccentric, inclined orbit from 400 km up.
    let r = 6_771_000.0;
    let speed = circular_speed(EARTH_GM, r) * 1.1;
    let start = State {
        position: [r, 0.0, 0.0],
        velocity: [0.0, speed * 0.5, speed * 0.75_f64.sqrt()],
    };
    let mut craft = ship(start);
    let initial = energy(start, EARTH_GM);
    let a = -EARTH_GM / (2.0 * initial);
    let period = TAU * (a.powi(3) / EARTH_GM).sqrt();
    let mut worst: f64 = 0.0;
    for lap in 1..=100 {
        craft.advance(&Point(EARTH_GM), lap as f64 * period, usize::MAX);
        let drift = (energy(craft.state(), EARTH_GM) / initial - 1.0).abs();
        worst = worst.max(drift);
    }
    assert!(worst < 1e-8, "energy drifted by {worst:e}");
    // A hundred periods later it is back where it began, give or take
    // the part of a step between the last step and the exact period.
    let end = craft.state().position;
    let late = (craft.seconds - 100.0 * period).abs();
    assert!(late <= STEP_SECONDS);
    assert!(
        distance(end, start.position) < 10.0 + speed * late,
        "{end:?}"
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn thrust_accelerates_a_ship_in_empty_space() {
    let mut craft = ship(State::default());
    craft.input.thrust = [0.0, 0.0, 9.81];
    let steps = craft.advance(&Nothing, 10.0, usize::MAX);
    assert_eq!(steps, 200);
    let state = craft.state();
    assert!((state.velocity[2] - 98.1).abs() < 1e-9);
    assert!((state.position[2] - 490.5).abs() < 1e-6);
    // The step cap holds back catch-up after a stall.
    assert_eq!(craft.advance(&Nothing, 1e9, 5), 5);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_saved_craft_steps_exactly_like_the_original() {
    let mut craft = ship(State {
        position: [7e6, 0.0, 0.0],
        velocity: [0.0, 0.0, 7_500.0],
    });
    craft.advance(&Point(EARTH_GM), 100.0, usize::MAX);
    let mut copy: Craft = serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    craft.advance(&Point(EARTH_GM), 200.0, usize::MAX);
    copy.advance(&Point(EARTH_GM), 200.0, usize::MAX);
    assert_eq!(craft.state(), copy.state());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn gravity_is_finite_and_continuous_inside_a_body() {
    let surface = 6_371_000.0;
    let a = |r: f64| length(Point(EARTH_GM).acceleration([r, 0.0, 0.0], 0.0));
    assert!((a(surface) - 9.82).abs() < 0.01, "{}", a(surface));
    assert!((a(surface * (1.0 - 1e-9)) / a(surface * (1.0 + 1e-9)) - 1.0).abs() < 1e-8);
    assert!((a(surface / 2.0) / a(surface) - 0.5).abs() < 1e-12);
    assert_eq!(a(0.0), 0.0);
}

/// A demo planet of a single star, with its system's field.
fn planet_field() -> (Field, Planet) {
    let galaxy = crate::demo_galaxy();
    let (system, planet) = galaxy
        .systems
        .iter()
        .flat_map(|s| s.planets.iter().map(move |p| (s, p)))
        .find(|(s, p)| s.barycentres_on(p.page).next().is_none() && p.mass_earth > 0.5)
        .unwrap();
    (Field::new(system.clone(), planet.page), planet.clone())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_ship_orbits_a_planet_moving_around_its_star() {
    let (field, planet) = planet_field();
    let seconds = 1.0e6;
    let bodies = field.bodies(seconds);
    let body = bodies.iter().find(|b| b.id == planet.id).unwrap();
    assert_eq!(
        body.state,
        crate::planet_state(field.system(), &planet, seconds)
    );
    let r = 2.0 * body.radius_m;
    let speed = circular_speed(body.gm, r);
    let state = State {
        position: std::array::from_fn(|i| body.state.position[i] + [r, 0.0, 0.0][i]),
        velocity: std::array::from_fn(|i| body.state.velocity[i] + [0.0, 0.0, speed][i]),
    };
    let mut craft = ship(state);
    craft.place(state, seconds);
    assert_eq!(
        field
            .select_reference(None, state.position, seconds)
            .map(|r| r.id),
        Some(planet.id.clone())
    );
    let period = TAU * r / speed;
    let (mut low, mut high) = (f64::MAX, 0.0_f64);
    let relative = |craft: &Craft| {
        let planet = crate::planet_state(field.system(), &planet, craft.seconds);
        let state = craft.state();
        State {
            position: std::array::from_fn(|i| state.position[i] - planet.position[i]),
            velocity: std::array::from_fn(|i| state.velocity[i] - planet.velocity[i]),
        }
    };
    let initial = energy(relative(&craft), body.gm);
    for part in 1..=60 {
        craft.advance(&field, seconds + period * part as f64 / 20.0, usize::MAX);
        let distance = length(relative(&craft).position);
        (low, high) = (low.min(distance), high.max(distance));
    }
    // Three laps later, still circling at its height, the star's tide aside.
    assert!(low > 0.99 * r && high < 1.01 * r, "{low} {high} {r}");
    let drift = (energy(relative(&craft), body.gm) / initial - 1.0).abs();
    assert!(drift < 1e-3, "{drift:e}");
    // Far outside the planet's Hill sphere, the star rules.
    let far = std::array::from_fn(|i| body.state.position[i] * 3.0);
    assert_eq!(
        field.select_reference(None, far, seconds).map(|r| r.kind),
        Some(ReferenceKind::Star)
    );
    // The step keeps the planet as its reference.
    assert_eq!(craft.reference.as_deref(), Some(planet.id.as_str()));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn elements_give_the_apsides_and_period_of_an_orbit() {
    let r = 7e6;
    let circular = Elements::of(
        State {
            position: [r, 0.0, 0.0],
            velocity: [0.0, 0.0, circular_speed(EARTH_GM, r)],
        },
        EARTH_GM,
    );
    let [low, high] = circular.apsides().unwrap();
    assert!((low / r - 1.0).abs() < 1e-12 && (high / r - 1.0).abs() < 1e-12);
    assert!((circular.period().unwrap() - TAU * (r.powi(3) / EARTH_GM).sqrt()).abs() < 1e-6);
    // Faster at periapsis: the far side swings out; escape speed and beyond is unbound.
    let fast = Elements::of(
        State {
            position: [r, 0.0, 0.0],
            velocity: [0.0, 0.0, circular_speed(EARTH_GM, r) * 1.2],
        },
        EARTH_GM,
    );
    let [low, high] = fast.apsides().unwrap();
    assert!((low / r - 1.0).abs() < 1e-12 && high > 2.0 * r);
    let escape = Elements::of(
        State {
            position: [r, 0.0, 0.0],
            velocity: [0.0, 0.0, circular_speed(EARTH_GM, r) * 1.5],
        },
        EARTH_GM,
    );
    assert_eq!(escape.apsides(), None);
}
