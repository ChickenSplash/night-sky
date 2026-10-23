//! Planetary atmospheres: a scattering shell around a planet's disc
//! (`atmosphere.wgsl`). The shell mesh only chooses which pixels run the
//! shader; the shader finds the ground and the top of the air itself.
use super::{
    MAX_LIGHTS,
    bodies::{Meshes, Pool, SpaceCameraCell, grow, hide, place},
    resolve::Frame,
};
use crate::scene::SpaceCamera;
use bevy::{
    math::DVec3,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

/// The shell mesh's radius over the top of the air: the sphere mesh's facets
/// lie slightly inside the sphere they approximate.
const MESH_MARGIN: f32 = 1.01;
/// Within this many near-plane distances of the shell mesh, the camera draws
/// it from inside: the near plane would otherwise cut a hole in the side
/// below. Generous, as oblique points are clipped closer than their distance.
const NEAR_MARGIN: f32 = 10.0;
/// Steps along a pixel's line of sight, and from each towards every star.
const VIEW_STEPS: f32 = 16.0;
const LIGHT_STEPS: f32 = 8.0;

/// A planet's air, physical: coefficients per metre, heights in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// Height of the top of the air, above which it is taken as empty.
    pub top: f64,
    /// Rayleigh scattering by gas at the ground, per channel, and its scale height.
    pub rayleigh: DVec3,
    pub rayleigh_height: f64,
    /// Mie scattering and grey absorption by haze at the ground, its scale
    /// height and its phase asymmetry (Cornette-Shanks `g`).
    pub mie: DVec3,
    pub mie_absorption: f64,
    pub mie_height: f64,
    pub mie_asymmetry: f64,
    /// Absorption peaking in a layer, such as ozone, falling linearly to
    /// zero `layer_width` either side of `layer_height`.
    pub absorption: DVec3,
    pub layer_height: f64,
    pub layer_width: f64,
}

impl Atmosphere {
    /// Earth's, as in Hillaire (2020): Rayleigh scattering for 680, 550 and
    /// 440 nm, a continental haze and the ozone layer.
    pub const EARTH: Self = Self {
        top: 100e3,
        rayleigh: DVec3::new(5.802e-6, 13.558e-6, 33.1e-6),
        rayleigh_height: 8e3,
        mie: DVec3::splat(3.996e-6),
        mie_absorption: 0.444e-6,
        mie_height: 1.2e3,
        mie_asymmetry: 0.8,
        absorption: DVec3::new(0.650e-6, 1.881e-6, 0.085e-6),
        layer_height: 25e3,
        layer_width: 15e3,
    };

    /// The air in units of a planet's radius, as the shader takes it.
    pub fn scaled(&self, radius: f64) -> Air {
        let per = |v: DVec3| (v * radius).as_vec3();
        let over = |h: f64| (h / radius) as f32;
        Air {
            top: 1.0 + over(self.top),
            rayleigh: per(self.rayleigh).extend(over(self.rayleigh_height)),
            mie: per(self.mie).extend((self.mie_absorption * radius) as f32),
            shape: Vec4::new(
                over(self.mie_height),
                self.mie_asymmetry as f32,
                over(self.layer_height),
                over(self.layer_width),
            ),
            absorption: per(self.absorption),
        }
    }
}

/// An atmosphere in units of its planet's radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Air {
    /// Radius of the top of the air.
    pub top: f32,
    /// Rayleigh scattering, and its scale height.
    pub rayleigh: Vec4,
    /// Mie scattering, and Mie absorption.
    pub mie: Vec4,
    /// Mie scale height and asymmetry, absorbing layer height and half width.
    pub shape: Vec4,
    pub absorption: Vec3,
}

/// A planet's atmosphere ready for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shell {
    pub centre: Vec3,
    /// The planet's radius, in render units.
    pub radius: f32,
    pub air: Air,
    /// Direction to each star lighting the planet, and the exposed colour of
    /// its flux; zero when unused.
    pub lights: [Vec4; MAX_LIGHTS],
    pub suns: [Vec4; MAX_LIGHTS],
}

#[derive(Component, Clone, Copy)]
pub(super) struct AtmosphereShell;

