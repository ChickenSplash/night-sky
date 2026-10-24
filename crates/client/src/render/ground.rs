//! Planets' ground as a cube-sphere of chunks (`ground.wgsl`): the six faces of
//! a cube bent onto the planet's spheroid, each a quadtree refined near the
//! camera.
//! - A chunk splits where its flat facets would stray more than
//!   `SPLIT_PIXELS` on screen from the ground they stand for, so the drawn
//!   ground meets the air's (`atmosphere.wgsl`) on any planet, at any zoom.
//! - Each vertex blends towards its parent level's surface as that level's
//!   split distance nears, so chunks neither pop as they split nor crack
//!   against coarser neighbours. Skirts hang under chunk edges for what
//!   rounding leaves.
//! - Chunks behind the horizon are skipped, and Bevy culls those out of view.
//! - Chunks are built from `space_simulation::surface` in f64 on worker
//!   threads, as offsets from their own centre, so they keep their precision
//!   beside the camera. A chunk stays drawn until its four children are ready.
use super::{
    BODY_SHADER, GROUND_SHADER, MAX_LIGHTS,
    bodies::{Placement, SpaceCameraCell, hide, place},
    debug::RenderDebug,
    lights::{GroundSource, Lights},
    resolve::{Frame, Shaded},
};
use crate::scene::SpaceCamera;
use bevy::{
    asset::RenderAssetUsages,
    camera::{primitives::Aabb, visibility::NoAutoAabb},
    math::DVec3,
    mesh::{
        Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexFormat,
    },
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshMaterial3d},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use big_space::prelude::*;
use serde::Serialize;
use space_shared::Planet;
use space_simulation::{Spheroid, surface};
use std::{
    collections::HashMap,
    f64::consts::{FRAC_PI_2, FRAC_PI_4},
};

/// Vertices along a chunk's side. With an even number of cells, a child's
/// even vertices are its parent's.
const GRID: usize = 33;
const CELLS: usize = GRID - 1;
/// About 0.6 m chunks with 2 cm cells on an Earth-sized planet.
const MAX_LEVEL: u8 = 24;
/// The most a chunk's facets may stray from its ground, in logical pixels.
const SPLIT_PIXELS: f64 = 0.5;
/// Where a vertex starts blending towards its parent level, as a share of
/// the distance at which the parent takes over.
const MORPH_START: f32 = 0.7;
/// Chunks built at once, and started per frame.
const IN_FLIGHT: usize = 32;
const STARTS_PER_FRAME: usize = 8;
/// Built chunks kept, drawn or not, before the least recently used go.
const CACHE: usize = 3_000;
/// A planet out of sight this many frames loses its chunks.
const FORGET_FRAMES: u64 = 600;

/// Per vertex: the offset to its parent level's surface, and the parent
/// level's error in metres (zero for a root chunk).
pub const ATTRIBUTE_MORPH: MeshVertexAttribute = MeshVertexAttribute::new(
    "GroundMorph",
    0x6e5f_9a3c_17d2_4b81,
    VertexFormat::Float32x4,
);

/// Each face's outward axis and its first axis along the face; the second is
/// their cross product, so cells wind outwards.
const FACES: [(DVec3, DVec3); 6] = [
    (DVec3::X, DVec3::Y),
    (DVec3::NEG_X, DVec3::Y),
    (DVec3::Y, DVec3::Z),
    (DVec3::NEG_Y, DVec3::Z),
    (DVec3::Z, DVec3::X),
    (DVec3::NEG_Z, DVec3::X),
];

/// A chunk: a square of one cube face at a level of the face's quadtree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Node {
    face: u8,
    level: u8,
    x: u32,
    y: u32,
}

impl Node {
    fn root(face: u8) -> Self {
        Self {
            face,
            level: 0,
            x: 0,
            y: 0,
        }
    }

