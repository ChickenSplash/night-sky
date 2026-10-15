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
fn every_query_replacement_animates_outgoing_chips() {
    for cause in [
        "reset",
        "type",
        "search text",
        "clear command",
        "search command",
        "edit",
    ] {
        let ctx = egui::Context::default();
        kit::plain_style(&ctx);
        let mut app = App::new();
        app.add_message::<UiCommand>().add_plugins(BrowserPlugin);
        let query = Query::parse("temperature_k=3000..9000 radius_solar=0.1..2").unwrap();
        let labels = filters::chips(&query)
            .into_iter()
            .map(|(_, label)| label)
            .collect::<Vec<_>>();
        app.world_mut().resource_mut::<Browser>().query = query;
        let frame = |app: &mut App, time| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.world_mut().resource_mut::<Browser>().chips(ui);
                    });
                },
            )
        };
        let clip = |output: &egui::FullOutput, label: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(shape.clip_rect)
                }
                _ => None,
            })
        };
        frame(&mut app, 0.0);
        let full = frame(&mut app, 0.2);
        match cause {
            "clear command" | "search command" => {
                let command = if cause == "clear command" {
                    UiCommand::ClearSearch
                } else {
                    UiCommand::Search {
                        query: "kind=planet within_ly=50".into(),
                    }
                };
                app.world_mut().write_message(command);
                app.update();
            }
            _ => {
                let mut browser = app.world_mut().resource_mut::<Browser>();
                match cause {
                    "reset" => browser.reset(),
                    "type" => filters::set_kind(&mut browser.query, "planet"),
                    "search text" => {
                        browser.text = "kind=planet within_ly=50".into();
                        browser.read_text();
                    }
                    "edit" => {
                        browser.query =
                            Query::parse("temperature_k=4000..8000 radius_solar=0.2..3").unwrap();
                    }
                    _ => unreachable!(),
                }
            }
        }
        frame(&mut app, 0.21);
        let exiting = frame(&mut app, 0.3);
        for label in &labels {
            let old = clip(&full, label).unwrap();
            let now = clip(&exiting, label)
                .unwrap_or_else(|| panic!("{cause} dropped {label} without an exit"));
            assert!(
                now.width() > 0.0 && now.width() < old.width(),
                "{cause}: {label} must shrink"
            );
        }
        let gone = frame(&mut app, 0.41);
        for label in &labels {
            assert!(
                clip(&gone, label).is_none(),
                "{cause}: {label} must finish exiting"
            );
        }
    }
}

#[test]
fn search_controls_precede_a_separate_chip_row_with_optional_system_scope() {
    for width in [1000.0, 480.0, 350.0] {
        for at_system in [false, true] {
            let ctx = egui::Context::default();
            kit::plain_style(&ctx);
            let searches = Searches::default();
            let nav = Navigation {
                arrived: at_system,
                visit: Some(crate::Visit::Generated(Box::new(
                    crate::galaxy_nav::generated("g78040000.2.854", bevy::math::DVec3::ZERO)
                        .unwrap(),
                ))),
                ..Default::default()
            };
            let universe = Universe(space_shared::Galaxy {
                source: String::new(),
                systems: vec![],
            });
            let field = crate::stars::stream::Starfield::default();
            let mut browser = Browser {
                query: Query::parse("kind=star temperature_k=3000..9000").unwrap(),
                ..Default::default()
            };
            let mut output = None;
            for frame in 0..3 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        time: Some(f64::from(frame) * 0.2),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            browser.body(
                                ui,
                                &View {
                                    searches: &searches,
                                    nav: &nav,
                                    universe: &universe,
                                    field: &field,
                                    craft: crate::craft::Now::none(),
                                },
                                &mut vec![],
                            );
                        });
                    },
                ));
            }
            let output = output.unwrap();
            let text = |label: &str| {
                output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
            };
            let search = text("Designation or ID, or ns search text…").unwrap();
            let within = text("Within:").unwrap();
            let show = text("Show:").unwrap();
            let add = text("+ Add filter").unwrap();
            let refresh = text("Refresh").unwrap();
            let kind = text("Type: Star").unwrap();
            assert!(within.top() > search.bottom());
            assert!(kind.top() > add.bottom().max(refresh.bottom()));
            assert_eq!(text("This system").is_some(), at_system);
            if width == 1000.0 {
                let preceding = if at_system {
                    text("This system").unwrap()
                } else {
                    show
                };
                assert!(add.left() > preceding.right());
                assert!((add.center().y - preceding.center().y).abs() < 1.0);
                if at_system {
                    assert!(preceding.left() > show.right());
                }
            }
            let chips = filters::chips(&browser.query);
            let chip = text(
                &chips
                    .iter()
                    .find(|(chip, _)| matches!(chip, Chip::Filter { .. }))
                    .unwrap()
                    .1,
            )
            .unwrap();
            assert!(chip.top() > add.bottom().max(refresh.bottom()));
            let reset = text("Reset").unwrap();
            assert!(reset.left() > refresh.right());
            assert!((reset.center().y - refresh.center().y).abs() < 1.0);
        }
    }
}

