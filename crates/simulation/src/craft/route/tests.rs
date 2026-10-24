use super::*;
use crate::craft::warp::GravityPolicy;
use crate::craft::{CraftKind, Frame, Order, WarpOrder};
use crate::system::atmosphere::{Composition, Gas, GasAtmosphere, ReferenceLevel};
use space_shared::{Barycentre, Orbit, Planet, Star, StarSystem};

fn star(letter: &str, mass_solar: f64) -> Star {
    Star {
        id: format!("t#{letter}"),
        page: 0,
        spectral: "G2 V".into(),
        kind: "Yellow dwarf".into(),
        mass_solar,
        radius_solar: 1.0,
        luminosity_solar: 1.0,
        temperature_k: 5_772.0,
        colour: [255; 3],
        spin: Default::default(),
        component: letter.into(),
    }
}

fn pair(letters: &str, members: [&str; 2], semi_major_au: f64) -> Barycentre {
    Barycentre {
        id: format!("t#{letters}"),
        page: 0,
        component: letters.into(),
        members: members.map(|m| format!("t#{m}")),
        orbit: Orbit {
            semi_major_au,
            ..Orbit::default()
        },
    }
}

/// Generated and airless: an unknown atmosphere refuses routes.
fn airless() -> space_shared::PlanetEnvironment {
    space_shared::PlanetEnvironment {
        irradiation_w_m2: 1_361.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: 255.0,
        age_gyr: 4.5,
        atmosphere: None,
    }
}

/// A hierarchical triple, (AB)C: A and B `ab_au` apart, C 400 AU from
/// their barycentre, with an Earth at 1 AU around A.
fn triple(ab_au: f64) -> Field {
    let planet = Planet {
        id: "t#A/0".into(),
        host: "t#A".into(),
        orbit: Orbit {
            semi_major_au: 1.0,
            ..Orbit::default()
        },
        radius_km: 6_371.0,
        mass_earth: 1.0,
        environment: Some(airless()),
        ..Planet::default()
    };
    Field::new(
        StarSystem {
            id: "t".into(),
            name: "Test".into(),
            position_ly: [0.0; 3],
            planets: vec![planet],
            stars: vec![star("A", 1.0), star("B", 1.0), star("C", 1.0)],
            barycentres: vec![
                pair("AB", ["A", "B"], ab_au),
                pair("ABC", ["AB", "C"], 400.0),
            ],
            page_stars: Vec::new(),
        },
        0,
    )
}

/// One Sun-like star, `t`, keyed by its own ID.
fn single() -> Field {
    let mut sun = star("A", 1.0);
    sun.id = "t".into();
    Field::new(
        StarSystem {
            id: "t".into(),
            name: "Single".into(),
            position_ly: [0.0; 3],
            planets: Vec::new(),
            stars: vec![sun],
            barycentres: Vec::new(),
            page_stars: Vec::new(),
        },
        0,
    )
}

/// A ship at rest `away` from a target's centre at time zero, nose along
/// `nose`, not yet in warp.
fn ship(field: &Field, target: &str, away: DVec3, nose: DVec3) -> Craft {
    let centre = field.target(target, 0.0).unwrap().state.position;
    let mut craft = Craft::new(
        "ship".into(),
        "Ship".into(),
        CraftKind::Ship,
        Frame::System { star: "t".into() },
        0,
    );
    craft.place(
        State {
            position: (DVec3::from_array(centre) + away).to_array(),
            velocity: [0.0; 3],
        },
        0,
    );
    craft.orientation = DQuat::from_rotation_arc(-DVec3::Z, nose.normalize()).to_array();
    craft
}

/// Step until the route ends, returning the seconds flown.
fn fly(craft: &mut Craft, field: &Field) -> f64 {
    let start = craft.seconds();
    for _ in 0..(2.0 * MAX_TRIP_S / STEP_SECONDS) as usize {
        if !craft.route.as_ref().unwrap().active() {
            break;
        }
        craft.step(field);
    }
    craft.seconds() - start
}

fn order(craft: &mut Craft, field: &Field, command: WarpOrder) -> Result<(), String> {
    Order::Warp { command }.apply(craft, Some(field))
}

fn to(target: &str, matched: bool, orbit: bool, time: Option<f64>) -> WarpOrder {
    WarpOrder::To {
        target: target.into(),
        matched,
        orbit,
        time,
    }
}

