//! Batched, camera-relative galactic points. A mesh contains many stars, not
//! one entity or material per star. Meshes are uploaded in bounded chunks.

use crate::{
    Navigation,
    render::{Lights, sky_dimming},
    scene::{CameraPose, CameraRig, SpaceCamera},
    stream::Starfield,
};
use bevy::{
    asset::{RenderAssetUsages, load_internal_asset, uuid_handle},
    camera::visibility::NoFrustumCulling,
    math::DVec3,
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshMaterial3d},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use big_space::prelude::*;
use space_simulation::{
    cells::Cell,
    galaxy::PC_METRES,
    population::{reach_pc, tier_limit},
    stream::StarPoint,
};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

const SHADER: Handle<Shader> = uuid_handle!("d12d0ce2-812e-4260-a528-74d3af35d480");
const BATCH: usize = 4096;
const UPLOAD_BUDGET: Duration = Duration::from_millis(3);
const MARGIN: f64 = 1.25;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct StarMaterial {
    /// Parsecs per render unit, apparent-magnitude cutoff and display gain.
    #[uniform(0)]
    settings: Vec4,
}

impl Material for StarMaterial {
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
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(2),
        ])?];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Resource, Default)]
struct Batches {
    active: HashMap<Cell, Vec<(Entity, Handle<Mesh>)>>,
    queued: VecDeque<(Cell, usize)>,
    built_at: HashMap<Cell, DVec3>,
    material: Option<Handle<StarMaterial>>,
}

pub struct StarfieldRenderPlugin;