#[test]
fn reset_stays_right_of_refresh_and_disappears_after_full_reset() {
    for width in [1000.0, 480.0, 350.0] {
        for query in [Query::default(), Query::parse("kind=star temperature_k=3000..9000 radius_solar=0.1..2 mass_solar=0.08..5 system.multiple=no").unwrap()] {
            let ctx = egui::Context::default();
            kit::plain_style(&ctx);
            let mut browser = Browser { query, text: "unfinished search".into(), text_error: Some("test error".into()), ..Default::default() };
            let searches = Searches::default();
            let nav = Navigation::default();
            let universe = Universe(space_shared::Galaxy { source: String::new(), systems: vec![] });
            let field = crate::stars::stream::Starfield::default();
            browser.query.scope = Scope::Within { radius_ly: 123.0 };
            browser.query.limit = 123;
            let time = std::cell::Cell::new(0.0);
            let right = std::cell::Cell::new(0.0);
            let frame = |browser: &mut Browser, events| {
                time.set(time.get() + 0.05);
                ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                    events, time: Some(time.get()), ..Default::default()
                }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        right.set(ui.max_rect().right());
                        browser.scope(ui, &View { searches: &searches, nav: &nav, universe: &universe, field: &field, craft: crate::craft::Now::none() }, &mut vec![]);
                    });
                })
            };
            for _ in 0..10 { frame(&mut browser, vec![]); }
            let output = frame(&mut browser, vec![]);
            let reset = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Reset" => Some(egui::Rect::from_min_size(text.pos, text.galley.size())),
                _ => None,
            }).unwrap();
            assert!((reset.right() + 10.0 - right.get()).abs() < 1.0, "reset must align to the row's right edge at width {width}: {reset:?}, right {}", right.get());
            assert!(output.shapes.iter().all(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) => text.pos.y <= reset.top() + 1.0,
                _ => true,
            }), "reset must be on the final wrapped row");
            let pos = reset.center();
            frame(&mut browser, vec![egui::Event::PointerMoved(pos)]);
            for pressed in [true, false] {
                frame(&mut browser, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
            }
            assert_eq!(browser.query, Query::default());
            assert!(browser.text.is_empty() && browser.text_error.is_none());
            let output = frame(&mut browser, vec![]);
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::epaint::Shape::Text(text) if text.galley.text() == "Reset")));
        }
    }
}

#[test]
fn reset_is_available_for_each_nondefault_search_control() {
    assert!(!Browser::default().can_reset());
    for query in [
        "within_ly=51",
        "limit=51",
        "kind=star",
        "family=stars",
        "temperature_k=3000..",
        "system.multiple=yes",
        "system=g78040000.2.854",
    ] {
        assert!(
            Browser {
                query: Query::parse(query).unwrap(),
                ..Default::default()
            }
            .can_reset(),
            "{query}"
        );
    }
    assert!(
        Browser {
            text: "unfinished designation".into(),
            ..Default::default()
        }
        .can_reset()
    );
}

