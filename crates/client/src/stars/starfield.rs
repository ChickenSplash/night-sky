//! Batched, camera-relative galactic points. A batch holds many stars, not
//! one entity or material per star, uploaded in bounded chunks and drawn by
//! `stars::render` as one instance per star.

use crate::{
    Navigation, Universe,
    render::Photometry,
    render::debug::RenderDebug,
    render::sky::radiance_gain,
    scene::{CameraPose, CameraRig, SpaceCamera},
    stars::render::{
        Globals, PALETTE, SHADER, StarDraw, StarFrame, StarInstance, StarRenderPlugin, StarUploads,
    },
    stars::stream::{Starfield, StreamMetrics},
};
use bevy::{asset::load_internal_asset, math::DVec3, prelude::*};
use space_simulation::{
    cells::Cell,
    galaxy::PC_METRES,
    light::visual_efficiency,
    population::{reach_pc, reach_sq_per_luminosity},
    stream::{Block, StarPoint, gap_pc},
};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

/// Loaded stars filtered per step, between checks of the upload budget.
const SLICE: usize = 4096;
/// Visible stars per uploaded batch. Each is a draw call, so few large
/// batches draw fastest.
const PACK: usize = 65_536;
/// Loaded stars per grid block (`StarCell::blocks`). Each batch lies within
/// one block and is placed from its centre, so its f32 offsets, and with them
/// the precise-sprite radii, scale with the block rather than the cell: at
/// the galactic centre a block is a small fraction of its cell.
const BLOCK: usize = 4 * PACK;
const UPLOAD_BUDGET: Duration = Duration::from_millis(3);
const MARGIN: f64 = 1.25;
/// A point closer than this many times its offset from its block's centre gets
/// a precise sprite. Batch vertices are f32 offsets from that centre, so their
/// error grows with the offset; beyond here it stays below a quarter pixel.
const PRECISE: f64 = 2e-3;
/// Keep the batched point while its precise replacement becomes visible. The
/// former is accurate to around a pixel at this cutoff; without overlap a
/// hidden sprite leaves a hole light-years from a bright star.
const BATCH_HIDE: f64 = PRECISE * 0.25;
/// Precise candidates are gathered this many times farther out than they
/// show, so a sprite is ready before it is needed.
const PREPARE: f64 = 2.0;
/// Precise candidates are gathered again after the camera moves this far.
const RESCAN_PC: f64 = 0.1;
/// A multiple system is one point until its stars could appear this many
/// pixels apart, well inside the point spread, so splitting never shows.
const SPLIT_PX: f64 = 0.5;
/// Split candidates are gathered for this share of the split angle, so the
/// view can narrow (more pixels per radian) this far before gathering again.
const SPLIT_SLACK: f64 = 0.5;
/// Farthest a multiple system is ever split, in parsecs: its widest pairs,
/// a few tenths of a parsec, show apart from about 600 pc.
const SPLIT_REACH_PC: f64 = 1_000.0;
/// Stars measured per gather of split candidates. Beyond it the reach halves
/// and the gather starts again, so crowded regions split only nearby systems.
const SPLIT_BUDGET: usize = 100_000;
const SPLIT_MIN_REACH_PC: f64 = 1e-6;
const SPLIT_ATTEMPTS: usize = 4;
const SPLIT_RETRY: Duration = Duration::from_millis(250);
/// Systems kept built for splitting before the cache is cleared.
const SPLIT_CACHE: usize = 4_096;

/// Each temperature's realistic colour with unit luminance and visual
/// efficiency (light per bolometric luminosity, as the eye and the exposure
/// see it), then its wireframe colour, linear.
fn palettes() -> ([Vec4; PALETTE], [Vec4; PALETTE]) {
    let photometry = Photometry::default();
    let kelvin = |i: usize| 1000.0 * 50f64.powf(i as f64 / (PALETTE - 1) as f64);
    let realistic = std::array::from_fn(|i| {
        let [r, g, b] = photometry.colour(kelvin(i)).map(|c| c as f32);
        Vec4::new(r, g, b, visual_efficiency(kelvin(i)) as f32)
    });
    let wireframe = std::array::from_fn(|i| {
        let [r, g, b] = space_simulation::spectral_colour(kelvin(i));
        let linear = Color::srgb_u8(r, g, b).to_linear();
        Vec4::new(linear.red, linear.green, linear.blue, 1.0)
    });
    (realistic, wireframe)
}

