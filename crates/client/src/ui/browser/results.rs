//! The shown request's progress and results, and the selected result's
//! facts and actions (the inspector's, so they act as they do there).
use super::{Browser, THEME, View};
use crate::actions::UiCommand;
use crate::search::Entry;
use crate::ui::kit;
use bevy_egui::egui::{self, Align2, pos2, vec2};
use space_simulation::search::registry::{self, KINDS};
use space_simulation::search::{Scope, SearchResult, State};

const ROW: f32 = 46.0;
/// The detail pane's width beside the results.
const DETAIL: f32 = 300.0;

pub(super) fn show(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    view: &View,
    out: &mut Vec<UiCommand>,
) {
    let theme = &THEME;
    // Only the request for the query as it stands, not an earlier one.
    let entry =
        view.searches.shown_entry.as_ref().filter(|e| {
            browser.sent.as_deref() == Some(e.query.as_str()) && browser.problem.is_none()
        });
    status(ui, browser, view, entry, out);
    let results: &[SearchResult] = entry.and_then(|e| e.results.as_deref()).unwrap_or(&[]);
    let selected = browser
        .selected
        .as_ref()
        .and_then(|object| results.iter().find(|r| r.object == *object));
    // Leave room for the footer.
    let height = (ui.available_height() - 48.0).max(120.0);
    if let Some(result) = selected {
        let wide = ui.available_width() >= 720.0;
        if wide {
            ui.horizontal_top(|ui| {
                // Exactly the room left by the divider and the pane, or the
                // window would widen to fit every frame.
                let spacing = ui.spacing().item_spacing.x;
                let left = (ui.available_width() - DETAIL - 1.0 - 2.0 * spacing).max(160.0);
                ui.allocate_ui_with_layout(
                    vec2(left, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| list(ui, browser, results, height, out, view),
                );
                let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
                ui.painter()
                    .vline(rect.center().x, rect.y_range(), (1.0, theme.line()));
                ui.allocate_ui_with_layout(
                    vec2(DETAIL, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| detail(ui, browser, result, view, out),
                );
            });
        } else {
            list(ui, browser, results, height * 0.5, out, view);
            kit::rule(ui, theme);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height * 0.5),
                egui::Layout::top_down(egui::Align::Min),
                |ui| detail(ui, browser, result, view, out),
            );
        }
    } else {
        list(ui, browser, results, height, out, view);
    }
}

fn radius_text(browser: &Browser) -> String {
    match &browser.query.scope {
        Scope::Within { radius_ly } => {
            format!("within {} ly", super::filters::number_text(*radius_ly))
        }
        Scope::System { .. } => "in this system".into(),
    }
}

/// One line on how the search is going, and a bar while it runs.
fn status(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    view: &View,
    entry: Option<&Entry>,
    out: &mut Vec<UiCommand>,
) {
    let theme = &THEME;
    let count = |n: usize| match n {
        1 => "1 result".to_owned(),
        n => format!("{} results", crate::format::grouped(n)),
    };
    let (text, bar) = if !view.searches.available() {
        (
            "Searching runs in the desktop app for now; the browser build gets its own search worker next.".to_owned(),
            None,
        )
    } else if let Some(problem) = &browser.problem {
        (format!("Can't search for this: {problem}"), None)
    } else if let Some(entry) = entry {
        let p = &entry.progress;
        let n = entry.result_count;
        let scope = radius_text(browser);
        match (&entry.error, p.state) {
            (Some(error), _) => (format!("Not found: {error}"), None),
            (None, State::Running) => {
                let radius = match browser.query.scope {
                    Scope::Within { radius_ly } => radius_ly,
                    Scope::System { .. } => 0.0,
                };
                let share = (radius > 0.0).then(|| (p.complete_to_ly / radius) as f32);
                (
                    format!(
                        "Searching {scope} · out to {:.1} ly · {} so far",
                        p.complete_to_ly,
                        count(n)
                    ),
                    Some(share),
                )
            }
            (None, State::Done) if n == 0 => (format!("Nothing matches {scope}"), None),
            (None, State::Done) if n as u32 == browser.query.limit => {
                (format!("The nearest {} {scope}", count(n)), None)
            }
            (None, State::Done) => (format!("{} {scope}", count(n)), None),
            (None, State::Capped) => (
                format!(
                    "{} · stopped at its work limit, complete out to {:.1} ly",
                    count(n),
                    p.complete_to_ly
                ),
                None,
            ),
            (None, State::Cancelled) => ("Stopped".into(), None),
        }
    } else if browser.sent.is_some() {
        ("Searching…".into(), Some(None))
    } else {
        ("".into(), None)
    };
    ui.horizontal(|ui| {
        // Room for the button at the right.
        let width = (ui.available_width() - 100.0).max(0.0);
        ui.allocate_ui_with_layout(
            vec2(width, theme.row),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .font(theme.small.id())
                            .color(theme.dim),
                    )
                    .truncate(),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let refresh = kit::button(ui, theme, "Refresh").on_hover_text(
                "Distances are from where you searched. Refresh searches again from where you are now.",
            );
            if refresh.clicked() && view.searches.available() {
                browser.refresh(out);
            }
        });
    });
    match bar {
        Some(done) => kit::progress(ui, theme, done),
        None => {
            ui.add_space(2.0);
        }
    }
}

fn distance_text(ly: f64) -> String {
    match ly {
        ly if ly < 10.0 => format!("{ly:.2} ly"),
        ly if ly < 100.0 => format!("{ly:.1} ly"),
        ly => format!("{} ly", crate::format::grouped(ly.round() as usize)),
    }
}

