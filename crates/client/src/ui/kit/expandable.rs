//! Inline dropdown rows, sharing the menu's arrow and reveal easing.
use super::interact::{ANIMATION_SECONDS, ease_out};
use super::{SPACING, Theme, item};
use bevy_egui::egui::{self, Align2, Rect, pos2, vec2};

pub fn expandable<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    detail: &str,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id("expansion");
    let response = item(ui, theme, label, "", *open);
    let row = response.rect;
    if response.clicked() {
        *open = !*open;
    }
    let amount = ui.ctx().animate_bool_with_time_and_easing(
        id.with("opening"),
        *open,
        ANIMATION_SECONDS,
        ease_out,
    );
    ui.painter().text(
        pos2(row.right() - 3.0 * SPACING, row.center().y),
        Align2::RIGHT_CENTER,
        detail,
        theme.small.id(),
        theme.dim,
    );
    super::dropdown::arrow(
        ui,
        theme,
        pos2(row.right() - SPACING - 3.0, row.center().y),
        amount,
    );
    if !*open && amount <= 0.0 {
        return None;
    }
    let height_id = id.with("height");
    let height = ui.data_mut(|data| data.get_temp::<f32>(height_id).unwrap_or(0.0));
    let start = ui.next_widget_position();
    let visible = Rect::from_min_size(start, vec2(ui.available_width(), height * amount));
    let content = Rect::from_min_size(
        start - vec2(0.0, SPACING * (1.0 - amount)),
        vec2(ui.available_width(), height.max(1.0)),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id.with("body"))
            .max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(visible));
    if !*open || amount < 1.0 {
        child.disable();
    }
    child.spacing_mut().item_spacing = vec2(SPACING, SPACING);
    child.add_space(SPACING);
    let result = body(&mut child);
    child.add_space(SPACING);
    let full_height = child.min_rect().height();
    ui.data_mut(|data| data.insert_temp(height_id, full_height));
    ui.allocate_exact_size(
        vec2(visible.width(), full_height * amount),
        egui::Sense::hover(),
    );
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_rows_keep_the_selection_highlight_until_folded() {
        let ctx = egui::Context::default();
        let theme = Theme::GAME;
        let mut open = false;
        let frame = |time, open: &mut bool| {
            ctx.run(
                egui::RawInput {
                    time: Some(time),
                    events: vec![egui::Event::PointerMoved(pos2(600.0, 400.0))],
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(700.0, 500.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_width(400.0);
                        expandable(ui, &theme, "Property", "range", open, |ui| {
                            ui.label("Inline values");
                        });
                    });
                },
            )
        };
        let selected = |output: egui::FullOutput| {
            output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Rect(rect) if rect.fill == theme.accent.gamma_multiply(Theme::SUBTLE_ACCENT_OPACITY)))
        };
        assert!(!selected(frame(0.0, &mut open)));
        open = true;
        frame(0.01, &mut open);
        assert!(selected(frame(0.3, &mut open)));
        assert!(selected(frame(1.0, &mut open)));
        open = false;
        frame(1.01, &mut open);
        assert!(!selected(frame(1.3, &mut open)));
    }

    #[test]
    fn inline_rows_ease_both_ways_rotate_and_disable_exiting_content() {
        let ctx = egui::Context::default();
        let theme = Theme::GAME;
        let mut open = false;
        let mut frame = |time, events: Vec<egui::Event>| {
            let mut after = 0.0;
            let mut enabled = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(500.0, 500.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.push_id("test", |ui| {
                            expandable(ui, &theme, "Property", "range", &mut open, |ui| {
                                enabled = Some(ui.is_enabled());
                                ui.allocate_exact_size(vec2(100.0, 80.0), egui::Sense::hover());
                            });
                        });
                        after = ui.next_widget_position().y;
                    });
                },
            );
            let arrows = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::LineSegment { points, stroke }
                        if stroke.color == theme.dim && stroke.width == 1.0 =>
                    {
                        Some(*points)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            (after, enabled, arrows)
        };
        let initial = frame(0.0, vec![]);
        let pos = pos2(100.0, 20.0);
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(0.01, vec![egui::Event::PointerMoved(pos), press(true)]);
        frame(0.02, vec![press(false)]);
        let opening = frame(0.11, vec![]);
        let full = frame(0.3, vec![]);
        assert!(opening.0 > initial.0 && opening.0 < full.0);
        assert_eq!(full.1, Some(true));
        assert!(initial.2[0][0].y < initial.2[0][1].y);
        assert!(full.2[0][0].y > full.2[0][1].y);
        // A long press must still close on release.
        frame(0.31, vec![press(true)]);
        frame(0.8, vec![]);
        frame(0.81, vec![press(false)]);
        let closing = frame(0.9, vec![]);
        assert!(closing.0 > initial.0 && closing.0 < full.0);
        assert_eq!(closing.1, Some(false));
        let closed = frame(1.1, vec![]);
        assert_eq!(closed.0, initial.0);
        assert_eq!(closed.1, None);
        assert_eq!(closed.2, initial.2);
    }
}
