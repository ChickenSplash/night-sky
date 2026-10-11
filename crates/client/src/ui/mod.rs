pub mod breadcrumb;
pub mod inspector;
pub mod tasks;
pub mod terminal;

use crate::actions::{Display, UiCommand};
use crate::presentation::PresentationMode;
use crate::{
    Clock, Navigation, Universe,
    scene::{CameraPose, CameraRig, CameraVelocity, Markers, speed_label},
};
use bevy::{math::DVec3, prelude::*};
use bevy_egui::{EguiContexts, egui};
use space_shared::Galaxy;
use space_simulation::SimulationClock;

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

fn settings(ui: &mut egui::Ui, nav: &Navigation, out: &mut Vec<UiCommand>) {
    heading(ui, "DISPLAY");
    for (option, mut on, label) in [
        (Display::Exaggerate, nav.exaggerate, "Readable planet sizes"),
        (Display::Orbits, nav.orbits, "Orbital paths"),
        (Display::Labels, nav.labels, "Planet labels"),
        (Display::Markers, nav.markers, "Body markers"),
        (
            Display::VelocityMarker,
            nav.velocity_marker,
            "Velocity marker",
        ),
    ] {
        if ui.checkbox(&mut on, label).changed() {
            out.push(UiCommand::SetDisplay { option, on });
        }
    }
    let mut degrees = nav.fov_deg;
    if ui
        .add(
            egui::Slider::new(&mut degrees, crate::FOV_DEG)
                .text("Field of view")
                .suffix("°"),
        )
        .changed()
    {
        out.push(UiCommand::SetFov { degrees });
    }
    muted(
        ui,
        "Simulation mode uses physical distances. The task table keeps true body sizes with schematic spacing. Orbits keep running in every presentation mode.",
    );
    ui.separator();
    heading(ui, "RENDERING");
    let mut realistic = nav.realistic;
    if ui.checkbox(&mut realistic, "Realistic lighting").changed() {
        out.push(UiCommand::SetDisplay {
            option: Display::Realistic,
            on: realistic,
        });
    }
    muted(
        ui,
        "Realistic lighting draws stars by temperature and true brightness, and lights planets by their star.",
    );
    ui.separator();
    heading(ui, "CONTROLS");
    egui::Grid::new("controls").num_columns(2).show(ui, |ui| {
        for (key, action) in HELP {
            ui.label(egui::RichText::new(key).monospace().color(ACCENT));
            ui.label(action);
            ui.end_row();
        }
    });
}

const HELP: [(&str, &str); 12] = [
    ("Click", "Select a star, again to visit"),
    ("Enter", "Visit the selected star"),
    ("Left drag", "Look around"),
    ("Right drag", "Orbit"),
    ("Shift + left drag", "Field of view"),
    ("Scroll", "Adjust movement speed"),
    ("Shift + scroll", "Zoom"),
    ("W A S D", "Move"),
    ("Q / E", "Roll left / right"),
    ("R / F", "Up / down"),
    ("Space", "Pause"),
    ("Esc", "Leave a text field"),
];

const YEAR: f64 = 31_557_600.0;
const MAX_RATE: f64 = 100.0 * YEAR;
/// A 400-point horizontal drag covers either HUD control's logarithmic range.
const HUD_DRAG_SPAN: f64 = 400.0;

/// Background-layer HUD text is deliberately not an egui window. egui's
/// `is_pointer_over_area` excludes it, so explicitly protect its hit region
/// and retain capture when a drag leaves that region.
pub(crate) fn pointer_captured(ctx: &egui::Context) -> bool {
    ctx.is_pointer_over_area()
        || ctx.wants_pointer_input()
        || [
            "time-pointer-capture",
            "speed-pointer-capture",
            "magnitude-pointer-capture",
            "exposure-pointer-capture",
            "compression-pointer-capture",
            "breadcrumb-pointer-capture",
            "icons-pointer-capture",
        ]
        .into_iter()
        .any(|id| {
            let hud = ctx.data(|data| data.get_temp::<(egui::Rect, bool)>(egui::Id::new(id)));
            hud.is_some_and(|(rect, dragging)| {
                dragging
                    || ctx.input(|input| {
                        // Hover only claims an idle pointer. While a button is held,
                        // ownership comes from the press origin, not the current
                        // position: a camera drag can pass straight over the HUD.
                        let pos = if input.pointer.any_down() {
                            input.pointer.press_origin()
                        } else {
                            input.pointer.hover_pos()
                        };
                        pos.is_some_and(|p| rect.contains(p))
                    })
            })
        })
}

