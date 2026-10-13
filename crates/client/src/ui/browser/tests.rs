use super::filters::{self, Chip, Slot};
use super::*;
use space_simulation::search::registry::{self, PROPERTIES};
use space_simulation::search::{
    DEFAULT_WORK_CAP, Filter, Origin, Search, SearchRequest, SearchResult, State, Test,
};

fn property(id: &str) -> usize {
    registry::property_index(id).unwrap()
}

#[test]
fn filter_picker_and_property_editors_have_no_header_divider() {
    let query = Query::default();
    for state in std::iter::once(picker::Picker::new()).chain(
        (0..PROPERTIES.len()).map(|property| picker::Picker::edit(&query, Slot::Own, property)),
    ) {
        let ctx = egui::Context::default();
        kit::plain_style(&ctx);
        let mut picker = Some(state);
        let mut query = query.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| picker::show(ctx, &THEME, &mut picker, &mut query),
        );
        assert!(
            !output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::epaint::Shape::LineSegment { points, stroke }
                    if stroke.color == THEME.line() && points[0].y == points[1].y
            )),
            "picker header should use spacing rather than a divider"
        );
    }
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

#[test]
fn opening_browser_keeps_its_height_stable_and_inside_the_viewport() {
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let searches = Searches::default();
    let nav = Navigation::default();
    let universe = Universe(space_shared::Galaxy {
        source: String::new(),
        systems: Vec::new(),
    });
    let field = crate::stars::stream::Starfield::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0));
    let mut browser = Browser {
        open: true,
        ..Default::default()
    };
    let mut previous = None;
    for frame in 0..30 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.05),
                ..Default::default()
            },
            |ctx| {
                window(
                    ctx,
                    &mut browser,
                    View {
                        searches: &searches,
                        nav: &nav,
                        universe: &universe,
                        field: &field,
                    },
                    &mut Vec::new(),
                );
            },
        );
        let bounds = ctx.memory(|memory| {
            memory
                .area_rect(egui::Id::new(("window", "Object browser")))
                .unwrap()
        });
        if frame > 1 {
            assert!(
                screen.contains_rect(bounds),
                "browser left the viewport: {bounds:?}"
            );
            assert!(
                (bounds.height() - previous.unwrap_or(bounds.height())).abs() < 1.0,
                "browser keeps growing: {bounds:?}"
            );
        }
        previous = Some(bounds.height());
    }
}

#[test]
fn result_rows_follow_scene_selection_and_emit_selection_commands() {
    use crate::{Selection, objects::ObjectRef};
    let results = run(Query::parse("kind=planet within_ly=20 limit=2").unwrap());
    assert_eq!(results.len(), 2);
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let searches = Searches::default();
    let universe = Universe(space_shared::Galaxy {
        source: String::new(),
        systems: vec![],
    });
    let field = crate::stars::stream::Starfield::default();
    let mut nav = Navigation::default();
    let frame = |nav: &Navigation, events| {
        let mut commands = Vec::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    results::list(
                        ui,
                        &results,
                        250.0,
                        &mut commands,
                        &View {
                            searches: &searches,
                            nav,
                            universe: &universe,
                            field: &field,
                        },
                    );
                });
            },
        );
        (output, commands)
    };
    let edges = |output: egui::FullOutput| {
        output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.fill == THEME.accent && rect.rect.width() == 2.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    frame(&nav, vec![]);
    assert!(edges(frame(&nav, vec![]).0).is_empty());
    let ObjectRef::Planet { id } = &results[0].object else {
        panic!("expected planet")
    };
    nav.select(Some(Selection::Planet(id.clone())));
    for _ in 0..20 {
        frame(&nav, vec![]);
    }
    let selected = edges(frame(&nav, vec![]).0);
    assert_eq!(selected.len(), 2);
    nav.select(None);
    for _ in 0..20 {
        frame(&nav, vec![]);
    }
    assert!(edges(frame(&nav, vec![]).0).is_empty());
    let ObjectRef::Planet { id: other } = &results[1].object else {
        panic!("expected planet")
    };
    nav.select(Some(Selection::Planet(other.clone())));
    for _ in 0..20 {
        frame(&nav, vec![]);
    }
    let other_edges = edges(frame(&nav, vec![]).0);
    assert_eq!(other_edges.len(), 2);
    assert!(other_edges[0].top() > selected[0].top());
    // Clicking the first row emits the application's normal planet selection.
    let pos = selected[0].center() + egui::vec2(30.0, 0.0);
    frame(&nav, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        let (_, commands) = frame(
            &nav,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        if !pressed {
            assert_eq!(commands, vec![UiCommand::Inspect { planet: id.clone() }]);
        }
    }
}

#[test]
#[allow(clippy::field_reassign_with_default)] // Searches has private service fields.
fn animated_result_panes_never_expand_their_parent_viewport() {
    use crate::{Selection, objects::ObjectRef};
    let query = Query::parse("kind=planet within_ly=20 limit=1").unwrap();
    let hits = run(query.clone());
    let ObjectRef::Planet { id } = &hits[0].object else {
        panic!("expected planet")
    };
    let mut searches = Searches::default();
    searches.shown_entry = Some(crate::search::Entry {
        id: 1,
        asker: crate::search::Asker::Game,
        query: query.to_text(),
        origin_pc: [0.0; 3],
        work_cap: DEFAULT_WORK_CAP,
        progress: space_simulation::search::Progress {
            state: State::Done,
            ..Default::default()
        },
        error: None,
        result_count: hits.len(),
        results: Some(hits.clone()),
        revision: 1,
    });
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let universe = Universe(space_shared::Galaxy {
        source: String::new(),
        systems: vec![],
    });
    let field = crate::stars::stream::Starfield::default();
    let mut nav = Navigation::default();
    let mut browser = Browser {
        sent: Some(query.to_text()),
        query,
        ..Default::default()
    };
    for frame in 0..40 {
        if frame == 5 {
            nav.select(Some(Selection::Planet(id.clone())));
        }
        if frame == 30 {
            nav.select(None);
        }
        let width = if (15..25).contains(&frame) {
            600.0
        } else {
            900.0
        };
        let _ = ctx.run(
            egui::RawInput {
                time: Some(f64::from(frame) * 0.03),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bounds = ui.available_rect_before_wrap();
                    results::show(
                        ui,
                        &mut browser,
                        &View {
                            searches: &searches,
                            nav: &nav,
                            universe: &universe,
                            field: &field,
                        },
                        &mut Vec::new(),
                    );
                    assert!(
                        bounds.expand(0.1).contains_rect(ui.min_rect()),
                        "animated children expanded their parent: {:?} beyond {bounds:?}",
                        ui.min_rect()
                    );
                });
            },
        );
    }
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
    assert_eq!(kit::parse_range_end(" 12,000 ", false), Ok(Some(12000.0)));
    assert_eq!(kit::parse_range_end("", true), Ok(None));
    assert!(
        kit::parse_range_end("2.5", true)
            .unwrap_err()
            .contains("whole")
    );
    assert!(kit::parse_range_end("lots", false).is_err());
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
