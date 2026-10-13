//! A floating window: movable by its whole padded header, resizable, opening
//! at the top right below the HUD icons.
use super::Theme;
use bevy_egui::egui::{self, Color32, Rect, Sense, Ui, epaint, pos2, vec2};

/// Space between a window and the screen's edges when it first opens.
const GAP: f32 = 12.0;

pub struct Window<'a> {
    pub title: &'a str,
    /// Small text above a framed title; text-mode windows have a prompt.
    pub eyebrow: &'a str,
    /// Default width and height: a share of the screen, then clamped.
    pub width: (f32, f32, f32),
    pub height: (f32, f32, f32),
    pub min_size: egui::Vec2,
}

impl<'a> Window<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            eyebrow: "",
            width: (0.4, 420.0, 760.0),
            height: (0.6, 320.0, 680.0),
            min_size: vec2(320.0, 200.0),
        }
    }

    /// Draws the window while `open`; its close button clears `open`.
    pub fn show(
        self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        add: impl FnOnce(&mut Ui),
    ) {
        if !*open {
            return;
        }
        let screen = ctx.content_rect();
        let size = vec2(
            (screen.width() * self.width.0).clamp(self.width.1, self.width.2),
            (screen.height() * self.height.0).clamp(self.height.1, self.height.2),
        );
        let padding = f32::from(theme.padding);
        let mut close = false;
        let position_id = egui::Id::new(("window-position", self.title));
        let mut movement = egui::Vec2::ZERO;
        let mut window = egui::Window::new(self.title)
            .id(egui::Id::new(("window", self.title)))
            .title_bar(false)
            .frame(
                egui::Frame::new()
                    .fill(theme.fill)
                    .stroke((theme.border_width, theme.border))
                    .corner_radius(theme.radius)
                    .inner_margin(theme.padding)
                    .shadow(epaint::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    }),
            )
            // `size` is the content's; the frame's padding and border go
            // outside it.
            .default_rect(Rect::from_min_size(
                pos2(
                    screen.right() - GAP - size.x - 2.0 * (padding + theme.border_width),
                    below_icons(ctx, screen.top() + GAP),
                ),
                size,
            ))
            .min_size(self.min_size)
            .movable(false)
            .resizable(true)
            .order(egui::Order::Foreground);
        if let Some(position) = ctx.data(|data| data.get_temp::<egui::Pos2>(position_id)) {
            window = window.current_pos(position);
        }
        let shown = window.show(ctx, |ui| {
            ui.style_mut().spacing.item_spacing = if theme.text_mode {
                vec2(8.0, 4.0)
            } else {
                vec2(10.0, 8.0)
            };
            let mut close_rect = Rect::NOTHING;
            let header = ui.horizontal(|ui| {
                header(ui, theme, &self);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if theme.text_mode { "[ × ]" } else { "×" };
                    let response =
                        super::widgets::chip_response(ui, theme, label.into(), theme.text);
                    close = response.clicked();
                    close_rect = response.rect;
                });
            });
            ui.add_space(4.0);
            // The whole header, including frame padding down to the rule, is
            // a handle. Cut out only the close button, not its whole column.
            let handle = Rect::from_min_max(
                header.response.rect.min - vec2(padding, padding),
                pos2(ui.max_rect().right() + padding, ui.cursor().top()),
            );
            let content_clip = ui.clip_rect();
            ui.set_clip_rect(content_clip.union(handle));
            for (index, rect) in [
                Rect::from_min_max(handle.min, pos2(close_rect.left(), handle.bottom())),
                Rect::from_min_max(pos2(close_rect.right(), handle.top()), handle.max),
                Rect::from_min_max(
                    pos2(close_rect.left(), handle.top()),
                    close_rect.right_top(),
                ),
                Rect::from_min_max(
                    close_rect.left_bottom(),
                    pos2(close_rect.right(), handle.bottom()),
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let response = ui
                    .interact(rect, ui.id().with(("move-header", index)), Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::AllScroll);
                if response.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::AllScroll);
                    movement = response.drag_delta();
                }
            }
            ui.set_clip_rect(content_clip);
            super::widgets::rule(ui, theme);
            ui.add_space(4.0);
            add(ui);
        });
        if let Some(shown) = shown {
            ctx.data_mut(|data| data.insert_temp(position_id, shown.response.rect.min + movement));
        }
        if close {
            *open = false;
        }
    }
}

/// A prompt and the title in text mode; else the eyebrow over a large title.
fn header(ui: &mut Ui, theme: &Theme, window: &Window) {
    if theme.text_mode {
        ui.label(
            egui::RichText::new("night-sky:~$ ")
                .font(theme.font.id())
                .color(theme.dim),
        );
        ui.label(
            egui::RichText::new(window.title)
                .font(theme.font.id())
                .color(theme.accent),
        );
    } else {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if !window.eyebrow.is_empty() {
                super::widgets::eyebrow(ui, theme, window.eyebrow);
            }
            ui.label(
                egui::RichText::new(window.title)
                    .font(theme.title.id())
                    .color(theme.text),
            );
        });
    }
}

/// Clears the HUD icons at the top right, so a window never covers them.
fn below_icons(ctx: &egui::Context, top: f32) -> f32 {
    ctx.data(|data| data.get_temp::<(Rect, bool)>(egui::Id::new("icons-pointer-capture")))
        .map_or(top, |(icons, _)| top.max(icons.bottom() + GAP))
}
