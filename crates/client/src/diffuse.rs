//! Faint unresolved Galactic light. A far sky sphere ray-integrates a smooth
//! disc and spiral-arm emissivity; individual visible stars stay in `starfield`.
//! This is an approximate glow, not dust extinction or a resolved star count.

use crate::{
    Navigation,
    render::{Lights, sky_dimming},
    scene::{CameraPose, CameraRig, SpaceCamera},
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    camera::visibility::NoFrustumCulling,
    math::DVec3,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshMaterial3d},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use big_space::prelude::*;
use space_simulation::galaxy::{GalaxyModel, PC_METRES};

const SHADER: Handle<Shader> = uuid_handle!("fd104454-3330-4f40-9ac9-bcad7da21b11");

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Glow {
    #[uniform(0)]
    eye: Vec4,
    /// Radial disc scale, thin height, arm height and outer edge in parsecs.
    #[uniform(0)]
    disc: Vec4,
    /// Spiral pitch, reference radius, phase and assisted display multiplier.
    #[uniform(0)]
    arm: Vec4,
}

impl Material for Glow {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The camera sits inside the sky sphere.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Resource)]
struct SkySphere {
    entity: Entity,
    black_hole: Entity,
    material: Handle<Glow>,
}

pub struct DiffusePlugin;
impl Plugin for DiffusePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "diffuse.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<Glow>::default())
            .add_systems(
                PostUpdate,
                sync.after(crate::scene::draw)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    mut sphere: Option<ResMut<SkySphere>>,
    nav: Res<Navigation>,
    lights: Res<Lights>,
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<Glow>>,
    mut opaque: ResMut<Assets<StandardMaterial>>,
    camera: Query<(&CellCoord, &ChildOf, &GlobalTransform), With<SpaceCamera>>,
    mut placement: Query<
        (
            &mut Transform,
            &mut GlobalTransform,
            &mut CellCoord,
            &mut Visibility,
        ),
        Without<SpaceCamera>,
    >,
) {
    let Ok((cell, parent, camera_transform)) = camera.single() else {
        return;
    };
    let model = GalaxyModel::milky_way();
    let eye = DVec3::from_array(model.home_pc)
        + rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let gain = if nav.realistic { 1.0 } else { 0.3 } * sky_dimming(&nav, &lights);
    let make = || Glow {
        eye: eye.as_vec3().extend(0.0),
        disc: Vec4::new(
            model.thin.scale_length_pc as f32,
            model.thin.scale_height_pc as f32,
            model.arms.scale_height_pc as f32,
            model.radius_pc as f32,
        ),
        arm: Vec4::new(
            model.spiral.pitch_rad as f32,
            model.spiral.reference_pc as f32,
            model.spiral.phase_rad as f32,
            gain,
        ),
    };
    if sphere.is_none() {
        let material = materials.add(make());
        let mesh = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
        commands
            .entity(parent.parent())
            .insert_if_new(Visibility::default());
        let entity = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                GlobalTransform::default(),
                *cell,
                Visibility::Hidden,
                NoFrustumCulling,
                ChildOf(parent.parent()),
            ))
            .id();
        let black_hole = commands
            .spawn((
                Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
                MeshMaterial3d(opaque.add(StandardMaterial {
                    base_color: Color::BLACK,
                    unlit: true,
                    ..default()
                })),
                Transform::default(),
                GlobalTransform::default(),
                *cell,
                Visibility::Hidden,
                NoFrustumCulling,
                ChildOf(parent.parent()),
            ))
            .id();
        commands.insert_resource(SkySphere {
            entity,
            black_hole,
            material,
        });
        return;
    }
    let sky = sphere.as_mut().expect("sphere exists");
    if materials
        .get(&sky.material)
        .is_some_and(|glow| glow.eye.xyz() != eye.as_vec3() || glow.arm.w != gain)
        && let Some(glow) = materials.get_mut(&sky.material)
    {
        glow.eye = eye.as_vec3().extend(0.0);
        glow.arm.w = gain;
    }
    if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
        placement.get_mut(sky.entity)
    {
        *grid_cell = *cell;
        *transform = Transform::from_translation(camera_transform.translation())
            .with_scale(Vec3::splat(1e18));
        *global = GlobalTransform::from(*transform);
        *visibility = Visibility::Inherited;
    }
    if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
        placement.get_mut(sky.black_hole)
    {
        *grid_cell = *cell;
        let delta = -eye * (PC_METRES / pose.render_scale());
        *transform = Transform::from_translation(camera_transform.translation() + delta.as_vec3())
            .with_scale(Vec3::splat(
                (model.black_hole_radius_metres() / pose.render_scale()) as f32,
            ));
        *global = GlobalTransform::from(*transform);
        *visibility = Visibility::Inherited;
    }
}
