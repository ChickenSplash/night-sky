//! Removable chips with unified end edges, independent remove tint and layout motion.
use super::interact::{control_sense, held_click, hover_amount, input_edges};
use super::{SPACING, Theme, TooltipExt, animated_rect};
use bevy_egui::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

/// What happened to a removable chip.
#[derive(Default)]
pub struct Pill {
    /// Its text was clicked.
    pub open: bool,
    /// Its `×` was clicked. The collection retains the outgoing visual.
    pub remove: bool,
}

struct Entry<K> {
    key: K,
    label: String,
    serial: u64,
}

/// Animated chip collection. Reconcile the current values on every frame:
/// callers never remove visuals themselves, so every disappearance animates.
pub struct Pills<K> {
    entries: Vec<Entry<K>>,
    serial: u64,
}

impl<K> Default for Pills<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            serial: 0,
        }
    }
}

impl<K: Clone + PartialEq> Pills<K> {
    /// Draw in an owning wrapping row. Returns actions only for current chips;
    /// removed or relabelled entries remain clipped and disabled until folded.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, values: &[(K, String)]) -> Vec<(K, Pill)> {
        for (key, label) in values {
            if !self
                .entries
                .iter()
                .any(|entry| &entry.key == key && &entry.label == label)
            {
                self.entries.push(Entry {
                    key: key.clone(),
                    label: label.clone(),
                    serial: self.serial,
                });
                self.serial += 1;
            }
        }
        let mut actions = Vec::new();
        self.entries.retain(|entry| {
            let present = values
                .iter()
                .any(|(key, label)| key == &entry.key && label == &entry.label);
            let id = ui.id().with(("pill", entry.serial));
            let (response, finished) = pill(ui, theme, id, &entry.label, present);
            if present && (response.open || response.remove) {
                ui.ctx().request_repaint();
                actions.push((entry.key.clone(), response));
            }
            !finished
        });
        actions
    }
}

/// One retained visual, only callable by the animation-owning collection.
fn pill(ui: &mut Ui, theme: &Theme, id: egui::Id, label: &str, present: bool) -> (Pill, bool) {
    let hidden = Rect::from_min_size(egui::Pos2::ZERO, vec2(0.0, 1.0));
    let shown = Rect::from_min_size(egui::Pos2::ZERO, vec2(1.0, 1.0));
    let amount = animated_rect(
        ui,
        id.with("presence"),
        hidden,
        if present { shown } else { hidden },
    )
    .width();
    if amount <= 0.0 {
        return (Pill::default(), !present);
    }
    let font = theme.font.id();
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme.text);
    let cross = ui
        .painter()
        .layout_no_wrap("×".into(), font, Color32::PLACEHOLDER);
    let height = text.size().y.max(ui.spacing().interact_size.y) + theme.input_padding().sum().y;
    let width = text.size().x + cross.size().x + 4.0 * SPACING;
    let (visible, _) = ui.allocate_exact_size(vec2(width * amount, height), Sense::hover());
    if visible.width() <= 0.0 {
        return (Pill::default(), !present);
    }
    let rect = Rect::from_min_size(visible.min, vec2(width, height));
    let split = rect.right() - cross.size().x - 2.0 * SPACING;
    let body = Rect::from_min_max(rect.min, pos2(split, rect.bottom()));
    let close = Rect::from_min_max(pos2(split, rect.top()), rect.max);
    let open = held_click(ui.interact(
        body.intersect(visible),
        id.with("open"),
        control_sense(present),
    ));
    let open = if present {
        open.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        open
    };
    let remove = held_click(ui.interact(
        close.intersect(visible),
        id.with("remove"),
        control_sense(present),
    ));
    let remove = if present {
        remove
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text_themed(theme, "Remove")
    } else {
        remove
    };
    let clip = ui.clip_rect();
    ui.set_clip_rect(clip.intersect(visible));
    let painter = ui.painter();
    painter.rect_filled(rect, 0, theme.input_fill());
    input_edges(
        ui,
        theme,
        visible,
        id,
        present
            && (open.hovered()
                || open.is_pointer_button_down_on()
                || open.has_focus()
                || remove.hovered()
                || remove.is_pointer_button_down_on()
                || remove.has_focus()),
    );
    let removal_hover = hover_amount(ui, &remove, present);
    painter.galley(
        pos2(rect.left() + SPACING, rect.center().y - text.size().y / 2.0),
        text,
        theme.text,
    );
    painter.galley(
        close.center() - cross.size() / 2.0,
        cross,
        theme.text.lerp_to_gamma(Theme::DESTRUCTIVE, removal_hover),
    );
    ui.set_clip_rect(clip);
    (
        Pill {
            open: present && open.clicked(),
            remove: present && remove.clicked(),
        },
        false,
    )
}

#[cfg(test)]
mod tests;
