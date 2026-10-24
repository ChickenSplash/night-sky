//! Budgeted batch packing, replacement and GPU upload lifetime.
use super::{bright_enough, instance, visible_reach_sq};
use crate::stars::{
    render::{PALETTE, StarInstance, StarUploads},
    stream::{Starfield, StreamMetrics},
};
use bevy::{math::DVec3, prelude::*};
use space_simulation::galaxy::{
    cells::Cell,
    population::reach_pc,
    stream::{Block, StarPoint},
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
pub(super) const BLOCK: usize = 4 * PACK;
const UPLOAD_BUDGET: Duration = Duration::from_millis(3);
/// One uploaded batch.
pub(super) struct Batch {
    pub(super) id: u64,
    /// Stars uploaded, one instance each.
    pub(super) stars: usize,
    /// Its block's centre, from which its stars are offsets.
    pub(super) centre_pc: [f64; 3],
    /// Half its block's edge.
    pub(super) half_pc: f64,
}

/// Stars bright enough to see from `eye`, as offsets from `centre`.
fn pack(
    stars: &mut Vec<StarInstance>,
    points: &[StarPoint],
    centre: [f64; 3],
    eye: DVec3,
    magnitude: f64,
) {
    let reach_sq = visible_reach_sq(magnitude);
    stars.extend(
        points
            .iter()
            .filter(|star| {
                bright_enough(
                    star,
                    DVec3::from_array(star.position_pc).distance(eye),
                    reach_sq,
                )
            })
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
pub(super) struct Batches {
    pub(super) active: HashMap<Cell, Vec<Batch>>,
    /// Finished batches of a rebuild still in progress, not yet drawn.
    fresh: HashMap<Cell, Vec<Batch>>,
    building: Option<Building>,
    pub(super) queued: VecDeque<(Cell, usize)>,
    built_at: HashMap<Cell, DVec3>,
    next_id: u64,
    /// Physical colours and the unchanged wireframe palette.
    pub(super) palettes: Option<([Vec4; PALETTE], [Vec4; PALETTE])>,
}

fn release(uploads: &StarUploads, batches: Vec<Batch>) {
    for batch in batches {
        uploads.remove(batch.id);
    }
}

impl Batches {
    pub(super) fn update(
        &mut self,
        field: &mut Starfield,
        metrics: &mut StreamMetrics,
        uploads: &StarUploads,
        eye: DVec3,
        magnitude: f64,
    ) {
        let batches = self;
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
                release(uploads, old);
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
                let threshold = 0.2 * reach_pc(owner.level(), magnitude);
                (eye.distance(built) > threshold
                    && !batches.queued.iter().any(|(c, _)| *c == owner))
                .then_some(owner)
            })
            .collect();
        batches
            .queued
            .extend(moved.into_iter().map(|cell| (cell, 0)));
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
                    release(uploads, old);
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
                magnitude,
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
                        release(uploads, old);
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
    }
}

#[cfg(test)]
mod tests;
