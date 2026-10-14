//! The **Add filter** picker: types, then properties of the kinds left, then
//! relationships; and the editor for one filter's values.
use super::filters::{self, Slot};
use crate::ui::kit::{self, Theme};
use bevy_egui::egui::{self, Color32};
use space_simulation::search::registry::{KINDS, PROPERTIES, RELATIONS, Values};
use space_simulation::search::{Filter, Query, Test};

pub struct Picker {
    find: String,
    editor: Option<Editor>,
    /// Focus the find field on the first frame.
    fresh: bool,
}

/// One filter's values while they are being chosen.
struct Editor {
    slot: Slot,
    property: usize,
    /// Chosen option IDs.
    chosen: Vec<String>,
    range: kit::RangeInput,
    flag: Option<bool>,
    /// Editing a chip, rather than adding a filter.
    existing: bool,
}

impl Picker {
    pub fn new() -> Self {
        Self {
            find: String::new(),
            editor: None,
            fresh: true,
        }
    }

    /// Opens straight to the editor of `property`'s filter in `slot`.
    pub fn edit(query: &Query, slot: Slot, property: usize) -> Self {
        Self {
            editor: Some(Editor::new(query, slot, property)),
            ..Self::new()
        }
    }
}

impl Editor {
    fn new(query: &Query, slot: Slot, property: usize) -> Self {
        let mut editor = Self {
            slot,
            property,
            chosen: Vec::new(),
            range: kit::RangeInput::default(),
            flag: None,
            existing: false,
        };
        if let Some(filter) = filters::filter(query, slot, property) {
            editor.existing = true;
            let end = |n: &Option<f64>| n.map_or(String::new(), filters::number_text);
            match &filter.test {
                Test::OneOf(ids) => editor.chosen = ids.clone(),
                Test::Range { min, max } => editor.range = kit::RangeInput::new(end(min), end(max)),
                Test::Is(flag) => editor.flag = Some(*flag),
            }
        }
        editor
    }

    /// The test the chosen values make, or why there is none yet.
    fn test(&self) -> Result<Test, String> {
        match PROPERTIES[self.property].values {
            Values::Choice { .. } if self.chosen.is_empty() => Err("Choose at least one".into()),
            Values::Choice { .. } => Ok(Test::OneOf(self.chosen.clone())),
            Values::Flag => self
                .flag
                .map(Test::Is)
                .ok_or_else(|| "Choose yes or no".into()),
            Values::Range { integer } => {
                let kit::NumericRange { min, max } = self.range.value(integer)?;
                match (min, max) {
                    (None, None) => Err("Give a minimum, a maximum or both".into()),
                    (Some(min), Some(max)) if min > max => {
                        Err("The minimum is above the maximum".into())
                    }
                    _ => Ok(Test::Range { min, max }),
                }
            }
        }
    }
}

/// Draws the picker while it is open, applying its choices to `query`.
pub fn show(ctx: &egui::Context, theme: &Theme, picker: &mut Option<Picker>, query: &mut Query) {
    let Some(state) = picker else {
        return;
    };
    let mut close = false;
    let id = egui::Id::new("filter-picker");
    // Above the browser window, which is in the foreground layer too.
    let modal = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).order(egui::Order::Tooltip))
        .frame(
            egui::Frame::new()
                .fill(theme.fill)
                .stroke((1.0, theme.border))
                .corner_radius(theme.radius)
                .inner_margin(theme.padding),
        )
        .backdrop_color(Color32::from_black_alpha(140))
        .show(ctx, |ui| {
            theme.style_scrollbars(ui.style_mut());
            ui.set_width(460.0);
            // Areas reuse their previous content height. Let the scroll list
            // grow beyond that inherited size, including after a short editor
            // or a narrowed find result, while leaving room around the modal.
            ui.set_max_height(
                (ctx.content_rect().height() - 40.0 - 2.0 * f32::from(theme.padding)).max(0.0),
            );
            ui.spacing_mut().item_spacing = egui::Vec2::splat(kit::SPACING);
            let title = match &state.editor {
                Some(editor) => editor_title(editor),
                None => "Add a filter".into(),
            };
            ui.horizontal(|ui| {
                ui.set_min_height(28.0);
                let galley = ui.painter().layout_no_wrap(
                    title,
                    egui::FontId::proportional(18.0),
                    theme.text,
                );
                let bounds = galley.mesh_bounds;
                let title = ui.label(galley);
                close |= kit::aligned_close_button(
                    ui,
                    theme,
                    bounds.translate(title.rect.min.to_vec2()),
                )
                .clicked();
            });
            ui.add_space(kit::SPACING);
            match &mut state.editor {
                Some(editor) => {
                    let (done, back) = edit(ui, theme, editor, query);
                    close |= done;
                    if back {
                        state.editor = None;
                    }
                }
                None => {
                    if let Some(next) = choose(ui, theme, state, query) {
                        match next {
                            Next::Close => close = true,
                            Next::Edit(slot, property) => {
                                state.editor = Some(Editor::new(query, slot, property));
                            }
                        }
                    }
                }
            }
        });
    crate::render::window_blur::register(ctx, id, modal.response.rect);
    if close || modal.should_close() {
        *picker = None;
    }
}

