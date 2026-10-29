use super::*;

fn record() -> LayeredPlanet {
    space_simulation::demo_galaxy().systems[0].planets[0]
        .initial_result
        .as_ref()
        .unwrap()
        .record
        .clone()
}

#[test]
fn radial_picking_keeps_thin_layers_and_does_not_expand_them() {
    let mut layer = record().layers[0].clone();
    layer.inner_radius_m = 999.999;
    layer.outer_radius_m = 1000.0;
    let mut core = layer.clone();
    core.inner_radius_m = 0.0;
    core.outer_radius_m = 999.999;
    let layers = [layer, core];
    assert_eq!(extent(&layers), Some(1000.0));
    assert_eq!(hit(&layers, 999.9995), Some(0));
    assert_eq!(hit(&layers, 999.998), Some(1));
    assert_eq!(hit(&layers, 0.0), Some(1));
    assert_eq!(hit(&layers, 1000.01), None);
    assert_eq!(hit(&layers, -1.0), None);
}

#[test]
fn invalid_or_empty_geometry_is_not_drawn() {
    assert_eq!(extent(&[]), None);
    let mut layer = record().layers[0].clone();
    layer.inner_radius_m = -1.0;
    assert_eq!(extent(&[layer.clone()]), None);
    layer.inner_radius_m = 0.0;
    layer.outer_radius_m = f64::NAN;
    assert_eq!(extent(&[layer]), None);
}

#[test]
fn selection_and_model_changes_reset_layer_selection_but_not_open_state() {
    let mut state = Inspector {
        open: true,
        ..Default::default()
    };
    let mut record = record();
    state.reconcile(
        Some(ObjectRef::Planet { id: "first".into() }),
        Some(record.clone()),
    );
    state.selected = Some(record.layers.len() - 1);
    let selected = state.selected;
    state.reconcile(state.target.clone(), Some(record.clone()));
    assert_eq!(state.selected, selected);
    record.model.push_str("-changed");
    state.reconcile(state.target.clone(), Some(record));
    assert_eq!(state.selected, Some(0));
    state.reconcile(Some(ObjectRef::Galaxy), None);
    assert!(state.selected.is_none());
    state.reconcile(None, None);
    assert!(state.target.is_none());
    assert!(state.open);
}

#[test]
fn active_inspector_record_is_the_planets_record() {
    let galaxy = space_simulation::demo_galaxy();
    let planet = &galaxy.systems[0].planets[0];
    let info = super::super::describe(
        &ObjectRef::Planet {
            id: planet.id.clone(),
        },
        &galaxy,
        None,
        Now::none(),
    );
    assert_eq!(
        info.interior.as_ref(),
        planet.initial_result.as_ref().map(|r| &r.record)
    );
}

#[test]
fn cross_section_and_overview_render_in_wide_and_narrow_layouts() {
    let record = record();
    for width in [340.0, 800.0] {
        let ctx = egui::Context::default();
        let mut selected = Some(0);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    diagram(ui, &record.layers, &mut selected);
                    overview(ui, &record, &mut selected);
                });
            },
        );
        assert_eq!(selected, Some(0));
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        for removed in [
            "Temperature is a model value",
            "Reference boundary:",
            "Active model:",
        ] {
            assert!(
                !texts.iter().any(|text| text.contains(removed)),
                "{removed}"
            );
        }
        for limitation in record
            .model_limitations
            .iter()
            .chain(&record.layers[0].model_limitations)
        {
            assert!(!texts.contains(&limitation.as_str()));
        }
    }
}

#[test]
fn overview_click_selects_a_layer_without_any_scene_command() {
    let record = record();
    let ctx = egui::Context::default();
    let mut selected = Some(record.layers.len() - 1);
    let draw = |selected: &mut Option<usize>, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 1600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| overview(ui, &record, selected));
            },
        )
    };
    let output = draw(&mut selected, Vec::new());
    let name = label(&record.layers[0]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.pos + egui::vec2(3.0, 3.0))
            }
            _ => None,
        })
        .expect("first layer label is drawn");
    let _ = draw(
        &mut selected,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let _ = draw(
        &mut selected,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(selected, Some(0));
}

#[test]
fn composition_shows_layer_relative_percentages_and_trace_material_masses() {
    let mut record = record();
    record.layers[0].mass_kg = 100.0;
    record.layers[0].materials = vec![
        space_shared::LayerMaterial {
            material: "nitrogen".into(),
            mass_kg: 1.0,
        },
        space_shared::LayerMaterial {
            material: "methane".into(),
            mass_kg: 1e-12,
        },
    ];
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 2500.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| overview(ui, &record, &mut Some(0)));
        },
    );
    let text = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("COMPOSITION · % OF LAYER MASS"));
    assert!(text.contains("1.00% · 1.0000e0 kg"), "{text}");
    assert!(text.contains("1.00e-12% · 1.0000e-12 kg"), "{text}");
    assert!(text.contains(material_label("methane")));
}

#[test]
fn layer_readouts_group_distances_pressures_and_temperatures() {
    let mut record = record();
    record.layers[0].outer_radius_m = 21_580_174.0;
    record.layers[0].inner_pressure_pa = 21_580.174 * 1e5;
    record.layers[0].temperature_k = Some(12345.0);
    let ctx = egui::Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 2500.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| overview(ui, &record, &mut Some(0)));
        },
    );
    let texts: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    for expected in ["21,580.174 km", "21,580.1740 bar", "12,345 K"] {
        assert!(texts.contains(&expected), "{expected}: {texts:?}");
    }
}
