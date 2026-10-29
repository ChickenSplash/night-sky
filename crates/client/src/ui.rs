//! The egui interface over the 3D view: the HUD icons and their windows, the
//! draggable HUD text, the breadcrumb inspector and the canvas overlay.
pub mod breadcrumb;
pub mod browser;
mod canvas;
pub(crate) mod hud;
pub mod inspector;
pub mod kit;
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

/// The HUD icons' glyph size, larger than the HUD text so they read as icons.
const ICON_SIZE: f32 = 20.0;

type Overlay<'w> = (Res<'w, Markers>, Res<'w, CameraVelocity>);
type CameraState<'w> = (
    Res<'w, CameraRig>,
    Res<'w, CameraPose>,
    Res<'w, crate::stars::stream::Starfield>,
    Res<'w, crate::craft::Fleet>,
    Res<'w, crate::input::pilot::Pilot>,
    Res<'w, crate::input::Bindings>,
    Res<'w, crate::input::Actions>,
);

/// Interface state that is not part of the scene.
#[derive(Default)]
pub struct Hud {
    styled: bool,
    tasks_open: bool,
    tasks: crate::ui::tasks::Tasks,
    time_drag_start: Option<f64>,
    speed_drag_start: Option<f64>,
    /// The limit when a magnitude drag began, and where it is now.
    magnitude_drag: Option<(f64, f64)>,
    exposure_drags: [Option<f64>; 2],
    inspector: crate::ui::inspector::Cache,
    object_inspector: inspector::window::Inspector,
}

pub fn draw_ui(
    interaction: (
        EguiContexts,
        MessageWriter<UiCommand>,
        ResMut<crate::devtools::DevTools>,
        ResMut<browser::Browser>,
        Res<crate::search::Searches>,
    ),
    (universe, loading): (Res<Universe>, Res<crate::planet_loading::PlanetLoading>),
    mut nav: ResMut<Navigation>,
    clock: Res<Clock>,
    overlay: Overlay,
    camera_state: CameraState,
    mut hud: Local<Hud>,
) {
    let (mut contexts, mut writer, mut tools, mut browser, searches) = interaction;
    let (markers, velocity) = overlay;
    let (rig, pose, starfield, fleet, pilot, bindings, actions) = camera_state;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let now = fleet.at(clock.0.seconds);
    if !hud.styled {
        kit::plain_style(ctx);
        hud.styled = true;
    }

    let mut out = Vec::new();
    let mut rect = ctx.available_rect();
    if rect.width() < 1.0 || rect.height() < 1.0 {
        rect = egui::Rect::from_min_size(rect.min, egui::vec2(1.0, 1.0));
    }
    let hud = &mut *hud;
    if actions.just_pressed(crate::input::Action::Inspect) {
        hud.object_inspector.open = !hud.object_inspector.open;
    }
    icons(
        ctx,
        rect,
        [
            ("⚙", "Debug console", &mut tools.open),
            ("✔", "Tasks", &mut hud.tasks_open),
            ("🔍", "Object browser (Ctrl+K)", &mut browser.open),
            ("ⓘ", "Object Inspector (I)", &mut hud.object_inspector.open),
        ],
    );
    browser::window(
        ctx,
        &mut browser,
        browser::View {
            searches: &searches,
            nav: &nav,
            universe: &universe,
            field: &starfield,
            craft: now,
        },
        &mut out,
    );
    crate::ui::tasks::window(
        ctx,
        &mut hud.tasks_open,
        &universe.0,
        &nav,
        &mut hud.tasks,
        &mut out,
    );
    if universe.is_changed() {
        hud.object_inspector.invalidate();
    }
    inspector::window::show(
        ctx,
        &mut hud.object_inspector,
        inspector::window::View {
            nav: &nav,
            galaxy: &universe.0,
            field: &starfield,
            now,
            loading: &loading,
        },
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
    time_controls(
        ctx,
        rect,
        &clock.0,
        fleet.held || fleet.safe_rate(clock.0.rate) < clock.0.rate,
        &mut hud.time_drag_start,
        &mut out,
    );
    if let Some(live) = pilot.ship.as_deref().and_then(|id| fleet.get(id)) {
        hud::pilot_readout(ctx, rect, &crate::input::pilot::readout(live, &bindings));
    }
    if let Some(stick) = &pilot.stick {
        hud::stick_reticle(ctx, stick);
    }
    let moving_with = match &nav.anchor {
        // The location's deepest crumb, named as the breadcrumb names it.
        Some(crate::Anchor::Body(_)) => crate::ui::breadcrumb::trail(&nav, &universe.0)
            .pop()
            .map(|crumb| crumb.label),
        Some(crate::Anchor::Craft(id)) => fleet.get(id).map(|live| live.craft.name.clone()),
        None => None,
    };
    speed_readout(
        ctx,
        rect,
        moving_with.as_deref(),
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
        (now, &loading),
        &mut out,
    );
    writer.write_batch(out);
}

/// ⚙ Debug console, ✔ Tasks, 🔍 Object browser and ⓘ Object Inspector, top to bottom,
/// free-floating at the top right of the view. Each toggles its window.
fn icons<const N: usize>(
    ctx: &egui::Context,
    rect: egui::Rect,
    icons: [(&str, &str, &mut bool); N],
) {
    use kit::TooltipExt;
    use kit::halo::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, halo_glyph};
    let id = egui::Id::new("hud-icons");
    let area = egui::Area::new(id)
        .order(egui::Order::Background)
        .constrain(false)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(rect.right_top() + egui::vec2(-HUD_PADDING, HUD_PADDING))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                for (glyph, hint, open) in icons {
                    let colour = |response: &egui::Response| {
                        if response.hovered() {
                            HOVER
                        } else if *open {
                            CURRENT
                        } else {
                            LINK
                        }
                    };
                    let response = if glyph == "ⓘ" {
                        kit::halo::inspector_icon(ui, ICON_SIZE, colour)
                    } else {
                        halo_glyph(ui, glyph, ICON_SIZE, egui::Sense::click(), colour)
                    }
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text_themed(&kit::Theme::GAME, hint);
                    if response.clicked() {
                        *open = !*open;
                    }
                }
            });
        });
    let bounds = area.response.rect;
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("icons-pointer-capture"), (bounds, false)));
}

#[cfg(test)]
mod tests;
