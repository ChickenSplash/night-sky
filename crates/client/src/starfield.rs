//! Batched, camera-relative galactic points. A batch holds many stars, not
//! one entity or material per star, uploaded in bounded chunks and drawn by
//! `starfield_render` as one instance per star.

use crate::{
    Navigation, Universe,
    render::Photometry,
    render_debug::RenderDebug,
    scene::{CameraPose, CameraRig, SpaceCamera},
    sky::{Sky, radiance_gain},
    starfield_render::{
        Globals, PALETTE, SHADER, StarDraw, StarFrame, StarInstance, StarRenderPlugin, StarUploads,
    },
    stream::{Starfield, StreamMetrics},
};
use bevy::{asset::load_internal_asset, math::DVec3, prelude::*};
use space_simulation::{
    cells::Cell,
    galaxy::PC_METRES,
    light::visual_efficiency,
    population::{reach_pc, reach_sq_per_luminosity},
    stream::{Block, StarCell, StarPoint, gap_pc},
};
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
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
/// A retained cell is rebuilt after the camera travels this share of its
/// tier's reach.
const REBUILD: f64 = 0.2;
/// Stars merged into one point share a cube whose diagonal spans at most this
/// many pixels, even from the closest the camera comes before a rebuild.
const MERGE_PX: f64 = 0.5;
/// A cell with merged stars is rebuilt once a pixel shrinks below this share
/// of the one it was merged for (zooming in, or a taller window).
const MERGE_ZOOM: f64 = 0.8;

/// Each temperature's realistic colour with unit luminance and visual
/// efficiency (light per bolometric luminosity, as the eye and the exposure
/// see it), then its assisted colour, linear.
fn palettes(physical: bool) -> ([Vec4; PALETTE], [Vec4; PALETTE]) {
    let photometry = Photometry::default();
    let kelvin = |i: usize| 1000.0 * 50f64.powf(i as f64 / (PALETTE - 1) as f64);
    let realistic = std::array::from_fn(|i| {
        let [r, g, b] = photometry.colour(kelvin(i), physical).map(|c| c as f32);
        Vec4::new(r, g, b, visual_efficiency(kelvin(i)) as f32)
    });
    let assisted = std::array::from_fn(|i| {
        let [r, g, b] = space_simulation::spectral_colour(kelvin(i));
        let linear = Color::srgb_u8(r, g, b).to_linear();
        Vec4::new(linear.red, linear.green, linear.blue, 1.0)
    });
    (realistic, assisted)
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
    /// Stars merged into its other points.
    merged: usize,
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
    StarInstance {
        position: std::array::from_fn(|axis| (star.position_pc[axis] - origin_pc[axis]) as f32),
        luminosity: star.luminosity_solar,
        temperature: star.temperature_k,
    }
}

/// How one cell's stars are filtered and merged, fixed when its build starts.
#[derive(Clone, Copy)]
struct Filter {
    eye: DVec3,
    magnitude: f64,
    /// Radians per pixel, or zero not to merge.
    pixel: f64,
    /// How far the camera may move before the cell is rebuilt.
    slack_pc: f64,
    /// Visual efficiency by temperature, as the shader's palette.
    efficiency: [f32; PALETTE],
}

impl Filter {
    /// Visual efficiency at `kelvin`, interpolated as the shader does.
    fn efficiency(&self, kelvin: f32) -> f64 {
        let last = (PALETTE - 1) as f32;
        let x = ((kelvin / 1000.0).ln() / 50f32.ln() * last).clamp(0.0, last);
        let i = (x as usize).min(PALETTE - 2);
        let t = x - i as f32;
        f64::from(self.efficiency[i] * (1.0 - t) + self.efficiency[i + 1] * t)
    }
}

