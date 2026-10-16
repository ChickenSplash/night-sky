use super::engine::{Hit, PC_PER_LY, distance, evaluate};
use super::query::{DEFAULT_LIMIT, Filter, Query, Scope, Sort, Test};
use super::registry::{FAMILIES, KINDS, PROPERTIES, Values};
use super::*;
use crate::cells::{Cell, ROOT_PC, Sky};
use crate::galaxy::{Component, GalaxyModel};
use crate::objects::ObjectRef;
use crate::population::TIERS;
use std::sync::OnceLock;

const RADIUS_LY: f64 = 15.0;
/// The radius of `ns search`'s default scope.
const WIDE_LY: f64 = 50.0;

fn home() -> [f64; 3] {
    GalaxyModel::milky_way().home_pc
}

type Systems = Vec<(String, Cell, Component, [f64; 3])>;

/// Every star system within `RADIUS_LY` of home, from every tier's cells
/// that overlap the sphere, each built in full.
fn near_home() -> &'static [(String, Cell, Component, [f64; 3])] {
    static NEAR: OnceLock<Systems> = OnceLock::new();
    NEAR.get_or_init(|| around_home(RADIUS_LY))
}

fn around_home(radius_ly: f64) -> Systems {
    {
        let (centre, radius) = (home(), radius_ly * PC_PER_LY);
        let mut sky = Sky::default();
        let mut found = Vec::new();
        for level in 0..TIERS {
            let size = ROOT_PC / (1u64 << level) as f64;
            let index = |x: f64| ((x + ROOT_PC / 2.0) / size).floor() as u32;
            let range = |axis: usize| index(centre[axis] - radius)..=index(centre[axis] + radius);
            for x in range(0) {
                for y in range(1) {
                    for z in range(2) {
                        let Some(cell) = Cell::from_coords(level, [x, y, z]) else {
                            continue;
                        };
                        for star in sky.cell_stars(cell) {
                            let d = distance(star.position_pc, centre);
                            if d <= radius {
                                found.push((star.id, cell, star.component, star.position_pc));
                            }
                        }
                    }
                }
            }
        }
        found
    }
}

fn brute_force(query: &Query) -> Vec<SearchResult> {
    brute_force_in(near_home(), query)
}

fn brute_force_in(
    systems: &[(String, Cell, Component, [f64; 3])],
    query: &Query,
) -> Vec<SearchResult> {
    let plan = query.plan().unwrap();
    let mut hits: Vec<Hit> = systems
        .iter()
        .flat_map(|(id, cell, component, p)| {
            evaluate(&plan, id, (*cell, *component), *p, distance(*p, home()))
        })
        .collect();
    hits.sort();
    hits.into_iter()
        .take(query.limit as usize)
        .map(|h| h.result)
        .collect()
}

fn request(text: &str) -> SearchRequest {
    SearchRequest {
        id: 1,
        query: Query::parse(text).unwrap(),
        origin: Origin::Position { pc: home() },
        work_cap: DEFAULT_WORK_CAP,
    }
}

fn run(request: SearchRequest, budget: u64, threads: usize) -> (Progress, Vec<SearchResult>) {
    let mut search = Search::new(request).unwrap();
    loop {
        let progress = search.step(budget, threads);
        if progress.state != State::Running {
            return (progress, search.results());
        }
    }
}

const QUERIES: [&str; 9] = [
    "within_ly=15 limit=1000",
    "kind=planet planet_class=sub_neptune within_ly=15 limit=1000",
    "kind=star star_class=m within_ly=15 limit=1000",
    "kind=system multiple=yes within_ly=15 limit=1000",
    "kind=star,planet within_ly=15 limit=10",
    "family=planetary radius_km=..3000 tidally_locked=yes within_ly=15",
    "kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=15 limit=1000",
    "kind=star system.planet_class=gas_giant within_ly=15 limit=1000",
    "kind=planet system.multiple=yes orbits.kind=star within_ly=15 limit=1000",
];

