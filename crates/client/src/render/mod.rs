//! Realistic bodies: physical photometry, one manual linear exposure and
//! per-body balance (`resolve`). The display pass lives in `exposure`.
//! - Every body shares the camera's point spread with the galactic stars
//!   (`PSF_SIGMA_PX`). Bodies smaller than a point on screen are point sprites
//!   carrying the body's whole light; a disc's limb is blurred by the same
//!   spread in a sprite around it, so nothing jumps at the switch.
//! - Close up, a star is a disc with limb darkening, granulation and a
//!   structured corona.
//! - Far over white, every body shows eye-like streaks, and light above white
//!   is stored as glare (`hdr.wgsl`), so bloom grows with stops, not light.
//! - Colours reach the GPU already exposed, so half-float HDR targets never see
//!   raw radiance.

pub mod atmosphere;
pub mod black_hole;
pub mod debug;
pub mod exposure;
pub mod glow;
pub mod sky;
pub mod window_blur;

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    camera::visibility::VisibilitySystems,
    pbr::{Material, MaterialPlugin},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

mod bodies;
mod lights;
mod resolve;

use bodies::*;
pub use lights::*;
pub use resolve::*;

const BODY_SHADER: Handle<Shader> = uuid_handle!("6f1f3c47-2a8e-4d61-9b53-4f8e0d7a91c2");
const POINT_SHADER: Handle<Shader> = uuid_handle!("0c9b5d2e-7a41-4e8f-a3d6-5b2f81c4e970");
const ATMOSPHERE_SHADER: Handle<Shader> = uuid_handle!("b4e2a9d1-58c3-4f7e-9a06-3d1c7e5f2b84");

/// Deep navy behind the wireframes; realistic space is black.
pub const SPACE_BLUE: Color = Color::srgb(0.015, 0.035, 0.09);

/// Bodies below this on-screen radius, in logical pixels, are drawn as points.
pub const POINT_PIXELS: f32 = 1.5;
/// The camera's point spread: a Gaussian of this sigma in physical pixels, as
/// for galactic stars (`starfield.wgsl` `SIGMA_PX`). Bloom adds the wide glow.
pub const PSF_SIGMA_PX: f32 = 0.55;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, BODY_SHADER, "body.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, POINT_SHADER, "point.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, ATMOSPHERE_SHADER, "atmosphere.wgsl", Shader::from_wgsl);
        app.add_plugins((
            MaterialPlugin::<BodyMaterial>::default(),
            MaterialPlugin::<PointMaterial>::default(),
            MaterialPlugin::<atmosphere::AtmosphereMaterial>::default(),
            crate::render::exposure::ExposurePlugin,
            window_blur::WindowBlurPlugin,
        ))
        .init_resource::<Lights>()
        .init_resource::<Photometry>()
        .init_resource::<Frame>()
        .init_resource::<Pool<BodyMaterial>>()
        .init_resource::<Pool<PointMaterial>>()
        .init_resource::<Pool<atmosphere::AtmosphereMaterial>>()
        .add_systems(Startup, setup)
        .add_systems(Update, switch_camera)
        .add_systems(
            PostUpdate,
            (
                shade,
                sync_bodies,
                sync_points,
                atmosphere::sync_atmospheres,
            )
                .chain()
                .after(crate::scene::draw)
                .before(VisibilitySystems::VisibilityPropagate),
        );
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct BodyMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    light: Vec4,
    /// Rotation of the body's surface, as a quaternion.
    #[uniform(0)]
    spin: Vec4,
    /// Granules per disc radius, on-screen radius in pixels and granulation
    /// contrast; zero for planets.
    #[uniform(0)]
    surface: Vec4,
    /// A planet's direction to each star lighting it, and the colour each
    /// gives a surface facing it (`Shaded`).
    #[uniform(0)]
    lights: [Vec4; MAX_LIGHTS],
    #[uniform(0)]
    tints: [Vec4; MAX_LIGHTS],
}

impl Material for BodyMaterial {
    fn fragment_shader() -> ShaderRef {
        BODY_SHADER.into()
    }
}

/// Fields as `Point` (`point.wgsl`), with `axis` projected onto the sprite.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default, PartialEq)]
pub struct PointMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    shape: Vec4,
    #[uniform(0)]
    edge: Vec4,
    #[uniform(0)]
    streaks: Vec4,
    #[uniform(0)]
    axis: Vec4,
}

impl Material for PointMaterial {
    fn fragment_shader() -> ShaderRef {
        POINT_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
}

#[cfg(test)]
mod tests;
