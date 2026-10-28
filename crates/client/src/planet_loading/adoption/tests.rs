use super::*;
use bevy::math::DVec3;
use space_simulation::system::generated::{Mode, generated_system_with_mode};

fn fixture() -> (
    PlanetLoading,
    Navigation,
    Galaxy,
    Fleet,
    CameraRig,
    CameraPose,
    String,
) {
    let key = "g3c0200000.2.128";
    let prepared = generated_system_with_mode(key, [0.0; 3], Mode::LayeredPrototype).unwrap();
    let (id, refinement) = prepared
        .jobs
        .iter()
        .find_map(|j| {
            let outcome = j.resolve();
            if !matches!(
                outcome,
                space_simulation::system::structure::prototype::Outcome::Layered(_)
            ) {
                return None;
            }
            Some((
                j.id.clone(),
                physical::prepare_refinement(&j.initial_result, &j.environment, &outcome).ok()?,
            ))
        })
        .unwrap();
    let mut loading = PlanetLoading::default();
    loading.enabled = true;
    loading.target(Some(key.into()));
    let cache = loading.cache.get_mut(key).unwrap();
    cache.planets.insert(
        id.clone(),
        PlanetState::Solved(Box::new(refinement.result.record.clone())),
    );
    cache.ready.insert(id.clone(), refinement);
    let nav = Navigation {
        visit: Some(crate::Visit::Generated(Box::new(prepared.system))),
        ..Default::default()
    };
    let pose = CameraPose {
        eye: DVec3::splat(1e14),
        ..Default::default()
    };
    (
        loading,
        nav,
        Galaxy::default(),
        Fleet::default(),
        CameraRig::placed_for_test(DVec3::ZERO),
        pose,
        id,
    )
}

#[test]
fn distant_camera_adopts_once_and_all_projections_validate() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    let before = nav.system(&galaxy).unwrap().clone();
    assert!(publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    let next = nav
        .system(&galaxy)
        .unwrap()
        .planets
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    physical::validate(next).unwrap().unwrap();
    assert_eq!(next.initial_result.as_ref().unwrap().revision.sequence, 1);
    let expected = next.clone();
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    // LRU eviction and disabling calculation cannot undo authoritative facts.
    l.target(None);
    for index in 0..8 {
        l.target(Some(format!("other-{index}")));
    }
    assert!(!l.cache.contains_key(&before.id));
    l.enabled = false;
    let mut revisit = before;
    assert!(fleet.charts.restore_system(&mut revisit));
    assert_eq!(
        revisit.planets.iter().find(|p| p.id == id).unwrap(),
        &expected
    );
    assert!(!fleet.charts.restore_system(&mut revisit));
    // Rebuilding an evicted candidate cache reuses the committed result instead
    // of solving it again and reporting an already-adopted base as stale.
    let prepared =
        generated_system_with_mode(&revisit.id, [0.0; 3], Mode::LayeredPrototype).unwrap();
    l.target(Some(revisit.id.clone()));
    let base = prepared
        .jobs
        .iter()
        .find(|j| j.id == id)
        .unwrap()
        .initial_result
        .revision
        .clone();
    l.cache
        .get_mut(&revisit.id)
        .unwrap()
        .bases
        .insert(id.clone(), base);
    l.queue.extend(prepared.jobs);
    l.reuse_adopted(&fleet);
    assert!(!l.queue.iter().any(|job| job.id == id));
    assert!(l.adoption(&id).unwrap().starts_with("Applied:"));
}

#[test]
fn close_camera_defers_without_moving_then_publishes_after_departure() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, mut pose, id) = fixture();
    let system = nav.system(&galaxy).unwrap();
    let old = system.planets.iter().find(|p| p.id == id).unwrap();
    pose.eye = crate::scene::page_frame(system, 0)
        * DVec3::from_array(space_simulation::planet_position(system, old, 0.0));
    let eye = pose.eye;
    let before = system.clone();
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert_eq!(nav.system(&galaxy).unwrap(), &before);
    assert_eq!(pose.eye, eye);
    assert!(l.adoption(&id).unwrap().contains("camera is near"));
    pose.eye = DVec3::splat(1e14);
    assert!(publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
}

#[test]
fn craft_system_is_pinned_even_far_from_the_changed_planet() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    let system = nav.system(&galaxy).unwrap().clone();
    let old = system.clone();
    fleet
        .spawn(system, 0, &id, 1e8, crate::craft::Launch::Circular, 0.0)
        .unwrap();
    let craft = fleet.craft[0].craft.clone();
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert_eq!(nav.system(&galaxy).unwrap(), &old);
    assert_eq!(fleet.craft[0].craft, craft);
    assert!(l.adoption(&id).unwrap().contains("craft retains"));
    fleet.despawn(None);
    assert!(publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    // New craft inherit the authoritative projection even if their caller had
    // only seeded fallback data, without moving an existing craft.
    fleet
        .spawn(old, 0, &id, 1e8, crate::craft::Launch::Circular, 0.0)
        .unwrap();
    assert_eq!(
        fleet.craft[0]
            .field
            .system()
            .planets
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .initial_result
            .as_ref()
            .unwrap()
            .revision
            .sequence,
        1
    );
}