#[test]
fn table_headers_match_shared_section_heading_typography() {
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let searches = Searches::default();
    let nav = Navigation::default();
    let universe = Universe(space_shared::Galaxy {
        source: String::new(),
        systems: vec![],
    });
    let field = crate::stars::stream::Starfield::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.allocate_ui(egui::vec2(600.0, 300.0), |ui| {
                        results::list(
                            ui,
                            &[],
                            300.0,
                            &mut Vec::new(),
                            &View {
                                searches: &searches,
                                nav: &nav,
                                universe: &universe,
                                field: &field,
                                craft: crate::craft::Now::none(),
                            },
                        );
                    });
                    kit::eyebrow(ui, &THEME, "Selected");
                });
            });
        },
    );
    let format = |label| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.galley.job.sections[0].format.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing heading {label}"))
    };
    for label in ["OBJECT · SYSTEM", "CLASS", "DISTANCE"] {
        assert_eq!(format(label), format("SELECTED"));
        let top = |label| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.pos.y)
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert!(
            (top(label) - top("SELECTED")).abs() < 0.1,
            "headings must share a horizontal baseline"
        );
    }
}

#[test]
fn type_dropdown_selects_registry_kinds_and_cleans_incompatible_filters() {
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let mut browser = Browser {
        query: Query::parse("kind=planet radius_km=..3000 orbits.star_class=m system.multiple=yes")
            .unwrap(),
        ..Default::default()
    };
    let time = std::cell::Cell::new(0.0);
    let frame = |browser: &mut Browser, events| {
        time.set(time.get() + 0.05);
        let mut commands = Vec::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                events,
                time: Some(time.get()),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| browser.chips(ui));
                browser.send(&mut commands);
            },
        );
        (output, commands)
    };
    let text_position = |output: &egui::FullOutput, label: &str, exact| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text)
                    if if exact {
                        text.galley.text() == label
                    } else {
                        text.galley.text().starts_with(label)
                    } =>
                {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing dropdown label: {label}"))
    };
    let click = |browser: &mut Browser, pos| {
        frame(browser, vec![egui::Event::PointerMoved(pos)]);
        frame(
            browser,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        frame(
            browser,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        )
    };
    frame(&mut browser, vec![]);
    let output = frame(&mut browser, vec![]).0;
    let dropdown = text_position(&output, "Type: Planet", false);
    click(&mut browser, dropdown);
    let output = frame(&mut browser, vec![]).0;
    for kind in space_simulation::search::registry::KINDS
        .iter()
        .filter(|kind| kind.available())
    {
        text_position(&output, kind.label, true);
    }
    let (_, commands) = click(&mut browser, text_position(&output, "Star", true));
    assert_eq!(browser.query.kinds, ["star"]);
    assert!(browser.query.filters.is_empty());
    assert_eq!(browser.query.relations.len(), 1);
    assert_eq!(browser.query.relations[0].relation, "system");
    assert!(browser.query.check().is_ok());
    assert_eq!(
        commands,
        vec![UiCommand::Search {
            query: browser.query.to_text()
        }]
    );
    let output = frame(&mut browser, vec![]).0;
    click(&mut browser, text_position(&output, "Type: Star", false));
    let output = frame(&mut browser, vec![]).0;
    click(&mut browser, text_position(&output, "All types", true));
    assert!(browser.query.kinds.is_empty() && browser.query.family.is_none());
    assert_eq!(
        browser.query.relations.len(),
        1,
        "All types preserves applicable property filters"
    );
}

#[test]
fn filter_picker_no_longer_offers_top_level_types() {
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    let mut state = Some(picker::Picker::new());
    let mut query = Query::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        },
        |ctx| picker::show(ctx, &THEME, &mut state, &mut query),
    );
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 800.0),
            )),
            ..Default::default()
        },
        |ctx| picker::show(ctx, &THEME, &mut state, &mut query),
    );
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text() == "TYPE")));
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text() == "PROPERTIES")));
    let top = |label| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some(text.pos.y),
                _ => None,
            })
            .unwrap()
    };
    assert!(
        (top("Surface temperature") - top("Star class") - (THEME.row + 6.0)).abs() < 0.1,
        "filter items must be contiguous"
    );
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::epaint::Shape::Text(text) if text.galley.text().contains("Results match every filter"))));
}

