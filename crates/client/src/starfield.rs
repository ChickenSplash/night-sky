//! Batched, camera-relative galactic points. A mesh contains many stars, not
//! one entity or material per star. Meshes are uploaded in bounded chunks.

use crate::{
    Navigation, Universe,
    render::Photometry,
    render_debug::RenderDebug,
    scene::{CameraPose, CameraRig, SpaceCamera},
    sky::{Sky, radiance_gain},
    stream::{Starfield, StreamMetrics},
};
use bevy::{
    asset::{RenderAssetUsages, load_internal_asset, uuid_handle},
    camera::visibility::NoFrustumCulling,
    math::DVec3,
    mesh::{MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshMaterial3d},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use big_space::prelude::*;
use space_simulation::{
    cells::Cell,
    galaxy::PC_METRES,
    light::visual_efficiency,
    population::{reach_pc, reach_sq_per_luminosity},
    stream::StarPoint,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

const SHADER: Handle<Shader> = uuid_handle!("d12d0ce2-812e-4260-a528-74d3af35d480");
const BATCH: usize = 4096;
const UPLOAD_BUDGET: Duration = Duration::from_millis(3);
const MARGIN: f64 = 1.25;
/// A point closer than this many times its offset from its cell's centre gets
/// a precise sprite. Batch vertices are f32 offsets from that centre, so their
/// error grows with the offset; beyond here it stays below a quarter pixel.
const PRECISE: f64 = 2e-3;
/// Keep the batched point while its precise replacement becomes visible. The
/// former is accurate to around a pixel at this cutoff; without overlap an
/// unloaded/hidden sprite leaves a hole light-years from a bright star.
const BATCH_HIDE: f64 = PRECISE * 0.25;
/// Precise sprites are built this many times farther out than they show, so
/// each has passed its hidden first frame before it takes over.
const PREPARE: f64 = 2.0;
/// Camera travel, in parsecs, before precise candidates are gathered again.
const RESCAN_PC: f64 = 0.1;
/// Realistic colours, log-spaced in temperature from 1,000 to 50,000 K.
const PALETTE: usize = 32;

/// Each temperature's colour with unit luminance, then its visual efficiency:
/// light per bolometric luminosity, as the eye and the exposure see it.
#[derive(Clone, ShaderType)]
struct Palette {
    colours: [Vec4; PALETTE],
}

impl Palette {
    fn new(physical: bool) -> Self {
        let photometry = Photometry::default();
        Self {
            colours: std::array::from_fn(|i| {
                let kelvin = 1000.0 * 50f64.powf(i as f64 / (PALETTE - 1) as f64);
                let colour = photometry.colour(kelvin, physical);
                let efficiency = visual_efficiency(kelvin);
                let [r, g, b] = colour.map(|c| c as f32);
                Vec4::new(r, g, b, efficiency as f32)
            }),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct StarMaterial {
    /// Parsecs per render unit, faintest visible luminosity per squared
    /// parsec, display light per luminosity per squared parsec, and one for
    /// realistic colours and visual brightness from `palette`.
    #[uniform(0)]
    settings: Vec4,
    /// For batches, `PRECISE` and the visited star's body range in parsecs:
    /// points that close are drawn alone, or as the body. Zero for precise sprites.
    /// Then debug switches (`RenderDebug`): one to skip rasterising points,
    /// and the footprint radius in physical pixels.
    #[uniform(1)]
    near: Vec4,
    #[uniform(2)]
    palette: Palette,
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

/// One uploaded batch.
struct Batch {
    entity: Entity,
    mesh: Handle<Mesh>,
    /// Stars uploaded, three vertices each.
    stars: usize,
}

#[derive(Resource, Default)]
struct Batches {
    active: HashMap<Cell, Vec<Batch>>,
    queued: VecDeque<(Cell, usize)>,
    built_at: HashMap<Cell, DVec3>,
    material: Option<Handle<StarMaterial>>,
    /// Whether the palette is the physical one.
    physical: bool,
}

/// A star that may come close enough to need a precise sprite.
struct Candidate {
    id: String,
    star: StarPoint,
    /// Parsecs from its cell's centre.
    offset_pc: f64,
}

/// Stars near the camera, each drawn alone at an f64 camera-relative position.
#[derive(Resource, Default)]
struct Precise {
    material: Option<Handle<StarMaterial>>,
    scanned_at: Option<DVec3>,
    scanned_body: f64,
    candidates: Vec<Candidate>,
    sprites: HashMap<String, (Entity, Handle<Mesh>)>,
}

pub struct StarfieldRenderPlugin;

impl Plugin for StarfieldRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "starfield.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<StarMaterial>::default())
            .init_resource::<Batches>()
            .init_resource::<Precise>()
            .add_systems(
                PostUpdate,
                sync.after(crate::sky::expose)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// Stars that could need a precise sprite before the camera moves `RESCAN_PC`.
/// `body_pc` is the visited star's body range, within which batches hide points.
fn candidates(field: &Starfield, eye: DVec3, body_pc: f64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (&cell, stars) in &field.cells {
        let half_diagonal = cell.size_pc() * 0.5 * 3.0_f64.sqrt();
        let reach = PREPARE * (PRECISE * half_diagonal).max(body_pc) + RESCAN_PC;
        let centre = DVec3::from_array(cell.centre_pc());
        stars.near(eye.to_array(), reach, |star| {
            let position = DVec3::from_array(star.position_pc);
            let offset_pc = position.distance(centre);
            let reach = PREPARE * (PRECISE * offset_pc).max(body_pc) + RESCAN_PC;
            if position.distance(eye) < reach {
                out.push(Candidate {
                    id: star.id(cell),
                    star: *star,
                    offset_pc,
                });
            }
        });
    }
    out
}

/// The same star at any distance has a stable vertex position within its cell;
/// camera and zoom move its mesh, not a million individual vertices.
fn mesh(points: &[StarPoint], centre: [f64; 3], eye: DVec3) -> (Mesh, usize) {
    let mut positions = Vec::with_capacity(points.len() * 3);
    let mut corners = Vec::with_capacity(points.len() * 3);
    let mut colours = Vec::with_capacity(points.len() * 3);
    let mut visible = 0;
    let reach = MARGIN * MARGIN * reach_sq_per_luminosity(crate::stream::MAGNITUDE);
    for star in points {
        let offset = DVec3::from_array(star.position_pc) - eye;
        let luminosity = f64::from(star.luminosity_solar);
        let squared = offset.length_squared();
        if squared > luminosity * reach {
            continue;
        }
        visible += 1;
        let pos: [f32; 3] =
            std::array::from_fn(|axis| (star.position_pc[axis] - centre[axis]) as f32);
        let rgb = space_simulation::spectral_colour(f64::from(star.temperature_k));
        let colour = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
        let tint = [colour.red, colour.green, colour.blue, star.luminosity_solar];
        // The shader places each corner by its index, which shares the
        // attribute with the temperature for realistic colours.
        for corner in 0..3 {
            positions.push(pos);
            corners.push([corner as f32, star.temperature_k]);
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
    mut metrics: ResMut<StreamMetrics>,
    mut batches: ResMut<Batches>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut precise: ResMut<Precise>,
    sky: Res<Sky>,
    universe: Res<Universe>,
    nav: Res<Navigation>,
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    debug: Res<RenderDebug>,
    camera: Query<(&CellCoord, &ChildOf, &GlobalTransform, &Camera), With<SpaceCamera>>,
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
    let sync_start = bevy::platform::time::Instant::now();
    let Ok((cell, parent, camera_transform, view)) = camera.single() else {
        return;
    };
    let pc_per_unit = pose.render_scale() / PC_METRES;
    let scale = pc_per_unit as f32;
    let size = view
        .physical_viewport_size()
        .unwrap_or(UVec2::ONE)
        .max(UVec2::ONE);
    let pixel = 2.0 * nav.tan_half_fov() / f64::from(size.y);
    let pixel_sr = pixel * pixel;
    // A point's light over its pixels: radiance gain over a pixel's solid angle.
    let gain = (radiance_gain(&nav, &sky, pixel_sr) / pixel_sr) as f32;
    let realistic = if nav.realistic { 1.0 } else { 0.0 };
    if batches.physical != nav.physical {
        batches.physical = nav.physical;
        for handle in batches.material.iter().chain(&precise.material) {
            if let Some(m) = materials.get_mut(handle) {
                m.palette = Palette::new(nav.physical);
            }
        }
    }
    // Within its body range the visited star is drawn as a body, so the batches
    // hide its point there, and it has no sprite.
    let body = nav.system(&universe.0).and_then(|system| {
        let range = crate::scene::body_range(&nav, system, &pose)?;
        Some((system.page_star(pose.page)?.id.as_str(), range / PC_METRES))
    });
    let body_pc = body.map_or(0.0, |(_, range)| range);
    let skip_pixels = if debug.point_pixels { 0.0 } else { 1.0 };
    let radius_px = debug.point_radius_px();
    let mut star_material = |precise: f32| {
        materials.add(StarMaterial {
            settings: Vec4::new(
                scale,
                (1.0 / reach_sq_per_luminosity(crate::stream::MAGNITUDE)) as f32,
                gain,
                realistic,
            ),
            near: Vec4::new(precise, 0.0, skip_pixels, radius_px),
            palette: Palette::new(nav.physical),
        })
    };
    let material = batches
        .material
        .get_or_insert_with(|| star_material(BATCH_HIDE as f32))
        .clone();
    let precise_material = precise
        .material
        .get_or_insert_with(|| star_material(0.0))
        .clone();
    for (handle, hide) in [(&material, body_pc as f32), (&precise_material, 0.0)] {
        if materials.get(handle).is_some_and(|m| {
            m.settings.x != scale
                || m.settings.z != gain
                || m.settings.w != realistic
                || m.near.y != hide
                || m.near.z != skip_pixels
                || m.near.w != radius_px
        }) && let Some(m) = materials.get_mut(handle)
        {
            m.settings.x = scale;
            m.settings.z = gain;
            m.settings.w = realistic;
            m.near.y = hide;
            m.near.z = skip_pixels;
            m.near.w = radius_px;
        }
    }
    let origin = rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let eye = origin + DVec3::from_array(home);
    let changed = !field.added.is_empty() || !field.removed.is_empty();
    for dropped in field.removed.drain(..) {
        metrics.loaded_at.remove(&dropped);
        batches.built_at.remove(&dropped);
        if let Some(old) = batches.active.remove(&dropped) {
            for batch in old {
                commands.entity(batch.entity).despawn();
                meshes.remove(&batch.mesh);
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
                for batch in old {
                    commands.entity(batch.entity).despawn();
                    meshes.remove(&batch.mesh);
                }
            }
            batches.built_at.insert(owner, eye);
        }
        let (geometry, visible) = mesh(&stars[offset..end], centre, eye);
        metrics.built_batches += 1;
        if offset == 0
            && let Some(at) = metrics.loaded_at.remove(&owner)
        {
            metrics.mesh_wait_ms += at.elapsed().as_secs_f64() * 1000.0;
            metrics.mesh_wait_samples += 1;
        }
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
            batches.active.entry(owner).or_default().push(Batch {
                entity,
                mesh: handle,
                stars: visible,
            });
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
    let shown = if debug.galactic_points {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let (mut draws, mut uploaded) = (0, 0);
    for (&owner, entities) in &batches.active {
        let delta = (DVec3::from_array(owner.centre_pc()) - eye) * factor;
        let centre = camera_transform.translation() + delta.as_vec3();
        for &Batch { entity, stars, .. } in entities {
            if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
                placements.get_mut(entity)
            {
                *grid_cell = *cell;
                *transform =
                    Transform::from_translation(centre).with_scale(Vec3::splat(factor as f32));
                *global = GlobalTransform::from(*transform);
                *visibility = shown;
                draws += 1;
                uploaded += stars;
            }
        }
    }
    metrics.batch_draws = if debug.galactic_points { draws } else { 0 };
    metrics.uploaded_stars = uploaded;
    // Close up, each star is placed alone by an f64 subtraction. The visited
    // star's sprite waits, hidden, while its body is drawn at the exact position.
    if changed
        || precise.scanned_body != body_pc
        || precise
            .scanned_at
            .is_none_or(|at| at.distance(eye) > RESCAN_PC)
    {
        precise.candidates = candidates(&field, eye, body_pc);
        precise.scanned_at = Some(eye);
        precise.scanned_body = body_pc;
    }
    let Precise {
        candidates,
        sprites,
        ..
    } = &mut *precise;
    let mut wanted = HashSet::new();
    let mut unready_precise = false;
    for candidate in candidates.iter() {
        let position = DVec3::from_array(candidate.star.position_pc);
        let distance = position.distance(eye);
        let radius = (PRECISE * candidate.offset_pc).max(body_pc);
        if distance >= PREPARE * radius {
            continue;
        }
        wanted.insert(candidate.id.as_str());
        let Some(&(entity, _)) = sprites.get(&candidate.id) else {
            if distance < BATCH_HIDE * candidate.offset_pc {
                unready_precise = true;
            }
            let (geometry, visible) = mesh(&[candidate.star], candidate.star.position_pc, eye);
            if visible > 0 {
                let handle = meshes.add(geometry);
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(precise_material.clone()),
                        Transform::default(),
                        GlobalTransform::default(),
                        *cell,
                        Visibility::Hidden,
                        NoFrustumCulling,
                        ChildOf(parent.parent()),
                    ))
                    .id();
                sprites.insert(candidate.id.clone(), (entity, handle));
            }
            continue;
        };
        if let Ok((mut transform, mut global, mut grid_cell, mut visibility)) =
            placements.get_mut(entity)
        {
            let centre = camera_transform.translation() + ((position - eye) * factor).as_vec3();
            *grid_cell = *cell;
            *transform = Transform::from_translation(centre).with_scale(Vec3::splat(factor as f32));
            *global = GlobalTransform::from(*transform);
            let drawn = body.is_some_and(|(id, _)| id == candidate.id);
            *visibility = if distance < radius && !drawn && debug.galactic_points {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        } else if distance < BATCH_HIDE * candidate.offset_pc {
            // The first frame after spawning a sprite has no placement yet.
            // Do not remove its batch representation before it is ready.
            unready_precise = true;
        }
    }
    if let Some(batch) = materials.get_mut(&material) {
        batch.near.x = if unready_precise {
            0.0
        } else {
            BATCH_HIDE as f32
        };
    }
    sprites.retain(|id, (entity, handle)| {
        let keep = wanted.contains(id.as_str());
        if !keep {
            commands.entity(*entity).despawn();
            meshes.remove(&*handle);
        }
        keep
    });
    metrics.precise_sprites = sprites.len();
    metrics.mesh_queue = batches.queued.len();
    metrics.oldest_mesh_ms = batches
        .queued
        .iter()
        .filter_map(|(cell, _)| metrics.loaded_at.get(cell))
        .map(|at| at.elapsed().as_secs_f64() * 1000.0)
        .fold(0.0, f64::max);
    metrics.rendering += sync_start.elapsed();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_only_upload_stars_near_enough_to_see() {
        let eye = DVec3::new(8200.0, 20.0, 0.0);
        let point = |distance: f64| StarPoint {
            position_pc: [8200.0 + distance, 20.0, 0.0],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let (geometry, count) = mesh(&[point(10.0), point(100.0)], [8200.0, 0.0, 0.0], eye);
        assert_eq!(count, 1);
        assert_eq!(geometry.count_vertices(), 3);
        let (geometry, count) = mesh(&[point(100.0)], [8200.0, 0.0, 0.0], eye);
        assert_eq!(count, 0);
        assert_eq!(geometry.count_vertices(), 0);
    }

    /// Repeats the GPU's f32 placement of a batched point: its f32 offset
    /// from the cell centre, scaled, plus the centre's camera-relative offset.
    #[test]
    fn batched_points_stay_within_a_quarter_pixel_outside_the_precise_radius() {
        let quarter_pixel = 0.25 * 2.0 * 25.0_f64.to_radians().tan() / 1000.0;
        let centre = DVec3::new(8192.0, 64.0, -64.0);
        for offset_pc in [0.5, 60.0, 110.0, 8_000.0, 110_000.0] {
            let star = centre + DVec3::new(0.577, -0.614, 0.538).normalize() * offset_pc;
            let away = DVec3::new(-0.3, 0.8, 0.52).normalize();
            let eye = star + away * PRECISE * offset_pc * 1.001;
            let exact_pc = star - eye;
            // From framing the star itself down to a planet-scale zoom.
            for zoom in [1.0, 1e-3, 1e-6] {
                let factor = 1000.0 / (exact_pc.length() * zoom);
                let vertex = (star - centre).as_vec3();
                let world = vertex * factor as f32 + ((centre - eye) * factor).as_vec3();
                let exact = exact_pc * factor;
                let angle = (world.as_dvec3() - exact).length() / exact.length();
                assert!(
                    angle < quarter_pixel,
                    "{offset_pc} pc, zoom {zoom}: {angle:e}"
                );
            }
        }
    }

    #[test]
    fn stars_near_the_camera_become_precise_candidates() {
        let cell = Cell::containing(10, [8200.0, 20.0, 0.0]).unwrap();
        let centre = DVec3::from_array(cell.centre_pc());
        let point = |position: DVec3, index| StarPoint {
            position_pc: position.to_array(),
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index,
        };
        let star = centre + DVec3::new(40.0, 0.0, 0.0);
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            space_simulation::stream::StarCell::new(cell, vec![point(star, 0), point(centre, 1)]),
        );
        let near = candidates(&field, star + DVec3::new(0.0, 0.05, 0.0), 0.0);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].id, point(star, 0).id(cell));
        assert!((near[0].offset_pc - 40.0).abs() < 1e-9);
        // Beyond the prepared radius and the rescan margin, nothing is gathered.
        let far = star + DVec3::new(0.0, PREPARE * PRECISE * 40.0 + RESCAN_PC + 0.01, 0.0);
        assert!(candidates(&field, far, 0.0).is_empty());
        // A visited star's body range reaches farther.
        assert_eq!(candidates(&field, far, 1.0).len(), 1);
    }

    #[test]
    fn precise_sprite_is_ready_before_batch_culls_at_interstellar_range() {
        let cell = Cell::containing(6, [8200.0, 20.0, 0.0]).unwrap();
        let centre = DVec3::from_array(cell.centre_pc());
        // A bright star far from its large cell's centre can switch several
        // light years before the actual 0.02 ly system handoff.
        let position = centre + DVec3::X * 980.0;
        let point = StarPoint {
            position_pc: position.to_array(),
            luminosity_solar: 10_000.0,
            temperature_k: 10_000.0,
            radius_solar: 10.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            space_simulation::stream::StarCell::new(cell, vec![point]),
        );
        let eye = position + DVec3::Z * (6.0 * space_simulation::LY_METRES / PC_METRES);
        let candidate = candidates(&field, eye, 0.0);
        assert_eq!(candidate.len(), 1);
        let radius = PRECISE * candidate[0].offset_pc;
        let distance = eye.distance(position);
        assert!(distance < PREPARE * radius);
        assert!(distance < radius, "sprite must be visible at 6 ly");
        assert!(
            distance > BATCH_HIDE * candidate[0].offset_pc,
            "batch must still cover the six-light-year transition"
        );
    }
}