fn editor_title(editor: &Editor) -> String {
    let label = PROPERTIES[editor.property].label;
    match editor.slot {
        Slot::Own => label.into(),
        Slot::Related(relation) => format!("{} · {label}", RELATIONS[relation].label),
    }
}

enum Next {
    Close,
    Edit(Slot, usize),
}

/// What a property's values look like, beside its name in the list.
fn shape(index: usize) -> String {
    let property = &PROPERTIES[index];
    match property.values {
        Values::Choice { options } => format!("{} choices", options.len()),
        Values::Flag => "yes or no".into(),
        Values::Range { .. } => {
            filters::unit_text(property).map_or("range".into(), |u| format!("range, {u}"))
        }
    }
}

/// The list of everything a filter can be.
fn choose(ui: &mut egui::Ui, theme: &Theme, state: &mut Picker, query: &mut Query) -> Option<Next> {
    let find = kit::field(
        ui,
        theme,
        "picker-find",
        "Find a type or property…",
        &mut state.find,
        f32::INFINITY,
    );
    if state.fresh {
        find.request_focus();
        state.fresh = false;
    }
    let needle = state.find.trim().to_lowercase();
    let wanted = |label: &str, id: &str| {
        needle.is_empty() || label.to_lowercase().contains(&needle) || id.contains(&needle)
    };
    let mut next = None;
    kit::ScrollArea::vertical()
        .id_salt("filter-picker-list")
        .scroll_source(kit::SCROLL_SOURCE)
        .max_height(560.0_f32.min((ui.ctx().content_rect().height() - 160.0).max(0.0)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let properties: Vec<_> = filters::offered_properties(query, Slot::Own)
                .into_iter()
                .filter(|&i| wanted(PROPERTIES[i].label, PROPERTIES[i].id))
                .collect();
            if !properties.is_empty() {
                group(ui, theme, "Properties");
                for index in properties {
                    if kit::item(ui, theme, PROPERTIES[index].label, &shape(index)).clicked() {
                        next = Some(Next::Edit(Slot::Own, index));
                    }
                }
            }
            for relation in filters::offered_relations(query) {
                let r = &RELATIONS[relation];
                let slot = Slot::Related(relation);
                let kinds: Vec<_> = KINDS
                    .iter()
                    .filter(|k| {
                        r.target_kinds().has(k.id)
                            && wanted(&format!("{} {}", r.label, k.label), k.id)
                    })
                    .collect();
                let properties: Vec<_> = filters::offered_properties(query, slot)
                    .into_iter()
                    .filter(|&i| {
                        wanted(
                            &format!("{} {}", r.label, PROPERTIES[i].label),
                            PROPERTIES[i].id,
                        )
                    })
                    .collect();
                if kinds.is_empty() && properties.is_empty() {
                    continue;
                }
                group(ui, theme, r.label);
                if kinds.len() > 1 {
                    for kind in kinds {
                        if kit::item(
                            ui,
                            theme,
                            &format!("{} {}", r.label, kind.label.to_lowercase()),
                            "type",
                        )
                        .clicked()
                        {
                            filters::set_related_kind(query, relation, kind.id);
                            next = Some(Next::Close);
                        }
                    }
                }
                for index in properties {
                    let label = format!("{} · {}", r.label, PROPERTIES[index].label);
                    if kit::item(ui, theme, &label, &shape(index)).clicked() {
                        next = Some(Next::Edit(slot, index));
                    }
                }
            }
        });
    next
}