#[test]
fn filter_picker_uses_a_tall_list_and_stays_inside_small_viewports() {
    for height in [800.0, 480.0] {
        let ctx = egui::Context::default();
        kit::plain_style(&ctx);
        let mut state = Some(picker::Picker::new());
        let mut query = Query::default();
        let mut painted = false;
        for _ in 0..4 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, height),
                    )),
                    ..Default::default()
                },
                |ctx| picker::show(ctx, &THEME, &mut state, &mut query),
            );
            let list = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == "Star class" => {
                    Some(shape.clip_rect)
                }
                _ => None,
            });
            // The first frame measures the modal without painting it.
            if let Some(list) = list {
                painted = true;
                let list = list.shrink(ctx.style().visuals.clip_rect_margin);
                let expected = 560.0_f32.min(height - 160.0);
                assert!(
                    (list.height() - expected).abs() < 3.0,
                    "picker list should use {expected} px, got {}",
                    list.height()
                );
                assert!(list.top() >= 0.0 && list.bottom() <= height);
            }
        }
        assert!(painted, "picker list must be painted");
    }
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
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| picker::show(ctx, &THEME, &mut picker, &mut query),
        );
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
        let centre = |close| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(text)
                        if if close {
                            text.galley.text() == "×"
                        } else {
                            text.galley.job.sections[0].format.font_id.size == 18.0
                        } =>
                    {
                        Some(text.pos.y + text.galley.mesh_bounds.center().y)
                    }
                    _ => None,
                })
                .unwrap()
        };
        assert!(
            (centre(true) - centre(false)).abs() < 1.0,
            "picker close glyph must align with the title"
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
                        craft: crate::craft::Now::none(),
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
fn results_scrollbar_only_appears_when_rows_overflow() {
    let results = run(Query::parse("kind=planet within_ly=20 limit=3").unwrap());
    assert_eq!(results.len(), 3);
    let ctx = egui::Context::default();
    kit::plain_style(&ctx);
    ctx.style_mut(|style| style.animation_time = 0.0);
    let searches = Searches::default();
    let nav = Navigation::default();
    let universe = Universe(space_shared::Galaxy {
        source: String::new(),
        systems: vec![],
    });
    let field = crate::stars::stream::Starfield::default();
    let frame = |height| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    results::list(
                        ui,
                        &results,
                        height,
                        &mut Vec::new(),
                        &View {
                            searches: &searches,
                            nav: &nav,
                            universe: &universe,
                            field: &field,
                            craft: crate::craft::Now::none(),
                        },
                    );
                });
            },
        )
    };
    for (height, expected) in [(300.0, 0), (100.0, 1), (300.0, 0)] {
        frame(height);
        let output = frame(height);
        let handles = output
            .shapes
            .iter()
            .filter(|shape| {
                matches!(&shape.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == THEME.accent.gamma_multiply(0.12)
                && (rect.rect.width() - 10.0).abs() < 0.1)
            })
            .count();
        assert_eq!(handles, expected, "scrollbar visibility at height {height}");
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
                            craft: crate::craft::Now::none(),
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
        live: None,
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
                            craft: crate::craft::Now::none(),
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
fn replacing_or_clearing_a_query_discards_stale_validation_errors() {
    let mut app = App::new();
    app.add_message::<UiCommand>().add_plugins(BrowserPlugin);
    for command in [
        UiCommand::Search {
            query: "kind=star within_ly=10".into(),
        },
        UiCommand::ClearSearch,
    ] {
        {
            let mut browser = app.world_mut().resource_mut::<Browser>();
            browser.text_error = Some("old input error".into());
            browser.problem = Some("old query error".into());
        }
        app.world_mut().write_message(command);
        app.update();
        let browser = app.world().resource::<Browser>();
        assert!(browser.text_error.is_none() && browser.problem.is_none());
    }
    let browser = app.world().resource::<Browser>();
    assert_eq!(browser.query, Query::default());
    assert!(browser.text.is_empty());
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
