//! Removable filter chips with independent edit/remove feedback and layout motion.
use super::interact::{ANIMATION_SECONDS, control_sense, ease_out, held_click, input_underline};
use super::{SPACING, Theme, TooltipExt};
use bevy_egui::egui::{self, Color32, Rect, Sense, Ui, pos2, vec2};

/// What happened to a removable chip.
#[derive(Default)]
pub struct Pill {
    /// Its text was clicked.
    pub open: bool,
    /// Its `×` was clicked and its exit animation has finished.
    pub remove: bool,
}

/// A square filter chip with separate edit and remove actions.
pub fn pill(ui: &mut Ui, theme: &Theme, label: &str) -> Pill {
    let id = ui.id().with(("pill", label));
    let removal_id = id.with("removal");
    let appearance_id = id.with("appearance");
    let now = ui.input(|input| input.time);
    let frame = ui.ctx().cumulative_frame_nr();
    let appeared = ui.data_mut(|data| {
        let (start, previous) = data
            .get_temp::<(f64, u64)>(appearance_id)
            .unwrap_or((now, frame));
        let start = if frame > previous + 1 { now } else { start };
        data.insert_temp(appearance_id, (start, frame));
        ease_out(((now - start) / f64::from(ANIMATION_SECONDS)).clamp(0.0, 1.0) as f32)
    });
    if appeared < 1.0 {
        ui.ctx().request_repaint();
    }
    let removal = ui.data_mut(|data| {
        let removal = data.get_temp::<(f64, u64)>(removal_id);
        // Replacing a query must not leave stale removal state when re-added.
        if removal.is_some_and(|(_, previous)| frame > previous + 1) {
            data.remove::<(f64, u64)>(removal_id);
            None
        } else {
            removal
        }
    });
    let remaining = removal.map_or(1.0, |(start, _)| {
        1.0 - ease_out(((now - start) / f64::from(ANIMATION_SECONDS)).clamp(0.0, 1.0) as f32)
    });
    if removal.is_some() && remaining <= 0.0 {
        ui.data_mut(|data| {
            data.remove::<(f64, u64)>(removal_id);
            data.remove::<(f64, u64)>(appearance_id);
        });
        return Pill {
            open: false,
            remove: true,
        };
    }
    if let Some((start, _)) = removal {
        ui.data_mut(|data| data.insert_temp(removal_id, (start, frame)));
        ui.ctx().request_repaint();
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
    let (visible, _) =
        ui.allocate_exact_size(vec2(width * appeared * remaining, height), Sense::hover());
    if visible.width() <= 0.0 {
        return Pill::default();
    }
    let rect = Rect::from_min_size(visible.min, vec2(width, height));
    let split = rect.right() - cross.size().x - 2.0 * SPACING;
    let body = Rect::from_min_max(rect.min, pos2(split, rect.bottom()));
    let close = Rect::from_min_max(pos2(split, rect.top()), rect.max);
    let open = held_click(ui.interact(
        body.intersect(visible),
        id.with("open"),
        control_sense(removal.is_none()),
    ))
    .on_hover_cursor(egui::CursorIcon::PointingHand);
    let remove = held_click(ui.interact(
        close.intersect(visible),
        id.with("remove"),
        control_sense(removal.is_none()),
    ))
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text_themed(theme, "Remove");
    let clip = ui.clip_rect();
    ui.set_clip_rect(clip.intersect(visible));
    let painter = ui.painter();
    painter.rect_filled(rect, 0, theme.input_fill());
    input_underline(
        ui,
        theme,
        body,
        open.id,
        open.hovered() || open.is_pointer_button_down_on() || open.has_focus(),
    );
    let destructive = Theme {
        accent: Theme::DESTRUCTIVE,
        ..*theme
    };
    let removal_hover = input_underline(
        ui,
        &destructive,
        close,
        remove.id,
        remove.hovered() || remove.is_pointer_button_down_on() || remove.has_focus(),
    );
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
    if remove.clicked() {
        ui.data_mut(|data| data.insert_temp(removal_id, (now, frame)));
        ui.ctx().request_repaint();
    }
    Pill {
        open: open.clicked(),
        remove: false,
    }
}
