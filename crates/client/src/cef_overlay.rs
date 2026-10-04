//! Opt-in native HTML proof: transparent rendering, input ownership and a tiny bridge.
use bevy::{
    camera::visibility::RenderLayers, core_pipeline::tonemapping::Tonemapping, prelude::*,
    render::view::Hdr,
};
use bevy_cef::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component)]
pub struct HtmlPanel;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PanelEvent {
    Layout { rects: Vec<[f32; 4]> },
    Command { command: crate::actions::UiCommand },
}

#[derive(Serialize)]
struct PanelState {
    paused: bool,
}

#[derive(Serialize)]
struct PanelTick {
    days: f64,
}

#[derive(Component)]
struct HtmlCamera;

pub fn install(app: &mut App) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = root.join("data/cef-overlay");
    std::fs::create_dir_all(root.join("data/logs")).expect("Cannot create CEF log directory");
    app.add_plugins(CefPlugin {
        root_cache_path: Some(cache.to_string_lossy().into_owned()),
        command_line_config: CommandLineConfig::default()
            .with_switch("disable-background-networking")
            .with_switch_value(
                "log-file",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../data/logs/cef-overlay.log"
                ),
            ),
        ..default()
    })
    .add_plugins(JsEmitEventPlugin::<PanelEvent>::default())
    .add_observer(receive)
    .add_systems(Startup, setup)
    .add_systems(Update, sync_hdr)
    .add_systems(Last, publish);
    crate::cef_input::install(app);
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<WebviewUiMaterial>>) {
    // Overlay after both the scene and egui, with no exposure or tone mapping.
    // Match the scene's HDR format to share its output, without changing it.
    let camera = commands
        .spawn((
            Camera2d,
            HtmlCamera,
            Tonemapping::None,
            Camera {
                order: 2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(29),
        ))
        .id();
    commands.spawn((
        Name::new("CEF HTML overlay proof of concept"),
        HtmlPanel,
        // Custom native routing retains drag ownership and always sends releases.
        // Disable the plugin's alpha-filtered pointer observers and focus picker.
        Pickable::IGNORE,
        WebviewSource::inline(include_str!("../../../web/cef-overlay.html")),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        UiTargetCamera(camera),
        GlobalZIndex(100),
        MaterialNode(materials.add(WebviewUiMaterial::default())),
    ));
}

fn receive(
    event: On<Receive<PanelEvent>>,
    panels: Query<(), With<HtmlPanel>>,
    mut input: ResMut<crate::cef_input::HtmlInput>,
    mut writer: MessageWriter<crate::actions::UiCommand>,
) {
    if panels.get(event.webview).is_err() {
        return;
    }
    match &event.payload {
        PanelEvent::Layout { rects } if valid_rects(rects) => {
            input.rects.clone_from(rects);
            input.ready = true;
        }
        PanelEvent::Command {
            command: command @ crate::actions::UiCommand::SetPaused { .. },
        } => {
            writer.write(command.clone());
        }
        _ => {} // This experiment cannot issue unrelated application commands.
    }
}

fn valid_rects(rects: &[[f32; 4]]) -> bool {
    rects.len() <= 8
        && rects
            .iter()
            .all(|r| r.iter().all(|v| v.is_finite()) && r[2] >= 0.0 && r[3] >= 0.0)
}

fn publish(
    mut commands: Commands,
    time: Res<Time>,
    clock: Res<crate::Clock>,
    input: Res<crate::cef_input::HtmlInput>,
    panels: Query<Entity, With<HtmlPanel>>,
    mut next: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if !input.ready || now < *next {
        return;
    }
    *next = now + 0.1;
    for entity in &panels {
        // Same event names and field units as the browser bridge. Repeat state
        // at 10 Hz so startup/listener races and egui changes self-correct.
        commands.trigger(HostEmitEvent::new(
            entity,
            "space:state",
            &PanelState {
                paused: clock.0.paused,
            },
        ));
        commands.trigger(HostEmitEvent::new(
            entity,
            "space:tick",
            &PanelTick {
                days: clock.0.seconds / 86_400.0,
            },
        ));
    }
}

fn sync_hdr(
    mut commands: Commands,
    nav: Res<crate::Navigation>,
    cameras: Query<(Entity, Has<Hdr>), With<HtmlCamera>>,
) {
    for (entity, hdr) in &cameras {
        if nav.realistic != hdr {
            if nav.realistic {
                commands.entity(entity).insert(Hdr);
            } else {
                commands.entity(entity).remove::<Hdr>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_only_queues_pause_commands_from_the_panel() {
        use crate::actions::UiCommand;
        let mut app = App::new();
        app.init_resource::<crate::cef_input::HtmlInput>()
            .add_message::<UiCommand>()
            .add_observer(receive);
        let panel = app.world_mut().spawn(HtmlPanel).id();
        let foreign = app.world_mut().spawn_empty().id();
        for (webview, command) in [
            (panel, UiCommand::SetPaused { paused: true }),
            (panel, UiCommand::SetRate { rate: 42.0 }),
            (foreign, UiCommand::SetPaused { paused: false }),
        ] {
            app.world_mut().trigger(Receive {
                webview,
                payload: PanelEvent::Command { command },
            });
        }
        let messages = app.world().resource::<Messages<UiCommand>>();
        let mut cursor = messages.get_cursor();
        assert_eq!(
            cursor.read(messages).cloned().collect::<Vec<_>>(),
            vec![UiCommand::SetPaused { paused: true }]
        );
    }

    #[test]
    fn bridge_accepts_layout_and_rejects_invalid_rectangles() {
        assert!(valid_rects(&[[0.0, 20.0, 100.0, 200.0]]));
        assert!(!valid_rects(&[[0.0, 0.0, -1.0, 10.0]]));
        assert!(!valid_rects(&[[f32::NAN, 0.0, 1.0, 1.0]]));
        assert!(!valid_rects(&[[0.0; 4]; 9]));
        let command: PanelEvent = serde_json::from_str(
            r#"{"kind":"command","command":{"type":"set_paused","paused":true}}"#,
        )
        .unwrap();
        assert!(matches!(
            command,
            PanelEvent::Command {
                command: crate::actions::UiCommand::SetPaused { paused: true }
            }
        ));
    }

    #[test]
    fn html_camera_matches_hdr_without_changing_other_cameras() {
        let mut app = App::new();
        app.init_resource::<crate::Navigation>()
            .add_systems(Update, sync_hdr);
        let html = app.world_mut().spawn(HtmlCamera).id();
        let other = app.world_mut().spawn_empty().id();

        app.update();
        assert!(app.world().entity(html).contains::<Hdr>());
        assert!(!app.world().entity(other).contains::<Hdr>());

        app.world_mut()
            .resource_mut::<crate::Navigation>()
            .realistic = false;
        app.update();
        assert!(!app.world().entity(html).contains::<Hdr>());

        app.world_mut()
            .resource_mut::<crate::Navigation>()
            .realistic = true;
        app.update();
        assert!(app.world().entity(html).contains::<Hdr>());
    }
}