#[test]
fn registry_is_consistent() {
    for (i, kind) in KINDS.iter().enumerate() {
        assert!(KINDS[..i].iter().all(|k| k.id != kind.id), "{}", kind.id);
        assert!(FAMILIES.iter().any(|f| f.id == kind.family), "{}", kind.id);
        if let Some(parent) = kind.parent {
            assert!(KINDS.iter().any(|k| k.id == parent), "{}", kind.id);
        }
    }
    for (i, property) in PROPERTIES.iter().enumerate() {
        assert!(PROPERTIES[..i].iter().all(|p| p.id != property.id));
        assert!(!property.kinds.is_empty());
        for kind in property.kinds {
            assert!(KINDS.iter().any(|k| k.id == *kind), "{}", property.id);
        }
        assert!(!property.id.contains('.'), "dots name relationships");
    }
    assert!(KINDS.len() <= 32, "KindSet holds 32 kinds");
    serde_json::to_string(&registry::catalogue()).unwrap();
}

#[test]
fn text_and_json_round_trip() {
    for text in [
        "within_ly=50",
        "kind=planet planet_class=sub_neptune,rocky radius_km=1000..5000 within_ly=12.5",
        "family=stars temperature_k=..3000 multiple=no within_ly=1000 limit=5",
        "TS 78040000-2-854 b within_ly=50",
        "kind=system star_count=3 planet_count=2.. system=g78040000.2.854 sort=-planet_count",
        "kind=star luminosity_solar=0.001..0.5 system=g1.0.0 sort=temperature_k limit=1000",
        "kind=planet planet_class=sub_neptune orbits.star_class=m,white_dwarf within_ly=50",
        "kind=star system.kind=planet system.radius_km=..3000 orbits.kind=star within_ly=50",
    ] {
        let query = Query::parse(text).unwrap();
        assert_eq!(query.to_text(), text);
        assert_eq!(Query::parse(&query.to_text()).unwrap(), query);
        let json = serde_json::to_string(&query).unwrap();
        assert_eq!(
            serde_json::from_str::<Query>(&json).unwrap(),
            query,
            "{json}"
        );
    }
    // Defaults and spellings that have one canonical form.
    let query = Query::parse("radius_km=5..5 resonant=true kind=planet").unwrap();
    assert_eq!(
        query.to_text(),
        "kind=planet radius_km=5 resonant=yes within_ly=50"
    );
    assert_eq!(query.limit, DEFAULT_LIMIT);
    assert_eq!(
        serde_json::from_str::<Query>("{}").unwrap(),
        Query::default()
    );
    let json: Query = serde_json::from_str(
        r#"{"kinds":["planet"],"filters":[{"property":"radius_km","range":{"min":1000,"max":null}}],
            "scope":{"within":{"radius_ly":20}}}"#,
    )
    .unwrap();
    assert_eq!(json.to_text(), "kind=planet radius_km=1000.. within_ly=20");
}

#[test]
fn refusals_give_a_reason() {
    let refused = |text: &str, reason: &str| {
        let error = Query::parse(text).and_then(|q| q.check()).expect_err(text);
        assert!(error.contains(reason), "{text}: {error}");
    };
    refused("colour=red", "unknown property `colour`");
    refused(
        "moons.radius_km=1..",
        "unknown relationship `moons.`; relationships are orbits., system.",
    );
    refused(
        "kind=star orbits.star_class=m",
        "`orbits.` applies to planet, ship, not star",
    );
    refused(
        "orbits.planet_count=1..",
        "`orbits.planet_count` applies to system, not star, planet",
    );
    refused(
        "orbits.kind=system",
        "orbits leads to star, planet, not system",
    );
    refused("system.kind=moon", "Moon are not generated yet");
    refused("system.colour=red", "unknown property `colour`");
    refused("system.star_class=m system.star_class=k", "given twice");
    // A relationship with nothing to match, possible only in JSON.
    let mut empty = Query::default();
    empty.relations.push(super::query::Relation {
        relation: "orbits".into(),
        kinds: Vec::new(),
        filters: Vec::new(),
    });
    assert!(
        empty
            .check()
            .unwrap_err()
            .contains("needs a kind or a filter")
    );
    refused(
        "kind=star radius_km=1..2",
        "`radius_km` applies to planet, not star",
    );
    refused(
        "star_class=m planet_class=rocky",
        "applies to planet, not star",
    );
    refused("kind=moon", "Moon are not generated yet");
    refused("kind=quasar", "unknown kind");
    refused("family=deep_sky", "nothing of that family");
    refused("kind=star family=stars", "not both");
    refused("radius_km=5..1", "minimum 5 is above maximum 1");
    refused("planet_class=lava", "is one of dwarf, rocky");
    refused("multiple=maybe", "yes or no");
    refused("radius_km=big", "needs a number");
    refused("within_ly=10001", "at most 10000");
    refused("within_ly=0", "above 0");
    refused("limit=0", "limit must be 1 to 1000");
    refused("kind=planet kind=star", "given twice");
    refused("within_ly=5 system=g1.0.0", "not both");
    refused("sort=radius_km kind=planet", "sort by distance only");
    refused("system=nowhere", "not a system ID");
    // A property needs only one of several kinds; the others drop out.
    let plan = Query::parse("kind=star,planet radius_km=..2000")
        .unwrap()
        .plan()
        .unwrap();
    assert!(plan.kinds.has("planet") && !plan.kinds.has("star"));
    let mut query = Query::default();
    query.filters.push(Filter {
        property: "radius_km".into(),
        test: Test::Range {
            min: None,
            max: None,
        },
    });
    assert!(query.check().unwrap_err().contains("at least one end"));
    query.filters[0].test = Test::Is(true);
    assert!(query.check().unwrap_err().contains("needs a range"));
}

