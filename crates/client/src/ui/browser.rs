//! The object browser: a window that builds a `Query` from a search box,
//! filter chips and a scope, sends it as `UiCommand::Search` whenever it
//! changes, and lists the shown request's results (`search::Searches`).
//! Opened by the third halo icon, Ctrl+K, or any `search` command, such as
//! `ns search --show`, which also fills in its filters.
mod filters;
mod picker;
mod results;

use crate::actions::UiCommand;
use crate::search::Searches;
use crate::ui::kit::TooltipExt;
use crate::ui::kit::{self, Theme};
use crate::{Navigation, Universe};
use bevy::prelude::*;
use bevy_egui::egui;
use filters::Chip;
use space_simulation::search::query::{DEFAULT_RADIUS_LY, MAX_LIMIT, MAX_RADIUS_LY};
use space_simulation::search::{Query, Scope};

const THEME: Theme = Theme::GAME;

#[derive(Resource, Default)]
pub struct Browser {
    pub open: bool,
    query: Query,
    /// The search box: a designation, or `ns search` text.
    text: String,
    /// Why the search box's text cannot be read.
    text_error: Option<String>,
    /// Why the query cannot be searched for, such as clashing filters.
    problem: Option<String>,
    /// The text of the query last sent, so any change sends it again.
    sent: Option<String>,
    picker: Option<picker::Picker>,
    /// Keeps outgoing chip visuals alive for every kind of query change.
    chip_list: kit::Pills<Chip>,
    inspector: crate::ui::inspector::Cache,
}

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>().add_systems(Update, follow);
    }
}

/// A `search` command from anywhere opens the browser on its query; the
/// browser's own come back here too, unchanged.
fn follow(mut commands: MessageReader<UiCommand>, mut browser: ResMut<Browser>) {
    for command in commands.read() {
        match command {
            UiCommand::Search { query } => {
                let Ok(query) = Query::parse(query) else {
                    continue;
                };
                let text = query.to_text();
                browser.text = query.text.clone();
                browser.query = query;
                browser.text_error = None;
                browser.problem = None;
                browser.sent = Some(text);
                browser.open = true;
            }
            UiCommand::ClearSearch => {
                browser.reset();
                // Nothing is searched for until the query changes.
                browser.sent = Some(browser.query.to_text());
            }
            _ => {}
        }
    }
}

/// What the window reads.
pub struct View<'a> {
    pub searches: &'a Searches,
    pub nav: &'a Navigation,
    pub universe: &'a Universe,
    pub field: &'a crate::stars::stream::Starfield,
    pub craft: crate::craft::Now<'a>,
}

pub fn window(ctx: &egui::Context, browser: &mut Browser, view: View, out: &mut Vec<UiCommand>) {
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
        browser.open = !browser.open;
    }
    let mut open = browser.open;
    kit::Window {
        title: "Object browser",
        eyebrow: "",
        width: (0.45, 560.0, 920.0),
        height: (0.78, 440.0, 900.0),
        min_size: egui::vec2(480.0, 380.0),
    }
    .show(ctx, &THEME, &mut open, |ui| browser.body(ui, &view, out));
    browser.open = open;
    if browser.open {
        picker::show(ctx, &THEME, &mut browser.picker, &mut browser.query);
        browser.send(out);
    }
}

impl Browser {
    /// Full reset, shared by the filter-row action and external clear commands.
    fn reset(&mut self) {
        self.query = Query::default();
        self.text.clear();
        self.text_error = None;
        self.problem = None;
    }

    /// Sends the query if it changed since it was last sent.
    fn send(&mut self, out: &mut Vec<UiCommand>) {
        let text = self.query.to_text();
        if self.sent.as_deref() == Some(&text) {
            return;
        }
        match self.query.check() {
            Ok(()) => {
                out.push(UiCommand::Search {
                    query: text.clone(),
                });
                self.problem = None;
            }
            Err(reason) => self.problem = Some(reason),
        }
        self.sent = Some(text);
    }

    /// Reads the search box: a designation, or whole `ns search` text, which
    /// replaces the filters.
    fn read_text(&mut self) {
        let text = self.text.trim();
        if text.is_empty() {
            self.query.text.clear();
            self.text_error = None;
            return;
        }
        match Query::parse(text) {
            Ok(parsed) => {
                let designation_only = Query {
                    text: parsed.text.clone(),
                    ..Query::default()
                } == parsed;
                if designation_only {
                    self.query.text = parsed.text;
                } else {
                    self.text = parsed.text.clone();
                    self.query = parsed;
                }
                self.text_error = None;
            }
            Err(reason) => self.text_error = Some(reason),
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, view: &View, out: &mut Vec<UiCommand>) {
        let theme = &THEME;
        let search = kit::field(
            ui,
            theme,
            "browser-search",
            "Designation or ID, or ns search text…",
            &mut self.text,
            f32::INFINITY,
        );
        if search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.read_text();
        }
        if let Some(error) = &self.text_error {
            kit::note(ui, theme, error);
        }
        self.scope(ui, view, out);
        self.chips(ui);
        ui.add_space(kit::SPACING);
        results::show(ui, self, view, out);
    }

