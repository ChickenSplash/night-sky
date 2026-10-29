use super::*;
use std::time::{Duration, Instant};

fn wait_for(hub: &Hub, id: u64) -> Entry {
    let start = Instant::now();
    loop {
        let entry = hub.get(id).unwrap();
        if entry.state() != "running" {
            return entry;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{entry:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn requests_are_kept_by_id_refused_and_cancelled() {
    let hub = Hub::new();
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let query = |text: &str| Query::parse(text).unwrap();
    let near = hub
        .submit(
            Asker::Api,
            query("kind=star within_ly=10"),
            home,
            DEFAULT_WORK_CAP,
            &[],
        )
        .unwrap();
    let other = hub
        .submit(
            Asker::Game,
            query("kind=planet within_ly=10 limit=3"),
            home,
            DEFAULT_WORK_CAP,
            &[],
        )
        .unwrap();
    assert_ne!(near, other);
    assert!(
        hub.submit(Asker::Api, query("kind=moon"), home, DEFAULT_WORK_CAP, &[])
            .unwrap_err()
            .contains("not generated")
    );
    let unknown = hub
        .submit(
            Asker::Api,
            query("TS 1-0-999999999999"),
            home,
            DEFAULT_WORK_CAP,
            &[],
        )
        .unwrap();
    let near = wait_for(&hub, near);
    assert_eq!(near.state(), "done");
    assert!(near.result_count > 0);
    assert_eq!(near.results.as_ref().unwrap().len(), near.result_count);
    assert_eq!(wait_for(&hub, other).result_count, 3);
    assert!(wait_for(&hub, unknown).error.unwrap().contains("no system"));
    let centre = hub
        .submit(
            Asker::Api,
            query("within_ly=50"),
            [0.0; 3],
            DEFAULT_WORK_CAP,
            &[],
        )
        .unwrap();
    hub.cancel(centre);
    assert_eq!(wait_for(&hub, centre).state(), "cancelled");
    let summaries = hub.summaries(true).unwrap();
    assert_eq!(summaries[0].id, centre, "newest first");
    assert!(summaries.iter().all(|e| e.results.is_none()));
    // Only a bounded number are kept.
    for _ in 0..KEPT {
        hub.submit(Asker::Api, query("TS 1-0-999999999999"), home, 1, &[])
            .unwrap();
    }
    assert_eq!(hub.summaries(true).unwrap().len(), KEPT);
    assert!(hub.get(near.id).is_none());
}

#[test]
fn live_craft_are_found_by_kind_name_and_among_generated_objects() {
    use crate::craft::{Fleet, Launch, live_objects};
    let galaxy = space_simulation::demo_galaxy();
    let system = galaxy.systems[0].clone();
    let page = system.first_page();
    let planet = system.on_page(page).next().unwrap().id.clone();
    let mut fleet = Fleet::default();
    let ship = fleet
        .spawn(system, page, &planet, 400_000.0, Launch::Circular, 0.0)
        .unwrap();
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    fleet.get_mut(&ship).unwrap().origin_pc = home;
    let hub = Hub::new();
    let live = live_objects(&fleet, &galaxy, 0.0);
    let search = |text: &str| {
        let query = Query::parse(text).unwrap();
        let id = hub
            .submit(Asker::Api, query, home, DEFAULT_WORK_CAP, &live)
            .unwrap();
        wait_for(&hub, id).results.unwrap()
    };
    let craft = space_simulation::objects::ObjectRef::Craft { id: ship.clone() };
    for text in ["kind=ship", "family=craft altitude_km=399..401", "Ship 1"] {
        let results = search(text);
        assert_eq!(results.len(), 1, "{text}");
        assert_eq!(results[0].object, craft, "{text}");
        assert_eq!(results[0].kind, "ship");
    }
    assert!(search("kind=ship altitude_km=500..").is_empty());
    // Nearest first among stars and planets: here, at the origin.
    let mixed = search("within_ly=10 limit=1000");
    assert_eq!(mixed[0].object, craft);
    assert!(mixed.len() > 1);
}