/// Stars bright enough to see from the filter's eye, as offsets from `centre`.
/// Within each grid box of `range` (whose ends are box boundaries), visible
/// stars sharing a cube smaller than `MERGE_PX` become one point with their
/// visual light, at their light's centre, with its mean temperature. Returns
/// the stars merged away.
fn pack(
    out: &mut Vec<StarInstance>,
    cell: &StarCell,
    range: Range<usize>,
    centre: [f64; 3],
    filter: &Filter,
) -> usize {
    let reach_sq = reach_sq_per_luminosity(filter.magnitude);
    let mut merged = 0;
    let mut keyed: Vec<([i64; 3], &StarPoint)> = Vec::new();
    for run in cell.box_runs(range) {
        keyed.clear();
        let mut nearest = f64::INFINITY;
        for star in &cell.stars[run] {
            if bright_enough(star, filter.eye, filter.magnitude) {
                let distance = DVec3::from_array(star.position_pc).distance(filter.eye);
                nearest = nearest.min(distance);
                keyed.push(([0; 3], star));
            }
        }
        let cube = MERGE_PX * filter.pixel * (nearest - filter.slack_pc) / 3f64.sqrt();
        // Far below the spacing of any stars, and keys must not overflow.
        if keyed.len() < 2 || cube < 1e-6 {
            out.extend(keyed.iter().map(|(_, star)| instance(star, centre)));
            continue;
        }
        for (index, (key, star)) in keyed.iter_mut().enumerate() {
            let distance = DVec3::from_array(star.position_pc).distance(filter.eye);
            // Only stars visible wherever the camera is before the rebuild,
            // so merging never shows one the shader would cut off.
            let stays =
                f64::from(star.luminosity_solar) * reach_sq >= (distance + filter.slack_pc).powi(2);
            *key = if stays {
                star.position_pc.map(|p| (p / cube).floor() as i64)
            } else {
                [i64::MIN, index as i64, 0]
            };
        }
        keyed.sort_unstable_by_key(|(key, _)| *key);
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            if let [(_, star)] = group {
                out.push(instance(star, centre));
                continue;
            }
            let (mut light, mut log_kelvin, mut position) = (0.0, 0.0, DVec3::ZERO);
            for (_, star) in group {
                let visual =
                    f64::from(star.luminosity_solar) * filter.efficiency(star.temperature_k);
                light += visual;
                log_kelvin += visual * f64::from(star.temperature_k).ln();
                position += visual * DVec3::from_array(star.position_pc);
            }
            let temperature = (log_kelvin / light).exp() as f32;
            let point = StarPoint {
                position_pc: (position / light).to_array(),
                luminosity_solar: (light / filter.efficiency(temperature)) as f32,
                temperature_k: temperature,
                ..*group[0].1
            };
            out.push(instance(&point, centre));
            merged += group.len() - 1;
        }
    }
    merged
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
    /// Stars merged into `packing`.
    merged: usize,
    filter: Filter,
}