fn kind_label(id: &str) -> &str {
    registry::kind_index(id).map_or(id, |i| KINDS[i].label)
}

/// One line of text, cut short with `…` to `width`.
fn fitted(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    ui.painter().layout_job(job)
}

fn list(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    results: &[SearchResult],
    height: f32,
    out: &mut Vec<UiCommand>,
    view: &View,
) {
    let theme = &THEME;
    let width = ui.available_width();
    let class_x = (width * 0.55).max(200.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 20.0), egui::Sense::hover());
    let head = egui::FontId::monospace(9.0);
    for (x, align, text) in [
        (
            rect.left() + theme.inset,
            Align2::LEFT_CENTER,
            "OBJECT · SYSTEM",
        ),
        (rect.left() + class_x, Align2::LEFT_CENTER, "CLASS"),
        (rect.right() - theme.inset, Align2::RIGHT_CENTER, "DISTANCE"),
    ] {
        ui.painter().text(
            pos2(x, rect.center().y),
            align,
            text,
            head.clone(),
            theme.dim,
        );
    }
    egui::ScrollArea::vertical()
        .id_salt("browser-results")
        .scroll_source(kit::SCROLL_SOURCE)
        .auto_shrink([false, false])
        .max_height(height - 24.0)
        .show_rows(ui, ROW, results.len(), |ui, rows| {
            for result in &results[rows] {
                let selected = browser.selected.as_ref() == Some(&result.object);
                let (rect, response, hover) = kit::list_row(ui, theme, ROW, selected);
                let name_width = class_x - theme.inset * 2.0;
                let top = rect.top() + 7.0;
                let colour = theme.text.lerp_to_gamma(theme.accent, hover * 0.5);
                let painter = ui.painter();
                painter.galley(
                    pos2(rect.left() + theme.inset + 4.0, top),
                    fitted(ui, &result.name, theme.font.id(), colour, name_width),
                    colour,
                );
                let secondary = format!(
                    "{} · {}",
                    result.system,
                    kind_label(&result.kind).to_lowercase()
                );
                painter.galley(
                    pos2(rect.left() + theme.inset + 4.0, top + 19.0),
                    fitted(ui, &secondary, theme.small.id(), theme.dim, name_width),
                    theme.dim,
                );
                let distance = distance_text(result.distance_ly);
                let distance_width = 80.0;
                painter.galley(
                    pos2(rect.left() + class_x, top + 7.0),
                    fitted(
                        ui,
                        &result.class,
                        theme.small.id(),
                        theme.text,
                        rect.width() - class_x - distance_width - theme.inset,
                    ),
                    theme.text,
                );
                painter.text(
                    pos2(rect.right() - theme.inset, rect.center().y),
                    Align2::RIGHT_CENTER,
                    distance,
                    theme.mono.id(),
                    theme.text,
                );
                if response.clicked() {
                    browser.selected = Some(result.object.clone());
                }
                if response.double_clicked()
                    && let Some(action) = crate::ui::inspector::actions(
                        &result.object,
                        view.nav,
                        &view.universe.0,
                        view.field,
                    )
                    .into_iter()
                    .next()
                {
                    out.push(action.command);
                }
            }
        });
}

/// The selected result's name, actions and facts.
fn detail(
    ui: &mut egui::Ui,
    browser: &mut Browser,
    result: &SearchResult,
    view: &View,
    out: &mut Vec<UiCommand>,
) {
    let theme = &THEME;
    let galaxy = &view.universe.0;
    let info = browser.inspector.get(&result.object, galaxy);
    kit::eyebrow(ui, theme, "Selected");
    let title = if info.title.is_empty() {
        &result.name
    } else {
        &info.title
    };
    ui.label(
        egui::RichText::new(title)
            .font(egui::FontId::proportional(18.0))
            .color(theme.text),
    );
    ui.label(
        egui::RichText::new(format!(
            "{} · {}",
            result.class,
            distance_text(result.distance_ly)
        ))
        .font(theme.small.id())
        .color(theme.dim),
    );
    let actions = crate::ui::inspector::actions(&result.object, view.nav, galaxy, view.field);
    ui.horizontal_wrapped(|ui| {
        for (index, action) in actions.iter().enumerate() {
            let response = if index == 0 {
                kit::primary_button(ui, theme, &action.label, true)
            } else {
                kit::button(ui, theme, &action.label)
            };
            if response.clicked() {
                out.push(action.command.clone());
            }
        }
        let id = match &result.object {
            space_simulation::objects::ObjectRef::Star { id }
            | space_simulation::objects::ObjectRef::Barycentre { id }
            | space_simulation::objects::ObjectRef::Planet { id }
            | space_simulation::objects::ObjectRef::List { id } => id.clone(),
            space_simulation::objects::ObjectRef::Galaxy => String::new(),
        };
        if kit::link(ui, theme, "Copy ID").on_hover_text(&id).clicked() {
            ui.ctx().copy_text(id);
        }
    });
    let info = browser.inspector.get(&result.object, galaxy);
    let rows: Vec<(&str, String)> = info
        .rows
        .iter()
        .map(|(label, value)| (label.as_str(), value.clone()))
        .collect();
    let note = info.note.clone();
    egui::ScrollArea::vertical()
        .id_salt("browser-detail")
        .scroll_source(kit::SCROLL_SOURCE)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            kit::facts(ui, theme, "browser-facts", &rows);
            if !note.is_empty() {
                kit::note(ui, theme, &note);
            }
        });
}
