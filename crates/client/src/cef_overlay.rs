//! Steps 1 and 2 only: native Bevy with one transparent Chromium UI surface.
//! Camera input arbitration and the application command bridge are not added yet.
use bevy::{
    camera::visibility::RenderLayers, core_pipeline::tonemapping::Tonemapping, prelude::*,
    render::view::Hdr,
};
use bevy_cef::prelude::*;

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
    .add_systems(Startup, setup)
    .add_systems(Update, sync_hdr);
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