    fn children(self) -> [Self; 4] {
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| Self {
            face: self.face,
            level: self.level + 1,
            x: self.x * 2 + dx,
            y: self.y * 2 + dy,
        })
    }

    /// Its corner and side in face coordinates, which run from -1 to 1.
    fn square(self) -> (f64, f64, f64) {
        let side = 2.0 / f64::from(1u32 << self.level);
        (
            -1.0 + side * f64::from(self.x),
            -1.0 + side * f64::from(self.y),
            side,
        )
    }

    /// The unit direction through a point of the chunk, `(i, j)` in cells
    /// from its corner.
    fn direction(self, i: f64, j: f64) -> DVec3 {
        let (s, t, side) = self.square();
        let step = side / CELLS as f64;
        direction(self.face, s + step * i, t + step * j)
    }
}

/// The unit direction through face coordinates `(s, t)`, in the planet's
/// axes. Equi-angular: equal steps of `s` are equal angles, which keeps cells
/// near one size where a plain cube's shrink towards its corners.
fn direction(face: u8, s: f64, t: f64) -> DVec3 {
    let (out, u) = FACES[usize::from(face)];
    let v = out.cross(u);
    (out + u * (s * FRAC_PI_4).tan() + v * (t * FRAC_PI_4).tan()).normalize()
}

/// The most a level's flat facets lie inside the smooth ground: the sag of a
/// cell's diagonal under the flattest curvature, $a^2/c$ at the poles.
fn error(shape: &Spheroid, level: u8) -> f64 {
    let cell = FRAC_PI_2 / (CELLS as f64 * f64::from(1u32 << level));
    shape.equatorial.powi(2) / shape.polar * 2.0 * cell * cell / 8.0
}

/// How far skirts hang under a level's chunk edges: past any gap a blend
/// towards a coarser neighbour can leave.
fn skirt(shape: &Spheroid, level: u8) -> f64 {
    8.0 * error(shape, level) + 0.01
}

/// A sphere holding a chunk, in metres in the planet's axes.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    centre: DVec3,
    radius: f64,
}

fn bounds(planet: &Planet, shape: &Spheroid, node: Node) -> Bounds {
    let half = CELLS as f64 / 2.0;
    let centre = surface::ground(planet, shape, node.direction(half, half));
    let mut radius: f64 = 0.0;
    for j in 0..=2 {
        for i in 0..=2 {
            let point = node.direction(half * f64::from(i), half * f64::from(j));
            radius = radius.max(surface::ground(planet, shape, point).distance(centre));
        }
    }
    // The ground bulges past those samples by up to the sag across half the
    // chunk, and the skirts hang below it.
    Bounds {
        centre,
        radius: radius + error(shape, node.level) * half * half + skirt(shape, node.level),
    }
}

/// The camera's height over the spheroid, measured from its centre and
/// shortened by the flattening, so never more than its height along the
/// normal. Zero below the spheroid. Terrain will lower it by the highest
/// ground.
fn floor(shape: &Spheroid, eye: DVec3) -> f64 {
    let sea = shape.along(eye.normalize_or(DVec3::Y)).length();
    ((eye.length() - sea) * (1.0 - shape.flattening())).max(0.0)
}

/// Whether `bounds` lies wholly behind a ball of radius `occluder` at the
/// centre, seen from `eye`: inside the cone that grazes the ball, and past
/// the circle where it grazes.
fn hidden(eye: DVec3, occluder: f64, bounds: Bounds) -> bool {
    let away = eye.length();
    let to = bounds.centre - eye;
    let along = to.length();
    if away <= occluder || along <= bounds.radius {
        return false;
    }
    let axis = -eye / away;
    let cone = (occluder / away).asin();
    let off_axis = (to.dot(axis) / along).clamp(-1.0, 1.0).acos();
    off_axis + (bounds.radius / along).asin() < cone
        && to.dot(axis) - bounds.radius > (away * away - occluder * occluder) / away
}

/// A chunk's mesh, in metres from its centre in the planet's axes.
struct Built {
    mesh: Mesh,
    aabb: Aabb,
    bounds: Bounds,
}