/// A ship lined up on `target` from `standoffs` standoffs away.
fn lined_up(field: &Field, target: &str, standoffs: f64) -> Craft {
    let standoff = field.target(target, 0.0).unwrap().standoff().unwrap();
    let away = DVec3::new(1.0, 0.3, -0.2).normalize() * standoff * standoffs;
    ship(field, target, away, -away)
}

#[test]
fn every_kind_of_body_has_a_standoff_between_its_surface_and_reach() {
    let field = triple(20.0);
    let radius = crate::SUN_RADIUS_METRES;
    let planet = field.target("t#A/0", 0.0).unwrap();
    assert_eq!(planet.standoff().unwrap(), 4.0 * 6_371_000.0);
    let star = field.target("t#B", 0.0).unwrap();
    assert_eq!(star.standoff().unwrap(), 4.0 * radius);
    // A barycentre's size is its pair's separation; it keeps clear of its
    // stars, not of an empty centre.
    let pair = field.target("t#AB", 0.0).unwrap();
    assert!((pair.size_m / AU_METRES - 20.0).abs() < 1e-9);
    assert!((pair.clear_m - (10.0 * AU_METRES + KEEP_CLEAR * radius)).abs() < 1.0);
    let standoff = pair.standoff().unwrap();
    assert!(standoff > pair.clear_m && standoff < pair.reach_m.unwrap());
    // A system's key names its outermost barycentre.
    let system = field.target("t", 0.0).unwrap();
    assert_eq!(system.id, "t#ABC");
    assert_eq!(system.standoff().unwrap(), 4.0 * 400.0 * AU_METRES);
    assert_eq!(system.system, voyage::page_star_id(field.system(), 0));
    // A single star has no stable zone to stay inside.
    let alone = single().target("t", 0.0).unwrap();
    assert_eq!(alone.reach_m, Some(f64::INFINITY));
    assert_eq!(alone.standoff().unwrap(), 4.0 * radius);
    // In a tight pair a star's stable zone barely clears its surface.
    let tight = triple(0.02);
    let refused = tight.target("t#B", 0.0).unwrap().standoff();
    assert!(refused.unwrap_err().contains("no safe standoff"));
}

#[test]
fn flies_to_each_kind_of_target_and_drops_out_at_its_standoff_on_time() {
    let (triple, single) = (triple(20.0), single());
    let targets = [
        (&triple, "t#A/0"),
        (&triple, "t#B"),
        (&triple, "t#AB"),
        (&triple, "t"),
        (&single, "t"),
    ];
    for (field, target) in targets {
        let mut craft = lined_up(field, target, 1e3);
        order(&mut craft, field, to(target, false, false, None)).unwrap();
        assert!(craft.warp.engaged);
        let route = craft.route.clone().unwrap();
        assert_eq!(route.trip_s, default_trip_s(route.distance_m));
        let flown = fly(&mut craft, field);
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived, "{target}: {route:?}");
        assert!(!craft.warp.engaged);
        // At the standoff, by at most a step's travel either side.
        assert!(
            (route.distance_m / route.standoff_m - 1.0).abs() < 0.1,
            "{target}: {route:?}"
        );
        assert!((flown - route.trip_s).abs() < 0.5, "{target}: {flown} s");
        assert_eq!(craft.autopilot, AttitudePilot::Manual);
        assert_eq!(craft.spin, [0.0; 3]);
    }
}

#[test]
fn arrival_keeps_velocity_matches_or_drops_into_a_rough_orbit() {
    let field = triple(20.0);
    let planet = "t#A/0";
    for (matched, orbit) in [(false, false), (true, false), (false, true)] {
        let mut craft = lined_up(&field, planet, 1e4);
        let before = craft.state().velocity;
        order(&mut craft, &field, to(planet, matched, orbit, None)).unwrap();
        fly(&mut craft, &field);
        let target = field.target(planet, craft.seconds()).unwrap();
        let relative =
            DVec3::from_array(craft.state().velocity) - DVec3::from_array(target.state.velocity);
        let r = distance(craft.state().position, target.state.position);
        match (matched, orbit) {
            (false, false) => {
                // Only gravity during the trip changed it.
                let change = DVec3::from_array(craft.state().velocity) - DVec3::from_array(before);
                assert!(change.length() < 10.0, "{change}");
                assert!(craft.warp.last_match.is_none());
            }
            (true, false) => {
                // Gravity acts on the rest of the step after dropout.
                assert!(relative.length() < 0.1, "{relative}");
            }
            _ => {
                let circular = circular_speed(target.gm, r);
                assert!((relative.length() / circular - 1.0).abs() < 1e-3);
                let radial = (DVec3::from_array(craft.state().position)
                    - DVec3::from_array(target.state.position))
                    / r;
                assert!(relative.dot(radial).abs() < 1e-4 * circular);
                assert!(craft.warp.last_match.as_ref().unwrap().orbit);
            }
        }
    }
}

