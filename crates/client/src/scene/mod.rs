//! The 3D scene: the camera, its controls, location and placement, and the
//! visited system's wireframes. Every submodule is re-exported, so callers use
//! `scene::` paths.
use bevy::prelude::*;
use big_space::prelude::*;

mod camera;
mod controls;
mod draw;
mod frames;
mod location;
mod placement;

pub use camera::*;
pub use controls::*;
pub use draw::*;
pub use frames::*;
pub use location::*;
pub use placement::*;

pub fn setup(mut commands: Commands, mut gizmo_store: ResMut<GizmoConfigStore>) {
    // UI owns the full window; the 3D camera owns only the unobscured centre.
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        bevy_egui::PrimaryEguiContext,
        bevy::camera::visibility::RenderLayers::layer(31),
    ));
    let (config, _) = gizmo_store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 1.25;
    commands.init_resource::<CameraPose>();
    commands.init_resource::<CameraRig>();
    commands.init_resource::<CameraVelocity>();
    commands.init_resource::<CameraMotion>();
    commands.init_resource::<Markers>();
    commands.spawn_big_space_default(|root| {
        root.spawn_spatial((
            SpaceCamera,
            FloatingOrigin,
            Camera3d {
                // Star points test their depth against it (`stars/points.rs`).
                // WebGL cannot sample multisampled depth textures: retain
                // its default render-attachment-only depth buffer.
                #[cfg(not(target_arch = "wasm32"))]
                depth_texture_usages:
                    (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                        | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                        .into(),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                near: 1000.0,
                // Past the galaxy even at a close follow camera's render scale.
                far: 1e30,
                fov: 50.0_f32.to_radians(),
                ..default()
            }),
            Msaa::Sample4,
            // GPU-driven culling drops every mesh with this far plane. WebGL 2
            // has no indirect drawing, so native now matches the browser.
            bevy::render::view::NoIndirectDrawing,
        ));
    });
}

#[cfg(test)]
mod tests;
