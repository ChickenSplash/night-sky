use super::filters::{self, Chip, Slot};
use super::*;
use space_simulation::search::registry::{self, PROPERTIES};
use space_simulation::search::{
    DEFAULT_WORK_CAP, Filter, Origin, Search, SearchRequest, SearchResult, State, Test,
};

fn property(id: &str) -> usize {
    registry::property_index(id).unwrap()
}

fn relation(id: &str) -> usize {
    registry::relation_index(id).unwrap()
}

fn one_of(id: &str, values: &[&str]) -> Filter {
    Filter {
        property: id.into(),
        test: Test::OneOf(values.iter().map(|v| (*v).to_owned()).collect()),
    }
}

fn run(query: Query) -> Vec<SearchResult> {
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let mut search = Search::new(SearchRequest {
        id: 1,
        query,
        origin: Origin::Position { pc: home },
        work_cap: DEFAULT_WORK_CAP,
    })
    .unwrap();
    while search.step(1_000_000, 2).state == State::Running {}
    search.results()
}

/// Session 3's acceptance: the same query built in the browser and typed in
/// the CLI gives identical results.
#[test]
fn a_query_built_with_chips_matches_the_cli() {
    let cli = "kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=20";
    let mut built = Query::default();
    filters::set_kind(&mut built, "planet");
    filters::set_filter(
        &mut built,
        Slot::Own,
        one_of("planet_class", &["sub_neptune"]),
    );
    filters::set_filter(
        &mut built,
        Slot::Related(relation("orbits")),
        one_of("star_class", &["m"]),
    );
    built.scope = Scope::Within { radius_ly: 20.0 };
    assert_eq!(built.to_text(), cli);
    let typed = Query::parse(cli).unwrap();
    let results = run(built);
    assert!(!results.is_empty());
    assert_eq!(results, run(typed));
}

#[test]
fn chips_read_as_text_and_remove_what_they_stand_for() {
    let mut query = Query::parse(
        "kind=planet planet_class=sub_neptune,ice_giant radius_km=1000..5000 orbits.star_class=m system.kind=star",
    )
    .unwrap();
    let chips = filters::chips(&query);
    let labels: Vec<_> = chips.iter().map(|(_, label)| label.as_str()).collect();
    assert_eq!(labels[0], "Type: Planet");
    assert!(
        labels[1].starts_with("Planet class: Sub-Neptune or "),
        "{labels:?}"
    );
    assert_eq!(labels[2], "Planet radius: 1,000–5,000 km");
    assert!(labels[3].starts_with("Orbits · Star class: "), "{labels:?}");
    assert_eq!(labels[4], "In a system with: Star");
    let flags = Query::parse("kind=planet tidally_locked=yes resonant=no").unwrap();
    let flags: Vec<_> = filters::chips(&flags)
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    assert_eq!(
        flags[1..],
        [
            "Tidally locked",
            "In resonance with the planet inside it: no"
        ]
    );
    filters::remove(
        &mut query,
        &Chip::Filter {
            slot: Slot::Related(relation("orbits")),
            property: property("star_class"),
        },
    );
    assert!(
        query.relations.iter().all(|r| r.relation != "orbits"),
        "an emptied relationship goes too"
    );
    filters::remove(&mut query, &Chip::Kinds);
    assert!(query.kinds.is_empty());
    assert!(query.check().is_ok());
}

#[test]
fn choosing_a_kind_drops_filters_it_cannot_have() {
    let mut query =
        Query::parse("kind=planet radius_km=..3000 orbits.star_class=m system.multiple=yes")
            .unwrap();
    filters::set_kind(&mut query, "star");
    assert_eq!(
        query.to_text(),
        "kind=star system.multiple=yes within_ly=50"
    );
    assert!(query.check().is_ok());
    // Only star properties are offered for stars.
    for index in filters::offered_properties(&query, Slot::Own) {
        assert!(
            PROPERTIES[index].kinds.contains(&"star"),
            "{}",
            PROPERTIES[index].id
        );
    }
    assert!(
        filters::offered_relations(&query)
            .iter()
            .all(|&r| r != relation("orbits"))
    );
}

#[test]
fn values_read_and_parse_like_a_person_writes_them() {
    let radius = &PROPERTIES[property("radius_km")];
    let range = |min, max| Test::Range { min, max };
    assert_eq!(
        filters::value_text(radius, &range(Some(1000.0), None)),
        "at least 1,000 km"
    );
    assert_eq!(
        filters::value_text(radius, &range(None, Some(2.5))),
        "at most 2.5 km"
    );
    assert_eq!(
        filters::value_text(radius, &range(Some(4.0), Some(4.0))),
        "4 km"
    );
    let mass = &PROPERTIES[property("mass_earth")];
    assert_eq!(
        filters::value_text(mass, &range(None, Some(2.0))),
        "at most 2 Earth masses"
    );
    assert_eq!(filters::parse_end(" 12,000 ", false), Ok(Some(12000.0)));
    assert_eq!(filters::parse_end("", true), Ok(None));
    assert!(
        filters::parse_end("2.5", true)
            .unwrap_err()
            .contains("whole")
    );
    assert!(filters::parse_end("lots", false).is_err());
}

#[test]
fn search_box_takes_a_designation_or_whole_ns_text() {
    let mut browser = Browser::default();
    filters::set_kind(&mut browser.query, "star");
    browser.text = "TS 1-2-3 b".into();
    browser.read_text();
    assert_eq!(browser.query.text, "TS 1-2-3 b");
    assert_eq!(
        browser.query.kinds,
        ["star"],
        "a designation keeps the filters"
    );
    browser.text = "kind=planet planet_class=gas_giant within_ly=100".into();
    browser.read_text();
    assert_eq!(
        browser.query.to_text(),
        "kind=planet planet_class=gas_giant within_ly=100"
    );
    assert!(browser.text.is_empty());
    browser.text = "kind=planet radius_km=big".into();
    browser.read_text();
    assert!(browser.text_error.is_some());
}

#[test]
fn edits_send_once_and_search_commands_open_the_browser_on_their_query() {
    let mut app = App::new();
    app.add_message::<UiCommand>().add_plugins(BrowserPlugin);
    let mut out = Vec::new();
    {
        let mut browser = app.world_mut().resource_mut::<Browser>();
        browser.send(&mut out);
        browser.send(&mut out);
    }
    assert_eq!(out.len(), 1, "an unchanged query is sent once");
    // The browser's own command comes back through `follow` unchanged.
    app.world_mut().write_message(out.pop().unwrap());
    app.update();
    app.world_mut().resource_mut::<Browser>().send(&mut out);
    assert!(out.is_empty());
    // Another front end's search opens the browser on its filters.
    app.world_mut().write_message(UiCommand::Search {
        query: "kind=star star_class=m within_ly=10".into(),
    });
    app.update();
    let browser = app.world().resource::<Browser>();
    assert!(browser.open);
    assert_eq!(filters::chips(&browser.query).len(), 2);
    // Clashing filters are explained, not sent.
    let mut browser = app.world_mut().resource_mut::<Browser>();
    browser.query.filters.push(Filter {
        property: "radius_km".into(),
        test: Test::Range {
            min: Some(1.0),
            max: None,
        },
    });
    browser.send(&mut out);
    assert!(out.is_empty());
    assert!(
        browser
            .problem
            .as_ref()
            .unwrap()
            .contains("applies to planet")
    );
}
