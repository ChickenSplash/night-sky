//! Batched, camera-relative galactic points. A batch holds many stars, not
//! one entity or material per star, uploaded in bounded chunks and drawn by
//! `stars::render` as one instance per star.

mod batches;
mod precise;
mod split;
mod view;

use batches::{Batch, Batches};
use precise::{BATCH_HIDE, Precise, star_position};
use split::SPLIT_PX;
pub(super) use split::Split;
use view::Sight;

use crate::{
    Navigation, Universe,
    render::Photometry,
    render::debug::RenderDebug,
    render::sky::radiance_gain,
    scene::{CameraPose, CameraRig, SpaceCamera},
    stars::render::{
        Globals, PALETTE, SHADER, StarDraw, StarFrame, StarInstance, StarRenderPlugin, StarUploads,
    },
    stars::stream::{Starfield, StreamMetrics, SyncFrame},
};
use bevy::{asset::load_internal_asset, math::DVec3, prelude::*};
use space_simulation::{
    galaxy::PC_METRES, galaxy::population::reach_sq_per_luminosity, galaxy::stream::StarPoint,
    light::visual_efficiency,
};

const MARGIN: f64 = 1.25;
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

/// Squared parsecs per solar luminosity within which a star is bright enough
/// to upload under the `magnitude` limit, for `bright_enough`.
fn visible_reach_sq(magnitude: f64) -> f64 {
    MARGIN * MARGIN * reach_sq_per_luminosity(magnitude)
}

/// Whether a star is bright enough to upload, `distance_pc` away, with
/// `reach_sq` from `visible_reach_sq`.
fn bright_enough(star: &StarPoint, distance_pc: f64, reach_sq: f64) -> bool {
    distance_pc * distance_pc <= f64::from(star.luminosity_solar) * reach_sq
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

/// `StarDraw::hide` for the star at `at` in a batch from `centre_pc`: packed
/// as `instance` packs it, so the shader's comparison is exact.
fn hide(at: [f64; 3], centre_pc: [f64; 3]) -> Vec4 {
    Vec3::from_array(std::array::from_fn(|axis| {
        (at[axis] - centre_pc[axis]) as f32
    }))
    .extend(1.0)
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
        app.add_plugins(crate::stars::points::StarPointsPlugin);
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
    let (render_extract_ms, render_prepare_ms) = uploads.render_timings();
    metrics.sync_latest = SyncFrame {
        render_extract_ms,
        render_prepare_ms,
        ..Default::default()
    };
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
    frame.points = debug.point_stars;
    // Within its body range the visited star is drawn as a body, so the batches
    // hide its point, and it has no sprite. Other stars there are unaffected.
    let visited = nav.system(&universe.0).and_then(|system| {
        crate::scene::body_range(&nav, system, &pose)?;
        Some(system.page_star(pose.page)?.id.as_str())
    });
    let body = visited.and_then(space_simulation::galaxy::cells::parse_cell_star);
    let origin = rig.galactic_ly(&pose) * (space_simulation::LY_METRES / PC_METRES);
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let eye = origin + DVec3::from_array(home);
    let changed = !field.added.is_empty() || !field.removed.is_empty();
    let upload_start = bevy::platform::time::Instant::now();
    batches.update(&mut field, &mut metrics, &uploads, eye, nav.magnitude);
    metrics.sync_latest.upload_ms = upload_start.elapsed().as_secs_f64() * 1000.0;
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
    split.update(
        &field,
        eye,
        angle,
        changed,
        debug.split_multiples && debug.galactic_points,
        &mut metrics,
    );
    let draws_start = bevy::platform::time::Instant::now();
    // Found once per visit; retried while its cell has not loaded.
    if precise
        .visited
        .is_none_or(|(key, at)| Some(key) != body || (at.is_none() && changed))
    {
        precise.visited = body.map(|key| (key, star_position(&field, key)));
    }
    let hidden = precise.visited.and_then(|(_, at)| at);
    let batch_near = Vec4::new(
        BATCH_HIDE as f32,
        0.0,
        if debug.split_multiples {
            angle as f32
        } else {
            0.0
        },
        split.reach_pc as f32,
    );
    let (mut uploaded, mut drawn_stars) = (0, 0);
    let (mut blocks, mut blocks_drawn) = (0, 0);
    for (&owner, entities) in &batches.active {
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
                    hide: hidden
                        .filter(|_| body.is_some_and(|(cell, _, _)| cell == owner))
                        .map_or(Vec4::ZERO, |at| hide(at, centre_pc)),
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
    metrics.sync_latest.draws_ms = draws_start.elapsed().as_secs_f64() * 1000.0;
    // Close up, each star is placed alone by an f64 subtraction. The visited
    // star has no sprite while its body is drawn at the exact position.
    precise.scan(&field, eye, nav.magnitude, changed, &mut metrics);
    // Each multiple system the batches hide as a point, star by star, where
    // its orbits have them now. The visited system is drawn as bodies.
    let shown_apart = split.prepare(
        (eye, angle, debug.galactic_points && debug.split_multiples),
        visited,
        clock.0.seconds,
        &sight,
        &mut frame,
        &mut metrics,
    );
    precise.draw(
        (eye, nav.magnitude, debug.galactic_points),
        body,
        &shown_apart,
        &mut frame,
        &mut metrics,
    );
    if !frame.precise.is_empty() {
        frame.draws.push(StarDraw {
            batch: None,
            origin: camera_at.as_vec3().extend(factor as f32),
            near: Vec4::ZERO,
            hide: Vec4::ZERO,
        });
    }
    metrics.mesh_queue = batches.queued.len();
    metrics.oldest_mesh_ms = batches
        .queued
        .iter()
        .filter_map(|(cell, _)| metrics.loaded_at.get(cell))
        .map(|at| at.elapsed().as_secs_f64() * 1000.0)
        .fold(0.0, f64::max);
    let sync_time = sync_start.elapsed();
    metrics.rendering += sync_time;
    metrics.sync_latest.total_ms = sync_time.as_secs_f64() * 1000.0;
}
