//! Native transport and transparent composition for the shared HTML UI.
use bevy::{
    camera::visibility::RenderLayers, core_pipeline::tonemapping::Tonemapping, prelude::*,
    render::view::Hdr,
};
use bevy_cef::prelude::*;
use serde::Deserialize;

#[derive(Component)]
pub struct HtmlPanel;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PanelEvent {
    Ready,
    Layout { rects: Vec<[f32; 4]> },
    Focus { keyboard: bool },
    Command { command: crate::actions::UiCommand },
}

#[derive(Component)]
struct HtmlCamera;

pub fn install(app: &mut App) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = root.join("data/cef-ui");
    std::fs::create_dir_all(root.join("data/logs")).expect("Cannot create CEF log directory");
    let mut config = CommandLineConfig::default()
        .with_switch("disable-background-networking")
        .with_switch_value(
            "log-file",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/logs/cef-ui.log"),
        );
    // Explicit development-only opt-in, never expose a debugging port by default.
    if let Some(port) = std::env::var("SPACE_UI_DEBUG_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p >= 1024)
    {
        static DEBUG_PORT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        config = config
            .with_switch_value(
                "remote-debugging-port",
                DEBUG_PORT.get_or_init(|| port.to_string()).as_str(),
            )
            .with_switch_value("remote-debugging-address", "127.0.0.1");
    }
    app.add_plugins(CefPlugin {
        root_cache_path: Some(cache.to_string_lossy().into_owned()),
        command_line_config: config,
        ..default()
    })
    .add_plugins(JsEmitEventPlugin::<PanelEvent>::default())
    .add_observer(receive)
    .add_systems(Startup, setup)
    .add_systems(Update, sync_hdr)
    .add_systems(
        Last,
        publish
            .after(crate::html_canvas::publish)
            .after(crate::ui_bridge::publish_galaxy)
            .after(crate::ui_bridge::publish_state)
            .after(crate::ui_bridge::publish_tick),
    );
    crate::cef_input::install(app);
    crate::cef_latency::install(app);
}

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<WebviewUiMaterial>>,
    benchmark: Option<Res<crate::native_benchmark::Benchmark>>,
) {
    let mode = benchmark.as_ref().map_or("", |b| b.spec.mode.as_str());
    let html = include_str!(concat!(env!("OUT_DIR"), "/native-ui.html"))
        .replace("<body>", &format!("<body data-benchmark=\"{mode}\">"));
    // Overlay after the scene, with no exposure or tone mapping.
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
        Name::new("Shared HTML UI"),
        HtmlPanel,
        // Custom native routing retains drag ownership and always sends releases.
        // Disable the plugin's alpha-filtered pointer observers and focus picker.
        Pickable::IGNORE,
        WebviewSource::inline(html),
        if mode == "runtime" {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        },
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
    mut ready: ResMut<crate::ui_bridge::BridgeReady>,
) {
    if panels.get(event.webview).is_err() {
        return;
    }
    match &event.payload {
        PanelEvent::Ready => {
            input.usable = true;
            ready.0 = true;
        }
        PanelEvent::Layout { rects } if valid_rects(rects) => {
            input.rects.clone_from(rects);
            input.ready = true;
        }
        PanelEvent::Focus { keyboard } => input.set_dom_focus(*keyboard),
        PanelEvent::Command { command } => {
            writer.write(command.clone());
        }
        _ => {}
    }
}

fn valid_rects(rects: &[[f32; 4]]) -> bool {
    rects.len() <= 128
        && rects
            .iter()
            .all(|r| r.iter().all(|v| v.is_finite()) && r[2] >= 0.0 && r[3] >= 0.0)
}

fn publish(
    mut commands: Commands,
    panels: Query<Entity, With<HtmlPanel>>,
    mut events: MessageReader<crate::ui_bridge::UiEvent>,
) {
    for event in events.read() {
        for entity in &panels {
            commands.trigger(HostEmitEvent {
                webview: entity,
                id: event.name.into(),
                payload: event.json.clone(),
            });
        }
    }
}

fn sync_hdr(
    mut commands: Commands,
    nav: Res<crate::Navigation>,
    mut cameras: Query<(Entity, Has<Hdr>, &mut Camera), With<HtmlCamera>>,
    benchmark: Option<Res<crate::native_benchmark::Benchmark>>,
    input: Res<crate::cef_input::HtmlInput>,
) {
    for (entity, hdr, mut camera) in &mut cameras {
        if input.ready && benchmark.as_ref().is_some_and(|b| b.spec.mode == "runtime") {
            camera.is_active = false;
        }
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
    fn bridge_queues_all_shared_commands_only_from_the_panel() {
        use crate::actions::UiCommand;
        let mut app = App::new();
        app.init_resource::<crate::cef_input::HtmlInput>()
            .init_resource::<crate::ui_bridge::BridgeReady>()
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
            vec![
                UiCommand::SetPaused { paused: true },
                UiCommand::SetRate { rate: 42.0 }
            ]
        );
    }

    #[test]
    fn bridge_accepts_layout_and_rejects_invalid_rectangles() {
        assert!(valid_rects(&[[0.0, 20.0, 100.0, 200.0]]));
        assert!(!valid_rects(&[[0.0, 0.0, -1.0, 10.0]]));
        assert!(!valid_rects(&[[f32::NAN, 0.0, 1.0, 1.0]]));
        assert!(!valid_rects(&[[0.0; 4]; 129]));
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
            .init_resource::<crate::cef_input::HtmlInput>()
            .add_systems(Update, sync_hdr);
        let html = app.world_mut().spawn((HtmlCamera, Camera::default())).id();
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