#[test]
fn search_radius_accepts_ten_thousand_light_years_in_text_and_json() {
    for radius_ly in [1001.0, 9999.5, 10_000.0] {
        let query = Query::parse(&format!("within_ly={radius_ly}")).unwrap();
        query.check().unwrap();
        let json = serde_json::to_string(&query).unwrap();
        let restored: Query = serde_json::from_str(&json).unwrap();
        restored.check().unwrap();
        assert_eq!(restored.scope, Scope::Within { radius_ly });
    }
    for radius_ly in [10_000.1, 10_001.0, f64::INFINITY, f64::NAN, 0.0, -1.0] {
        let query = Query {
            scope: Scope::Within { radius_ly },
            ..Query::default()
        };
        assert!(query.check().is_err(), "radius {radius_ly} must be refused");
    }
}

#[test]
fn results_match_a_brute_force_near_home() {
    assert!(near_home().len() > 50, "{}", near_home().len());
    for text in QUERIES {
        let (progress, results) = run(request(text), u64::MAX, 1);
        assert_eq!(progress.state, State::Done, "{text}");
        let expected = brute_force(&Query::parse(text).unwrap());
        assert!(!expected.is_empty(), "{text}");
        assert_eq!(results, expected, "{text}");
    }
}

#[test]
fn filters_hold_on_their_results() {
    let (_, results) = run(request(QUERIES[1]), u64::MAX, 1);
    assert!(results.iter().all(|r| r.class == "Sub-Neptune"));
    let (_, results) = run(request(QUERIES[2]), u64::MAX, 1);
    assert!(
        results.iter().all(|r| r.class == "Red dwarf"),
        "{results:?}"
    );
    let (_, results) = run(request(QUERIES[3]), u64::MAX, 1);
    assert!(results.iter().all(|r| r.class != "Single star"));
    // Members of a multiple are listed one by one; a single star once.
    let (_, results) = run(request(QUERIES[0]), u64::MAX, 1);
    let mut objects: Vec<_> = results.iter().map(|r| &r.object).collect();
    let all = objects.len();
    objects.sort_by_key(|o| format!("{o:?}"));
    objects.dedup();
    assert_eq!(objects.len(), all);
    assert!(results.iter().any(|r| r.kind == "system"));
    assert!(
        results
            .iter()
            .any(|r| matches!(&r.object, ObjectRef::Star { id } if id.contains('#')))
    );
    assert!(
        results
            .windows(2)
            .all(|w| w[0].distance_ly <= w[1].distance_ly)
    );
}

#[test]
fn threads_and_slicing_change_nothing() {
    for text in [QUERIES[0], QUERIES[1], QUERIES[4]] {
        let (_, whole) = run(request(text), u64::MAX, 1);
        for (budget, threads) in [(5_000, 1), (u64::MAX, 4), (20_000, 3)] {
            let (progress, results) = run(request(text), budget, threads);
            assert_eq!(progress.state, State::Done);
            assert_eq!(results, whole, "{text} {budget} {threads}");
        }
    }
}