#[test]
fn trip_times_from_ten_seconds_to_an_hour_are_kept() {
    let field = triple(20.0);
    for time in [MIN_TRIP_S, 60.0, 600.0, MAX_TRIP_S] {
        let mut craft = lined_up(&field, "t#A/0", 1e4);
        order(&mut craft, &field, to("t#A/0", true, false, Some(time))).unwrap();
        let flown = fly(&mut craft, &field);
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived);
        assert!((flown - time).abs() < 0.5, "{time} s trip took {flown} s");
        assert!(route.late_s.abs() < 0.5);
    }
}

#[test]
fn the_trip_time_includes_turning_round_first() {
    let field = triple(20.0);
    let target = "t#A/0";
    let standoff = field.target(target, 0.0).unwrap().standoff().unwrap();
    let away = DVec3::X * standoff * 1e4;
    // Facing away, the turn alone at 30 °/s takes most of ten seconds, so
    // the trip runs late and says so from the start.
    let mut craft = ship(&field, target, away, away);
    order(
        &mut craft,
        &field,
        to(target, false, false, Some(MIN_TRIP_S)),
    )
    .unwrap();
    let late = craft.route.as_ref().unwrap().late_s;
    assert!(late > 0.0);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Align);
    craft.step(&field);
    assert_eq!(craft.warp.requested_ms, MIN_SPEED, "held low while turning");
    let flown = fly(&mut craft, &field);
    assert!(
        (flown - MIN_TRIP_S - late).abs() < 1.5,
        "{flown} s, {late} s late"
    );
    // With time to spare, the same turn still arrives on time.
    let mut craft = ship(&field, target, away, away);
    order(&mut craft, &field, to(target, false, false, Some(30.0))).unwrap();
    assert!(craft.route.as_ref().unwrap().late_s.abs() < 1e-9);
    let flown = fly(&mut craft, &field);
    assert!((flown - 30.0).abs() < 0.5, "{flown} s");
}

#[test]
fn both_gravity_policies_arrive() {
    let field = triple(20.0);
    for policy in [GravityPolicy::Active, GravityPolicy::Suspended] {
        let mut craft = lined_up(&field, "t#B", 100.0);
        order(&mut craft, &field, WarpOrder::Gravity { policy }).unwrap();
        order(&mut craft, &field, to("t#B", true, false, Some(120.0))).unwrap();
        let flown = fly(&mut craft, &field);
        assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
        assert!((flown - 120.0).abs() < 0.5, "{policy:?}: {flown} s");
    }
}

#[test]
fn a_moving_target_is_met_despite_the_ships_own_drift() {
    let field = triple(20.0);
    // A slow trip, so the planet moves far further than its standoff, from
    // a ship already drifting fast across the line.
    let mut craft = lined_up(&field, "t#A/0", 1e3);
    let mut state = craft.state();
    state.velocity = [0.0, 0.0, 80_000.0];
    craft.place(state, 0);
    order(&mut craft, &field, to("t#A/0", true, false, Some(1_800.0))).unwrap();
    fly(&mut craft, &field);
    let route = craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived);
    assert!(route.distance_m > 0.9 * route.standoff_m);
}

