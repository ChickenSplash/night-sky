use super::*;

#[test]
fn disabled_by_default_and_switching_discards_stale_results() {
    let mut l = PlanetLoading::default();
    assert!(!l.enabled);
    l.target(Some("first".into()));
    let epoch = l.epoch;
    let cancellation = l.cancel.clone();
    l.target(Some("second".into()));
    assert!(cancellation.load(Ordering::Relaxed));
    l.sender
        .send(Message::Prepared {
            epoch,
            result: Err("stale error".into()),
        })
        .unwrap();
    l.receive();
    assert!(l.cache["second"].error.is_none());
    l.target(None);
    assert!(l.active().is_none());
    assert!(l.queue.is_empty());
}

#[test]
fn results_change_the_revision_and_reach_status_and_readout() {
    let mut l = PlanetLoading::default();
    l.enabled = true;
    l.target(Some("sys".into()));
    l.cache.get_mut("sys").unwrap().names = vec![("sys/1".into(), "sys b".into())];
    assert!(matches!(l.status("sys/1"), Layered::Waiting));
    let before = l.status("sys/1").key();
    l.sender
        .send(Message::Planet {
            epoch: l.epoch,
            id: "sys/1".into(),
            result: Ok(Err(coupled::Error::DenseGasUnsupported)),
        })
        .unwrap();
    l.receive();
    assert_ne!(l.status("sys/1").key(), before);
    assert!(matches!(
        l.status("sys/1"),
        Layered::State(PlanetState::Unsupported(_))
    ));
    let readout = l.readout();
    let planet = &readout["system"]["planets"][0];
    assert_eq!(planet["designation"], "sys b");
    assert_eq!(planet["state"], "unsupported");
    assert!(planet["layered"].is_null());
    l.enabled = false;
    assert!(matches!(l.status("sys/1"), Layered::Off));
}

#[test]
fn cache_is_bounded_and_revisits_keep_finished_refusals() {
    let mut l = PlanetLoading::default();
    l.target(Some("first".into()));
    l.cache.get_mut("first").unwrap().planets.insert(
        "p".into(),
        PlanetState::Unsupported(coupled::Error::DenseGasUnsupported),
    );
    l.cache
        .get_mut("first")
        .unwrap()
        .planets
        .insert("unfinished".into(), PlanetState::Running);
    l.target(Some("second".into()));
    l.target(Some("first".into()));
    assert!(matches!(
        l.cache["first"].planets["p"],
        PlanetState::Unsupported(_)
    ));
    assert!(!l.cache["first"].planets.contains_key("unfinished"));
    for i in 0..10 {
        l.target(Some(i.to_string()));
    }
    assert_eq!(l.cache.len(), CACHE_SYSTEMS);
    assert!(!l.cache.contains_key("first"));
}

#[test]
fn result_drain_is_bounded_per_frame_and_errors_are_explicit() {
    let mut l = PlanetLoading::default();
    l.target(Some("system".into()));
    for i in 0..5 {
        l.sender
            .send(Message::Planet {
                epoch: l.epoch,
                id: i.to_string(),
                result: Ok(Err(coupled::Error::DenseGasUnsupported)),
            })
            .unwrap();
    }
    l.receive();
    assert_eq!(l.cache["system"].planets.len(), RESULTS_PER_FRAME);
    l.receive();
    assert_eq!(l.cache["system"].planets.len(), 2 * RESULTS_PER_FRAME);
}

#[test]
fn leaving_does_not_restart_the_lingering_selected_system() {
    let mut l = PlanetLoading::default();
    l.enabled = true;
    l.aim(Some("first".into()), Some("first".into()));
    l.aim(None, Some("first".into()));
    assert!(l.active().is_none());
    l.aim(None, Some("first".into()));
    assert!(l.active().is_none());
    l.aim(None, Some("second".into()));
    assert_eq!(l.active().unwrap().0, "second");
}

#[test]
fn completed_cache_needs_no_new_preparation() {
    let mut l = PlanetLoading::default();
    l.target(Some("first".into()));
    l.sender
        .send(Message::Prepared {
            epoch: l.epoch,
            result: Ok((vec![], 1.0)),
        })
        .unwrap();
    l.receive();
    l.target(None);
    l.target(Some("first".into()));
    assert!(!l.pending_prepare);
}

#[test]
fn selected_planet_runs_first_and_solves_off_thread() {
    use space_simulation::system::{
        composition::{Budget, Inventory},
        materials::Material,
    };
    let mut l = PlanetLoading::default();
    l.budget = Arc::new(background::Budget::new(2));
    l.target(Some("system".into()));
    l.pending_prepare = false;
    let host = Inventory::new(&[(Material::Silicate, 1e21)]).unwrap();
    let budget = Budget::new(host, host, Inventory::default(), Inventory::default()).unwrap();
    let environment = space_shared::PlanetEnvironment {
        irradiation_w_m2: 0.0,
        assumed_bond_albedo: 0.3,
        equilibrium_temperature_k: 250.0,
        age_gyr: 4.5,
        atmosphere: None,
    };
    for id in ["other", "selected"] {
        l.cache
            .get_mut("system")
            .unwrap()
            .planets
            .insert(id.into(), PlanetState::Queued);
        l.queue.push_back(PlanetJob {
            id: id.into(),
            designation: id.into(),
            budget,
            environment: environment.clone(),
        });
    }
    l.dispatch(Some("selected"));
    assert_eq!(l.queue.front().unwrap().id, "other");
    assert!(matches!(
        l.cache["system"].planets["selected"],
        PlanetState::Running
    ));
    let message = l
        .receiver
        .get_mut()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(matches!(&message, Message::Planet { id, result: Ok(Ok(_)), .. } if id == "selected"));
    l.sender.send(message).unwrap();
    l.receive();
    assert!(matches!(
        l.cache["system"].planets["selected"],
        PlanetState::Solved(_)
    ));
}

#[test]
fn preparation_runs_in_the_background_and_queues_immutable_inputs() {
    let mut l = PlanetLoading::default();
    l.budget = Arc::new(background::Budget::new(2));
    let id = "g13b6ffffcfeba.2.1";
    l.target(Some(id.into()));
    l.dispatch(None);
    assert!(l.preparing);
    assert!(l.queue.is_empty());
    let message = l
        .receiver
        .get_mut()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert!(
        matches!(&message, Message::Prepared { result: Ok((jobs, _)), .. } if !jobs.is_empty())
    );
    l.sender.send(message).unwrap();
    l.receive();
    assert!(!l.preparing);
    assert!(!l.queue.is_empty());
    assert!(l.cache[id].initial_ms.is_some());
    let system = space_simulation::generated_system(id, [0.0; 3]).unwrap();
    let shown: Vec<_> = system
        .planets
        .iter()
        .map(|p| p.designation.as_str())
        .collect();
    let listed: Vec<_> = l.cache[id].names.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(listed, shown);
    assert!(
        l.cache[id]
            .planets
            .values()
            .all(|s| matches!(s, PlanetState::Queued))
    );
}