#[test]
fn nearest_first_stops_early() {
    let (all, _) = run(request("within_ly=15 limit=1000"), u64::MAX, 1);
    let (few, results) = run(request("within_ly=15 limit=3"), u64::MAX, 1);
    assert_eq!(results.len(), 3);
    assert!(few.work < all.work, "{few:?} {all:?}");
}

#[test]
fn a_capped_search_is_exact_within_its_complete_radius() {
    let text = "within_ly=15 limit=1000";
    let (whole, all) = run(request(text), u64::MAX, 1);
    let mut capped = request(text);
    capped.work_cap = whole.work / 2;
    let (progress, results) = run(capped, 7_000, 2);
    assert_eq!(progress.state, State::Capped);
    assert!(progress.complete_to_ly < RADIUS_LY, "{progress:?}");
    let within = |rs: &[SearchResult]| -> Vec<SearchResult> {
        rs.iter()
            .filter(|r| r.distance_ly <= progress.complete_to_ly)
            .cloned()
            .collect()
    };
    assert_eq!(within(&results), within(&all));
}

#[test]
fn designations_and_ids_find_their_object() {
    let (_, results) = run(request("within_ly=15 limit=1000"), u64::MAX, 1);
    let mut kinds = std::collections::HashSet::new();
    for result in results
        .iter()
        .filter(|r| kinds.insert((r.kind.clone(), r.name.contains('('))))
    {
        let id = match &result.object {
            ObjectRef::Star { id } | ObjectRef::Planet { id } => id.clone(),
            other => panic!("{other:?}"),
        };
        for text in [&result.name, &id] {
            let (progress, found) = run(request(text), u64::MAX, 1);
            assert_eq!(progress.state, State::Done);
            assert_eq!(found.len(), 1, "{text}: {found:?}");
            assert_eq!(found[0].object, result.object, "{text}");
            assert!((found[0].distance_ly - result.distance_ly).abs() < 1e-9);
        }
    }
    assert!(kinds.len() >= 3, "{kinds:?}");
    // Kinds and filters still apply.
    let planet = results.iter().find(|r| r.kind == "planet").unwrap();
    let (_, found) = run(request(&format!("{} kind=star", planet.name)), u64::MAX, 1);
    assert!(found.is_empty());
    for text in ["TS nothing", "g1.0.999999999999", "TS 1-0-999999999999"] {
        assert!(Search::new(request(text)).is_err(), "{text}");
    }
}

#[test]
fn one_system_sorts_by_a_property() {
    let (_, results) = run(request("kind=planet within_ly=15 limit=1000"), u64::MAX, 1);
    let key = crate::multiple::system_key(match &results[0].object {
        ObjectRef::Planet { id } => id,
        other => panic!("{other:?}"),
    })
    .to_owned();
    let mut query = Query::parse(&format!("kind=planet system={key}")).unwrap();
    query.sort = Sort::Property {
        id: "radius_km".into(),
        descending: true,
    };
    let planets = crate::cells::cell_bodies(&key).unwrap().planets;
    let (progress, found) = run(
        SearchRequest {
            query,
            ..request("")
        },
        u64::MAX,
        1,
    );
    assert_eq!(progress.state, State::Done);
    assert_eq!(found.len(), planets.len());
    let mut radii: Vec<f64> = planets.iter().map(|p| p.body.radius_km).collect();
    radii.sort_by(|a, b| b.total_cmp(a));
    let order: Vec<f64> = found
        .iter()
        .map(|r| {
            planets
                .iter()
                .find(|p| ObjectRef::Planet { id: p.id() } == r.object)
                .unwrap()
                .body
                .radius_km
        })
        .collect();
    assert_eq!(order, radii);
    assert!(matches!(
        Query::parse("system=g1.0.0").unwrap().scope,
        Scope::System { .. }
    ));
}