#[test]
fn cancelling_or_taking_over_leaves_warp_as_it_was() {
    let field = triple(20.0);
    let takeovers = [
        Order::Rotate {
            pitch: 0.0,
            yaw: 0.5,
            roll: 0.0,
        },
        Order::Point {
            target: PointTarget::Vector([1.0, 0.0, 0.0]),
            up: None,
        },
        Order::Stop,
        Order::Release,
        Order::Cut,
        Order::Warp {
            command: WarpOrder::Up,
        },
        Order::Warp {
            command: WarpOrder::Set { speed_ms: 1e6 },
        },
        Order::Warp {
            command: WarpOrder::Sweep { rate: -1.0 },
        },
        Order::Warp {
            command: WarpOrder::Cancel,
        },
    ];
    for takeover in takeovers {
        let mut craft = lined_up(&field, "t#B", 1e3);
        order(&mut craft, &field, to("t#B", false, false, Some(60.0))).unwrap();
        for _ in 0..200 {
            craft.step(&field);
        }
        assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Cruise);
        let (speed, orientation) = (craft.warp.actual_ms, craft.orientation);
        takeover.apply(&mut craft, Some(&field)).unwrap();
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Released, "{takeover:?}");
        assert!(craft.warp.engaged);
        assert_eq!(
            (craft.warp.actual_ms, craft.orientation),
            (speed, orientation)
        );
        if matches!(takeover, Order::Warp { .. }) {
            assert_eq!(craft.autopilot, AttitudePilot::Manual);
        }
        let requested = craft.warp.requested_ms;
        craft.step(&field);
        if craft.warp.sweep == 0.0 {
            assert_eq!(craft.warp.requested_ms, requested, "{takeover:?}");
        } else {
            assert!(craft.warp.requested_ms < requested, "{takeover:?}");
        }
    }
    let mut craft = lined_up(&field, "t#B", 1e3);
    let cancel = WarpOrder::Cancel;
    assert!(order(&mut craft, &field, cancel.clone()).is_err());
    order(&mut craft, &field, to("t#B", false, false, None)).unwrap();
    order(&mut craft, &field, WarpOrder::Disengage).unwrap();
    assert_eq!(craft.route.as_ref().unwrap().end.as_deref(), Some(TAKEOVER));
    assert!(order(&mut craft, &field, cancel).is_err());
}

#[test]
fn refused_routes_leave_the_ship_unchanged() {
    let field = triple(20.0);
    let tight = triple(0.02);
    let craft = lined_up(&field, "t#B", 1e3);
    let refusals: [(&Field, WarpOrder, &str); 6] = [
        (&field, to("t/99", false, false, None), "No body"),
        (&field, to("ship", false, false, None), "itself"),
        (&tight, to("t#B", false, false, None), "no safe standoff"),
        (&field, to("t#B", false, false, Some(9.0)), "Trip time"),
        (&field, to("t#B", false, false, Some(3_601.0)), "Trip time"),
        (&field, to("t#B", false, false, Some(f64::NAN)), "Trip time"),
    ];
    for (field, command, reason) in refusals {
        let mut tried = craft.clone();
        let err = order(&mut tried, field, command).unwrap_err();
        assert!(err.contains(reason), "{err}");
        assert_eq!(tried, craft);
    }
    // Another system's target, or any from galactic space, waits for its
    // system's data without engaging warp; a thrusting ship is refused.
    for field in [Some(&field), None] {
        let mut tried = craft.clone();
        Order::Warp {
            command: to("elsewhere#A/2", false, true, None),
        }
        .apply(&mut tried, field)
        .unwrap();
        let route = tried.route.as_ref().unwrap();
        assert_eq!(
            (route.stage, route.system.as_str()),
            (Stage::Plotting, "elsewhere")
        );
        assert!(!tried.warp.engaged);
        assert_eq!(tried.autopilot, craft.autopilot);
    }
    // Already there.
    let mut near = lined_up(&field, "t#B", 0.5);
    let before = near.clone();
    let err = order(&mut near, &field, to("t#B", false, false, None)).unwrap_err();
    assert!(err.contains("Already within"), "{err}");
    assert_eq!(near, before);
    // Sublight thrust must be cut before warp engages.
    let mut thrusting = craft.clone();
    thrusting.input.controls.throttle = 0.5;
    let before = thrusting.clone();
    assert!(order(&mut thrusting, &field, to("t#B", false, false, None)).is_err());
    assert_eq!(thrusting, before);
}

#[test]
fn a_route_replays_exactly_from_a_snapshot() {
    let field = triple(20.0);
    let mut craft = lined_up(&field, "t#A/0", 1e4);
    order(&mut craft, &field, to("t#A/0", false, true, Some(40.0))).unwrap();
    for _ in 0..300 {
        craft.step(&field);
    }
    let mut restored: Craft =
        serde_json::from_str(&serde_json::to_string(&craft).unwrap()).unwrap();
    fly(&mut craft, &field);
    fly(&mut restored, &field);
    assert_eq!(restored, craft);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
}

