//! The **Add filter** picker: types, then properties of the kinds left, then
//! relationships; and the editor for one filter's values.
use super::filters::{self, Slot};
use crate::ui::kit::{self, Theme};
use bevy_egui::egui::{self, Color32};
use space_simulation::search::registry::{self, KINDS, PROPERTIES, RELATIONS, Values};
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
    min: String,
    max: String,
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
            min: String::new(),
            max: String::new(),
            flag: None,
            existing: false,
        };
        if let Some(filter) = filters::filter(query, slot, property) {
            editor.existing = true;
            let end = |n: &Option<f64>| n.map_or(String::new(), filters::number_text);
            match &filter.test {
                Test::OneOf(ids) => editor.chosen = ids.clone(),
                Test::Range { min, max } => (editor.min, editor.max) = (end(min), end(max)),
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
                let min = filters::parse_end(&self.min, integer)?;
                let max = filters::parse_end(&self.max, integer)?;
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
                .fill(theme.fill.to_opaque())
                .stroke((1.0, theme.border))
                .corner_radius(theme.radius)
                .inner_margin(theme.padding),
        )
        .backdrop_color(Color32::from_black_alpha(140))
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
            let title = match &state.editor {
                Some(editor) => editor_title(editor),
                None => "Add a filter".into(),
            };
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(title)
                        .font(egui::FontId::proportional(18.0))
                        .color(theme.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close |= kit::link(ui, theme, "×").clicked();
                });
            });
            kit::rule(ui, theme);
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
    egui::ScrollArea::vertical()
        .id_salt("filter-picker-list")
        .scroll_source(kit::SCROLL_SOURCE)
        .max_height(420.0)
        .show(ui, |ui| {
            let kinds: Vec<_> = KINDS
                .iter()
                .filter(|k| k.available() && wanted(k.label, k.id))
                .collect();
            if !kinds.is_empty() {
                group(ui, theme, "Type");
                for kind in kinds {
                    let family = registry::family(kind.family).map_or("", |f| f.label);
                    if kit::item(ui, theme, kind.label, family).clicked() {
                        filters::set_kind(query, kind.id);
                        next = Some(Next::Close);
                    }
                }
            }
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
    kit::note(
        ui,
        theme,
        "Results match every filter. Relationship filters all hold for the same related object.",
    );
    next
}

fn group(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(6.0);
    kit::eyebrow(ui, theme, text);
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
        Values::Range { .. } => {
            let unit = filters::unit_text(property).unwrap_or("");
            kit::note(ui, theme, "Leave either end blank to leave it open.");
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 40.0) / 2.0;
                kit::field(
                    ui,
                    theme,
                    "range-min",
                    &format!("Minimum {unit}"),
                    &mut editor.min,
                    width,
                );
                kit::field(
                    ui,
                    theme,
                    "range-max",
                    &format!("Maximum {unit}"),
                    &mut editor.max,
                    width,
                );
            });
        }
    }
    let test = editor.test();
    if let (Err(reason), true) = (
        &test,
        editor.existing || !editor.min.is_empty() || !editor.max.is_empty(),
    ) {
        kit::note(ui, theme, reason);
    }
    ui.add_space(6.0);
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