/// One uploaded batch.
struct Batch {
    id: u64,
    /// Stars uploaded, one instance each.
    stars: usize,
    /// Its block's centre, from which its stars are offsets.
    centre_pc: [f64; 3],
    /// Half its block's edge.
    half_pc: f64,
}

/// Whether a star is bright enough to upload, seen from `eye` with the
/// `magnitude` limit.
fn bright_enough(star: &StarPoint, eye: DVec3, magnitude: f64) -> bool {
    let reach = MARGIN * MARGIN * reach_sq_per_luminosity(magnitude);
    let squared = (DVec3::from_array(star.position_pc) - eye).length_squared();
    squared <= f64::from(star.luminosity_solar) * reach
}

/// A star's GPU record, `origin_pc` being where its draw is placed from.
fn instance(star: &StarPoint, origin_pc: [f64; 3]) -> StarInstance {
    StarInstance::new(
        std::array::from_fn(|axis| (star.position_pc[axis] - origin_pc[axis]) as f32),
        star.luminosity_solar,
        star.temperature_k,
        star.separation_au,
    )
}

/// Stars bright enough to see from `eye`, as offsets from `centre`.
fn pack(
    stars: &mut Vec<StarInstance>,
    points: &[StarPoint],
    centre: [f64; 3],
    eye: DVec3,
    magnitude: f64,
) {
    stars.extend(
        points
            .iter()
            .filter(|star| bright_enough(star, eye, magnitude))
            .map(|star| instance(star, centre)),
    );
}

/// The cell whose batches are being built, possibly across several frames.
struct Building {
    owner: Cell,
    /// Rebuilding a cell already drawn: its new batches wait in `fresh` and
    /// replace the old ones together, so the cell never shows a hole.
    replacing: bool,
    blocks: Vec<Block>,
    /// The block being packed: no batch spans two.
    block: usize,
    packing: Vec<StarInstance>,
}

#[derive(Resource, Default)]
struct Batches {
    active: HashMap<Cell, Vec<Batch>>,
    /// Finished batches of a rebuild still in progress, not yet drawn.
    fresh: HashMap<Cell, Vec<Batch>>,
    building: Option<Building>,
    queued: VecDeque<(Cell, usize)>,
    built_at: HashMap<Cell, DVec3>,
    next_id: u64,
    /// Physical colours and the unchanged wireframe palette.
    palettes: Option<([Vec4; PALETTE], [Vec4; PALETTE])>,
}

/// A star that may come close enough to need a precise sprite.
struct Candidate {
    id: String,
    star: StarPoint,
    /// Parsecs from its block's centre.
    offset_pc: f64,
}

/// Stars near the camera, each placed by an f64 subtraction every frame.
#[derive(Resource, Default)]
struct Precise {
    scanned_at: Option<DVec3>,
    scanned_body: f64,
    candidates: Vec<Candidate>,
}

/// Multiple systems that may show their stars apart, gathered now and then,
/// and the stars of those that do, built once.
#[derive(Resource)]
pub(super) struct Split {
    enabled: bool,
    /// Systems are split only within this distance, in parsecs; the batches
    /// hide them as points only within it too.
    reach_pc: f64,
    /// The reach for the next gather: shrinks where gathers find too many
    /// stars, grows back where they find few.
    next_reach_pc: f64,
    scanned_at: Option<DVec3>,
    /// The angle and movement gathered for (`StarCell::resolved`).
    scanned_angle: f64,
    slack_pc: f64,
    gathered_at: Option<bevy::platform::time::Instant>,
    candidates: Vec<(String, StarPoint)>,
    /// Each split system's stars and pairs, and its plane's rotation.
    built: HashMap<String, (space_shared::StarSystem, bevy::math::DQuat)>,
}

impl Default for Split {
    fn default() -> Self {
        Self {
            enabled: true,
            reach_pc: SPLIT_REACH_PC,
            next_reach_pc: SPLIT_REACH_PC,
            scanned_at: None,
            scanned_angle: 0.0,
            slack_pc: 0.0,
            gathered_at: None,
            candidates: Vec::new(),
            built: HashMap::new(),
        }
    }
}

