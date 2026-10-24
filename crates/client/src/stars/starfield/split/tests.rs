use super::super::precise::RESCAN_PC;
use super::*;
use space_simulation::galaxy::cells::Cell;

#[test]
fn re_enabling_splitting_discards_the_old_search_state() {
    let mut split = Split {
        reach_pc: 0.0,
        next_reach_pc: SPLIT_MIN_REACH_PC,
        scanned_at: Some(DVec3::ZERO),
        gathered_at: Some(bevy::platform::time::Instant::now()),
        ..Split::default()
    };
    split.set_enabled(false);
    assert!(!split.enabled);
    assert!(split.scanned_at.is_none());
    split.set_enabled(true);
    assert!(split.enabled);
    assert_eq!(split.next_reach_pc, SPLIT_REACH_PC);
    assert!(split.scanned_at.is_none());
    assert!(split.gathered_at.is_none());
    // Unchanged toggles must not invalidate useful cached work each frame.
    split.scanned_at = Some(DVec3::ZERO);
    split.set_enabled(true);
    assert_eq!(split.scanned_at, Some(DVec3::ZERO));
}

#[test]
fn empty_split_search_expands_despite_a_busy_grid_leaf() {
    let cell = Cell::containing(16, [0.01; 3]).unwrap();
    let eye = DVec3::splat(0.01);
    let point = StarPoint {
        position_pc: (eye + DVec3::X * 0.01).to_array(),
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 1000.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index: 0,
    };
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![point; 20_000]),
    );
    let mut split = Split {
        next_reach_pc: 0.001,
        ..Split::default()
    };
    assert_eq!(split.gather(&field, eye, 1e-4), 20_000);
    assert!(split.candidates.is_empty());
    assert!(split.next_reach_pc > split.reach_pc);
    split.gathered_at = Some(bevy::platform::time::Instant::now() - SPLIT_RETRY);
    assert!(split.retry_due());
}

#[test]
fn overloaded_split_search_recovers_without_camera_movement() {
    let cell = Cell::containing(16, [0.01; 3]).unwrap();
    let eye = DVec3::splat(0.01);
    let point = StarPoint {
        position_pc: eye.to_array(),
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 1000.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index: 0,
    };
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![point; SPLIT_BUDGET + 1]),
    );
    let mut split = Split::default();
    assert_eq!(
        split.gather(&field, eye, 1e-4),
        SPLIT_ATTEMPTS * (SPLIT_BUDGET + 1)
    );
    assert_eq!(split.reach_pc, 0.0);
    assert!(split.next_reach_pc > 0.0);
    assert!(split.candidates.is_empty());
    split.gathered_at = Some(bevy::platform::time::Instant::now() - SPLIT_RETRY);
    assert!(split.retry_due());
    // A cheaper neighbourhood, without a camera or view change, recovers.
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, vec![point]),
    );
    assert_eq!(split.gather(&field, eye, 1e-4), 1);
    assert!(split.reach_pc > 0.0);
    assert_eq!(split.candidates.len(), 1);
    assert!(split.next_reach_pc > split.reach_pc);
    for _ in 0..20 {
        split.gather(&field, eye, 1e-4);
    }
    assert_eq!(split.reach_pc, SPLIT_REACH_PC);
    assert!(!split.retry_due());
}

#[test]
fn dense_split_search_shrinks_its_margin_and_keeps_nearby_systems() {
    let cell = Cell::containing(16, [0.01; 3]).unwrap();
    let eye = DVec3::splat(0.01);
    let nearby = StarPoint {
        position_pc: (eye + DVec3::X * 0.001).to_array(),
        luminosity_solar: 1.0,
        temperature_k: 5772.0,
        radius_solar: 1.0,
        separation_au: 1000.0,
        component: space_simulation::galaxy::Component::ThinDisc,
        index: 0,
    };
    let distant = StarPoint {
        position_pc: (eye + DVec3::X * 0.8).to_array(),
        ..nearby
    };
    let mut points = vec![distant; SPLIT_BUDGET + 1];
    points.push(nearby);
    let mut field = Starfield::default();
    field.cells.insert(
        cell,
        space_simulation::galaxy::stream::StarCell::new(cell, points),
    );
    let mut split = Split::default();
    for _ in 0..4 {
        split.gather(&field, eye, 1e-4);
    }
    assert!(split.reach_pc > 0.0);
    assert!(split.reach_pc < 1.0);
    assert!(split.slack_pc < RESCAN_PC);
    assert_eq!(split.candidates.len(), 1);
    assert_eq!(split.candidates[0].1, nearby);
}

#[test]
fn a_split_system_is_its_stars_where_their_bodies_are_drawn() {
    let id = "g2e4960.3.6000";
    let (system, plane) = split_system(id);
    assert_eq!(system.stars.len(), 3);
    // The same plane as the system's page when it is visited.
    let visited = crate::navigation::galaxy_nav::generated(id, DVec3::ZERO).unwrap();
    assert_eq!(plane, crate::scene::page_frame(&visited, 0));
    let at = DVec3::new(30.0, -2.0, 5.0);
    let seconds = 1.5e10;
    let stars: Vec<_> = apart(&system, plane, at, seconds).collect();
    let mut light = 0.0;
    for (star, body) in stars.iter().zip(&system.stars) {
        let local = space_simulation::member_position(&visited, 0, &body.id, seconds);
        let expected = at + plane * DVec3::from_array(local) / PC_METRES;
        let drawn = Vec3::from_array(star.position).as_dvec3();
        assert!(drawn.distance(expected) < 1e-5, "{drawn} {expected}");
        light += f64::from(star.luminosity);
        // Never hidden again as a multiple.
        assert!(star.separation_au() < 1e-15);
    }
    let point: f64 = visited.stars.iter().map(|s| s.luminosity_solar).sum();
    assert!((light / point - 1.0).abs() < 1e-6);
    // C is over a thousand AU from the close pair.
    let separation = space_simulation::star::multiple::widest_separation_au(&system.barycentres);
    assert!(separation > 1_000.0);
    let packed = StarInstance::new([0.0; 3], 1.0, 5772.0, separation as f32);
    assert!((packed.separation_au() / separation - 1.0).abs() < 2e-3);
}

#[test]
fn a_crowded_view_keeps_every_system_it_splits_cached() {
    let (system, plane) = split_system("g2e4960.3.6000");
    let entry = |used| (system.clone(), plane, used);
    // More systems in use than the cache's spare room: none is dropped.
    let shown = SPLIT_CACHE + 100;
    let mut built: Built = (0..shown).map(|i| (format!("s{i}"), entry(7))).collect();
    assert!(!prune(&mut built, 7, shown));
    assert_eq!(built.len(), shown);
    // Past the spare room, only those not split this frame go.
    built.extend((0..SPLIT_CACHE + 1).map(|i| (format!("old{i}"), entry(6))));
    assert!(prune(&mut built, 7, shown));
    assert_eq!(built.len(), shown);
    assert!(built.values().all(|(_, _, used)| *used == 7));
}