#[test]
fn the_profile_fits_its_time_and_scales_its_default() {
    let (distance, standoff) = (1e12, 1e7);
    let tau = fit_tau(distance, standoff, 30.0);
    assert!((profile_seconds(distance, standoff, tau) - 30.0).abs() < 1e-9);
    assert_eq!(fit_tau(distance, standoff, 0.0), MIN_TAU_S);
    // At the top speed the profile is cut off and the trip stretches.
    let far = 1e5 * crate::LY_METRES;
    assert!(profile_seconds(far, standoff, 1.0) > (far / standoff).ln());
    assert_eq!(default_trip_s(0.01 * AU_METRES), SHORT_TRIP_S);
    assert!((default_trip_s(100.0 * AU_METRES) - 40.0).abs() < 1e-9);
    assert_eq!(default_trip_s(crate::LY_METRES), LONG_TRIP_S);
    assert_eq!(align_seconds(0.01, 0.5), 0.0);
    // Facing away at 30 °/s: about six seconds flat out, then easing in.
    let turn = align_seconds(std::f64::consts::PI, 30f64.to_radians());
    assert!((7.0..10.0).contains(&turn), "{turn}");
}

fn planet(
    index: u32,
    semi_major_au: f64,
    phase_rad: f64,
    radius_km: f64,
    mass_earth: f64,
) -> Planet {
    Planet {
        id: format!("t/{index}"),
        host: "t".into(),
        orbit: Orbit {
            semi_major_au,
            phase_rad,
            ..Orbit::default()
        },
        radius_km,
        mass_earth,
        environment: Some(airless()),
        ..Planet::default()
    }
}

/// One star, `t`, with a Jupiter `t/0` at 1 AU, a Saturn `t/2` at 1.5 AU and
/// an Earth `t/1` at 2 AU in line behind it, and an Earth `t/3` at 1 AU
/// opposite.
fn blocked() -> Field {
    Field::new(blocked_system(), 0)
}

fn blocked_system() -> StarSystem {
    let mut sun = star("A", 1.0);
    sun.id = "t".into();
    StarSystem {
        id: "t".into(),
        name: "Blocked".into(),
        position_ly: [0.0; 3],
        planets: vec![
            planet(0, 1.0, 0.0, 69_911.0, 317.8),
            planet(1, 2.0, 0.0, 6_371.0, 1.0),
            planet(2, 1.5, 0.0, 58_232.0, 95.2),
            planet(3, 1.0, std::f64::consts::PI, 6_371.0, 1.0),
        ],
        stars: vec![sun],
        barycentres: Vec::new(),
        page_stars: Vec::new(),
    }
}

/// `blocked`, with planet `index` spun fast and given a gas atmosphere at
/// `pressure_pa` and `temperature_k`, so its clearance grows with both its
/// equatorial bulge and its air.
fn airy(
    index: usize,
    gases: &[(Gas, f64)],
    level: ReferenceLevel,
    pressure_pa: f64,
    temperature_k: f64,
) -> Field {
    let mut system = blocked_system();
    let planet = &mut system.planets[index];
    planet.spin.period_hours = 10.0;
    let gm = planet.mass_earth * crate::EARTH_SOLAR * crate::SUN_GM;
    let gas = GasAtmosphere::new(
        Composition::new(gases).unwrap(),
        level,
        pressure_pa,
        temperature_k,
        None,
    )
    .unwrap();
    planet.environment.as_mut().unwrap().atmosphere =
        Some(gas.to_data(gm, planet.radius_km * 1e3).unwrap());
    Field::new(system, 0)
}

fn obstacle(field: &Field, id: &str) -> Obstacle {
    field
        .obstacles(0.0)
        .into_iter()
        .find(|o| o.id == id)
        .unwrap()
}

#[test]
fn atmosphere_and_oblateness_widen_a_planets_clearance() {
    let bare = obstacle(&blocked(), "t/0");
    let field = airy(
        0,
        &[(Gas::Hydrogen, 0.86), (Gas::Helium, 0.14)],
        ReferenceLevel::OneBar,
        1e5,
        165.0,
    );
    let jupiter = obstacle(&field, "t/0");
    let shape = crate::system::shape::planet_shape(&field.system().planets[0]);
    assert!(shape.equatorial > 69_911e3 * 1.01, "{shape:?}");
    let column = GasAtmosphere::try_from(
        field.system().planets[0]
            .environment
            .as_ref()
            .unwrap()
            .atmosphere
            .as_ref()
            .unwrap(),
    )
    .unwrap()
    .column(317.8 * crate::EARTH_SOLAR * crate::SUN_GM, 69_911e3)
    .unwrap();
    // Past the bulge and the visible air, not just the mean radius.
    assert!(
        jupiter.exclusion_m > shape.equatorial + column.extent_m,
        "{jupiter:?}"
    );
    assert!(jupiter.exclusion_m > bare.exclusion_m);
    assert!(jupiter.clear_m > jupiter.exclusion_m);
    // The arrival standoff stays outside the exclusion.
    let target = field.target("t/0", 0.0).unwrap();
    assert!(target.standoff().unwrap() > jupiter.exclusion_m);
}