fn dragged_rate(start: f64, delta: f32) -> f64 {
    dragged_value(start, delta, 1.0, MAX_RATE)
}

fn dragged_value(start: f64, delta: f32, min: f64, max: f64) -> f64 {
    (start.clamp(min, max).ln() + f64::from(delta) / HUD_DRAG_SPAN * (max / min).ln())
        .clamp(min.ln(), max.ln())
        .exp()
        .clamp(min, max)
}

fn rate_text(rate: f64) -> String {
    let (unit, scale) = [
        ("years/s", YEAR),
        ("days/s", 86_400.0),
        ("hours/s", 3_600.0),
        ("minutes/s", 60.0),
        ("seconds/s", 1.0),
    ]
    .into_iter()
    .find(|(_, scale)| rate >= *scale)
    .unwrap_or(("seconds/s", 1.0));
    format!("{:.2} {unit}", rate / scale)
}

fn time_controls(
    ctx: &egui::Context,
    rect: egui::Rect,
    clock: &SimulationClock,
    drag_start: &mut Option<f64>,
    out: &mut Vec<UiCommand>,
) {
    use crate::ui::breadcrumb::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, halo_text};

    egui::Area::new(egui::Id::new("time-controls"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(rect.center_bottom() - egui::vec2(0.0, HUD_PADDING))
        .show(ctx, |ui| {
            ui.set_width(260.0);
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            ui.vertical_centered(|ui| {
                halo_text(
                    ui,
                    &elapsed_text(clock.seconds / 86_400.0),
                    egui::Sense::hover(),
                    |_| CURRENT,
                );
                let mut label = rate_text(clock.rate);
                if clock.paused {
                    label.push_str(" · paused");
                }
                let response = halo_text(ui, &label, egui::Sense::click_and_drag(), |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                })
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text(if clock.paused {
                    "Click to resume · Drag left/right to adjust speed"
                } else {
                    "Click to pause · Drag left/right to adjust speed"
                });
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("time-pointer-capture"),
                        (
                            response.rect,
                            response.dragged() || response.is_pointer_button_down_on(),
                        ),
                    );
                });
                if response.drag_started() {
                    *drag_start = Some(clock.rate);
                }
                if response.dragged()
                    && let Some(start) = *drag_start
                {
                    out.push(UiCommand::SetRate {
                        rate: dragged_rate(
                            start,
                            response.total_drag_delta().unwrap_or_default().x,
                        ),
                    });
                }
                if response.drag_stopped() {
                    *drag_start = None;
                }
                if response.clicked() {
                    out.push(UiCommand::SetPaused {
                        paused: !clock.paused,
                    });
                }
            });
        });
}

fn elapsed_text(days: f64) -> String {
    if days >= 365.25 {
        format!("T + {:.2} yr", days / 365.25)
    } else {
        format!("T + {days:.2} days")
    }
}

const JUPITER_MASSES: f64 = 317.83;

pub(crate) fn mass_text(earths: f64) -> String {
    if earths >= 0.1 * JUPITER_MASSES {
        format!("{:.2} Jupiter masses", earths / JUPITER_MASSES)
    } else if earths < 0.1 {
        format!("{earths:.4} Earth masses")
    } else {
        format!("{earths:.2} Earth masses")
    }
}

pub(crate) fn period_text(days: f64) -> String {
    if days >= 730.0 {
        format!("{:.1} years", days / 365.25)
    } else {
        format!("{days:.1} days")
    }
}

pub(crate) fn rotation_text(hours: f64) -> String {
    if hours >= 48.0 {
        period_text(hours / 24.0)
    } else if hours >= 2.0 {
        format!("{hours:.1} hours")
    } else {
        format!("{:.0} minutes", hours * 60.0)
    }
}

