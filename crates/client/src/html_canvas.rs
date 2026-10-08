//! Renderer-independent screen annotations and scene picking for the shared HTML UI.
use crate::{
    Navigation, Universe,
    actions::{PanelInsets, UiCommand},
    scene::{CameraPose, CameraRig, Markers},
    ui_bridge::{BridgeReady, UiEvent},
};
use bevy::{prelude::*, window::PrimaryWindow};
use serde::Serialize;

pub fn install(app: &mut App) {
    app.add_systems(Update, viewport.before(crate::scene::place_camera))
        .add_systems(Last, publish);
}

fn viewport(
    windows: Query<&Window, With<PrimaryWindow>>,
    insets: Res<PanelInsets>,
    mut nav: ResMut<Navigation>,
) {
    let Ok(window) = windows.single() else { return };
    let [top, right, bottom, left] = insets.0.unwrap_or_default();
    let scale = window.scale_factor();
    nav.viewport = Some(bevy::camera::Viewport {
        physical_position: UVec2::new((left * scale) as u32, (top * scale) as u32),
        physical_size: UVec2::new(
            ((window.width() - left - right).max(1.0) * scale) as u32,
            ((window.height() - top - bottom).max(1.0) * scale) as u32,
        ),
        ..default()
    });
}

#[derive(Serialize)]
struct Annotation {
    x: f32,
    y: f32,
    radius: f32,
    label: String,
    colour: [u8; 3],
    selected: bool,
    hovered: bool,
    marker: bool,
    table: bool,
    width: f32,
}
#[derive(Serialize)]
struct Direction {
    point: [f32; 2],
    prograde: bool,
}
#[derive(Serialize)]
struct Overlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_frame: Option<u32>,
    size: [f32; 2],
    rect: [f32; 4],
    bodies: Vec<Annotation>,
    picked: Option<([f32; 2], String)>,
    directions: Vec<Direction>,
}

fn project(rect: [f32; 4], vector: Vec3, tan_half: f32) -> Option<[f32; 2]> {
    if vector.z >= 0.0 {
        return None;
    }
    let [x, y, w, h] = rect;
    let scale = h / (2.0 * tan_half * -vector.z);
    let point = [
        x + w / 2.0 + vector.x * scale,
        y + h / 2.0 - vector.y * scale,
    ];
    contains(rect, point).then_some(point)
}
fn contains([x, y, w, h]: [f32; 4], [px, py]: [f32; 2]) -> bool {
    px >= x && py >= y && px < x + w && py < y + h
}

