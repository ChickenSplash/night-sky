use super::*;

const EARTH_GM: f64 = EARTH_SOLAR * SUN_GM;
const EARTH_RADIUS: f64 = 6_371_000.0;

/// Earth alone, fixed at the origin, as its own orbit reference.
struct Earth;

impl Gravity for Earth {
    fn acceleration(&self, position: [f64; 3], _: f64) -> [f64; 3] {
        let mut sum = [0.0; 3];
        pull(&mut sum, position, [0.0; 3], EARTH_GM, EARTH_RADIUS);
        sum
    }
    fn reference(&self, _: Option<&str>, _: [f64; 3], _: f64) -> Option<State> {
        Some(State::default())
    }
    fn orbit_reference(&self, _: Option<&str>, _: [f64; 3], _: f64) -> Option<Reference> {
        Some(Reference {
            id: "earth".into(),
            kind: ReferenceKind::Planet,
            gm: EARTH_GM,
            radius_m: EARTH_RADIUS,
            state: State::default(),
            reach_m: f64::INFINITY,
        })
    }
    fn closest(&self, position: [f64; 3], _: f64) -> Option<Closest> {
        let r = distance(position, [0.0; 3]);
        Some(Closest {
            id: "earth".into(),
            timescale: (r.powi(3) / EARTH_GM).sqrt(),
            inside: r < EARTH_RADIUS,
        })
    }
}

fn ship(state: State, seconds: f64) -> Craft {
    let frame = Frame::System { star: "t".into() };
    let mut craft = Craft::new("c".into(), "C".into(), CraftKind::Ship, frame, seconds);
    craft.place(state, seconds);
    craft
}