#[test]
fn a_route_round_an_atmosphere_never_enters_its_exclusion() {
    let field = airy(
        0,
        &[(Gas::Hydrogen, 0.86), (Gas::Helium, 0.14)],
        ReferenceLevel::OneBar,
        1e5,
        165.0,
    );
    let (jupiter, behind) = (centre(&field, "t/0"), centre(&field, "t/2"));
    let exclusion = obstacle(&field, "t/0").exclusion_m;
    let line = (jupiter - behind).normalize();
    let mut craft = ship_at(jupiter + line * 40.0 * exclusion, behind);
    order(&mut craft, &field, to("t/2", true, false, Some(40.0))).unwrap();
    let mut nearest = f64::INFINITY;
    for _ in 0..(2.0 * MAX_TRIP_S / STEP_SECONDS) as usize {
        if !craft.route.as_ref().unwrap().active() {
            break;
        }
        craft.step(&field);
        let at = DVec3::from_array(craft.state().position);
        nearest = nearest.min(at.distance(DVec3::from_array(
            field.target("t/0", craft.seconds()).unwrap().state.position,
        )));
    }
    assert_eq!(
        craft.route.as_ref().unwrap().stage,
        Stage::Arrived,
        "{:?}",
        craft.route
    );
    assert!(nearest > exclusion, "{nearest} within {exclusion}");
}

#[test]
fn unknown_air_a_start_inside_the_exclusion_or_an_unsafe_arrival_is_refused() {
    // Not generated is not airless.
    let mut system = blocked_system();
    system.planets[1].environment = None;
    let unknown = Field::new(system, 0);
    let craft = ship_at(
        centre(&unknown, "t/3") + DVec3::X * 1e9,
        centre(&unknown, "t/3"),
    );
    let mut tried = craft.clone();
    let err = order(&mut tried, &unknown, to("t/3", false, false, None)).unwrap_err();
    assert!(err.contains("unknown"), "{err}");
    assert_eq!(tried, craft);
    // Low inside an Earth's maintenance-free height: leave it by hand first.
    let field = airy(
        3,
        &[(Gas::Nitrogen, 0.79), (Gas::Oxygen, 0.21)],
        ReferenceLevel::Surface,
        101_325.0,
        288.0,
    );
    let earth = obstacle(&field, "t/3");
    let low = centre(&field, "t/3") + DVec3::Z * (6_371e3 + 120e3);
    assert!(low.distance(earth.centre) < earth.exclusion_m);
    let craft = ship_at(low, centre(&field, "t/0"));
    let mut tried = craft.clone();
    let err = order(&mut tried, &field, to("t/0", false, false, None)).unwrap_err();
    assert!(err.contains("inside the navigation exclusion"), "{err}");
    assert_eq!(tried, craft);
    // Another ship below that height cannot be met without entering it.
    let parked = Target::craft(
        "craft-9",
        State {
            position: low.to_array(),
            velocity: [0.0; 3],
        },
        [0.0; 3],
    );
    let mut chaser = ship_at(centre(&field, "t/3") + DVec3::X * 1e9, low);
    let before = chaser.clone();
    let err = chaser
        .fly_to(Some(&field), &[parked], "craft-9", ArrivalMode::Match, None)
        .unwrap_err();
    assert!(err.contains("Arrival region"), "{err}");
    assert_eq!(chaser, before);
}

/// A flight's record: seconds flown, the nearest it came to any body's
/// keep-clear sphere, as a share of its radius, and the bodies steered
/// round.
struct Flight {
    seconds: f64,
    nearest: f64,
    nearest_body: String,
    around: std::collections::BTreeSet<String>,
}

