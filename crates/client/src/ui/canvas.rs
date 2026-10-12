//! Captions, planet labels, hover rings, the velocity marker and clicks on
//! the 3D view itself.
use super::{ACCENT, pointer_captured};
use crate::actions::UiCommand;
use crate::presentation::PresentationMode;
use crate::{
    Navigation,
    scene::{CameraPose, CameraRig, CameraVelocity, Markers},
};
use bevy::{math::DVec3, prelude::*};
use bevy_egui::egui;
use space_shared::Galaxy;

/// Captions, labels, hover rings and clicks on the 3D view itself.
#[allow(clippy::too_many_arguments)]
pub(super) fn canvas_overlay(
    ctx: &egui::Context,
    rect: egui::Rect,
    galaxy: &Galaxy,
    nav: &Navigation,
    markers: &Markers,
    rig: &CameraRig,
    pose: &CameraPose,
    field: &crate::stars::stream::Starfield,
    velocity: &CameraVelocity,
    out: &mut Vec<UiCommand>,
) {
    if galaxy.systems.is_empty() {
        egui::Area::new(egui::Id::new("loading-state"))
            .fixed_pos(rect.center() - egui::vec2(160.0, 45.0))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.spinner();
                ui.heading("Loading your universe");
                ui.label(&galaxy.source);
                ui.label("Systems appear as each list finishes loading.");
            });
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("annotations"),
        ))
        .with_clip_rect(rect);

    if nav.velocity_marker {
        draw_velocity_marker(
            &painter,
            rect,
            rig,
            velocity.marker_metres_per_second,
            nav.tan_half_fov() as f32,
        );
    }

    // Windows such as Settings float over the view; clicks there are theirs.
    let pointer = ctx
        .input(|i| i.pointer.interact_pos())
        .filter(|_| !pointer_captured(ctx));
    let mut click = pointer.is_some() && ctx.input(|i| i.pointer.primary_clicked());
    if let Some(star) = nav.selected_star() {
        let camera =
            rig.rotation().inverse() * (star.position_ly - rig.galactic_ly(pose)).as_vec3();
        if let Some(point) =
            project(rect, camera, nav.tan_half_fov() as f32).filter(|p| rect.contains(*p))
        {
            painter.circle_stroke(point, 10.0, egui::Stroke::new(1.5, ACCENT));
            painter.text(
                point + egui::vec2(16.0, -8.0),
                egui::Align2::LEFT_CENTER,
                crate::galaxy_nav::label(galaxy, &star.id),
                egui::FontId::proportional(12.0),
                ACCENT,
            );
        }
    }
    // Bevy viewport coordinates are window logical pixels; egui has a further UI scale.
    let scale = ctx.zoom_factor();
    let system = nav.system(galaxy);
    // A horizontal table needs its captions below each planet: putting them to
    // the right covers the next planet, especially on pages with many slots.
    let table_column = markers
        .0
        .windows(2)
        .map(|pair| (pair[1].point.x - pair[0].point.x).abs() / scale)
        .filter(|gap| *gap > 1.0)
        .fold(f32::INFINITY, f32::min);
    for marker in &markers.0 {
        let point = egui::pos2(marker.point.x / scale, marker.point.y / scale);
        if !rect.contains(point) {
            continue;
        }
        let c = marker.colour;
        let tint = egui::Color32::from_rgb(c[0], c[1], c[2]);
        let radius = (marker.radius / scale).max(7.0);
        let hovered = pointer.is_some_and(|p| rect.contains(p) && p.distance(point) < radius + 8.0);
        let selected = nav.selected_planet(galaxy) == Some(marker.planet);
        if selected || hovered {
            painter.circle_stroke(point, radius + 5.0, egui::Stroke::new(1.0, tint));
        }
        // Markers remain visible at true physical body sizes.
        if nav.markers && marker.radius < 6.0 {
            painter.circle_stroke(point, 4.0, egui::Stroke::new(1.0, tint));
        }
        // Task labels would smear across the screen mid-flight.
        if selected
            || hovered
            || ((nav.labels || nav.presentation_mode != PresentationMode::Simulation)
                && !rig.flying())
        {
            let table = nav.presentation_mode == PresentationMode::Table && !rig.flying();
            let width = if table {
                (table_column - 12.0).clamp(40.0, 180.0)
            } else {
                220.0
            };
            let text = painter.layout(
                marker.label.clone(),
                egui::FontId::proportional(12.0),
                egui::Color32::PLACEHOLDER,
                width,
            );
            let (x, y) = if table {
                (
                    (point.x - text.size().x * 0.5).clamp(
                        rect.left() + 10.0,
                        (rect.right() - 10.0 - text.size().x).max(rect.left() + 10.0),
                    ),
                    point.y + radius + 10.0,
                )
            } else {
                let right = point.x + radius + 10.0;
                let x = if right + text.size().x > rect.right() - 10.0 {
                    (point.x - radius - 10.0 - text.size().x).max(rect.left() + 10.0)
                } else {
                    right
                };
                (x, point.y - text.size().y * 0.5)
            };
            // Labels sit over star glare in tables and close-ups.
            crate::ui::breadcrumb::halo_galley(&painter, egui::pos2(x, y), text, tint);
        }
        // A second click on the selected planet follows it.
        if hovered && click {
            click = false;
            if selected {
                out.push(UiCommand::Follow);
            } else if let Some(planet) = system.and_then(|s| s.planets.get(marker.planet)) {
                out.push(UiCommand::Inspect {
                    planet: planet.id.clone(),
                });
            }
        }
    }
    // Any other click picks from the streamed galaxy, list pages included.
    if click
        && let Some(pointer) = pointer.filter(|p| rect.contains(*p))
        && let Some(star) = crate::galaxy_nav::pick_pointer(
            field,
            rig,
            pose,
            rig.rotation(),
            [
                f64::from(pointer.x - rect.min.x),
                f64::from(pointer.y - rect.min.y),
            ],
            [f64::from(rect.width()), f64::from(rect.height())],
            (nav.tan_half_fov(), nav.magnitude),
        )
    {
        out.push(UiCommand::PickStar { id: star.id });
    }
}