impl Split {
    fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            *self = Self {
                enabled,
                ..Self::default()
            };
        }
    }

    fn gather(&mut self, field: &Starfield, eye: DVec3, angle: f64) -> usize {
        let mut reach = self.next_reach_pc.max(SPLIT_MIN_REACH_PC);
        let mut examined = 0;
        self.candidates.clear();
        // Zero is safe for the current frame's shader cutoff, but is never
        // retained as the next search distance.
        self.reach_pc = 0.0;
        for _ in 0..SPLIT_ATTEMPTS {
            let slack = (0.01 * reach).min(5.0);
            self.slack_pc = slack;
            if let Some((found, measured)) =
                split_candidates(field, eye, (reach, angle * SPLIT_SLACK, slack))
            {
                examined += measured;
                let empty = found.is_empty();
                self.candidates = found;
                self.reach_pc = reach;
                self.next_reach_pc = if empty || measured < SPLIT_BUDGET / 8 {
                    (reach * 2.0).min(SPLIT_REACH_PC)
                } else {
                    reach
                };
                break;
            }
            examined += SPLIT_BUDGET + 1;
            reach = (reach * 0.5).max(SPLIT_MIN_REACH_PC);
            self.next_reach_pc = reach;
        }
        self.scanned_at = Some(eye);
        self.scanned_angle = angle * SPLIT_SLACK;
        self.gathered_at = Some(bevy::platform::time::Instant::now());
        examined
    }

    fn retry_due(&self) -> bool {
        (self.reach_pc == 0.0 || self.next_reach_pc > self.reach_pc)
            && self
                .gathered_at
                .is_none_or(|at| at.elapsed() >= SPLIT_RETRY)
    }
}

/// A system's stars and pairs, for `member_position`.
fn split_system(id: &str) -> (space_shared::StarSystem, bevy::math::DQuat) {
    let (stars, barycentres) = space_simulation::system_stars(id);
    let system = space_shared::StarSystem {
        id: id.to_owned(),
        name: String::new(),
        position_ly: [0.0; 3],
        planets: Vec::new(),
        stars,
        barycentres,
        page_stars: Vec::new(),
    };
    (system, crate::scene::star_frame(id))
}

/// A multiple system's stars as precise sprites, its barycentre `at` parsecs
/// from the eye, placed as `scene::draw` places them as bodies.
fn apart(
    system: &space_shared::StarSystem,
    plane: bevy::math::DQuat,
    at: DVec3,
    seconds: f64,
) -> impl Iterator<Item = StarInstance> + '_ {
    system.stars.iter().map(move |member| {
        let offset = space_simulation::member_position(system, 0, &member.id, seconds);
        let position = at + plane * DVec3::from_array(offset) / PC_METRES;
        StarInstance::new(
            position.to_array().map(|v| v as f32),
            member.luminosity_solar as f32,
            member.temperature_k as f32,
            0.0,
        )
    })
}

/// Multiple systems within `reach_pc` of `eye` whose stars could appear
/// `angle` radians apart from anywhere within `slack_pc` of it, unless more
/// than `SPLIT_BUDGET` stars must be measured to find them. Returns them and
/// the stars measured.
fn split_candidates(
    field: &Starfield,
    eye: DVec3,
    (reach_pc, angle, slack_pc): (f64, f64, f64),
) -> Option<(Vec<(String, StarPoint)>, usize)> {
    let (mut found, mut examined) = (Vec::new(), 0);
    for (&cell, stars) in &field.cells {
        if gap_pc(cell, eye.to_array()) > reach_pc + slack_pc {
            continue;
        }
        stars.resolved(
            eye.to_array(),
            (reach_pc + slack_pc, angle, slack_pc),
            SPLIT_BUDGET,
            &mut examined,
            &mut |star| found.push((star.id(cell), *star)),
        );
        if examined > SPLIT_BUDGET {
            return None;
        }
    }
    Some((found, examined))
}

pub struct StarfieldRenderPlugin;

impl Plugin for StarfieldRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "starfield.wgsl", Shader::from_wgsl);
        app.add_plugins(StarRenderPlugin)
            .init_resource::<Batches>()
            .init_resource::<Precise>()
            .init_resource::<Split>()
            .add_systems(PostUpdate, sync.after(crate::scene::draw));
        #[cfg(not(target_arch = "wasm32"))]
        app.add_plugins(crate::stars::points::StarPointsPlugin);
    }
}