fn fly_watched(craft: &mut Craft, field: &Field) -> Flight {
    let start = craft.seconds();
    let mut flight = Flight {
        seconds: 0.0,
        nearest: f64::INFINITY,
        nearest_body: String::new(),
        around: Default::default(),
    };
    for _ in 0..(2.0 * MAX_TRIP_S / STEP_SECONDS) as usize {
        let route = craft.route.as_ref().unwrap();
        if !route.active() {
            break;
        }
        flight.around.extend(route.around.clone());
        craft.step(field);
        let target = &craft.route.as_ref().unwrap().target;
        let obstacles = field.obstacles(craft.seconds());
        for obstacle in obstacles.into_iter().filter(|o| o.id != *target) {
            let share = obstacle
                .centre
                .distance(DVec3::from_array(craft.state().position))
                / obstacle.clear_m;
            if share < flight.nearest {
                flight.nearest = share;
                flight.nearest_body = obstacle.id;
            }
        }
    }
    flight.seconds = craft.seconds() - start;
    flight
}

fn centre(field: &Field, id: &str) -> DVec3 {
    DVec3::from_array(field.target(id, 0.0).unwrap().state.position)
}

/// A ship at rest at `at`, facing `towards`.
fn ship_at(at: DVec3, towards: DVec3) -> Craft {
    let mut craft = ship(&single(), "t", DVec3::X, towards - at);
    craft.place(
        State {
            position: at.to_array(),
            velocity: [0.0; 3],
        },
        0,
    );
    craft
}

#[test]
fn a_target_directly_or_barely_behind_a_planet_is_reached_round_it_on_time() {
    let field = blocked();
    let (jupiter, behind) = (centre(&field, "t/0"), centre(&field, "t/2"));
    let clear = KEEP_CLEAR * 69_911e3;
    let line = (jupiter - behind).normalize();
    let side = line.any_orthonormal_vector();
    // Straight through its centre, and grazing 0.95 keep-clear radii out.
    for miss in [0.0, 0.95] {
        let at = jupiter + line * 30.0 * clear + side * miss * clear * 31.0 / 30.0;
        let mut craft = ship_at(at, behind);
        order(&mut craft, &field, to("t/2", true, false, Some(40.0))).unwrap();
        let route = craft.route.clone().unwrap();
        assert_eq!(route.around.as_deref(), Some("t/0"), "{miss}");
        assert!(
            route.detour_m > 0.0 && route.late_s.abs() < 1e-6,
            "{route:?}"
        );
        let flight = fly_watched(&mut craft, &field);
        let route = craft.route.as_ref().unwrap();
        assert_eq!(route.stage, Stage::Arrived, "{miss}: {route:?}");
        assert!(flight.around.contains("t/0"));
        assert!(
            flight.nearest > 0.9,
            "{miss}: {} of {}",
            flight.nearest,
            flight.nearest_body
        );
        assert!(
            (flight.seconds - 40.0).abs() < 1.0,
            "{miss}: {} s",
            flight.seconds
        );
    }
}

#[test]
fn two_bodies_in_a_row_are_both_wrapped_round() {
    let field = blocked();
    let (jupiter, far) = (centre(&field, "t/0"), centre(&field, "t/1"));
    let line = (jupiter - far).normalize();
    let mut craft = ship_at(jupiter + line * 1e9, far);
    order(&mut craft, &field, to("t/1", false, false, Some(40.0))).unwrap();
    let flight = fly_watched(&mut craft, &field);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
    assert_eq!(
        flight.around,
        ["t/0", "t/2"].map(String::from).into(),
        "{:?}",
        craft.route
    );
    assert!(
        flight.nearest > 0.9,
        "{} of {}",
        flight.nearest,
        flight.nearest_body
    );
    assert!((flight.seconds - 40.0).abs() < 1.0, "{} s", flight.seconds);
}

#[test]
fn a_binary_companion_on_the_line_is_steered_round() {
    let field = triple(20.0);
    let (a, b) = (centre(&field, "t#A"), centre(&field, "t#B"));
    let line = (b - a).normalize();
    let mut craft = ship_at(b + line * 50.0 * crate::SUN_RADIUS_METRES, a);
    order(&mut craft, &field, to("t#A", true, false, Some(30.0))).unwrap();
    assert_eq!(craft.route.as_ref().unwrap().around.as_deref(), Some("t#B"));
    let flight = fly_watched(&mut craft, &field);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
    assert!(
        flight.nearest > 0.9,
        "{} of {}",
        flight.nearest,
        flight.nearest_body
    );
    assert!((flight.seconds - 30.0).abs() < 1.0, "{} s", flight.seconds);
}