fn build(planet: &Planet, shape: &Spheroid, node: Node) -> Built {
    let bounds = bounds(planet, shape, node);
    let at = |i: usize, j: usize| j * GRID + i;
    let ground: Vec<DVec3> = (0..GRID * GRID)
        .map(|k| {
            let direction = node.direction((k % GRID) as f64, (k / GRID) as f64);
            surface::ground(planet, shape, direction)
        })
        .collect();
    // The parent level's surface under each vertex: its own vertices lie on
    // the even ones, and its cells' diagonals run as these do.
    let parent = |i: usize, j: usize| {
        let p = |i: usize, j: usize| ground[at(i, j)];
        match (i % 2, j % 2) {
            (0, 0) => p(i, j),
            (1, 0) => (p(i - 1, j) + p(i + 1, j)) / 2.0,
            (0, _) => (p(i, j - 1) + p(i, j + 1)) / 2.0,
            _ => (p(i - 1, j - 1) + p(i + 1, j + 1)) / 2.0,
        }
    };
    let parent_error = if node.level == 0 {
        0.0
    } else {
        error(shape, node.level - 1) as f32
    };
    let mut positions = Vec::with_capacity(GRID * GRID + 4 * GRID);
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut morphs = Vec::with_capacity(positions.capacity());
    for (k, point) in ground.iter().enumerate() {
        let (i, j) = (k % GRID, k / GRID);
        positions.push((*point - bounds.centre).as_vec3());
        normals.push(shape.normal(*point).as_vec3());
        let morph = if node.level == 0 {
            Vec3::ZERO
        } else {
            (parent(i, j) - *point).as_vec3()
        };
        morphs.push(morph.extend(parent_error));
    }
    let mut indices: Vec<u16> = Vec::with_capacity(CELLS * CELLS * 6 + 4 * CELLS * 12);
    for j in 0..CELLS {
        for i in 0..CELLS {
            let [a, b, c, d] =
                [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)].map(|v| v as u16);
            indices.extend([a, b, c, a, c, d]);
        }
    }
    // Skirts, wound both ways so they show from either side.
    let depth = skirt(shape, node.level) as f32;
    let edges: [Vec<usize>; 4] = [
        (0..GRID).map(|i| at(i, 0)).collect(),
        (0..GRID).map(|j| at(CELLS, j)).collect(),
        (0..GRID).map(|i| at(i, CELLS)).collect(),
        (0..GRID).map(|j| at(0, j)).collect(),
    ];
    for edge in edges {
        let first = positions.len();
        for &top in &edge {
            positions.push(positions[top] - normals[top] * depth);
            normals.push(normals[top]);
            morphs.push(morphs[top]);
        }
        for k in 0..CELLS {
            let [a, b] = [edge[k], edge[k + 1]].map(|v| v as u16);
            let [c, d] = [first + k, first + k + 1].map(|v| v as u16);
            indices.extend([a, b, d, a, d, c, a, d, b, a, c, d]);
        }
    }
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (p, m) in positions.iter().zip(&morphs) {
        for q in [*p, *p + m.truncate()] {
            min = min.min(q);
            max = max.max(q);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(ATTRIBUTE_MORPH, morphs);
    mesh.insert_indices(Indices::U16(indices));
    Built {
        mesh,
        aabb: Aabb::from_min_max(min, max),
        bounds,
    }
}

/// Shaded as `body.wgsl` shades a planet, with `morph` for `ground.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone, Default, PartialEq)]
#[bind_group_data(GroundKey)]
pub struct GroundMaterial {
    #[uniform(0)]
    colour: Vec4,
    #[uniform(0)]
    light: Vec4,
    #[uniform(0)]
    spin: Vec4,
    #[uniform(0)]
    surface: Vec4,
    #[uniform(0)]
    lights: [Vec4; MAX_LIGHTS],
    #[uniform(0)]
    tints: [Vec4; MAX_LIGHTS],
    /// Metres per render unit, `MORPH_START`, the split distance per metre of
    /// error, and a root chunk's error.
    #[uniform(1)]
    morph: Vec4,
    /// Tint chunks by level (`RenderDebug::ground_levels`).
    levels: bool,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct GroundKey {
    levels: bool,
}

impl From<&GroundMaterial> for GroundKey {
    fn from(material: &GroundMaterial) -> Self {
        Self {
            levels: material.levels,
        }
    }
}

impl Material for GroundMaterial {
    fn vertex_shader() -> ShaderRef {
        GROUND_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        BODY_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            ATTRIBUTE_MORPH.at_shader_location(2),
        ])?];
        if key.bind_group_data.levels {
            for defs in [
                &mut descriptor.vertex.shader_defs,
                &mut descriptor
                    .fragment
                    .as_mut()
                    .expect("ground has a fragment stage")
                    .shader_defs,
            ] {
                defs.extend(["LEVELS".into(), "VERTEX_COLORS".into()]);
            }
        }
        Ok(())
    }
}

