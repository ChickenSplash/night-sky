//! Unresolved Galactic light. A far sky sphere ray-integrates the galaxy
//! model's stars too faint to be points, dimmed and reddened by its dust;
//! stars bright enough stay in `starfield`. There is no scattered light.

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
use space_simulation::{
    galaxy::{Component, GalaxyModel, PC_METRES},
    population::{GLOW_DISTANCES, flux_per_luminosity, unresolved_light},
};

const SHADER: Handle<Shader> = uuid_handle!("fd104454-3330-4f40-9ac9-bcad7da21b11");

/// Unresolved-light entries: component × distance.
const LIGHT: usize = 6 * GLOW_DISTANCES;

/// The galaxy model's parameters, as `diffuse.wgsl` documents them.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct Glow {
    #[uniform(0)]
    eye: Vec4,
    #[uniform(0)]
    nucleus: Vec4,
    #[uniform(0)]
    bar: Vec4,
    #[uniform(0)]
    bar_axis: Vec4,
    #[uniform(0)]
    thin: Vec4,
    #[uniform(0)]
    arms: Vec4,
    #[uniform(0)]
    thick: Vec4,
    #[uniform(0)]
    spiral: Vec4,
    #[uniform(0)]
    spiral_extent: Vec4,
    #[uniform(0)]
    halo: Vec4,
    #[uniform(0)]
    dust: Vec4,
    #[uniform(0)]
    dust_hole: Vec4,
    /// Central dust height, galaxy radius and display light per unit of flux.
    #[uniform(0)]
    extent: Vec4,
    #[uniform(0)]
    light: [Vec4; LIGHT],
}

impl Glow {
    fn new(model: &GalaxyModel) -> Self {
        let f = |x: f64| x as f32;
        let disc = |c: Component| {
            let d = match c {
                Component::ThinDisc => &model.thin,
                Component::Arms => &model.arms,
                _ => &model.thick,
            };
            Vec4::new(
                f(d.scale_length_pc),
                f(d.scale_height_pc),
                f(model.stars_in(c)),
                f(d.arm_strength),
            )
        };
        let tables = Component::ALL.map(|c| unresolved_light(c, crate::stream::MAGNITUDE));
        let (sin, cos) = model.bar_angle_rad.sin_cos();
        let [long, high, across] = model.bar_pc;
        let (spiral, dust) = (&model.spiral, &model.dust);
        Self {
            eye: Vec4::ZERO,
            nucleus: Vec4::new(
                f(model.nucleus_pc),
                f(model.stars_in(Component::Nucleus)),
                0.0,
                0.0,
            ),
            bar: Vec4::new(
                f(long),
                f(high),
                f(across),
                f(model.stars_in(Component::Bulge)),
            ),
            bar_axis: Vec4::new(f(cos), f(sin), 0.0, 0.0),
            thin: disc(Component::ThinDisc),
            arms: disc(Component::Arms),
            thick: disc(Component::ThickDisc),
            spiral: Vec4::new(
                spiral.arms as f32,
                f(spiral.pitch_rad.tan()),
                f(spiral.phase_rad),
                f(spiral.reference_pc),
            ),
            spiral_extent: Vec4::new(
                f(spiral.inner_pc),
                f(spiral.outer_pc),
                spiral.sharpness as f32,
                f(spiral.mean()),
            ),
            halo: Vec4::new(
                f(model.halo_core_pc),
                f(model.halo_flattening),
                f(model.radius_pc),
                f(model.stars_in(Component::Halo) / model.halo_volume()),
            ),
            dust: Vec4::new(
                f(dust.midplane_mag_per_pc),
                f(dust.scale_length_pc),
                f(dust.scale_height_pc),
                f(dust.arm_strength),
            ),
            dust_hole: Vec4::new(
                f(dust.hole_pc[0]),
                f(dust.hole_pc[1]),
                f(dust.centre_mag_per_pc),
                f(dust.centre_pc[0]),
            ),
            extent: Vec4::new(f(dust.centre_pc[1]), f(model.radius_pc), 0.0, 0.0),
            light: std::array::from_fn(|i| {
                let [r, g, b] = tables[i / GLOW_DISTANCES][i % GLOW_DISTANCES];
                Vec4::new(f(r), f(g), f(b), 0.0)
            }),
        }
    }
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
    sky_light: Option<Res<crate::sky::Sky>>,
    mut model: Local<Option<(GalaxyModel, Glow)>>,
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
    let (model, template) = model.get_or_insert_with(|| {
        let model = GalaxyModel::milky_way();
        let glow = Glow::new(&model);
        (model, glow)
    });
    let eye = DVec3::from_array(model.home_pc)
        + rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let eye_far = eye
        .as_vec3()
        .extend((eye.length() + model.radius_pc) as f32);
    // The glow shares the points' display light per unit of flux, and the
    // exposure metered on the streamed stars' sky.
    let metered = sky_light.map_or(1.0, |sky| sky.exposure);
    let gain = (crate::sky::POINT_LIGHT * flux_per_luminosity()) as f32
        * sky_dimming(&nav, &lights)
        * metered;
    if sphere.is_none() {
        let mut glow = template.clone();
        glow.eye = eye_far;
        glow.extent.z = gain;
        let material = materials.add(glow);
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
        .is_some_and(|glow| glow.eye != eye_far || glow.extent.z != gain)
        && let Some(glow) = materials.get_mut(&sky.material)
    {
        glow.eye = eye_far;
        glow.extent.z = gain;
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