#[test]
fn disabled_unplaced_and_stale_results_never_publish() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    l.enabled = false;
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    l.enabled = true;
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &CameraRig::default(),
        &pose,
        0.0
    ));
    if let Some(crate::Visit::Generated(system)) = &mut nav.visit {
        system
            .planets
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .initial_result
            .as_mut()
            .unwrap()
            .revision
            .inputs
            .push_str(":new");
    }
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert!(l.adoption(&id).unwrap().contains("Discarded"));
    assert!(l.cache[l.active.as_ref().unwrap()].ready.is_empty());
}

#[test]
fn conflicting_loaded_copies_are_not_partially_published() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    let before = nav.system(&galaxy).unwrap().clone();
    let mut other = before.clone();
    other
        .planets
        .iter_mut()
        .find(|p| p.id == id)
        .unwrap()
        .initial_result
        .as_mut()
        .unwrap()
        .revision
        .inputs
        .push_str(":different");
    galaxy.systems.push(other.clone());
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert_eq!(nav.system(&galaxy).unwrap(), &before);
    assert_eq!(galaxy.systems[0], other);
}

#[test]
fn changed_inputs_restart_preparation_and_reject_the_old_epoch() {
    let (mut l, mut nav, galaxy, _, _, _, id) = fixture();
    let key = l.active.clone().unwrap();
    let base = l.cache[&key].ready[&id].base.clone();
    l.cache
        .get_mut(&key)
        .unwrap()
        .bases
        .insert(id.clone(), base);
    let epoch = l.epoch;
    if let Some(crate::Visit::Generated(system)) = &mut nav.visit {
        system
            .planets
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .initial_result
            .as_mut()
            .unwrap()
            .revision
            .inputs
            .push_str(":changed");
    }
    l.invalidate_changed_inputs(&nav, &galaxy);
    assert_ne!(l.epoch, epoch);
    assert!(l.pending_prepare);
    l.sender
        .send(Message::Planet {
            epoch,
            id,
            result: Err("old worker error".into()),
        })
        .unwrap();
    l.receive();
    assert!(l.cache[&key].planets.is_empty());
}