#[derive(Component, Clone, Copy)]
pub(super) struct GroundChunk;

struct Chunk {
    entity: Entity,
    bounds: Bounds,
    /// The last frame it was drawn or passed through.
    used: u64,
    /// The frame it was spawned, whose commands only apply at its end.
    born: u64,
}

enum Slot {
    Building(Task<Built>),
    Ready(Chunk),
}

struct PlanetGround {
    planet: Planet,
    shape: Spheroid,
    material: Handle<GroundMaterial>,
    slots: HashMap<Node, Slot>,
    seen: u64,
    /// The frame its roots were spawned.
    born: u64,
    /// The frame being selected for.
    now: u64,
}

impl PlanetGround {
    /// A built chunk whose entity can be placed this frame. One spawned this
    /// frame cannot be yet, and switching to it would leave a frame with
    /// neither it nor its parent drawn.
    fn ready(&self, node: Node) -> Option<&Chunk> {
        match self.slots.get(&node) {
            Some(Slot::Ready(chunk)) if chunk.born < self.now => Some(chunk),
            _ => None,
        }
    }
}

/// Every planet's chunks, built and building.
#[derive(Resource, Default)]
pub(super) struct Grounds {
    planets: HashMap<u64, PlanetGround>,
    frame: u64,
    drawn: Vec<Entity>,
}

impl Grounds {
    /// Whether a planet's ground can be drawn in place of its sphere this
    /// frame (`sync_bodies` runs before `sync_ground` counts the frame).
    pub(super) fn shows(&self, key: u64) -> bool {
        self.planets
            .get(&key)
            .is_some_and(|ground| ground.born <= self.frame)
    }
}

/// The last frame's ground, for the developer API's `performance.ground`.
#[derive(Resource, Default, Clone, Copy, Debug, Serialize)]
pub struct GroundStats {
    pub planets: usize,
    pub drawn: usize,
    /// Behind the horizon, so neither drawn nor refined.
    pub hidden: usize,
    /// Built and kept, drawn or not.
    pub cached: usize,
    pub building: usize,
    /// Finished this frame.
    pub built: usize,
    pub deepest_level: u8,
}

/// One planet's chunks to draw this frame, and those wanted next.
struct Selection<'a> {
    ground: &'a PlanetGround,
    /// The camera, in metres in the planet's axes.
    eye: DVec3,
    /// No ground is nearer the camera than this: its height, a little under.
    floor: f64,
    /// Split distance per metre of error.
    reach: f64,
    draw: Vec<Node>,
    passed: Vec<Node>,
    wanted: Vec<(f64, Node)>,
    hidden: usize,
}

impl Selection<'_> {
    fn visit(&mut self, node: Node, bounds: Bounds) {
        if hidden(self.eye, self.ground.shape.polar, bounds) {
            self.hidden += 1;
            return;
        }
        // Never more than the true distance to any vertex, which the
        // shader's blending measures, so levels still meet at the switch.
        let distance = (bounds.centre.distance(self.eye) - bounds.radius).max(self.floor);
        if node.level < MAX_LEVEL && distance < error(&self.ground.shape, node.level) * self.reach {
            let children = node.children();
            let ready = children.map(|child| self.ground.ready(child).map(|c| c.bounds));
            if ready.iter().all(Option::is_some) {
                self.passed.push(node);
                for (child, bounds) in children.into_iter().zip(ready.into_iter().flatten()) {
                    self.visit(child, bounds);
                }
                return;
            }
            for child in children {
                if !self.ground.slots.contains_key(&child) {
                    self.wanted.push((distance, child));
                }
            }
        }
        self.draw.push(node);
    }
}