/// From 400 km up, faster than circular: an orbit out to about 10,000 km.
fn eccentric() -> (Craft, Elements) {
    let r = EARTH_RADIUS + 400_000.0;
    let state = State {
        position: [r, 0.0, 0.0],
        velocity: [0.0, 1_500.0, circular_speed(EARTH_GM, r) * 1.1],
    };
    (ship(state, 0.0), Elements::of(state, EARTH_GM))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_bound_prediction_matches_fine_steps_and_its_apsides() {
    let (craft, orbit) = eccentric();
    let before = craft.clone();
    let prediction = predict(&craft, &Earth).unwrap();
    assert_eq!(craft, before, "the real craft is untouched");
    assert_eq!(prediction.end, Some(End::Horizon));
    assert!(prediction.bound);
    let period = orbit.period().unwrap();
    assert!((prediction.achieved_seconds() / (period * (PERIODS + LEAD)) - 1.0).abs() < 1e-9);
    assert!(
        prediction.samples.len() < MAX_STEPS / 2,
        "{}",
        prediction.samples.len()
    );
    // Against the real fixed steps, a twentieth of a second each.
    let end = *prediction.samples.last().unwrap();
    let mut fine = craft.clone();
    fine.advance(&Earth, end.seconds - STEP_SECONDS / 2.0, usize::MAX);
    let mut last = fine.clone();
    last.step(&Earth);
    let share = (end.seconds - fine.seconds) / STEP_SECONDS;
    let expected: [f64; 3] = std::array::from_fn(|i| {
        let [a, b] = [fine.state().position[i], last.state().position[i]];
        a + (b - a) * share
    });
    let error = distance(expected, end.position);
    assert!(error < 2_000.0, "{error} m after a lap");
    // Sampled apsides agree with the two-body ones.
    let [low, high] = orbit.apsides().unwrap();
    let periapses: Vec<_> = prediction
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Periapsis { distance_m, .. } => Some(*distance_m),
            _ => None,
        })
        .collect();
    let apoapses: Vec<_> = prediction
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Apoapsis { distance_m, .. } => Some(*distance_m),
            _ => None,
        })
        .collect();
    assert_eq!(apoapses.len(), 1, "{:?}", prediction.events);
    assert!(
        (apoapses[0] / high - 1.0).abs() < 5e-5,
        "{} {high}",
        apoapses[0]
    );
    // It starts just past periapsis, so meets it again only at the end.
    assert_eq!(periapses.len(), 1, "{:?}", prediction.events);
    assert!(
        (periapses[0] / low - 1.0).abs() < 5e-5,
        "{} {low}",
        periapses[0]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn escapes_have_a_finite_horizon_and_impacts_stop_at_the_surface() {
    let r = EARTH_RADIUS + 400_000.0;
    let escape = ship(
        State {
            position: [r, 0.0, 0.0],
            velocity: [0.0, 0.0, circular_speed(EARTH_GM, r) * 1.6],
        },
        0.0,
    );
    let prediction = predict(&escape, &Earth).unwrap();
    assert!(!prediction.bound);
    assert_eq!(prediction.end, Some(End::Horizon));
    let last = prediction.samples.last().unwrap();
    assert!(distance(last.position, [0.0; 3]) > 1.5 * r);

    let falling = ship(
        State {
            position: [r, 0.0, 0.0],
            velocity: [-200.0, 0.0, 6_000.0],
        },
        0.0,
    );
    let prediction = predict(&falling, &Earth).unwrap();
    assert_eq!(
        prediction.end,
        Some(End::Impact {
            body: "earth".into()
        })
    );
    let last = prediction.samples.last().unwrap();
    let depth = EARTH_RADIUS - distance(last.position, [0.0; 3]);
    assert!((0.0..50_000.0).contains(&depth), "{depth}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_prediction_runs_in_slices_and_says_how_far_it_got() {
    let (craft, _) = eccentric();
    let mut predictor = Predictor::new(&craft, &Earth).unwrap();
    assert!(!predictor.run(&Earth, 100));
    let partial = predictor.prediction();
    assert_eq!(partial.end, None);
    assert_eq!(partial.samples.len(), 101);
    assert!(partial.achieved_seconds() < partial.horizon_seconds / 2.0);
    while !predictor.run(&Earth, 100) {}
    assert_eq!(
        predictor.into_prediction(),
        predict(&craft, &Earth).unwrap(),
        "slices give the same path as one go"
    );
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

fn around(field: &Field, planet: &Planet, height: f64, speed_share: f64) -> Craft {
    let seconds = 1.0e6;
    let body = field.reference_by_id(&planet.id, seconds).unwrap();
    let r = body.radius_m + height;
    let speed = circular_speed(body.gm, r) * speed_share;
    let state = State {
        position: std::array::from_fn(|i| body.state.position[i] + [r, 0.0, 0.0][i]),
        velocity: std::array::from_fn(|i| body.state.velocity[i] + [0.0, 0.0, speed][i]),
    };
    ship(state, seconds)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn an_orbit_around_a_moving_planet_closes_relative_to_it() {
    let (field, planet) = planet_field();
    let craft = around(&field, &planet, 1_000_000.0, 1.0);
    let prediction = predict(&craft, &field).unwrap();
    assert_eq!(prediction.reference, planet.id);
    assert_eq!(prediction.end, Some(End::Horizon));
    assert!(prediction.bound);
    assert!(
        !prediction
            .events
            .iter()
            .any(|e| matches!(e, Event::Reference { .. }))
    );
    // Relative to the planet the lap closes, the star's tide aside, though
    // the planet itself has moved on by thousands of kilometres.
    let first = prediction.samples[0];
    let r = distance(first.position, [0.0; 3]);
    let lap = prediction
        .samples
        .iter()
        .find(|s| s.seconds - first.seconds >= prediction.period_seconds.unwrap())
        .unwrap();
    assert!(distance(lap.position, first.position) < 0.01 * r);
    let moved = distance(
        crate::planet_position(field.system(), &planet, lap.seconds),
        crate::planet_position(field.system(), &planet, first.seconds),
    );
    assert!(moved > 10.0 * r, "{moved}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn an_escape_reports_leaving_the_planet_for_its_star() {
    let (field, planet) = planet_field();
    let craft = around(&field, &planet, 1_000_000.0, 2.0);
    let prediction = predict(&craft, &field).unwrap();
    assert_eq!(prediction.reference, planet.id);
    assert!(!prediction.bound);
    let star = field.system().stars_on(planet.page).next().unwrap();
    let exit = prediction.events.iter().find_map(|e| match e {
        Event::Reference { seconds, id } => Some((seconds, id)),
        _ => None,
    });
    let (seconds, id) = exit.expect("an exit event");
    assert_eq!(id, &star.id);
    // Past the Hill sphere, by the margin, when it leaves.
    let hill = field.reference_by_id(&planet.id, 0.0).unwrap().reach_m;
    let at = prediction
        .samples
        .iter()
        .find(|s| s.seconds == *seconds)
        .unwrap();
    let out = distance(at.position, [0.0; 3]) / hill;
    assert!((1.0 + SWITCH_MARGIN..1.2).contains(&out), "{out}");
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_whole_lap_stays_ahead_until_a_refresh_is_due() {
    let (craft, orbit) = eccentric();
    let prediction = predict(&craft, &Earth).unwrap();
    let period = orbit.period().unwrap();
    assert_eq!(prediction.period_seconds, Some(period));
    for share in [0.0, 0.2, LEAD - 0.01] {
        let now = craft.seconds + share * period;
        // One revolution exactly, which for Earth alone is one period, so
        // the path meets the craft without drawing over itself.
        let end = prediction.lap_end(now).unwrap();
        assert!(((end - now) / period - 1.0).abs() < 1e-4, "{share}: {end}");
        let here = prediction.position_at(now).unwrap();
        let there = prediction.position_at(end).unwrap();
        assert!(distance(here, there) < 1_000.0, "{share}");
        assert!(prediction.ahead(now).last().unwrap().seconds <= end);
        assert!(prediction.upcoming(now).all(|e| e.seconds() <= end));
    }
}

/// Earth with a steady sideways push, standing in for a strong tug from a
/// moon or passing planet.
struct Tugged;

impl Gravity for Tugged {
    fn acceleration(&self, position: [f64; 3], seconds: f64) -> [f64; 3] {
        let mut sum = Earth.acceleration(position, seconds);
        sum[1] += 0.05;
        sum
    }
    fn reference(&self, id: Option<&str>, position: [f64; 3], seconds: f64) -> Option<State> {
        Earth.reference(id, position, seconds)
    }
    fn orbit_reference(
        &self,
        current: Option<&str>,
        position: [f64; 3],
        seconds: f64,
    ) -> Option<Reference> {
        Earth.orbit_reference(current, position, seconds)
    }
    fn closest(&self, position: [f64; 3], seconds: f64) -> Option<Closest> {
        Earth.closest(position, seconds)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn only_a_revolution_that_ends_at_the_craft_closes() {
    let (craft, _) = eccentric();
    let quiet = predict(&craft, &Earth).unwrap();
    assert!(quiet.closing_lap(0.0).is_some());
    // Pushed off course, it still sweeps a revolution, but ends elsewhere.
    let bent = predict(&craft, &Tugged).unwrap();
    let end = bent.lap_end(0.0).expect("a revolution");
    let miss = distance(bent.position_at(end).unwrap(), bent.samples[0].position);
    let r = distance(bent.samples[0].position, [0.0; 3]);
    assert!(miss > CLOSE_SHARE * r, "{miss}");
    assert_eq!(bent.closing_lap(0.0), None);
    assert_eq!(bent.shown_end(0.0), end);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn a_slow_escape_still_reaches_the_edge_of_the_hill_sphere() {
    let (field, planet) = planet_field();
    // Barely unbound: it climbs out far slower than it starts.
    let craft = around(&field, &planet, 1_000_000.0, 1.42);
    let prediction = predict(&craft, &field).unwrap();
    assert!(!prediction.bound);
    assert!(
        prediction
            .events
            .iter()
            .any(|e| matches!(e, Event::Reference { .. })),
        "{:?} after {} days",
        prediction.end,
        prediction.achieved_seconds() / 86_400.0
    );
    assert_ne!(prediction.end, Some(End::Truncated));
}
