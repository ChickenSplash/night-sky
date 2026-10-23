use super::*;

fn empty_arrival(id: &str) -> Arrival {
    Arrival::new(
        Field::new(
            StarSystem {
                id: id.into(),
                name: id.into(),
                position_ly: [0.0; 3],
                planets: Vec::new(),
                stars: Vec::new(),
                barycentres: Vec::new(),
                page_stars: Vec::new(),
            },
            0,
        ),
        [0.0; 3],
    )
}

fn put_region(cache: &mut Cache, i: i64) {
    cache.accept(
        &Wait::Region(Region([i, 0, 0])),
        Reply::Region {
            candidates: Vec::new(),
        },
        &Galaxy::default(),
    );
}

#[test]
fn active_fleets_can_exceed_cache_targets_without_reload_churn() {
    let mut cache = Cache::default();
    let regions = REGIONS + 5;
    let arrivals = ARRIVALS + 5;
    for i in 0..regions {
        put_region(&mut cache, i as i64);
    }
    for i in 0..arrivals {
        cache.insert_arrival(i.to_string(), empty_arrival(&i.to_string()));
    }
    for round in 1..10 {
        cache.round(round);
        for i in 0..regions {
            assert!(cache.region(Region([i as i64, 0, 0])).is_some());
        }
        for i in 0..arrivals {
            assert!(cache.arrival(&i.to_string()).is_some());
        }
        assert_eq!(cache.counts(), (regions, arrivals));
    }
    // Once the fleet no longer uses them, even without new inserts, shrink.
    cache.round(10);
    cache.round(11);
    assert_eq!(cache.counts(), (REGIONS, ARRIVALS));
}

#[test]
fn idle_cache_is_bounded_and_reads_protect_recent_entries() {
    let mut cache = Cache::default();
    for i in 0..200 {
        cache.round(i);
        put_region(&mut cache, i);
        cache.insert_arrival(i.to_string(), empty_arrival(&i.to_string()));
        assert!(cache.counts().0 <= REGIONS);
        assert!(cache.counts().1 <= ARRIVALS);
    }
    // Touch the oldest remaining region, then add another two rounds later.
    assert!(cache.region(Region([192, 0, 0])).is_some());
    cache.round(201);
    put_region(&mut cache, 201);
    assert!(cache.region(Region([192, 0, 0])).is_some());
    assert!(cache.region(Region([193, 0, 0])).is_none());
}

#[test]
fn library_and_async_cache_share_list_replacement_and_legacy_rules() {
    let lists = crate::demo_galaxy();
    let list = &lists.systems[0];
    let page = list.first_page();
    let candidate = Candidate {
        id: page_star_id(list, page),
        position_pc: [1.0, 2.0, 3.0],
        reach_pc: 1.0,
    };
    let wait = Wait::System(candidate.clone());
    let mut library = Library::new(lists.clone());
    library.supply(&wait);
    let mut cache = Cache::default();
    // Even a completed generated reply must not override a charted page.
    cache.accept(&wait, Reply::System { system: None }, &lists);
    let a = library.arrival(&candidate.id).unwrap();
    let b = cache.arrival(&candidate.id).unwrap();
    assert_eq!(a.field.system(), b.field.system());
    assert_eq!(a.field.system(), list);
    assert_eq!(a.field.page(), page);
    assert_eq!(a.origin_pc, candidate.position_pc);

    let mut legacy = list.clone();
    legacy.page_stars.clear();
    let lists = Galaxy {
        systems: vec![legacy.clone()],
        ..Galaxy::default()
    };
    let region = Region::of(listed_origin_pc(&legacy, page));
    let reply = Reply::Region {
        candidates: vec![
            Candidate {
                id: "z".into(),
                ..candidate.clone()
            },
            Candidate {
                id: "a".into(),
                ..candidate
            },
        ],
    };
    let mut one = Cache::default();
    let mut two = Cache::default();
    one.accept(&Wait::Region(region), reply.clone(), &lists);
    two.accept(&Wait::Region(region), reply, &lists);
    let candidates = one.region(region).unwrap();
    assert_eq!(Some(candidates), two.region(region));
    assert!(candidates.windows(2).all(|pair| pair[0].id <= pair[1].id));
    let legacy_candidate = candidates
        .iter()
        .find(|c| c.id == page_star_id(&legacy, page))
        .unwrap()
        .clone();
    let wait = Wait::System(legacy_candidate.clone());
    let mut library = Library::new(lists.clone());
    library.supply(&wait);
    assert!(one.chart(&wait, &lists));
    assert_eq!(
        library
            .arrival(&legacy_candidate.id)
            .unwrap()
            .field
            .system(),
        one.arrival(&legacy_candidate.id).unwrap().field.system()
    );
}

#[test]
fn stars_are_located_from_seeds_else_as_legacy_list_pages() {
    let lists = crate::demo_galaxy();
    let list = &lists.systems[0];
    let page = list.first_page();
    let mut library = Library::new(lists.clone());
    // A charted page's cell star is found from seeds, as regions list it.
    let star = page_star_id(list, page);
    library.supply(&Wait::Star(star.clone()));
    let located = library.locate(&star).unwrap().unwrap().clone();
    let region = Region::of(located.position_pc);
    library.supply(&Wait::Region(region));
    assert!(library.region(region).unwrap().contains(&located));
    // A legacy page is placed by its list, without seeds.
    let mut legacy = list.clone();
    legacy.page_stars.clear();
    let lists = Galaxy {
        systems: vec![legacy.clone()],
        ..Galaxy::default()
    };
    let id = page_star_id(&legacy, page);
    let mut cache = Cache::default();
    assert!(cache.chart(&Wait::Star(id.clone()), &lists));
    let candidate = cache.locate(&id).unwrap().unwrap();
    let region = Region::of(candidate.position_pc);
    assert!(legacy_candidates(region, &lists).contains(candidate));
    // An ID no cell or list holds names no system.
    library.supply(&Wait::Star("nowhere".into()));
    assert_eq!(library.locate("nowhere"), Some(None));
}