fn spawn_chunk(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    built: Built,
    material: &Handle<GroundMaterial>,
    (cell, parent): (&CellCoord, &ChildOf),
    now: u64,
) -> Chunk {
    let entity = commands
        .spawn((
            GroundChunk,
            Mesh3d(meshes.add(built.mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            *cell,
            Visibility::Hidden,
            built.aabb,
            NoAutoAabb,
            ChildOf(parent.parent()),
        ))
        .id();
    Chunk {
        entity,
        bounds: built.bounds,
        used: 0,
        born: now,
    }
}

fn material(body: &Shaded, morph: Vec4, levels: bool) -> GroundMaterial {
    GroundMaterial {
        colour: body.colour.extend(1.0),
        light: body.light,
        spin: Vec4::from(Quat::IDENTITY),
        surface: body.surface,
        lights: body.lights,
        tints: body.tints,
        morph,
        levels,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_ground(
    mut commands: Commands,
    frame: Res<Frame>,
    lights: Res<Lights>,
    debug: Res<RenderDebug>,
    mut grounds: ResMut<Grounds>,
    mut stats: ResMut<GroundStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GroundMaterial>>,
    camera: Query<(SpaceCameraCell, &Camera, &Projection), With<SpaceCamera>>,
    mut chunks: Query<Placement, (With<GroundChunk>, Without<SpaceCamera>)>,
) {
    let grounds = &mut *grounds;
    grounds.frame += 1;
    let now = grounds.frame;
    let previous = std::mem::take(&mut grounds.drawn);
    *stats = GroundStats::default();
    let Ok(((cell, parent, _), camera, projection)) = camera.single() else {
        return;
    };
    // Split distance per metre of error: logical pixels per radian over the
    // pixels allowed.
    let reach = match (projection, camera.logical_viewport_size()) {
        (Projection::Perspective(p), Some(size)) => {
            f64::from(size.y) / (2.0 * f64::from(p.fov / 2.0).tan()) / SPLIT_PIXELS
        }
        _ => 0.0,
    };
    let pool = AsyncComputeTaskPool::get();
    let (mut building, mut started) = (0, 0);
    for ground in grounds.planets.values_mut() {
        for slot in ground.slots.values_mut() {
            if let Slot::Building(task) = slot {
                if let Some(built) = check_ready(task) {
                    *slot = Slot::Ready(spawn_chunk(
                        &mut commands,
                        &mut meshes,
                        built,
                        &ground.material,
                        (cell, parent),
                        now,
                    ));
                    stats.built += 1;
                } else {
                    building += 1;
                }
            }
        }
    }
    if debug.ground && reach > 0.0 {
        for body in &frame.bodies {
            let Some(source) = body
                .ground
                .and_then(|key| lights.grounds.iter().find(|g| g.key == key))
            else {
                continue;
            };
            let ground = grounds.planets.entry(source.key).or_insert_with(|| {
                new_ground(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    source,
                    (cell, parent),
                    now,
                )
            });
            ground.seen = now;
            ground.now = now;
            let morph = Vec4::new(
                lights.scale as f32,
                MORPH_START,
                reach as f32,
                error(&ground.shape, 0) as f32,
            );
            let changed = material(body, morph, debug.ground_levels);
            if materials
                .get(&ground.material)
                .is_some_and(|m| *m != changed)
                && let Some(m) = materials.get_mut(&ground.material)
            {
                *m = changed;
            }
            let eye = source.basis.inverse() * -source.from_eye;
            let mut selection = Selection {
                ground,
                eye,
                floor: floor(&ground.shape, eye),
                reach,
                draw: Vec::new(),
                passed: Vec::new(),
                wanted: Vec::new(),
                hidden: 0,
            };
            for face in 0..6 {
                let root = Node::root(face);
                if let Some(chunk) = ground.ready(root) {
                    selection.visit(root, chunk.bounds);
                }
            }
            let Selection {
                draw,
                passed,
                mut wanted,
                hidden,
                ..
            } = selection;
            stats.hidden += hidden;
            for node in draw.iter().chain(&passed) {
                if let Some(Slot::Ready(chunk)) = ground.slots.get_mut(node) {
                    chunk.used = now;
                }
            }
            for node in &draw {
                let Some(chunk) = ground.ready(*node) else {
                    continue;
                };
                let Ok(placement) = chunks.get_mut(chunk.entity) else {
                    continue;
                };
                let centre = source.from_eye + source.basis * chunk.bounds.centre;
                place(
                    placement,
                    *cell,
                    Transform::from_translation(lights.eye + (centre / lights.scale).as_vec3())
                        .with_rotation(source.basis.as_quat())
                        .with_scale(Vec3::splat((1.0 / lights.scale) as f32)),
                );
                grounds.drawn.push(chunk.entity);
                stats.deepest_level = stats.deepest_level.max(node.level);
            }
            stats.drawn += draw.len();
            // Nearest first, coarsest first among equals.
            wanted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.level.cmp(&b.1.level)));
            let room = IN_FLIGHT
                .saturating_sub(building)
                .min(STARTS_PER_FRAME - started);
            for (_, node) in wanted.into_iter().take(room) {
                let (planet, shape) = (ground.planet.clone(), ground.shape);
                let task = pool.spawn(async move { build(&planet, &shape, node) });
                ground.slots.insert(node, Slot::Building(task));
                building += 1;
                started += 1;
            }
        }
    }
    for entity in previous {
        if !grounds.drawn.contains(&entity)
            && let Ok((.., visibility)) = chunks.get_mut(entity)
        {
            hide(visibility);
        }
    }
    forget(&mut commands, grounds, now);
    stats.planets = grounds.planets.len();
    stats.building = building;
    stats.cached = grounds
        .planets
        .values()
        .flat_map(|g| g.slots.values())
        .filter(|slot| matches!(slot, Slot::Ready(_)))
        .count();
}

/// A planet's ground, its six root chunks built at once.
fn new_ground(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GroundMaterial>,
    source: &GroundSource,
    camera: (&CellCoord, &ChildOf),
    now: u64,
) -> PlanetGround {
    // The grid has no visibility of its own, which would hide its children.
    commands
        .entity(camera.1.parent())
        .insert_if_new(Visibility::default());
    let material = materials.add(GroundMaterial::default());
    let slots = (0..6)
        .map(|face| {
            let node = Node::root(face);
            let built = build(&source.planet, &source.shape, node);
            let chunk = spawn_chunk(commands, meshes, built, &material, camera, now);
            (node, Slot::Ready(chunk))
        })
        .collect();
    PlanetGround {
        planet: source.planet.clone(),
        shape: source.shape,
        material,
        slots,
        seen: 0,
        born: now,
        now,
    }
}

/// Drops planets long out of sight, then the least recently used chunks
/// past `CACHE`. Roots and chunks used this frame stay.
fn forget(commands: &mut Commands, grounds: &mut Grounds, now: u64) {
    let despawn = |commands: &mut Commands, slot: Slot| {
        if let Slot::Ready(chunk) = slot {
            commands.entity(chunk.entity).despawn();
        }
    };
    let stale: Vec<u64> = grounds
        .planets
        .iter()
        .filter(|(_, g)| now.saturating_sub(g.seen) > FORGET_FRAMES)
        .map(|(key, _)| *key)
        .collect();
    for key in stale {
        if let Some(ground) = grounds.planets.remove(&key) {
            for slot in ground.slots.into_values() {
                despawn(commands, slot);
            }
        }
    }
    let mut old: Vec<(u64, u64, Node)> = grounds
        .planets
        .iter()
        .flat_map(|(key, g)| {
            g.slots.iter().filter_map(move |(node, slot)| match slot {
                Slot::Ready(chunk) if node.level > 0 && chunk.used < now => {
                    Some((chunk.used, *key, *node))
                }
                _ => None,
            })
        })
        .collect();
    let ready = grounds
        .planets
        .values()
        .flat_map(|g| g.slots.values())
        .filter(|slot| matches!(slot, Slot::Ready(_)))
        .count();
    if ready <= CACHE {
        return;
    }
    old.sort_unstable_by_key(|(used, ..)| *used);
    for (_, key, node) in old.into_iter().take(ready - CACHE) {
        if let Some(slot) = grounds
            .planets
            .get_mut(&key)
            .and_then(|g| g.slots.remove(&node))
        {
            despawn(commands, slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planet() -> Planet {
        let galaxy = space_simulation::demo_galaxy();
        galaxy.systems[0].planets[0].clone()
    }

    fn earth() -> Spheroid {
        Spheroid::of(6_371_000.0, 1.0 / 298.0)
    }

    fn positions(mesh: &Mesh, attribute: MeshVertexAttribute) -> Vec<[f32; 4]> {
        match mesh.attribute(attribute) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => {
                v.iter().map(|p| [p[0], p[1], p[2], 0.0]).collect()
            }
            Some(bevy::mesh::VertexAttributeValues::Float32x4(v)) => v.clone(),
            _ => panic!("missing attribute"),
        }
    }

    /// The surface each grid vertex of a chunk is drawn at, fully blended
    /// towards its parent or not at all, in metres in the planet's axes.
    fn drawn(node: Node, blend: f32) -> Vec<DVec3> {
        let built = build(&planet(), &earth(), node);
        let points = positions(&built.mesh, Mesh::ATTRIBUTE_POSITION);
        let morphs = positions(&built.mesh, ATTRIBUTE_MORPH);
        points
            .iter()
            .zip(&morphs)
            .take(GRID * GRID)
            .map(|(p, m)| {
                built.bounds.centre
                    + DVec3::new(
                        f64::from(p[0] + m[0] * blend),
                        f64::from(p[1] + m[1] * blend),
                        f64::from(p[2] + m[2] * blend),
                    )
            })
            .collect()
    }

    #[test]
    fn neighbouring_faces_share_their_edges() {
        // +X's edge at t = 1 is +Z's at s = 1, both running along +Y.
        for k in 0..=8 {
            let along = -1.0 + 0.25 * f64::from(k);
            assert!(direction(0, along, 1.0).distance(direction(4, 1.0, along)) < 1e-12);
        }
    }

    #[test]
    fn chunks_lie_on_the_ground_within_their_error() {
        let shape = earth();
        for node in [
            Node::root(2),
            Node {
                face: 0,
                level: 5,
                x: 7,
                y: 20,
            },
            Node {
                face: 5,
                level: 12,
                x: 1000,
                y: 3000,
            },
        ] {
            let built = build(&planet(), &shape, node);
            let points = drawn(node, 0.0);
            let sea = |p: DVec3| shape.along(p.normalize()).length();
            for p in &points {
                // Vertices are on the ground, to f32 offsets from the centre.
                let tolerance = built.bounds.radius * 1e-6 + 1e-3;
                assert!((p.length() - sea(*p)).abs() < tolerance, "{node:?}");
            }
            // Mid-cell, the facet sits no further in than the level's error.
            let (a, c) = (points[0], points[GRID + 1]);
            let mid = (a + c) / 2.0;
            let sag = sea(mid) - mid.length();
            assert!(
                sag >= -1e-3 && sag <= error(&shape, node.level) * 1.01 + 1e-3,
                "{sag}"
            );
        }
    }

    #[test]
    fn a_blended_child_lies_on_its_parents_facets() {
        let parent = Node {
            face: 1,
            level: 6,
            x: 10,
            y: 33,
        };
        let coarse = drawn(parent, 0.0);
        for (index, child) in parent.children().into_iter().enumerate() {
            let fine = drawn(child, 1.0);
            let (ox, oy) = ((index % 2) * CELLS / 2, (index / 2) * CELLS / 2);
            for j in (0..GRID).step_by(2) {
                for i in (0..GRID).step_by(2) {
                    let expected = coarse[(oy + j / 2) * GRID + ox + i / 2];
                    assert!(
                        fine[j * GRID + i].distance(expected) < 0.05,
                        "{child:?} {i} {j}"
                    );
                }
            }
            // An odd vertex lands midway along its parent's diagonal.
            let expected = (fine[0] + fine[2 * GRID + 2]) / 2.0;
            assert!(fine[GRID + 1].distance(expected) < 0.05);
        }
    }

    #[test]
    fn the_far_side_is_behind_the_horizon() {
        let shape = earth();
        let eye = DVec3::X * shape.equatorial * 1.5;
        let far = bounds(
            &planet(),
            &shape,
            Node {
                face: 1,
                level: 3,
                x: 3,
                y: 3,
            },
        );
        let near = bounds(
            &planet(),
            &shape,
            Node {
                face: 0,
                level: 3,
                x: 3,
                y: 3,
            },
        );
        assert!(hidden(eye, shape.polar, far));
        assert!(!hidden(eye, shape.polar, near));
        // Standing on the ground, the chunk underfoot is never hidden.
        let feet = shape.along(DVec3::new(0.3, 0.2, 0.9).normalize()) * 1.000_000_3;
        for level in [0, 8, 16, 24] {
            let mut node = Node::root(4);
            for _ in 0..level {
                node = *node
                    .children()
                    .iter()
                    .min_by(|a, b| {
                        let d = |n: &Node| n.direction(16.0, 16.0).distance(feet.normalize());
                        d(a).total_cmp(&d(b))
                    })
                    .unwrap();
            }
            assert!(!hidden(feet, shape.polar, bounds(&planet(), &shape, node)));
        }
    }

    /// Selects from `eye` until every wanted chunk is built, as frames would.
    fn settle(shape: Spheroid, eye: DVec3, reach: f64) -> (Vec<Node>, usize) {
        let planet = planet();
        let mut ground = PlanetGround {
            planet: planet.clone(),
            shape,
            material: Handle::default(),
            slots: HashMap::new(),
            seen: 0,
            born: 0,
            now: 1,
        };
        let ready = |node| {
            Slot::Ready(Chunk {
                entity: Entity::PLACEHOLDER,
                bounds: bounds(&planet, &shape, node),
                used: 0,
                born: 0,
            })
        };
        for face in 0..6 {
            ground
                .slots
                .insert(Node::root(face), ready(Node::root(face)));
        }
        loop {
            let mut selection = Selection {
                ground: &ground,
                eye,
                floor: floor(&shape, eye),
                reach,
                draw: Vec::new(),
                passed: Vec::new(),
                wanted: Vec::new(),
                hidden: 0,
            };
            for face in 0..6 {
                let root = Node::root(face);
                let bounds = ground.ready(root).unwrap().bounds;
                selection.visit(root, bounds);
            }
            let (draw, wanted, hidden) = (selection.draw, selection.wanted, selection.hidden);
            if wanted.is_empty() {
                return (draw, hidden);
            }
            for (_, node) in wanted {
                ground.slots.insert(node, ready(node));
            }
        }
    }

    #[test]
    fn standing_on_the_ground_refines_underfoot_and_skips_the_far_side() {
        let shape = earth();
        let reach = 1_000.0 / (2.0 * 25f64.to_radians().tan()) / SPLIT_PIXELS;
        let up = DVec3::new(0.3, 0.5, -0.8).normalize();
        let feet = shape.along(up);
        let eye = feet + shape.normal(feet) * 1.7;
        let (draw, hidden) = settle(shape, eye, reach);
        let deepest = draw.iter().map(|n| n.level).max().unwrap();
        let under = draw
            .iter()
            .min_by(|a, b| {
                let d = |n: &Node| n.direction(16.0, 16.0).distance(up);
                d(a).total_cmp(&d(b))
            })
            .unwrap();
        assert_eq!(under.level, deepest);
        assert!(deepest >= 10, "{deepest}");
        assert!(draw.len() < 400, "{} drawn", draw.len());
        assert!(hidden > 0);
        // From far away, the roots alone do.
        let (draw, _) = settle(shape, up * shape.equatorial * 30.0, reach);
        assert!(draw.iter().all(|n| n.level <= 1), "{draw:?}");
    }

    #[test]
    fn splitting_reaches_the_ground_near_the_camera() {
        let shape = earth();
        // At 1.7 m, a 60 degree view 1,000 pixels high.
        let reach = 1_000.0 / (2.0 * (30f64).to_radians().tan()) / SPLIT_PIXELS;
        let level = (0..=MAX_LEVEL)
            .find(|&level| error(&shape, level) * reach < 1.7)
            .unwrap();
        assert!((8..=16).contains(&level), "{level}");
    }
}
