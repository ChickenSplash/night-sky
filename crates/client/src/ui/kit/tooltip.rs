//! Opaque, theme-matched tooltips, independent of translucent window fills.
use super::Theme;
use bevy_egui::egui;

pub trait TooltipExt {
    fn on_hover_text_themed(self, theme: &Theme, text: impl Into<egui::WidgetText>) -> Self;
}

impl TooltipExt for egui::Response {
    fn on_hover_text_themed(self, theme: &Theme, text: impl Into<egui::WidgetText>) -> Self {
        let text = text.into();
        let mut tooltip = egui::Tooltip::for_enabled(&self);
        tooltip.popup = tooltip.popup.frame(theme.tooltip_frame());
        tooltip.show(|ui| {
            ui.style_mut().override_font_id = Some(theme.small.id());
            ui.visuals_mut().override_text_color = Some(theme.text);
            ui.add(egui::Label::new(text).wrap());
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips_are_opaque_and_match_each_owning_window_theme() {
        for theme in [Theme::GAME, Theme::TERMINAL] {
            let ctx = egui::Context::default();
            super::super::plain_style(&ctx);
            ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
            let mut pointer = egui::Pos2::ZERO;
            let mut output = None;
            for frame in 0..6 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 400.0),
                        )),
                        time: Some(f64::from(frame) * 0.2),
                        events: vec![egui::Event::PointerMoved(pointer)],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let response = ui.button("Hover here");
                            pointer = response.rect.center();
                            response.on_hover_text_themed(&theme, "Themed tooltip help");
                        });
                    },
                ));
            }
            let output = output.unwrap();
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::epaint::Shape::Text(text) if text.galley.text() == "Themed tooltip help")));
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::epaint::Shape::Rect(rect) if rect.fill == theme.fill.to_opaque()
                    && rect.fill.a() == 255 && rect.stroke.color == theme.border
                    && rect.stroke.width == theme.border_width
                    && rect.corner_radius == egui::CornerRadius::same(theme.radius))));
        }
    }
}