#[test]
fn every_property_can_be_filtered() {
    for property in PROPERTIES {
        let value = match property.values {
            Values::Range { .. } => "0..".to_owned(),
            Values::Choice { options } => options
                .iter()
                .map(|(o, _)| *o)
                .collect::<Vec<_>>()
                .join(","),
            Values::Flag => "yes".to_owned(),
        };
        let text = format!("{}={value} within_ly=15 limit=1000", property.id);
        let (_, results) = run(request(&text), u64::MAX, 1);
        assert_eq!(
            results,
            brute_force(&Query::parse(&text).unwrap()),
            "{text}"
        );
        assert!(
            results
                .iter()
                .all(|r| property.kinds.contains(&r.kind.as_str())),
            "{text}"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_service_answers_cancels_and_refuses() {
    use std::time::Duration;
    let service = SearchService::spawn(2);
    service.submit(SearchRequest {
        id: 7,
        ..request("kind=planet within_ly=15")
    });
    service.submit(SearchRequest {
        id: 8,
        query: Query::parse("kind=moon").unwrap(),
        ..request("")
    });
    // A search at the galactic centre runs for a long time; cancel it.
    service.submit(SearchRequest {
        id: 9,
        query: Query::parse("star_class=o within_ly=50").unwrap(),
        origin: Origin::Position { pc: [0.0; 3] },
        work_cap: DEFAULT_WORK_CAP,
    });
    let mut done = false;
    let mut refused = false;
    let mut cancelled = false;
    let mut asked = false;
    while !(done && refused && cancelled) {
        let update = service
            .wait(Duration::from_secs(60))
            .expect("the service answers");
        match update {
            SearchUpdate::Progress {
                id: 7,
                progress,
                results,
            } if progress.state == State::Done => {
                assert!(!results.is_empty());
                done = true;
            }
            SearchUpdate::Refused { id: 8, reason } => {
                assert!(reason.contains("not generated"));
                refused = true;
            }
            SearchUpdate::Progress {
                id: 9, progress, ..
            } => {
                if progress.state == State::Cancelled {
                    cancelled = true;
                } else {
                    assert_eq!(progress.state, State::Running);
                    if !asked {
                        service.cancel(9);
                        asked = true;
                    }
                }
            }
            _ => {}
        }
    }
}

#[test]
fn designations_resolve_to_ids() {
    use super::engine::resolve;
    let planet = |id: &str| ObjectRef::Planet { id: id.into() };
    let star = |id: &str| ObjectRef::Star { id: id.into() };
    for (text, object) in [
        ("TS 1-2-3", star("g1.2.3")),
        ("TS 1-2-3 b", planet("g1.2.3/0")),
        ("TS 1-2-3 B", star("g1.2.3#B")),
        ("TS 1-2-3 Ac", planet("g1.2.3#A/1")),
        ("TS 1-2-3 (AB) d", planet("g1.2.3#AB/2")),
        ("g1.2.3#AB/2", planet("g1.2.3#AB/2")),
        ("g1.2.3#C", star("g1.2.3#C")),
    ] {
        assert_eq!(resolve(text), Ok(("g1.2.3".into(), object)), "{text}");
    }
    for text in [
        "TS 1-2",
        "TS 1-2-3 b c",
        "TS 1-2-3 Abc",
        "TS 1-2-3 (AB)",
        "g1.2.3/x",
        "Sol",
    ] {
        assert!(resolve(text).is_err(), "{text}");
    }
}

/// Star class as `star.rs` names its stars, independently of the engine.
fn class_of(star: &space_shared::Star) -> &'static str {
    match star.kind.as_str() {
        "Red dwarf" => "m",
        "Orange dwarf" => "k",
        "Yellow dwarf" => "g",
        "White dwarf" => "white_dwarf",
        kind if kind.contains("supergiant") => "supergiant",
        kind if kind.contains("giant") => "giant",
        _ => "other",
    }
}

#[test]
fn relationships_match_their_systems() {
    let bodies = |id: &str| crate::cells::cell_bodies(crate::multiple::system_key(id)).unwrap();
    // Planets orbiting a red dwarf, a pair of stars counting when either is.
    let (_, planets) = run(request("kind=planet within_ly=15 limit=1000"), u64::MAX, 1);
    let (_, found) = run(
        request("kind=planet orbits.star_class=m within_ly=15 limit=1000"),
        u64::MAX,
        1,
    );
    let expected: Vec<_> = planets
        .iter()
        .filter(|r| {
            let ObjectRef::Planet { id } = &r.object else {
                unreachable!()
            };
            let system = bodies(id);
            let planet = system.planets.iter().find(|p| p.id() == *id).unwrap();
            let hosts: Vec<_> = system
                .stars
                .iter()
                .filter(|s| {
                    planet.host_letters.is_empty() || planet.host_letters.contains(&s.component)
                })
                .collect();
            hosts.iter().any(|s| class_of(s) == "m")
        })
        .cloned()
        .collect();
    assert!(!expected.is_empty() && expected.len() < planets.len());
    assert_eq!(found, expected);
    // Stars with a gas giant in their system.
    let (_, stars) = run(request("kind=star within_ly=15 limit=1000"), u64::MAX, 1);
    let (_, found) = run(
        request("kind=star system.planet_class=gas_giant within_ly=15 limit=1000"),
        u64::MAX,
        1,
    );
    let expected: Vec<_> = stars
        .iter()
        .filter(|r| {
            let ObjectRef::Star { id } = &r.object else {
                unreachable!()
            };
            bodies(id)
                .planets
                .iter()
                .any(|p| p.body.class == crate::system::Class::GasGiant)
        })
        .cloned()
        .collect();
    assert!(!expected.is_empty() && expected.len() < stars.len());
    assert_eq!(found, expected);
    // Stars with a red dwarf companion: never themselves.
    let (_, found) = run(
        request("kind=star system.star_class=m within_ly=15 limit=1000"),
        u64::MAX,
        1,
    );
    assert!(!found.is_empty());
    for result in &found {
        let ObjectRef::Star { id } = &result.object else {
            unreachable!()
        };
        let system = bodies(id);
        assert!(
            system
                .stars
                .iter()
                .any(|s| s.id != *id && class_of(s) == "m"),
            "{result:?}"
        );
    }
}

#[test]
fn a_pair_host_matches_when_either_star_does() {
    // An orange dwarf (A) and red dwarf (B) with planets around the pair,
    // and a red dwarf (C), 8.9 ly from home.
    let planets = |filter: &str| -> Vec<String> {
        let query = Query::parse(&format!("kind=planet {filter} system=gb92c924926.2.54")).unwrap();
        let (_, found) = run(
            SearchRequest {
                query,
                ..request("")
            },
            u64::MAX,
            1,
        );
        found
            .into_iter()
            .map(|r| match r.object {
                ObjectRef::Planet { id } => id,
                other => panic!("{other:?}"),
            })
            .collect()
    };
    let pair = |ids: &[String]| ids.iter().filter(|id| id.contains("#AB/")).count();
    let around_k = planets("orbits.star_class=k");
    let around_m = planets("orbits.star_class=m");
    assert!(pair(&around_k) >= 2, "{around_k:?}");
    assert_eq!(pair(&around_k), pair(&around_m));
    assert_eq!(
        pair(&around_k),
        around_k.len(),
        "only the pair has an orange dwarf"
    );
    assert!(planets("orbits.star_class=g").is_empty());
}

/// Session 2's acceptance query, at its full radius.
#[test]
fn red_dwarf_sub_neptunes_match_a_brute_force_at_50_ly() {
    let systems = around_home(WIDE_LY);
    let text = "kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=50 limit=1000";
    let query = Query::parse(text).unwrap();
    let expected = brute_force_in(&systems, &query);
    let (progress, results) = run(request(text), u64::MAX, 4);
    assert_eq!(progress.state, State::Done);
    // More than the limit: the nearest 1,000, so stopping early is tested.
    assert_eq!(results.len(), 1000);
    assert_eq!(results, expected);
}

/// A ship around the first planet near home with one, and that planet.
fn live_ship() -> (live::LiveObject, crate::multiple::Hosted) {
    let (key, cell, component, position_pc, planet) = near_home()
        .iter()
        .find_map(|(key, cell, component, at)| {
            let bodies = crate::population::tiered_bodies(key, *component, cell.level());
            let planet = bodies.planets.first()?.clone();
            Some((key.clone(), *cell, *component, *at, planet))
        })
        .unwrap();
    let _ = (cell, component);
    let ship = live::LiveObject {
        result: SearchResult {
            object: ObjectRef::Craft {
                id: "craft-1".into(),
            },
            kind: "ship".into(),
            name: "Ship 1".into(),
            class: "Ship".into(),
            system: key.clone(),
            distance_ly: 0.0,
            position_pc,
        },
        system_key: key,
        altitude_km: 400.0,
        speed_kms: 7.7,
        orbits: Some(planet.id()),
    };
    (ship, planet)
}

#[test]
fn live_ships_match_kinds_filters_scopes_text_and_what_they_orbit() {
    let (ship, planet) = live_ship();
    let class = super::engine::planet_class(planet.body.class);
    let other = if class == "dwarf" { "rocky" } else { "dwarf" };
    let key = ship.system_key.clone();
    let found = |text: &str| {
        let query = Query::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
        !live::matching(&query, home(), std::slice::from_ref(&ship)).is_empty()
    };
    for text in [
        "within_ly=15".to_owned(),
        "kind=ship altitude_km=300..500 speed_kms=7..8".into(),
        "family=craft".into(),
        format!("kind=ship system={key}"),
        format!("orbits.planet_class={class}"),
        "orbits.kind=planet".into(),
        "craft-1".into(),
        "ship 1".into(),
    ] {
        assert!(found(&text), "{text}");
    }
    for text in [
        "kind=planet within_ly=15".to_owned(),
        "altitude_km=500..".into(),
        format!("orbits.planet_class={other}"),
        "orbits.kind=star".into(),
        "craft-2".into(),
    ] {
        assert!(!found(&text), "{text}");
    }
    let query = Query::parse("kind=ship").unwrap();
    let far = [(home()[0] + 100.0) * 1.0, home()[1], home()[2]];
    assert!(live::matching(&query, far, std::slice::from_ref(&ship)).is_empty());
    // Planets orbit stars, so this names only craft.
    let refused = Query::parse("kind=planet orbits.kind=planet")
        .unwrap()
        .check();
    assert!(refused.unwrap_err().contains("not planets"));
    // Around a pair, a ship orbits each of its stars, as planets do.
    let pair = near_home().iter().find_map(|(key, cell, component, at)| {
        let bodies = crate::population::tiered_bodies(key, *component, cell.level());
        let pair = bodies.barycentres.first()?.clone();
        Some((key.clone(), *at, pair, bodies.stars))
    });
    let (key, position_pc, pair, stars) = pair.unwrap();
    let ship = live::LiveObject {
        result: SearchResult {
            position_pc,
            ..ship.result.clone()
        },
        system_key: key,
        orbits: Some(pair.id.clone()),
        ..ship
    };
    let class = |star: &space_shared::Star| super::engine::star_class(star);
    let member = stars.iter().find(|s| pair.component.contains(&s.component));
    let query = Query::parse(&format!("orbits.star_class={}", class(member.unwrap()))).unwrap();
    assert_eq!(
        live::matching(&query, home(), std::slice::from_ref(&ship)).len(),
        1
    );
    let query = Query::parse("orbits.kind=planet").unwrap();
    assert!(live::matching(&query, home(), std::slice::from_ref(&ship)).is_empty());
    // The engine has no ships to find and finishes at once.
    let (progress, results) = run(request("kind=ship within_ly=10000"), 1, 1);
    assert_eq!((progress.state, progress.evaluated), (State::Done, 0));
    assert!(results.is_empty());
}

#[test]
fn live_hits_merge_in_distance_or_property_order() {
    let (ship, _) = live_ship();
    let query = Query::parse("within_ly=15 limit=1000").unwrap();
    let (_, found) = run(request("within_ly=15 limit=1000"), u64::MAX, 1);
    let hits = live::matching(&query, home(), std::slice::from_ref(&ship));
    let merged = live::merge(&query, found.clone(), hits.clone());
    assert_eq!(merged.len(), found.len() + 1);
    assert!(
        merged
            .windows(2)
            .all(|w| w[0].distance_ly <= w[1].distance_ly)
    );
    let limited = Query::parse("within_ly=15 limit=3").unwrap();
    assert_eq!(live::merge(&limited, found.clone(), hits).len(), 3);
    // Sorted by a craft property within a system, craft come first.
    let text = format!("system={} sort=-altitude_km", ship.system_key);
    let query = Query::parse(&text).unwrap();
    let (_, found) = run(request(&text), u64::MAX, 1);
    let mut low = ship.clone();
    low.altitude_km = 100.0;
    low.result.object = ObjectRef::Craft {
        id: "craft-2".into(),
    };
    let hits = live::matching(&query, home(), &[low, ship]);
    let merged = live::merge(&query, found, hits);
    let ids: Vec<_> = merged
        .iter()
        .take(2)
        .map(|r| match &r.object {
            ObjectRef::Craft { id } => id.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(ids, ["craft-1", "craft-2"]);
}