/// Stars that could need a precise sprite before the camera moves `RESCAN_PC`.
/// `body_pc` is the visited star's body range, within which batches hide points.
fn candidates(field: &Starfield, eye: DVec3, body_pc: f64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (&cell, stars) in &field.cells {
        let half_diagonal = cell.size_pc() * 0.5 * 3.0_f64.sqrt();
        let reach = PREPARE * (PRECISE * half_diagonal).max(body_pc) + RESCAN_PC;
        if gap_pc(cell, eye.to_array()) > reach {
            continue;
        }
        for block in stars.blocks(BLOCK) {
            let reach = PREPARE * (PRECISE * block.half_diagonal_pc()).max(body_pc) + RESCAN_PC;
            let centre = DVec3::from_array(block.centre_pc);
            stars.near_in(&block, eye.to_array(), reach, |_, star| {
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
    }
    out
}

/// The view's four side planes in f64, for culling blocks: a sphere outside
/// any of them cannot reach the screen. Planes widen by a margin in pixels, so
/// a star at a block's edge keeps its whole footprint.
struct Sight {
    right: DVec3,
    up: DVec3,
    forward: DVec3,
    tan_x: f64,
    tan_y: f64,
}

impl Sight {
    fn new(camera: &GlobalTransform, tan_half_fov: f64, size: UVec2, margin_px: f64) -> Self {
        let pad = margin_px * 2.0 * tan_half_fov / f64::from(size.y);
        Self {
            right: camera.right().as_dvec3(),
            up: camera.up().as_dvec3(),
            forward: camera.forward().as_dvec3(),
            tan_x: tan_half_fov * f64::from(size.x) / f64::from(size.y) + pad,
            tan_y: tan_half_fov + pad,
        }
    }

    /// Whether a sphere `offset` from the eye can show. Conservative: a few
    /// spheres just off screen, or just behind the camera, pass.
    fn sees(&self, offset: DVec3, radius: f64) -> bool {
        let depth = offset.dot(self.forward);
        let inside = |lateral: f64, tan: f64| {
            (lateral.abs() - depth * tan) / (1.0 + tan * tan).sqrt() <= radius
        };
        inside(offset.dot(self.right), self.tan_x) && inside(offset.dot(self.up), self.tan_y)
    }
}

fn release(uploads: &StarUploads, batches: Vec<Batch>) {
    for batch in batches {
        uploads.remove(batch.id);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut field: ResMut<Starfield>,
    mut metrics: ResMut<StreamMetrics>,
    mut batches: ResMut<Batches>,
    (mut precise, mut split): (ResMut<Precise>, ResMut<Split>),
    mut frame: ResMut<StarFrame>,
    uploads: Res<StarUploads>,
    view: (
        Res<Universe>,
        Res<Navigation>,
        Res<RenderDebug>,
        Res<crate::Clock>,
    ),
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    camera: Query<(&GlobalTransform, &Camera), With<SpaceCamera>>,
) {
    let (universe, nav, debug, clock) = view;
    let sync_start = bevy::platform::time::Instant::now();
    metrics.split_latest = Default::default();
    let Ok((camera_transform, view)) = camera.single() else {
        return;
    };
    let pc_per_unit = pose.render_scale() / PC_METRES;
    let size = view
        .physical_viewport_size()
        .unwrap_or(UVec2::ONE)
        .max(UVec2::ONE);
    let pixel = 2.0 * nav.tan_half_fov() / f64::from(size.y);
    let pixel_sr = pixel * pixel;
    // A point's light over its pixels: radiance gain over a pixel's solid angle.
    let gain = (radiance_gain(&nav, pixel_sr) / pixel_sr) as f32;
    let realistic = if nav.realistic { 1.0 } else { 0.0 };
    let (palette, wireframe) = *batches.palettes.get_or_insert_with(palettes);
    let skip_pixels = if debug.point_pixels { 0.0 } else { 1.0 };
    let radius_px = 2.0;
    frame.globals = Some(Globals {
        settings: Vec4::new(
            pc_per_unit as f32,
            (1.0 / reach_sq_per_luminosity(nav.magnitude)) as f32,
            gain,
            realistic,
        ),
        debug: Vec4::new(skip_pixels, radius_px, 0.0, 0.0),
        palette,
        wireframe,
    });
    frame.draws.clear();
    frame.precise.clear();
    frame.points = debug.point_stars && cfg!(not(target_arch = "wasm32"));
    // Within its body range the visited star is drawn as a body, so the batches
    // hide its point there, and it has no sprite.
    let body = nav.system(&universe.0).and_then(|system| {
        let range = crate::scene::body_range(&nav, system, &pose)?;
        Some((system.page_star(pose.page)?.id.as_str(), range / PC_METRES))
    });
    let body_pc = body.map_or(0.0, |(_, range)| range);
    let origin = rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let eye = origin + DVec3::from_array(home);
    let changed = !field.added.is_empty() || !field.removed.is_empty();
    for dropped in field.removed.drain(..) {
        metrics.loaded_at.remove(&dropped);
        batches.built_at.remove(&dropped);
        for old in [
            batches.active.remove(&dropped),
            batches.fresh.remove(&dropped),
        ]
        .into_iter()
        .flatten()
        {
            release(&uploads, old);
        }
        if batches
            .building
            .as_ref()
            .is_some_and(|building| building.owner == dropped)
        {
            batches.building = None;
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
            let threshold = 0.2 * reach_pc(owner.level(), nav.magnitude);
            (eye.distance(built) > threshold && !batches.queued.iter().any(|(c, _)| *c == owner))
                .then_some(owner)
        })
        .collect();
    batches
        .queued
        .extend(moved.into_iter().map(|cell| (cell, 0)));
    let batches = &mut *batches;
    // A few thousand stars per step: never turn one large cell into a frame stall.
    let start = bevy::platform::time::Instant::now();
    while let Some((owner, offset)) = batches.queued.pop_front() {
        let Some(loaded) = field.cells.get(&owner) else {
            continue;
        };
        let stars = &loaded.stars;
        if offset >= stars.len() {
            continue;
        }
        if offset == 0 {
            if let Some(old) = batches.fresh.remove(&owner) {
                release(&uploads, old);
            }
            if let Some(at) = metrics.loaded_at.remove(&owner) {
                metrics.mesh_wait_ms += at.elapsed().as_secs_f64() * 1000.0;
                metrics.mesh_wait_samples += 1;
            }
            batches.built_at.insert(owner, eye);
        }
        if batches
            .building
            .as_ref()
            .is_none_or(|building| building.owner != owner || offset == 0)
        {
            batches.building = Some(Building {
                owner,
                replacing: batches.active.contains_key(&owner),
                blocks: loaded.blocks(BLOCK),
                block: 0,
                packing: Vec::new(),
            });
        }
        let next_id = &mut batches.next_id;
        let building = batches.building.as_mut().expect("set above");
        while building.blocks[building.block].end <= offset {
            building.block += 1;
        }
        let block = building.blocks[building.block];
        let end = (offset + SLICE).min(block.end);
        pack(
            &mut building.packing,
            &stars[offset..end],
            block.centre_pc,
            eye,
            nav.magnitude,
        );
        metrics.built_batches += 1;
        let done = end == stars.len();
        let mut finished = None;
        if building.packing.len() >= PACK || end == block.end {
            let packing = std::mem::take(&mut building.packing);
            if !packing.is_empty() {
                *next_id += 1;
                let batch = Batch {
                    id: *next_id,
                    stars: packing.len(),
                    centre_pc: block.centre_pc,
                    half_pc: block.half_pc,
                };
                uploads.add(batch.id, packing);
                finished = Some((building.replacing, batch));
            }
        }
        if let Some((replacing, batch)) = finished {
            let into = if replacing {
                &mut batches.fresh
            } else {
                &mut batches.active
            };
            into.entry(owner).or_default().push(batch);
        }
        if done {
            if batches.building.take().is_some_and(|b| b.replacing) {
                if let Some(old) = batches.active.remove(&owner) {
                    release(&uploads, old);
                }
                if let Some(new) = batches.fresh.remove(&owner) {
                    batches.active.insert(owner, new);
                }
            }
        } else {
            batches.queued.push_front((owner, end));
        }
        if start.elapsed() >= UPLOAD_BUDGET {
            break;
        }
    }
    // Camera-relative subtraction in f64 *before* conversion to GPU floats.
    let factor = PC_METRES / pose.render_scale();
    let camera_at = camera_transform.translation().as_dvec3();
    let sight = Sight::new(
        camera_transform,
        nav.tan_half_fov(),
        size,
        f64::from(radius_px) + 2.0,
    );
    // Multiple systems whose stars could show apart: gathered again when the
    // camera has moved, the view narrowed or the cells changed.
    let angle = SPLIT_PX * pixel;
    split.set_enabled(debug.split_multiples && debug.galactic_points);
    if !split.enabled {
        metrics.split_examined = 0;
    } else if changed
        || split.retry_due()
        || angle < split.scanned_angle
        || angle > 4.0 * split.scanned_angle
        || split
            .scanned_at
            .is_none_or(|at| at.distance(eye) > split.slack_pc)
    {
        let search_start = bevy::platform::time::Instant::now();
        metrics.split_examined = split.gather(&field, eye, angle);
        let search_time = search_start.elapsed();
        metrics.split_search += search_time;
        metrics.split_latest.search_ms = search_time.as_secs_f64() * 1000.0;
        metrics.split_searches += 1;
        metrics.split_checked += metrics.split_examined;
    }
    let batch_near = Vec4::new(
        BATCH_HIDE as f32,
        body_pc as f32,
        if debug.split_multiples {
            angle as f32
        } else {
            0.0
        },
        split.reach_pc as f32,
    );
    let (mut uploaded, mut drawn_stars) = (0, 0);
    let (mut blocks, mut blocks_drawn) = (0, 0);
    for entities in batches.active.values() {
        let mut block = None;
        for &Batch {
            id,
            stars,
            centre_pc,
            half_pc,
        } in entities
        {
            let offset = DVec3::from_array(centre_pc) - eye;
            let seen = debug.galactic_points
                && (!debug.cull_blocks || sight.sees(offset, half_pc * 3f64.sqrt()));
            // A block's batches are consecutive and share its centre.
            if block != Some(centre_pc) {
                block = Some(centre_pc);
                blocks += 1;
                blocks_drawn += usize::from(seen);
            }
            uploaded += stars;
            if seen {
                drawn_stars += stars;
                frame.draws.push(StarDraw {
                    batch: Some(id),
                    origin: (camera_at + offset * factor)
                        .as_vec3()
                        .extend(factor as f32),
                    near: batch_near,
                });
            }
        }
    }
    metrics.batch_draws = frame.draws.len();
    metrics.batches = batches.active.values().map(Vec::len).sum();
    metrics.blocks = blocks;
    metrics.blocks_drawn = blocks_drawn;
    metrics.drawn_stars = drawn_stars;
    metrics.uploaded_stars = uploaded;
    // Close up, each star is placed alone by an f64 subtraction. The visited
    // star has no sprite while its body is drawn at the exact position.
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
    // Each multiple system the batches hide as a point, star by star, where
    // its orbits have them now. The visited system is drawn as bodies.
    let split_start = bevy::platform::time::Instant::now();
    metrics.split_sprites = 0;
    metrics.split_offscreen_sprites = 0;
    metrics.split_candidates = split.candidates.len();
    let mut shown_apart = std::collections::HashSet::new();
    if split.built.len() > SPLIT_CACHE {
        split.built.clear();
        metrics.split_cache_clears += 1;
        metrics.split_latest.cache_clears += 1;
    }
    let split = &mut *split;
    for (id, star) in &split.candidates {
        let position = DVec3::from_array(star.position_pc);
        let distance = position.distance(eye);
        let separation =
            instance(star, eye.to_array()).separation_au() * space_simulation::stream::AU_PC;
        if !debug.galactic_points
            || !debug.split_multiples
            || distance >= split.reach_pc
            || separation <= angle * distance
            || body.is_some_and(|(visited, _)| visited == id)
        {
            continue;
        }
        let (system, plane) = split.built.entry(id.clone()).or_insert_with(|| {
            metrics.split_rebuilt += 1;
            metrics.split_latest.rebuilt += 1;
            let start = bevy::platform::time::Instant::now();
            let system = split_system(id);
            metrics.split_latest.generation_ms += start.elapsed().as_secs_f64() * 1000.0;
            system
        });
        let at = position - eye;
        let orbit_start = bevy::platform::time::Instant::now();
        metrics.split_sprites += system.stars.len();
        // Conservative system-sized sphere, not a per-member projection.
        // This measures off-screen work without changing rendering policy.
        if !sight.sees(at, separation) {
            metrics.split_offscreen_sprites += system.stars.len();
        }
        frame
            .precise
            .extend(apart(system, *plane, at, clock.0.seconds));
        metrics.split_latest.orbit_sprite_ms += orbit_start.elapsed().as_secs_f64() * 1000.0;
        shown_apart.insert(id.as_str());
    }
    metrics.split_systems = shown_apart.len();
    metrics.split_cached = split.built.len();
    let preparation_time = split_start.elapsed();
    metrics.split_preparation += preparation_time;
    metrics.split_latest.preparation_ms = preparation_time.as_secs_f64() * 1000.0;
    for candidate in &precise.candidates {
        if shown_apart.contains(candidate.id.as_str()) {
            continue;
        }
        let position = DVec3::from_array(candidate.star.position_pc);
        let radius = (PRECISE * candidate.offset_pc).max(body_pc);
        let drawn = body.is_some_and(|(id, _)| id == candidate.id);
        if position.distance(eye) < radius
            && !drawn
            && debug.galactic_points
            && bright_enough(&candidate.star, eye, nav.magnitude)
        {
            frame
                .precise
                .push(instance(&candidate.star, eye.to_array()));
        }
    }
    metrics.precise_sprites = frame.precise.len();
    if !frame.precise.is_empty() {
        frame.draws.push(StarDraw {
            batch: None,
            origin: camera_at.as_vec3().extend(factor as f32),
            near: Vec4::ZERO,
        });
    }
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
    use space_simulation::population::VISIBLE_MAGNITUDE;

    #[test]
    fn blocks_outside_the_view_are_culled() {
        // Looking down -Z with a 90° vertical field, twice as wide as high.
        let camera = GlobalTransform::IDENTITY;
        let sight = Sight::new(&camera, 1.0, UVec2::new(2000, 1000), 0.0);
        assert!(sight.sees(DVec3::new(0.0, 0.0, -100.0), 1.0));
        assert!(sight.sees(DVec3::new(190.0, 0.0, -100.0), 1.0));
        assert!(!sight.sees(DVec3::new(0.0, 0.0, 100.0), 1.0));
        assert!(!sight.sees(DVec3::new(0.0, 150.0, -100.0), 1.0));
        assert!(!sight.sees(DVec3::new(250.0, 0.0, -100.0), 1.0));
        // A block reaching into the view from outside it, or around the eye.
        assert!(sight.sees(DVec3::new(0.0, 150.0, -100.0), 40.0));
        assert!(sight.sees(DVec3::new(0.0, 0.0, 5.0), 10.0));
    }

    #[test]
    fn batches_only_upload_stars_near_enough_to_see() {
        let eye = DVec3::new(8200.0, 20.0, 0.0);
        let point = |distance: f64| StarPoint {
            position_pc: [8200.0 + distance, 20.0, 0.0],
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            separation_au: 0.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let mut stars = Vec::new();
        pack(
            &mut stars,
            &[point(10.0), point(100.0)],
            [8200.0, 0.0, 0.0],
            eye,
            VISIBLE_MAGNITUDE,
        );
        assert_eq!(stars.len(), 1);
        assert_eq!(stars[0].position, [10.0, 20.0, 0.0]);
        stars.clear();
        pack(
            &mut stars,
            &[point(100.0)],
            [8200.0, 0.0, 0.0],
            eye,
            VISIBLE_MAGNITUDE,
        );
        assert!(stars.is_empty());
    }

    /// Repeats the GPU's f32 placement of a batched point: its f32 offset
    /// from the block centre, scaled, plus the centre's camera-relative offset.
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
            separation_au: 0.0,
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
            separation_au: 0.0,
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

    #[test]
    fn re_enabling_splitting_discards_the_old_search_state() {
        let mut split = Split {
            reach_pc: 0.0,
            next_reach_pc: SPLIT_MIN_REACH_PC,
            scanned_at: Some(DVec3::ZERO),
            gathered_at: Some(bevy::platform::time::Instant::now()),
            ..Split::default()
        };
        split.set_enabled(false);
        assert!(!split.enabled);
        assert!(split.scanned_at.is_none());
        split.set_enabled(true);
        assert!(split.enabled);
        assert_eq!(split.next_reach_pc, SPLIT_REACH_PC);
        assert!(split.scanned_at.is_none());
        assert!(split.gathered_at.is_none());
        // Unchanged toggles must not invalidate useful cached work each frame.
        split.scanned_at = Some(DVec3::ZERO);
        split.set_enabled(true);
        assert_eq!(split.scanned_at, Some(DVec3::ZERO));
    }

    #[test]
    fn empty_split_search_expands_despite_a_busy_grid_leaf() {
        let cell = Cell::containing(16, [0.01; 3]).unwrap();
        let eye = DVec3::splat(0.01);
        let point = StarPoint {
            position_pc: (eye + DVec3::X * 0.01).to_array(),
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            separation_au: 1000.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            space_simulation::stream::StarCell::new(cell, vec![point; 20_000]),
        );
        let mut split = Split {
            next_reach_pc: 0.001,
            ..Split::default()
        };
        assert_eq!(split.gather(&field, eye, 1e-4), 20_000);
        assert!(split.candidates.is_empty());
        assert!(split.next_reach_pc > split.reach_pc);
        split.gathered_at = Some(bevy::platform::time::Instant::now() - SPLIT_RETRY);
        assert!(split.retry_due());
    }

    #[test]
    fn overloaded_split_search_recovers_without_camera_movement() {
        let cell = Cell::containing(16, [0.01; 3]).unwrap();
        let eye = DVec3::splat(0.01);
        let point = StarPoint {
            position_pc: eye.to_array(),
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            separation_au: 1000.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let mut field = Starfield::default();
        field.cells.insert(
            cell,
            space_simulation::stream::StarCell::new(cell, vec![point; SPLIT_BUDGET + 1]),
        );
        let mut split = Split::default();
        assert_eq!(
            split.gather(&field, eye, 1e-4),
            SPLIT_ATTEMPTS * (SPLIT_BUDGET + 1)
        );
        assert_eq!(split.reach_pc, 0.0);
        assert!(split.next_reach_pc > 0.0);
        assert!(split.candidates.is_empty());
        split.gathered_at = Some(bevy::platform::time::Instant::now() - SPLIT_RETRY);
        assert!(split.retry_due());
        // A cheaper neighbourhood, without a camera or view change, recovers.
        field.cells.insert(
            cell,
            space_simulation::stream::StarCell::new(cell, vec![point]),
        );
        assert_eq!(split.gather(&field, eye, 1e-4), 1);
        assert!(split.reach_pc > 0.0);
        assert_eq!(split.candidates.len(), 1);
        assert!(split.next_reach_pc > split.reach_pc);
        for _ in 0..20 {
            split.gather(&field, eye, 1e-4);
        }
        assert_eq!(split.reach_pc, SPLIT_REACH_PC);
        assert!(!split.retry_due());
    }

    #[test]
    fn dense_split_search_shrinks_its_margin_and_keeps_nearby_systems() {
        let cell = Cell::containing(16, [0.01; 3]).unwrap();
        let eye = DVec3::splat(0.01);
        let nearby = StarPoint {
            position_pc: (eye + DVec3::X * 0.001).to_array(),
            luminosity_solar: 1.0,
            temperature_k: 5772.0,
            radius_solar: 1.0,
            separation_au: 1000.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        };
        let distant = StarPoint {
            position_pc: (eye + DVec3::X * 0.8).to_array(),
            ..nearby
        };
        let mut points = vec![distant; SPLIT_BUDGET + 1];
        points.push(nearby);
        let mut field = Starfield::default();
        field
            .cells
            .insert(cell, space_simulation::stream::StarCell::new(cell, points));
        let mut split = Split::default();
        for _ in 0..4 {
            split.gather(&field, eye, 1e-4);
        }
        assert!(split.reach_pc > 0.0);
        assert!(split.reach_pc < 1.0);
        assert!(split.slack_pc < RESCAN_PC);
        assert_eq!(split.candidates.len(), 1);
        assert_eq!(split.candidates[0].1, nearby);
    }

    #[test]
    fn a_split_system_is_its_stars_where_their_bodies_are_drawn() {
        let id = "g2e4960.3.6000";
        let (system, plane) = split_system(id);
        assert_eq!(system.stars.len(), 3);
        // The same plane as the system's page when it is visited.
        let visited = crate::galaxy_nav::generated(id, DVec3::ZERO).unwrap();
        assert_eq!(plane, crate::scene::page_frame(&visited, 0));
        let at = DVec3::new(30.0, -2.0, 5.0);
        let seconds = 1.5e10;
        let stars: Vec<_> = apart(&system, plane, at, seconds).collect();
        let mut light = 0.0;
        for (star, body) in stars.iter().zip(&system.stars) {
            let local = space_simulation::member_position(&visited, 0, &body.id, seconds);
            let expected = at + plane * DVec3::from_array(local) / PC_METRES;
            let drawn = Vec3::from_array(star.position).as_dvec3();
            assert!(drawn.distance(expected) < 1e-5, "{drawn} {expected}");
            light += f64::from(star.luminosity);
            // Never hidden again as a multiple.
            assert!(star.separation_au() < 1e-15);
        }
        let point: f64 = visited.stars.iter().map(|s| s.luminosity_solar).sum();
        assert!((light / point - 1.0).abs() < 1e-6);
        // C is over a thousand AU from the close pair.
        let separation = space_simulation::multiple::widest_separation_au(&system.barycentres);
        assert!(separation > 1_000.0);
        let packed = StarInstance::new([0.0; 3], 1.0, 5772.0, separation as f32);
        assert!((packed.separation_au() / separation - 1.0).abs() < 2e-3);
    }
}
