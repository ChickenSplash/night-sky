//! Native scrolling with shared ease-out opacity for scrollbar feedback.
use super::interact::{ANIMATION_SECONDS, ease_out};
use super::{SCROLL_SOURCE, Theme};
use bevy_egui::egui::{self, Ui};

/// Native scrolling with kit feedback. Wheel and scrollbar input are enabled;
/// content dragging is off, so long presses stay with the controls inside.
pub struct ScrollArea {
    area: egui::ScrollArea,
    salt: egui::Id,
}

impl ScrollArea {
    pub fn vertical() -> Self {
        Self::new(egui::ScrollArea::vertical())
    }
    pub fn both() -> Self {
        Self::new(egui::ScrollArea::both())
    }
    fn new(area: egui::ScrollArea) -> Self {
        Self {
            area: area.scroll_source(SCROLL_SOURCE),
            salt: egui::Id::new("scroll_area"),
        }
    }
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.salt = egui::Id::new(&salt);
        self.area = self.area.id_salt(salt);
        self
    }
    pub fn scroll_source(mut self, source: egui::scroll_area::ScrollSource) -> Self {
        self.area = self.area.scroll_source(source);
        self
    }
    pub fn auto_shrink(mut self, shrink: impl Into<egui::Vec2b>) -> Self {
        self.area = self.area.auto_shrink(shrink);
        self
    }
    pub fn max_height(mut self, height: f32) -> Self {
        self.area = self.area.max_height(height);
        self
    }
    pub fn min_scrolled_height(mut self, height: f32) -> Self {
        self.area = self.area.min_scrolled_height(height);
        self
    }
    pub fn stick_to_bottom(mut self, stick: bool) -> Self {
        self.area = self.area.stick_to_bottom(stick);
        self
    }
    pub fn scroll_bar_visibility(
        mut self,
        visibility: egui::scroll_area::ScrollBarVisibility,
    ) -> Self {
        self.area = self.area.scroll_bar_visibility(visibility);
        self
    }
    pub fn show<R>(
        self,
        ui: &mut Ui,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        let original = ui.spacing().scroll;
        animate_scrollbar(ui, self.salt);
        let result = self.area.show(ui, add);
        ui.spacing_mut().scroll = original;
        result
    }
    pub fn show_rows<R>(
        self,
        ui: &mut Ui,
        height: f32,
        count: usize,
        add: impl FnOnce(&mut Ui, std::ops::Range<usize>) -> R,
    ) -> egui::scroll_area::ScrollAreaOutput<R> {
        let original = ui.spacing().scroll;
        animate_scrollbar(ui, self.salt);
        let result = self.area.show_rows(ui, height, count, add);
        ui.spacing_mut().scroll = original;
        result
    }
}

fn animate_scrollbar(ui: &mut Ui, salt: egui::Id) {
    let id = ui.make_persistent_id(salt);
    // Native scrollbar response IDs in the pinned egui 0.33 implementation.
    let active = [0_usize, 1].into_iter().any(|axis| {
        ui.ctx()
            .read_response(id.with(axis))
            .is_some_and(|response| {
                response.hovered() || response.dragged() || response.is_pointer_button_down_on()
            })
    });
    let amount = ui.ctx().animate_bool_with_time_and_easing(
        id.with("kit-scrollbar-highlight"),
        active,
        ANIMATION_SECONDS,
        ease_out,
    );
    let opacity = Theme::SUBTLE_ACCENT_OPACITY + (1.0 - Theme::SUBTLE_ACCENT_OPACITY) * amount;
    let scroll = &mut ui.spacing_mut().scroll;
    scroll.dormant_handle_opacity = opacity;
    scroll.active_handle_opacity = opacity;
    scroll.interact_handle_opacity = opacity;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::kit::{Theme, plain_style};

    #[test]
    fn scrollbar_opacity_eases_in_and_out_without_affecting_layout() {
        let ctx = egui::Context::default();
        plain_style(&ctx);
        ctx.style_mut(|style| style.animation_time = 0.0);
        let theme = Theme::GAME;
        let frame = |time, pointer| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 400.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ScrollArea::vertical()
                            .id_salt("animated-scrollbar-test")
                            .max_height(140.0)
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.allocate_space(egui::vec2(ui.available_width(), 1000.0));
                            });
                        assert_eq!(ui.spacing().scroll.dormant_handle_opacity, 0.12);
                    });
                },
            )
        };
        let handle = |output: egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if (rect.rect.width() - 10.0).abs() < 0.1 && rect.fill.a() > 0 =>
                    {
                        Some((rect.rect, rect.fill))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let outside = egui::pos2(0.0, 0.0);
        frame(0.0, outside);
        let idle = handle(frame(0.2, outside));
        assert_eq!(idle.1, theme.accent.gamma_multiply(0.12));
        let pointer = idle.0.center();
        frame(0.21, pointer);
        let appearing = handle(frame(0.3, pointer));
        let hovered = handle(frame(0.5, pointer));
        assert!(appearing.1.a() > idle.1.a() && appearing.1.a() < hovered.1.a());
        assert_eq!(hovered.1, theme.accent);
        assert_eq!(hovered.0, idle.0);
        frame(0.51, outside);
        let leaving = handle(frame(0.6, outside));
        assert!(leaving.1.a() > idle.1.a() && leaving.1.a() < hovered.1.a());
        assert_eq!(handle(frame(0.8, outside)).1, idle.1);
    }
}