impl Plugin for StarfieldRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "starfield.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<StarMaterial>::default())
            .init_resource::<Batches>()
            .add_systems(
                PostUpdate,
                sync.after(crate::scene::draw)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// The same star at any distance has a stable vertex position within its cell;
/// camera and zoom move its mesh, not a million individual vertices.
fn mesh(points: &[StarPoint], centre: [f64; 3], eye: DVec3, tier: usize) -> (Mesh, usize) {
    let mut positions = Vec::with_capacity(points.len() * 3);
    let mut corners = Vec::with_capacity(points.len() * 3);
    let mut colours = Vec::with_capacity(points.len() * 3);
    let mut visible = 0;
    for star in points {
        let limit = MARGIN
            * reach_pc(tier, crate::stream::MAGNITUDE)
            * (f64::from(star.luminosity_solar) / tier_limit(tier)).sqrt();
        if DVec3::from_array(star.position_pc).distance_squared(eye) > limit * limit {
            continue;
        }
        visible += 1;
        let pos: [f32; 3] =
            std::array::from_fn(|axis| (star.position_pc[axis] - centre[axis]) as f32);
        let rgb = space_simulation::spectral_colour(f64::from(star.temperature_k));
        let colour = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
        let tint = [colour.red, colour.green, colour.blue, star.luminosity_solar];
        for corner in [[-1.0, -1.0], [3.0, -1.0], [-1.0, 3.0]] {
            positions.push(pos);
            corners.push(corner);
            colours.push(tint);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, corners);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    (mesh, visible)
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    mut field: ResMut<Starfield>,
    mut batches: ResMut<Batches>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StarMaterial>>,
    nav: Res<Navigation>,
    lights: Res<Lights>,
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    camera: Query<(&CellCoord, &ChildOf, &GlobalTransform), With<SpaceCamera>>,
    mut placements: Query<
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
    let pc_per_unit = pose.render_scale() / PC_METRES;
    let material = batches
        .material
        .get_or_insert_with(|| {
            materials.add(StarMaterial {
                settings: Vec4::new(
                    pc_per_unit as f32,
                    crate::stream::MAGNITUDE as f32,
                    12.0,
                    sky_dimming(&nav, &lights),
                ),
            })
        })
        .clone();
    let scale = pc_per_unit as f32;
    let dimming = sky_dimming(&nav, &lights);
    if materials
        .get(&material)
        .is_some_and(|m| m.settings.x != scale || m.settings.w != dimming)
        && let Some(m) = materials.get_mut(&material)
    {
        m.settings.x = scale;
        m.settings.w = dimming;
    }
    let origin = rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let eye = origin + DVec3::from_array(home);
    for dropped in field.removed.drain(..) {
        batches.built_at.remove(&dropped);
        if let Some(entities) = batches.active.remove(&dropped) {
            for (entity, handle) in entities {
                commands.entity(entity).despawn();
                meshes.remove(&handle);
            }
        }
        batches.queued.retain(|(cell, _)| *cell != dropped);
    }
    for added in field.added.drain(..) {
        if !batches.queued.iter().any(|(cell, _)| *cell == added) {
            batches.queued.push_back((added, 0));
        }
    }
    let moved: Vec<Cell> = batches
        .built_at
        .iter()
        .filter_map(|(&owner, &built)| {
            let threshold = 0.2 * reach_pc(owner.level(), crate::stream::MAGNITUDE);
            (eye.distance(built) > threshold && !batches.queued.iter().any(|(c, _)| *c == owner))
                .then_some(owner)
        })
        .collect();
    batches
        .queued
        .extend(moved.into_iter().map(|cell| (cell, 0)));
    // A few thousand stars per frame: never turn one large cell into a frame stall.
    let start = bevy::platform::time::Instant::now();
    while let Some((owner, offset)) = batches.queued.pop_front() {
        let Some(stars) = field.cells.get(&owner).map(|c| &c.stars) else {
            continue;
        };
        if offset >= stars.len() {
            continue;
        }
        let end = (offset + BATCH).min(stars.len());
        let centre = owner.centre_pc();
        if offset == 0 {
            if let Some(old) = batches.active.remove(&owner) {
                for (entity, handle) in old {
                    commands.entity(entity).despawn();
                    meshes.remove(&handle);
                }
            }
            batches.built_at.insert(owner, eye);
        }
        let (geometry, visible) = mesh(&stars[offset..end], centre, eye, owner.level());
        if visible > 0 {
            let handle = meshes.add(geometry);
            commands
                .entity(parent.parent())
                .insert_if_new(Visibility::default());
            let entity = commands
                .spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    GlobalTransform::default(),
                    *cell,
                    Visibility::Hidden,
                    NoFrustumCulling,
                    ChildOf(parent.parent()),
                ))
                .id();
            batches
                .active
                .entry(owner)
                .or_default()
                .push((entity, handle));
        }
        if end < stars.len() {
            batches.queued.push_front((owner, end));
        }
        if start.elapsed() >= UPLOAD_BUDGET {
            break;
        }
    }
    // Camera-relative subtraction in f64 *before* conversion to GPU floats.
    let factor = PC_METRES / pose.render_scale();
    for (&owner, entities) in &batches.active {
        let delta = (DVec3::from_array(owner.centre_pc()) - eye) * factor;
        let centre = camera_transform.translation() + delta.as_vec3();
        for &(entity, _) in entities {
            if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
                placements.get_mut(entity)
            {
                *grid_cell = *cell;
                *transform =
                    Transform::from_translation(centre).with_scale(Vec3::splat(factor as f32));
                *global = GlobalTransform::from(*transform);
                *visibility = Visibility::Inherited;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_only_upload_stars_near_enough_to_see() {
        let tier = 12;
        let eye = DVec3::new(8200.0, 20.0, 0.0);
        let point = |distance: f64| StarPoint {
            position_pc: [8200.0 + distance, 20.0, 0.0],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let (geometry, count) = mesh(&[point(10.0), point(100.0)], [8200.0, 0.0, 0.0], eye, tier);
        assert_eq!(count, 1);
        assert_eq!(geometry.count_vertices(), 3);
        let (geometry, count) = mesh(&[point(100.0)], [8200.0, 0.0, 0.0], eye, tier);
        assert_eq!(count, 0);
        assert_eq!(geometry.count_vertices(), 0);
    }
}