pub(crate) fn tilt_text(radians: f64) -> String {
    let degrees = radians.to_degrees();
    format!(
        "{degrees:.1}°{}",
        if degrees > 90.0 { " · retrograde" } else { "" }
    )
}

pub(crate) fn star_radius_text(solar: f64) -> String {
    if solar < 0.05 {
        format!("{:.0} km", solar * 695_700.0)
    } else {
        format!("{solar:.2} solar radii")
    }
}

pub(crate) fn luminosity_text(solar: f64) -> String {
    if solar < 0.01 {
        format!("{solar:.1e}")
    } else if solar < 10.0 {
        format!("{solar:.3}")
    } else {
        format!("{solar:.0}")
    }
}

/// Day, month and year of a Unix time in milliseconds (UTC).
fn civil_date(ms: i64) -> (i64, &'static str, i64) {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Howard Hinnant's days-to-civil algorithm.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (day, MONTHS[month as usize - 1], year)
}

pub(crate) fn due_short(ms: i64) -> String {
    let days = (ms - crate::network::now_ms()) as f64 / 86_400_000.0;
    if days.abs() < 1.0 {
        "Today".into()
    } else if days < 0.0 {
        format!("{:.0}d late", -days)
    } else if days > 45.0 {
        let (day, month, _) = civil_date(ms);
        format!("{day} {month}")
    } else {
        format!("in {days:.0}d")
    }
}

pub(crate) fn due_text(ms: i64) -> String {
    let (day, month, year) = civil_date(ms);
    format!("{day} {month} {year} · {}", due_short(ms).to_lowercase())
}

/// Measured velocity above a draggable set speed, like the time controls.
fn speed_readout(
    ctx: &egui::Context,
    rect: egui::Rect,
    set_speed: f64,
    velocity: f64,
    drag_start: &mut Option<f64>,
    out: &mut Vec<UiCommand>,
) {
    use crate::ui::breadcrumb::{CURRENT, HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, halo_text};

    egui::Area::new(egui::Id::new("speed-readout"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(rect.left_bottom() + egui::vec2(HUD_PADDING, -HUD_PADDING))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = HUD_LINE_GAP;
            halo_text(
                ui,
                &format!("Velocity {}", speed_label(velocity)),
                egui::Sense::hover(),
                |_| CURRENT,
            );
            let response = halo_text(
                ui,
                &format!("Set speed {}", speed_label(set_speed)),
                egui::Sense::click_and_drag(),
                |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                },
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text("Drag left/right to adjust movement speed");
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("speed-pointer-capture"),
                    (
                        response.rect,
                        response.dragged() || response.is_pointer_button_down_on(),
                    ),
                );
            });
            if response.drag_started() {
                *drag_start = Some(set_speed);
            }
            if response.dragged()
                && let Some(start) = *drag_start
            {
                out.push(UiCommand::SetMoveSpeed {
                    speed: dragged_value(
                        start,
                        response.total_drag_delta().unwrap_or_default().x,
                        crate::scene::MIN_SPEED,
                        crate::scene::MAX_SPEED,
                    ),
                });
            }
            if response.drag_stopped() {
                *drag_start = None;
            }
        });
}

/// The magnitude limit `start` dragged `delta` points, to a tenth: the same
/// span as the other HUD controls covers its whole range.
fn dragged_magnitude(start: f64, delta: f32) -> f64 {
    let (min, max) = (*crate::MAGNITUDE.start(), *crate::MAGNITUDE.end());
    ((start + f64::from(delta) / HUD_DRAG_SPAN * (max - min)).clamp(min, max) * 10.0).round() / 10.0
}

