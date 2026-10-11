//! The egui interface over the 3D view: the HUD icons and their windows, the
//! draggable HUD text, the breadcrumb inspector and the canvas overlay.
pub mod breadcrumb;
mod canvas;
mod hud;
pub mod inspector;
mod settings;
pub mod tasks;
pub mod terminal;

use crate::actions::UiCommand;
use crate::{
    Clock, Navigation, Universe,
    scene::{CameraPose, CameraRig, CameraVelocity, Markers},
};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use canvas::canvas_overlay;
pub(crate) use hud::pointer_captured;
use hud::{exposure_controls, magnitude_control, speed_readout, time_controls};
use settings::settings;

pub(crate) const MUTED: egui::Color32 = egui::Color32::from_rgb(118, 144, 174);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 214, 240);
const PANEL: egui::Color32 = egui::Color32::from_rgb(7, 15, 31);
/// The HUD icons' glyph size, larger than the HUD text so they read as icons.
const ICON_SIZE: f32 = 20.0;

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(MUTED).strong());
}

pub(crate) fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).size(11.0).color(MUTED));
}

type Overlay<'w> = (Res<'w, Markers>, Res<'w, CameraVelocity>);

/// Interface state that is not part of the scene.
#[derive(Default)]
pub struct Hud {
    styled: bool,
    settings: bool,
    tasks_open: bool,
    tasks: crate::ui::tasks::Tasks,
    time_drag_start: Option<f64>,
    speed_drag_start: Option<f64>,
    /// The limit when a magnitude drag began, and where it is now.
    magnitude_drag: Option<(f64, f64)>,
    exposure_drags: [Option<f64>; 2],
    inspector: crate::ui::inspector::Cache,
}

pub fn draw_ui(
    interaction: (EguiContexts, MessageWriter<UiCommand>),
    universe: Res<Universe>,
    mut nav: ResMut<Navigation>,
    clock: Res<Clock>,
    overlay: Overlay,
    camera_state: (
        Res<CameraRig>,
        Res<CameraPose>,
        Res<crate::stars::stream::Starfield>,
    ),
    mut hud: Local<Hud>,
) {
    let (mut contexts, mut writer) = interaction;
    let (markers, velocity) = overlay;
    let (rig, pose, starfield) = camera_state;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !hud.styled {
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(214, 228, 244));
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(24, 65, 85);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(15, 31, 49);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        ctx.set_style(style);
        hud.styled = true;
    }

    let mut out = Vec::new();
    let mut rect = ctx.available_rect();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        rect = egui::Rect::from_min_size(rect.min, egui::vec2(1.0, 1.0));
    }
    let hud = &mut *hud;
    let anchor = icons(ctx, rect, &mut hud.settings, &mut hud.tasks_open);
    let mut open = hud.settings;
    egui::Window::new("Settings")
        .open(&mut open)
        .pivot(egui::Align2::RIGHT_TOP)
        .default_pos(anchor)
        .default_width(340.0)
        .default_height(640.0)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| settings(ui, &nav, &mut out));
        });
    hud.settings = open;
    crate::ui::tasks::window(
        ctx,
        anchor,
        &mut hud.tasks_open,
        &universe.0,
        &mut nav,
        &mut hud.tasks,
        &mut out,
    );

    let pixels = ctx.pixels_per_point();
    nav.viewport = Some(bevy::camera::Viewport {
        physical_position: UVec2::new(
            (rect.min.x * pixels).max(0.0) as u32,
            (rect.min.y * pixels).max(0.0) as u32,
        ),
        physical_size: UVec2::new(
            (rect.width() * pixels).max(1.0) as u32,
            (rect.height() * pixels).max(1.0) as u32,
        ),
        ..default()
    });
    time_controls(ctx, rect, &clock.0, &mut hud.time_drag_start, &mut out);
    speed_readout(
        ctx,
        rect,
        nav.move_speed,
        velocity.metres_per_second.length(),
        &mut hud.speed_drag_start,
        &mut out,
    );
    magnitude_control(ctx, rect, nav.magnitude, &mut hud.magnitude_drag, &mut out);
    exposure_controls(ctx, rect, &nav, &mut hud.exposure_drags, &mut out);
    canvas_overlay(
        ctx,
        rect,
        &universe.0,
        &nav,
        &markers,
        &rig,
        &pose,
        &starfield,
        &velocity,
        &mut out,
    );
    if universe.is_changed() {
        hud.inspector.invalidate();
    }
    crate::ui::breadcrumb::show(
        ctx,
        rect,
        &nav,
        &universe.0,
        &starfield,
        &mut hud.inspector,
        &mut out,
    );
    writer.write_batch(out);
}

/// ⚙ Settings above ✔ Tasks, free-floating at the top right of the view.
/// Each toggles its window. Returns where those windows first open: just
/// below the icons, aligned to their right edge.
fn icons(
    ctx: &egui::Context,
    rect: egui::Rect,
    settings: &mut bool,
    tasks: &mut bool,
) -> egui::Pos2 {
    use crate::ui::breadcrumb::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, halo_glyph};
    let id = egui::Id::new("hud-icons");
    let area = egui::Area::new(id)
        .order(egui::Order::Background)
        .constrain(false)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(rect.right_top() + egui::vec2(-HUD_PADDING, HUD_PADDING))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                for (glyph, hint, open) in [("⚙", "Settings", settings), ("✔", "Tasks", tasks)]
                {
                    let response =
                        halo_glyph(ui, glyph, ICON_SIZE, egui::Sense::click(), |response| {
                            if response.hovered() {
                                HOVER
                            } else if *open {
                                CURRENT
                            } else {
                                LINK
                            }
                        })
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(hint);
                    if response.clicked() {
                        *open = !*open;
                    }
                }
            });
        });
    let bounds = area.response.rect;
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("icons-pointer-capture"), (bounds, false)));
    egui::pos2(bounds.right(), bounds.bottom() + HUD_LINE_GAP)
}

#[cfg(test)]
mod tests;