fn group(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(kit::SPACING);
    kit::eyebrow(ui, theme, text);
    ui.add_space(kit::SPACING);
}

/// The editor for one filter. Returns whether it was applied, and whether
/// to go back to the list.
fn edit(ui: &mut egui::Ui, theme: &Theme, editor: &mut Editor, query: &mut Query) -> (bool, bool) {
    let property = &PROPERTIES[editor.property];
    match property.values {
        Values::Choice { options } => {
            kit::note(ui, theme, "Any of the chosen:");
            ui.horizontal_wrapped(|ui| {
                for (id, label) in options {
                    let on = editor.chosen.iter().any(|c| c == id);
                    if kit::choice(ui, theme, label, on) {
                        if on {
                            editor.chosen.retain(|c| c != id);
                        } else {
                            editor.chosen.push((*id).to_owned());
                        }
                    }
                }
            });
        }
        Values::Flag => {
            ui.horizontal(|ui| {
                for (label, value) in [("Yes", true), ("No", false)] {
                    if kit::choice(ui, theme, label, editor.flag == Some(value)) {
                        editor.flag = Some(value);
                    }
                }
            });
        }
        Values::Range { integer } => {
            let unit = filters::unit_text(property).unwrap_or("");
            kit::note(ui, theme, "Leave either end blank to leave it open.");
            let range = kit::range_field(
                ui,
                theme,
                property.id,
                &mut editor.range,
                kit::RangeField {
                    unit,
                    integer,
                    slider: slider_span(property.id),
                },
            );
            debug_assert_eq!(range.value, editor.range.value(integer));
            if range.response.changed() {
                ui.ctx().request_repaint();
            }
        }
    }
    let test = editor.test();
    if let (Err(reason), true) = (
        &test,
        editor.existing || !editor.range.min.is_empty() || !editor.range.max.is_empty(),
    ) {
        kit::note(ui, theme, reason);
    }
    let mut done = false;
    let mut back = false;
    ui.horizontal(|ui| {
        let label = if editor.existing {
            "Update filter"
        } else {
            "Add filter"
        };
        if kit::primary_button(ui, theme, label, test.is_ok()).clicked()
            && let Ok(test) = test
        {
            filters::set_filter(
                query,
                editor.slot,
                Filter {
                    property: property.id.into(),
                    test,
                },
            );
            done = true;
        }
        if !editor.existing && kit::button(ui, theme, "Back").clicked() {
            back = true;
        }
    });
    (done, back)
}

/// Property-specific exploration spans, not physical or query validation bounds.
fn slider_span(property: &str) -> Option<kit::RangeSlider> {
    use kit::RangeScale::{Linear, Logarithmic};
    let (min, max, scale, step) = match property {
        "temperature_k" => (1000.0, 50000.0, Linear, Some(1.0)),
        "mass_solar" => (0.08, 100.0, Logarithmic, None),
        "radius_solar" => (0.01, 1000.0, Logarithmic, None),
        "luminosity_solar" => (0.0001, 1000000.0, Logarithmic, None),
        "star_count" => (1.0, 4.0, Linear, Some(1.0)),
        "planet_count" => (0.0, 30.0, Linear, Some(1.0)),
        "radius_km" => (100.0, 100000.0, Logarithmic, None),
        "mass_earth" => (0.0001, 10000.0, Logarithmic, None),
        "semi_major_au" => (0.01, 1000.0, Logarithmic, None),
        "eccentricity" => (0.0, 1.0, Linear, Some(0.001)),
        "period_days" => (0.1, 1000000.0, Logarithmic, None),
        _ => return None,
    };
    Some(kit::RangeSlider {
        min,
        max,
        scale,
        step,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_numeric_property_has_a_valid_slider_configuration() {
        for property in PROPERTIES {
            if let Values::Range { integer } = property.values {
                let span = slider_span(property.id)
                    .unwrap_or_else(|| panic!("missing slider for {}", property.id));
                assert!(span.min.is_finite() && span.max.is_finite() && span.min < span.max);
                if span.scale == kit::RangeScale::Logarithmic {
                    assert!(span.min > 0.0);
                }
                if integer {
                    assert_eq!(span.step, Some(1.0));
                    assert_eq!(span.min.fract(), 0.0);
                    assert_eq!(span.max.fract(), 0.0);
                }
            } else {
                assert!(slider_span(property.id).is_none());
            }
        }
    }
}