#[derive(Resource, Default)]
struct Batches {
    active: HashMap<Cell, Vec<Batch>>,
    /// Finished batches of a rebuild still in progress, not yet drawn.
    fresh: HashMap<Cell, Vec<Batch>>,
    building: Option<Building>,
    queued: VecDeque<(Cell, usize)>,
    /// Where each cell was built from, and the pixel its merging assumed
    /// (zero unmerged).
    built_at: HashMap<Cell, (DVec3, f64)>,
    /// Whether the batches merge stars (`RenderDebug::merge_stars`).
    merging: bool,
    next_id: u64,
    /// The colour tables, and whether they are the physical ones.
    palettes: Option<(bool, [Vec4; PALETTE], [Vec4; PALETTE])>,
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

pub struct StarfieldRenderPlugin;

impl Plugin for StarfieldRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "starfield.wgsl", Shader::from_wgsl);
        app.add_plugins(StarRenderPlugin)
            .init_resource::<Batches>()
            .init_resource::<Precise>()
            .add_systems(PostUpdate, sync.after(crate::sky::expose));
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
    mut precise: ResMut<Precise>,
    mut frame: ResMut<StarFrame>,
    uploads: Res<StarUploads>,
    view: (Res<Sky>, Res<Universe>, Res<Navigation>, Res<RenderDebug>),
    pose: Res<CameraPose>,
    rig: Res<CameraRig>,
    camera: Query<(&GlobalTransform, &Camera), With<SpaceCamera>>,
) {
    let (sky, universe, nav, debug) = view;
    let sync_start = bevy::platform::time::Instant::now();
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
    let gain = (radiance_gain(&nav, &sky, pixel_sr) / pixel_sr) as f32;
    let realistic = if nav.realistic { 1.0 } else { 0.0 };
    if batches
        .palettes
        .as_ref()
        .is_none_or(|(physical, ..)| *physical != nav.physical)
    {
        let (realistic, assisted) = palettes(nav.physical);
        batches.palettes = Some((nav.physical, realistic, assisted));
    }
    let skip_pixels = if debug.point_pixels { 0.0 } else { 1.0 };
    let radius_px = debug.point_radius_px();
    let (_, palette, assisted) = batches.palettes.expect("set above");
    frame.globals = Some(Globals {
        settings: Vec4::new(
            pc_per_unit as f32,
            (1.0 / reach_sq_per_luminosity(nav.magnitude)) as f32,
            gain,
            realistic,
        ),
        debug: Vec4::new(skip_pixels, radius_px, 0.0, 0.0),
        palette,
        assisted,
    });
    frame.draws.clear();
    frame.precise.clear();
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
    // Switching merging rebuilds everything.
    let remerge = std::mem::replace(&mut batches.merging, debug.merge_stars) != debug.merge_stars;
    let merge_pixel = if debug.merge_stars { pixel } else { 0.0 };
    let moved: Vec<Cell> = batches
        .built_at
        .iter()
        .filter_map(|(&owner, &(built, built_pixel))| {
            let threshold = REBUILD * reach_pc(owner.level(), nav.magnitude);
            let zoomed = merge_pixel < MERGE_ZOOM * built_pixel;
            ((remerge || zoomed || eye.distance(built) > threshold)
                && !batches.queued.iter().any(|(c, _)| *c == owner))
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
            batches.built_at.insert(owner, (eye, merge_pixel));
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
                merged: 0,
                filter: Filter {
                    eye,
                    magnitude: nav.magnitude,
                    pixel: merge_pixel,
                    slack_pc: REBUILD * reach_pc(owner.level(), nav.magnitude),
                    efficiency: palette.map(|entry| entry.w),
                },
            });
        }
        let next_id = &mut batches.next_id;
        let building = batches.building.as_mut().expect("set above");
        while building.blocks[building.block].end <= offset {
            building.block += 1;
        }
        let block = building.blocks[building.block];
        let end = loaded.box_boundary(offset + SLICE).min(block.end);
        building.merged += pack(
            &mut building.packing,
            loaded,
            offset..end,
            block.centre_pc,
            &building.filter,
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
                    merged: std::mem::take(&mut building.merged),
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
    let batch_near = Vec4::new(BATCH_HIDE as f32, body_pc as f32, 0.0, 0.0);
    let (mut uploaded, mut drawn_stars) = (0, 0);
    let (mut blocks, mut blocks_drawn) = (0, 0);
    for entities in batches.active.values() {
        let mut block = None;
        for &Batch {
            id,
            stars,
            centre_pc,
            half_pc,
            ..
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
    metrics.merged_stars = batches.active.values().flatten().map(|b| b.merged).sum();
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
    for candidate in &precise.candidates {
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

    /// Packs every star of a level-10 cell (128 pc) around home.
    fn packed(points: Vec<StarPoint>, filter: &Filter) -> (Vec<StarInstance>, usize) {
        let cell = Cell::containing(10, [8250.0, 20.0, 0.0]).unwrap();
        let cell = StarCell::new(cell, points);
        let mut out = Vec::new();
        let merged = pack(
            &mut out,
            &cell,
            0..cell.stars.len(),
            [8200.0, 0.0, 0.0],
            filter,
        );
        (out, merged)
    }

    fn filter(pixel: f64, slack_pc: f64) -> Filter {
        Filter {
            eye: DVec3::new(8200.0, 20.0, 0.0),
            magnitude: VISIBLE_MAGNITUDE,
            pixel,
            slack_pc,
            efficiency: palettes(true).0.map(|entry| entry.w),
        }
    }

    fn star(x: f64, luminosity_solar: f32, temperature_k: f32) -> StarPoint {
        StarPoint {
            position_pc: [x, 20.0, 0.0],
            luminosity_solar,
            temperature_k,
            radius_solar: 1.0,
            component: space_simulation::galaxy::Component::ThinDisc,
            index: 0,
        }
    }

    #[test]
    fn batches_only_upload_stars_near_enough_to_see() {
        let (stars, _) = packed(
            vec![star(8210.0, 1.0, 5772.0), star(8300.0, 1.0, 5772.0)],
            &filter(0.0, 0.0),
        );
        assert_eq!(stars.len(), 1);
        assert_eq!(stars[0].position, [10.0, 20.0, 0.0]);
        let (stars, _) = packed(vec![star(8300.0, 1.0, 5772.0)], &filter(0.0, 0.0));
        assert!(stars.is_empty());
    }

    #[test]
    fn stars_sharing_a_sub_pixel_cube_merge_keeping_their_visual_light() {
        let stars = vec![
            star(8300.0, 1000.0, 4000.0),
            star(8300.0, 3000.0, 9000.0),
            // A pixel is 0.1 pc here: a parsec apart stays apart.
            star(8301.0, 1000.0, 5772.0),
        ];
        let (unmerged, merged) = packed(stars.clone(), &filter(0.0, 0.0));
        assert_eq!((unmerged.len(), merged), (3, 0));
        let at = filter(1e-3, 0.0);
        let (points, merged) = packed(stars, &at);
        assert_eq!((points.len(), merged), (2, 1));
        let both = points.iter().find(|p| p.position[0] < 100.5).unwrap();
        let visual = |l: f32, t: f32| f64::from(l) * at.efficiency(t);
        let expected = visual(1000.0, 4000.0) + visual(3000.0, 9000.0);
        let light = visual(both.luminosity, both.temperature);
        assert!((light / expected - 1.0).abs() < 1e-5, "{light} {expected}");
        assert!(both.temperature > 4000.0 && both.temperature < 9000.0);
        assert!((both.position[0] - 100.0).abs() < 1e-4);
    }

    #[test]
    fn stars_near_their_cutoff_or_the_camera_never_merge() {
        // Visible from here (with the upload margin), but not from 10 pc
        // farther, where the camera may go before a rebuild.
        let faint = vec![star(8300.0, 25.0, 5772.0), star(8300.0, 25.0, 5772.0)];
        let (points, merged) = packed(faint, &filter(1e-3, 10.0));
        assert_eq!((points.len(), merged), (2, 0));
        // Merging assumes the camera can come `slack_pc` closer.
        let close = vec![star(8300.0, 1000.0, 5772.0), star(8300.0, 1000.0, 5772.0)];
        assert_eq!(packed(close.clone(), &filter(1e-3, 100.0)).1, 0);
        assert_eq!(packed(close, &filter(1e-3, 50.0)).1, 1);
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