#[test]
fn active_routes_pin_even_an_unrelated_system() {
    use space_simulation::craft::route::{ArrivalMode, Route, Stage};
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    let unrelated = space_simulation::generated_system("g13b6ffffcfeba.2.1", [0.0; 3]).unwrap();
    let body = unrelated.planets[0].id.clone();
    fleet
        .spawn(
            unrelated,
            0,
            &body,
            1e8,
            crate::craft::Launch::Circular,
            0.0,
        )
        .unwrap();
    fleet.craft[0].craft.route = Some(Route {
        target: id,
        system: l.active.clone().unwrap(),
        arrival: ArrivalMode::Keep,
        stage: Stage::Plotting,
        requested_s: None,
        trip_s: 60.0,
        start_tick: 0,
        standoff_m: 1e8,
        tau_s: 10.0,
        distance_m: 1e12,
        eta_s: 60.0,
        late_s: 0.0,
        capped: false,
        detour_m: 0.0,
        bends_s: 0.0,
        around: None,
        slowed_by: None,
        aligned: false,
        end: None,
        clearance_warning: None,
    });
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    fleet.craft[0].craft.route.as_mut().unwrap().stage = Stage::Arrived;
    assert!(publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
}

#[test]
fn failed_or_paused_refinement_keeps_valid_active_facts_and_can_resume() {
    let (mut l, mut nav, mut galaxy, _, _, _, id) = fixture();
    let original = nav
        .system(&galaxy)
        .unwrap()
        .planets
        .iter()
        .find(|p| p.id == id)
        .unwrap()
        .clone();
    let key = l.active.clone().unwrap();
    let candidate = l.cache[&key].ready[&id].clone();
    l.cache.get_mut(&key).unwrap().planets.insert(
        id.clone(),
        PlanetState::Failed("injected worker panic".into()),
    );
    assert!(refresh_status(&l, &mut nav, &mut galaxy));
    let failed = nav
        .system(&galaxy)
        .unwrap()
        .planets
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(failed.physical, original.physical);
    assert_eq!(failed.environment, original.environment);
    assert!(matches!(
        failed.initial_result.as_ref().unwrap().refinement,
        space_shared::RefinementStatus::Failed { .. }
    ));
    physical::validate(failed).unwrap().unwrap();
    assert!(candidate.project(failed).is_ok());
    l.enabled = false;
    assert!(refresh_status(&l, &mut nav, &mut galaxy));
    let paused = nav
        .system(&galaxy)
        .unwrap()
        .planets
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    assert!(matches!(
        paused.initial_result.as_ref().unwrap().refinement,
        space_shared::RefinementStatus::Unavailable { .. }
    ));
    physical::validate(paused).unwrap().unwrap();
}

#[test]
fn camera_guard_encloses_growth_and_blocks_flights_or_table_layouts() {
    let (_, mut nav, galaxy, _, rig, mut pose, id) = fixture();
    let system = nav.system(&galaxy).unwrap();
    // Bare, explicitly synthetic geometry isolates the safety bound from EOS
    // prescriptions. The camera is outside the old bound but inside the new.
    let mut old = system.planets.iter().find(|p| p.id == id).unwrap().clone();
    old.physical = None;
    old.initial_result = None;
    old.environment.as_mut().unwrap().atmosphere = None;
    old.radius_km = 6371.0;
    old.mass_earth = 1.0;
    old.spin = Default::default();
    let mut larger = old.clone();
    larger.radius_km *= 3.0;
    let centre = DVec3::from_array(space_simulation::planet_position(system, &old, 0.0));
    pose.eye = crate::scene::page_frame(system, 0) * (centre + DVec3::Y * old.radius_km * 4000.0);
    assert!(camera_guard(system, &old, &old, &nav, &rig, &pose, 0.0).is_ok());
    assert!(camera_guard(system, &old, &larger, &nav, &rig, &pose, 0.0).is_err());
    pose.eye = DVec3::splat(1e14);
    nav.fly = true;
    assert!(
        camera_guard(
            nav.system(&galaxy).unwrap(),
            &old,
            &larger,
            &nav,
            &rig,
            &pose,
            0.0
        )
        .is_err()
    );
    nav.fly = false;
    nav.presentation_mode = PresentationMode::Table;
    assert!(
        camera_guard(
            nav.system(&galaxy).unwrap(),
            &old,
            &larger,
            &nav,
            &rig,
            &pose,
            0.0
        )
        .is_err()
    );
}

#[test]
fn update_barrier_publishes_then_stops_invalidating_unchanged_world_caches() {
    #[derive(Resource, Default)]
    struct Changes(Vec<bool>);
    let (mut loading, nav, galaxy, fleet, rig, pose, id) = fixture();
    loading.pending_prepare = false;
    let key = loading.active.clone().unwrap();
    let cache = loading.cache.get_mut(&key).unwrap();
    cache.prepared = true;
    cache.expected = cache.planets.len();
    let mut app = App::new();
    app.insert_resource(loading)
        .insert_resource(nav)
        .insert_resource(crate::Universe(galaxy))
        .insert_resource(fleet)
        .insert_resource(rig)
        .insert_resource(pose)
        .init_resource::<crate::Clock>()
        .init_resource::<Changes>()
        .add_message::<crate::actions::UiCommand>()
        .add_systems(
            Update,
            (
                crate::planet_loading::restore,
                crate::planet_loading::update,
                |universe: Res<crate::Universe>, mut changes: ResMut<Changes>| {
                    changes.0.push(universe.is_changed())
                },
            )
                .chain(),
        );
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Changes>().0, [true, false]);
    let nav = app.world().resource::<Navigation>();
    let universe = app.world().resource::<crate::Universe>();
    assert_eq!(
        nav.system(&universe.0)
            .unwrap()
            .planets
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .initial_result
            .as_ref()
            .unwrap()
            .revision
            .sequence,
        1
    );
}

#[test]
fn safety_checks_are_bounded_and_deferred_entries_do_not_starve_later_planets() {
    let (mut l, mut nav, mut galaxy, mut fleet, rig, pose, id) = fixture();
    let key = l.active.clone().unwrap();
    let refinement = l.cache[&key].ready[&id].clone();
    // Unloaded entries exercise deferral before projection. Their placeholders
    // must never publish and must not prevent the valid loaded entry advancing.
    for placeholder in ["a-unloaded", "b-unloaded"] {
        l.cache
            .get_mut(&key)
            .unwrap()
            .ready
            .insert(placeholder.into(), refinement.clone());
    }
    assert!(!publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert_eq!(l.cache[&key].adoption.len(), RESULTS_PER_FRAME);
    assert!(publish(
        &mut l,
        &mut nav,
        &mut galaxy,
        &mut fleet,
        &rig,
        &pose,
        0.0
    ));
    assert!(l.adoption(&id).unwrap().starts_with("Applied"));
}