#[test]
fn bodies_well_off_the_line_change_nothing() {
    let field = blocked();
    let (jupiter, behind) = (centre(&field, "t/0"), centre(&field, "t/2"));
    let clear = KEEP_CLEAR * 69_911e3;
    let line = (jupiter - behind).normalize();
    let at = jupiter + line * 30.0 * clear + line.any_orthonormal_vector() * 10.0 * clear;
    let mut craft = ship_at(at, behind);
    order(&mut craft, &field, to("t/2", false, false, Some(30.0))).unwrap();
    let route = craft.route.clone().unwrap();
    assert_eq!(
        (route.around, route.detour_m, route.bends_s),
        (None, 0.0, 0.0)
    );
    let flight = fly_watched(&mut craft, &field);
    assert!(flight.around.is_empty());
    assert!((flight.seconds - 30.0).abs() < 0.5, "{} s", flight.seconds);
}

#[test]
fn a_planet_moving_onto_the_line_is_steered_round_when_it_gets_there() {
    let field = blocked();
    // From 10 AU in to the star over 20 minutes, the ship crosses 1 AU
    // about a third of the way through. Aim the line where the Earth `t/3`
    // will be then: at the start it is well clear.
    let crossing = 0.37 * 1_200.0;
    let earth = DVec3::from_array(field.target("t/3", crossing).unwrap().state.position);
    let star = centre(&field, "t");
    let mut craft = ship_at(star + (earth - star) * 10.0, star);
    order(&mut craft, &field, to("t", false, false, Some(1_200.0))).unwrap();
    assert_eq!(craft.route.as_ref().unwrap().around, None);
    let flight = fly_watched(&mut craft, &field);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
    assert!(flight.around.contains("t/3"), "{:?}", flight.around);
    assert!(
        flight.nearest > 0.9,
        "{} of {}",
        flight.nearest,
        flight.nearest_body
    );
    assert!(
        (flight.seconds - 1_200.0).abs() < 1.0,
        "{} s",
        flight.seconds
    );
}

#[test]
fn leaving_from_behind_a_planet_climbs_round_it() {
    // Low over the Earth's night side, bound for the far star C: the line
    // runs through the planet. Interstellar departures start the same way.
    let field = triple(20.0);
    let (earth, far) = (centre(&field, "t#A/0"), centre(&field, "t#C"));
    let away = (earth - far).normalize();
    let at = earth + away * (6_371e3 + 400e3);
    let mut craft = ship_at(at, at + away.any_orthonormal_vector());
    order(&mut craft, &field, to("t#C", false, false, Some(30.0))).unwrap();
    assert_eq!(
        craft.route.as_ref().unwrap().around.as_deref(),
        Some("t#A/0")
    );
    let flight = fly_watched(&mut craft, &field);
    let route = craft.route.as_ref().unwrap();
    assert_eq!(route.stage, Stage::Arrived, "{route:?}");
    // It starts inside the keep-clear sphere and never sinks deeper.
    let start = (6_371e3 + 400e3) / (KEEP_CLEAR * 6_371e3);
    assert!(
        flight.nearest > 0.98 * start,
        "{} of {}",
        flight.nearest,
        flight.nearest_body
    );
    assert!((flight.seconds - 30.0).abs() < 1.0, "{} s", flight.seconds);
}

#[test]
fn a_time_too_short_for_its_detour_is_flown_as_fast_as_it_can_be() {
    let field = blocked();
    let (jupiter, behind) = (centre(&field, "t/0"), centre(&field, "t/2"));
    let line = (jupiter - behind).normalize();
    let at = jupiter + line * 80_000e3;
    // Facing along the horizon, so the bend, not the first turn, costs most,
    // and turning slowly.
    let side = line.any_orthonormal_vector();
    let mut craft = ship_at(at, at + side);
    craft.warp.turn_rate_dps = 10.0;
    order(
        &mut craft,
        &field,
        to("t/2", false, false, Some(MIN_TRIP_S)),
    )
    .unwrap();
    let route = craft.route.clone().unwrap();
    assert!(route.late_s > 0.0, "{route:?}");
    assert_eq!(route.slowed_by.as_deref(), Some("the detour round `t/0`"));
    let flight = fly_watched(&mut craft, &field);
    assert_eq!(craft.route.as_ref().unwrap().stage, Stage::Arrived);
    assert!(
        flight.nearest > 0.9 * 80.0 / (1.5 * 69.911),
        "{}",
        flight.nearest
    );
    let route_after = craft.route.as_ref().unwrap();
    assert_eq!(route_after.slowed_by, route.slowed_by);
    let planned = MIN_TRIP_S + route.late_s;
    assert!(
        (flight.seconds - planned).abs() < 2.0,
        "{} s, planned {planned} s",
        flight.seconds
    );
}