/// The draggable star point magnitude limit, at the bottom right. It applies
/// on release, as each change reloads the stars.
fn magnitude_control(
    ctx: &egui::Context,
    rect: egui::Rect,
    magnitude: f64,
    drag: &mut Option<(f64, f64)>,
    out: &mut Vec<UiCommand>,
) {
    use crate::ui::breadcrumb::{HOVER, HUD_PADDING, LINK, halo_text};

    egui::Area::new(egui::Id::new("magnitude-control"))
        .order(egui::Order::Background)
        .movable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(rect.right_bottom() + egui::vec2(-HUD_PADDING, -HUD_PADDING))
        .show(ctx, |ui| {
            let shown = drag.map_or(magnitude, |(_, now)| now);
            let response = halo_text(
                ui,
                &format!("Magnitude limit {shown:.1}"),
                egui::Sense::click_and_drag(),
                |response| {
                    if response.hovered() || response.dragged() || response.has_focus() {
                        HOVER
                    } else {
                        LINK
                    }
                },
            )
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
            .on_hover_text(
                "Faintest star drawn as a point; fainter ones join the galaxy glow · Drag left/right, stars reload on release",
            );
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("magnitude-pointer-capture"),
                    (
                        response.rect,
                        response.dragged() || response.is_pointer_button_down_on(),
                    ),
                );
            });
            if response.drag_started() {
                *drag = Some((magnitude, magnitude));
            }
            if response.dragged()
                && let Some((start, now)) = drag
            {
                *now = dragged_magnitude(*start, response.total_drag_delta().unwrap_or_default().x);
            }
            if response.drag_stopped()
                && let Some((_, now)) = drag.take()
                && now != magnitude
            {
                out.push(UiCommand::SetMagnitude { magnitude: now });
            }
        });
}

/// Two immediate draggable text controls, aligned above the magnitude limit.
fn exposure_controls(
    ctx: &egui::Context,
    rect: egui::Rect,
    nav: &Navigation,
    drags: &mut [Option<f64>; 2],
    out: &mut Vec<UiCommand>,
) {
    use crate::ui::breadcrumb::{HOVER, HUD_LINE_GAP, HUD_PADDING, LINK, SIZE, halo_text};
    let height = ctx.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(SIZE)));
    for (index, (id, value, text, help)) in [
        ("exposure", nav.exposure, format!("Exposure {:+.2} stops", nav.exposure), "Drag left/right: 20 pixels per stop. +1 stop doubles light, -1 halves it. Applies immediately, with no automatic adaptation."),
        ("compression", nav.compression, format!("Dynamic Range Compression {:.0}%", nav.compression * 100.0), "Drag left/right: 400 pixels covers 0 to 100%. Exposes each star, corona and planet for itself: 0% is one physical exposure, 100% shows every body correctly exposed. The galaxy always follows Exposure. Applies immediately."),
    ].into_iter().enumerate() {
        egui::Area::new(egui::Id::new(("manual-control", id)))
            .order(egui::Order::Background)
            .movable(false)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(rect.right_bottom() + egui::vec2(-HUD_PADDING, -HUD_PADDING - (2 - index) as f32 * (height + HUD_LINE_GAP)))
            .show(ctx, |ui| {
                let response = halo_text(ui, &text, egui::Sense::click_and_drag(), |r| {
                    if nav.realistic && (r.hovered() || r.dragged() || r.has_focus()) { HOVER } else { LINK }
                }).on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text(
                    if nav.realistic { help } else { "Turn on realistic lighting in Settings to adjust this." });
                ctx.data_mut(|data| data.insert_temp(egui::Id::new(format!("{id}-pointer-capture")),
                    (response.rect, response.dragged() || response.is_pointer_button_down_on())));
                if response.drag_started() && nav.realistic { drags[index] = Some(value); }
                if nav.realistic && response.dragged() && let Some(start) = drags[index] {
                    let delta = f64::from(response.total_drag_delta().unwrap_or_default().x);
                    out.push(if index == 0 {
                        UiCommand::SetExposure { stops: (start + delta / 20.0).clamp(*crate::render::exposure::STOPS.start(), *crate::render::exposure::STOPS.end()) }
                    } else {
                        UiCommand::SetCompression { amount: (start + delta / HUD_DRAG_SPAN).clamp(0.0, 1.0) }
                    });
                }
                if response.drag_stopped() || !nav.realistic { drags[index] = None; }
            });
    }
}

/// Captions, labels, hover rings and clicks on the 3D view itself.
#[allow(clippy::too_many_arguments)]
fn canvas_overlay(
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
                tint,
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
            painter.galley(egui::pos2(x, y), text, tint);
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
fn project(rect: egui::Rect, camera: Vec3, tan_half: f32) -> Option<egui::Pos2> {
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

#[cfg(test)]
mod tests;