#[allow(clippy::too_many_arguments)]
pub fn publish(
    windows: Query<&Window, With<PrimaryWindow>>,
    ready: Res<BridgeReady>,
    universe: Res<Universe>,
    nav: Res<Navigation>,
    markers: Res<Markers>,
    rig: Res<CameraRig>,
    pose: Res<CameraPose>,
    field: Res<crate::stream::Starfield>,
    velocity: Res<crate::scene::CameraVelocity>,
    buttons: Res<ButtonInput<MouseButton>>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "cef-ui"))] html: Option<
        Res<crate::cef_input::HtmlInput>,
    >,
    mut press: Local<Option<(Vec2, bool)>>,
    mut writer: MessageWriter<UiEvent>,
    mut commands: MessageWriter<UiCommand>,
    mut last: Local<String>,
    #[cfg(all(not(target_arch = "wasm32"), feature = "cef-ui"))] mut probe: Option<
        ResMut<crate::cef_latency::Probe>,
    >,
) {
    if !ready.0 {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(viewport) = &nav.viewport else {
        return;
    };
    let scale = window.scale_factor();
    let origin = viewport.physical_position.as_vec2() / scale;
    let size = viewport.physical_size.as_vec2() / scale;
    let rect = [origin.x, origin.y, size.x, size.y];
    #[cfg(all(not(target_arch = "wasm32"), feature = "cef-ui"))]
    let blocked = html.is_some_and(|h| h.pointer);
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "cef-ui")))]
    let blocked = false;
    let pointer = window
        .cursor_position()
        .filter(|p| !blocked && contains(rect, p.to_array()));
    if buttons.just_pressed(MouseButton::Left) {
        *press = pointer.map(|p| (p, false));
    }
    if let Some((start, dragged)) = press.as_mut() {
        *dragged |= window
            .cursor_position()
            .is_none_or(|p| p.distance(*start) > 4.0);
    }
    let mut click = buttons.just_released(MouseButton::Left)
        && press
            .take_if(|_| buttons.just_released(MouseButton::Left))
            .is_some_and(|(_, dragged)| !dragged);
    if !window.focused {
        *press = None;
        click = false;
    }
    let table = nav.view == crate::views::View::Table && !rig.flying();
    let column = markers
        .0
        .windows(2)
        .map(|p| (p[1].point.x - p[0].point.x).abs())
        .filter(|w| *w > 1.0)
        .fold(f32::INFINITY, f32::min);
    let mut bodies = Vec::new();
    for marker in &markers.0 {
        if !contains(rect, marker.point.to_array()) {
            continue;
        }
        let radius = marker.radius.max(7.0);
        let hovered = pointer.is_some_and(|p| p.distance(marker.point) < radius + 8.0);
        let selected = nav.selected == Some(marker.planet);
        if hovered && click {
            click = false;
            if selected {
                commands.write(UiCommand::Follow);
            } else if let Some(planet) = nav
                .system(&universe.0)
                .and_then(|s| s.planets.get(marker.planet))
            {
                commands.write(UiCommand::Inspect {
                    planet: planet.id.clone(),
                });
            }
        }
        bodies.push(Annotation {
            x: marker.point.x,
            y: marker.point.y,
            radius,
            label: if selected
                || hovered
                || ((nav.labels || nav.view != crate::views::View::Orbit) && !rig.flying())
            {
                marker.label.clone()
            } else {
                String::new()
            },
            colour: marker.colour,
            selected,
            hovered,
            marker: nav.markers && marker.radius < 6.0,
            table,
            width: if table {
                (column - 12.0).clamp(40.0, 180.0)
            } else {
                220.0
            },
        });
    }
    if click
        && let Some(pointer) = pointer
        && let Some(star) = crate::galaxy_nav::pick_pointer(
            &field,
            &rig,
            &pose,
            rig.rotation(),
            [(pointer.x - origin.x) as f64, (pointer.y - origin.y) as f64],
            [size.x as f64, size.y as f64],
            nav.tan_half_fov(),
        )
    {
        commands.write(UiCommand::PickStar { id: star.id });
    }
    let picked = nav.picked.as_ref().and_then(|star| {
        project(
            rect,
            rig.rotation().inverse() * (star.position_ly - rig.galactic_ly(&pose)).as_vec3(),
            nav.tan_half_fov() as f32,
        )
        .map(|p| (p, crate::galaxy_nav::label(&universe.0, &star.id)))
    });
    let mut directions = Vec::new();
    if nav.velocity_marker
        && velocity.marker_metres_per_second.length_squared() >= 1.0
        && size.min_element() >= 60.0
    {
        let direction =
            rig.rotation().inverse() * velocity.marker_metres_per_second.normalize().as_vec3();
        for (v, prograde) in [(direction, true), (-direction, false)] {
            if let Some(point) = project(rect, v, nav.tan_half_fov() as f32) {
                directions.push(Direction { point, prograde });
            }
        }
    }
    #[cfg(all(not(target_arch = "wasm32"), feature = "cef-ui"))]
    let probe_frame = probe.as_mut().map(|p| p.stamp());
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "cef-ui")))]
    let probe_frame = None;
    if let Ok(json) = serde_json::to_string(&Overlay {
        probe_frame,
        size: [window.width(), window.height()],
        rect,
        bodies,
        picked,
        directions,
    }) && (*last != json || ready.is_changed())
    {
        *last = json.clone();
        writer.write(UiEvent {
            name: "space:overlay",
            json,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picking_app() -> App {
        let galaxy = space_simulation::demo_galaxy();
        let mut app = App::new();
        app.insert_resource(Universe(galaxy))
            .insert_resource(BridgeReady(true))
            .insert_resource(Navigation {
                visit: Some(crate::Visit::List(0)),
                viewport: Some(bevy::camera::Viewport {
                    physical_size: UVec2::new(800, 600),
                    ..default()
                }),
                ..default()
            })
            .init_resource::<CameraRig>()
            .init_resource::<CameraPose>()
            .init_resource::<crate::stream::Starfield>()
            .init_resource::<crate::scene::CameraVelocity>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(Markers(vec![crate::scene::Marker {
                point: Vec2::new(400.0, 300.0),
                radius: 4.0,
                label: "Planet".into(),
                colour: [255, 0, 0],
                planet: 0,
            }]))
            .add_message::<UiEvent>()
            .add_message::<UiCommand>()
            .add_systems(Update, publish);
        let mut window = Window {
            resolution: (800, 600).into(),
            focused: true,
            ..default()
        };
        window.set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        app.world_mut().spawn((window, PrimaryWindow));
        app
    }

    fn click(app: &mut App, drag: bool) -> Vec<UiCommand> {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        if drag {
            let mut windows = app.world_mut().query::<&mut Window>();
            windows
                .single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(Some(Vec2::new(450.0, 300.0)));
            app.update();
            windows
                .single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        let messages = app.world().resource::<Messages<UiCommand>>();
        messages.get_cursor().read(messages).cloned().collect()
    }

    #[test]
    fn planet_click_selects_then_follows_without_an_egui_context() {
        let mut app = picking_app();
        let id = app.world().resource::<Universe>().0.systems[0].planets[0]
            .id
            .clone();
        assert_eq!(
            click(&mut app, false),
            vec![UiCommand::Inspect { planet: id }]
        );
        app.world_mut()
            .resource_mut::<Messages<UiCommand>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut().resource_mut::<Navigation>().selected = Some(0);
        assert_eq!(click(&mut app, false), vec![UiCommand::Follow]);
    }

    #[test]
    fn dragging_away_and_back_does_not_select_a_planet() {
        assert!(click(&mut picking_app(), true).is_empty());
    }

    #[cfg(feature = "cef-ui")]
    #[test]
    fn native_panel_owned_clicks_never_pick_the_scene_behind_them() {
        let mut app = picking_app();
        let mut input = crate::cef_input::HtmlInput::default();
        input.pointer = true;
        app.insert_resource(input);
        assert!(click(&mut app, false).is_empty());
    }

    #[test]
    fn projection_respects_fov_viewport_and_front_plane() {
        let rect = [10.0, 20.0, 1600.0, 900.0];
        assert_eq!(project(rect, -Vec3::Z, 1.0), Some([810.0, 470.0]));
        assert!(project(rect, Vec3::Z, 1.0).is_none());
        assert!(project(rect, Vec3::new(10.0, 0.0, -1.0), 1.0).is_none());
        assert!(project(rect, Vec3::new(1.0, 0.0, -1.0), 0.2).is_none());
    }
}