    fn chips(&mut self, ui: &mut egui::Ui) {
        let theme = &THEME;
        ui.horizontal_wrapped(|ui| {
            self.type_choice(ui);
            let chips = filters::chips(&self.query)
                .into_iter()
                .filter(|(chip, _)| !matches!(chip, Chip::Kinds))
                .collect::<Vec<_>>();
            for (chip, pill) in self.chip_list.show(ui, theme, &chips) {
                if pill.remove {
                    filters::remove(&mut self.query, &chip);
                } else if pill.open {
                    self.picker = Some(match chip {
                        Chip::Filter { slot, property } => {
                            picker::Picker::edit(&self.query, slot, property)
                        }
                        Chip::Kinds | Chip::RelatedKinds(_) => picker::Picker::new(),
                    });
                }
            }
        });
    }

    fn can_reset(&self) -> bool {
        self.query != Query::default() || !self.text.is_empty()
    }

    fn type_choice(&mut self, ui: &mut egui::Ui) {
        use space_simulation::search::registry::{self, KINDS};
        let mut options = vec![("", "All types")];
        options.extend(
            KINDS
                .iter()
                .filter(|kind| kind.available())
                .map(|kind| (kind.id, kind.label)),
        );
        let (label, selected) = if let Some(family) = &self.query.family {
            (
                registry::family(family)
                    .map_or(family.as_str(), |family| family.label)
                    .to_owned(),
                None,
            )
        } else if self.query.kinds.is_empty() {
            ("All types".to_owned(), Some(""))
        } else {
            let label = self
                .query
                .kinds
                .iter()
                .map(|id| registry::kind_index(id).map_or(id.as_str(), |index| KINDS[index].label))
                .collect::<Vec<_>>()
                .join(" or ");
            (
                label,
                if self.query.kinds.len() == 1 {
                    Some(self.query.kinds[0].as_str())
                } else {
                    None
                },
            )
        };
        if let Some(kind) = kit::dropdown(
            ui,
            &THEME,
            "browser-type",
            &format!("Type: {label}"),
            selected,
            &options,
        ) {
            if kind.is_empty() {
                filters::remove(&mut self.query, &Chip::Kinds);
            } else {
                filters::set_kind(&mut self.query, &kind);
            }
        }
    }

    /// Radius or the visited system, and the limit.
    fn scope(&mut self, ui: &mut egui::Ui, view: &View, out: &mut Vec<UiCommand>) {
        let theme = &THEME;
        let here = view
            .nav
            .location(&view.universe.0)
            .map(|system| crate::navigation::objects::star_id(system, view.nav.page))
            .map(|id| space_simulation::star::multiple::system_key(&id).to_owned())
            .filter(|key| space_simulation::galaxy::cells::parse_cell_star(key).is_some());
        ui.horizontal_wrapped(|ui| {
            let mut radius = match self.query.scope {
                Scope::Within { radius_ly } => radius_ly,
                Scope::System { .. } => DEFAULT_RADIUS_LY,
            };
            if kit::number_field(
                ui,
                theme,
                "Within",
                &mut radius,
                f64::MIN_POSITIVE..=MAX_RADIUS_LY,
                "ly",
            )
            .changed()
            {
                self.query.scope = Scope::Within { radius_ly: radius };
            }
            kit::number_field(ui, theme, "Show", &mut self.query.limit, 1..=MAX_LIMIT, "");
            if let Some(key) = &here {
                let chosen = matches!(&self.query.scope, Scope::System { id } if id == key);
                if kit::choice(ui, theme, "This system", chosen) {
                    self.query.scope = Scope::System { id: key.clone() };
                }
            }
            if kit::primary_button(ui, theme, "+ Add filter", true).clicked() {
                self.picker = Some(picker::Picker::new());
            }
            let reset = self.can_reset();
            let size = kit::button_size(ui, theme, "Refresh");
            let required = size.x + if reset {
                kit::button_size(ui, theme, "Reset").x + kit::SPACING
            } else { 0.0 };
            let remaining = ui.available_size_before_wrap().x;
            let width = if remaining >= required { remaining } else { ui.max_rect().width() };
            ui.allocate_ui_with_layout(egui::vec2(width, size.y), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if reset && kit::destructive_button(ui, theme, "Reset")
                    .on_hover_text_themed(theme, "Clears search text, Type and all filters. Restores Within to 50 ly and Show to 50.")
                    .clicked() {
                    self.reset();
                }
                if kit::button(ui, theme, "Refresh")
                    .on_hover_text_themed(theme, "Distances are from where you searched. Refresh searches again from where you are now.")
                    .clicked()
                {
                    self.refresh(out);
                }
            });
        });
    }

    /// Sends the query again, from where the camera is now.
    fn refresh(&mut self, out: &mut Vec<UiCommand>) {
        if self.problem.is_some() {
            return;
        }
        let text = self.query.to_text();
        out.push(UiCommand::Search {
            query: text.clone(),
        });
        self.sent = Some(text);
    }
}

#[cfg(test)]
mod tests;