#[derive(Asset, TypePath, AsBindGroup, Clone, Default, PartialEq)]
#[bind_group_data(AtmosphereKey)]
pub struct AtmosphereMaterial {
    /// The planet's centre from the camera, in render units, and its radius.
    #[uniform(0)]
    centre: Vec4,
    /// Top of the air in radii, and view and light steps.
    #[uniform(0)]
    steps: Vec4,
    #[uniform(0)]
    rayleigh: Vec4,
    #[uniform(0)]
    mie: Vec4,
    #[uniform(0)]
    shape: Vec4,
    #[uniform(0)]
    absorption: Vec4,
    #[uniform(0)]
    lights: [Vec4; MAX_LIGHTS],
    #[uniform(0)]
    suns: [Vec4; MAX_LIGHTS],
    /// The camera is inside the shell mesh, or so close that the near plane
    /// would cut its near side: its far side is drawn instead, over everything.
    inside: bool,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct AtmosphereKey {
    inside: bool,
}

impl From<&AtmosphereMaterial> for AtmosphereKey {
    fn from(material: &AtmosphereMaterial) -> Self {
        Self {
            inside: material.inside,
        }
    }
}

impl Material for AtmosphereMaterial {
    fn fragment_shader() -> ShaderRef {
        super::ATMOSPHERE_SHADER.into()
    }

    /// Scattered light is added; what lies behind is dimmed by the
    /// transmittance, carried in alpha.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Sorted at the shell's near side, not its centre, so its planet's limb
    /// sprites, which share the centre, always draw first, beneath it.
    fn depth_bias(&self) -> f32 {
        self.centre.w * self.steps.x * MESH_MARGIN
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.inside {
            descriptor.primitive.cull_mode = Some(Face::Front);
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_compare = CompareFunction::Always;
            }
        }
        Ok(())
    }
}

impl Shell {
    /// Radius of the shell mesh, in render units.
    fn extent(&self) -> f32 {
        self.radius * self.air.top * MESH_MARGIN
    }

    /// `near` is the camera's near plane, in render units.
    fn material(&self, eye: Vec3, near: f32) -> AtmosphereMaterial {
        let from_eye = self.centre - eye;
        AtmosphereMaterial {
            centre: from_eye.extend(self.radius),
            steps: Vec4::new(self.air.top, VIEW_STEPS, LIGHT_STEPS, 0.0),
            rayleigh: self.air.rayleigh,
            mie: self.air.mie,
            shape: self.air.shape,
            absorption: self.air.absorption.extend(0.0),
            lights: self.lights,
            suns: self.suns,
            inside: from_eye.length() < self.extent() + NEAR_MARGIN * near,
        }
    }
}

pub(super) fn sync_atmospheres(
    mut commands: Commands,
    frame: Res<Frame>,
    meshes: Res<Meshes>,
    debug: Res<super::debug::RenderDebug>,
    (mut pool, mut materials): (
        ResMut<Pool<AtmosphereMaterial>>,
        ResMut<Assets<AtmosphereMaterial>>,
    ),
    camera: Query<(SpaceCameraCell, &Projection), With<SpaceCamera>>,
    mut shells: Query<super::bodies::Placement, (With<AtmosphereShell>, Without<SpaceCamera>)>,
) {
    let Ok(((cell, parent, view), projection)) = camera.single() else {
        return;
    };
    let near = match projection {
        Projection::Perspective(p) => p.near,
        _ => 0.0,
    };
    let shown: &[Shell] = if debug.atmospheres {
        &frame.shells
    } else {
        &[]
    };
    grow(
        &mut commands,
        &mut pool,
        &mut materials,
        &meshes.sphere,
        shown.len(),
        (cell, parent),
        AtmosphereShell,
    );
    let eye = view.translation();
    for (index, (entity, material)) in pool.0.iter().enumerate() {
        let Ok((transform, global, shell_cell, visibility)) = shells.get_mut(*entity) else {
            continue;
        };
        let Some(shell) = shown.get(index) else {
            hide(visibility);
            continue;
        };
        place(
            (transform, global, shell_cell, visibility),
            *cell,
            Transform::from_translation(shell.centre).with_scale(Vec3::splat(shell.extent())),
        );
        let changed = shell.material(eye, near);
        if materials.get(material).is_some_and(|m| *m != changed)
            && let Some(m) = materials.get_mut(material)
        {
            *m = changed;
        }
    }
}