/// Screen point of a camera-space position or direction, through the 3D
/// camera's perspective (`tan_half` of its vertical field of view). `None`
/// behind the camera.
pub(super) fn project(rect: egui::Rect, camera: Vec3, tan_half: f32) -> Option<egui::Pos2> {
    if camera.z >= 0.0 {
        return None;
    }
    let size = rect.height() / (2.0 * tan_half * -camera.z);
    Some(egui::pos2(
        rect.center().x + camera.x * size,
        rect.center().y - camera.y * size,
    ))
}

/// Prograde (dot) and retrograde (cross), placed where the direction of travel
/// vanishes in the 3D view, so they sit over the scene's own motion. Each is
/// hidden when its direction is off-screen, including pure sideways travel.
fn draw_velocity_marker(
    painter: &egui::Painter,
    rect: egui::Rect,
    rig: &CameraRig,
    velocity: DVec3,
    tan_half: f32,
) {
    if velocity.length_squared() < 1.0 || rect.width() < 60.0 || rect.height() < 60.0 {
        return;
    }
    let direction = rig.rotation().inverse() * velocity.normalize().as_vec3();
    let stroke = egui::Stroke::new(1.5, ACCENT);
    for (vector, prograde) in [(direction, true), (-direction, false)] {
        let Some(point) = project(rect, vector, tan_half).filter(|p| rect.contains(*p)) else {
            continue;
        };
        painter.circle_stroke(point, 9.0, stroke);
        if prograde {
            painter.circle_filled(point, 2.5, ACCENT);
        } else {
            for sign in [-1.0, 1.0] {
                painter.line_segment(
                    [
                        point + egui::vec2(-4.0, 4.0 * sign),
                        point + egui::vec2(4.0, -4.0 * sign),
                    ],
                    stroke,
                );
            }
        }
    }
}
