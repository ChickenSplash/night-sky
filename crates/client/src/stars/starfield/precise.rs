//! Nearby-star candidate scanning and camera-relative precise sprites.
use super::batches::BLOCK;
use super::{bright_enough, instance, visible_reach_sq};
use crate::stars::{
    render::StarFrame,
    stream::{Starfield, StreamMetrics},
};
use bevy::{math::DVec3, prelude::*};
use space_simulation::galaxy::{
    cells::Cell,
    stream::{StarPoint, gap_pc},
};
use std::collections::HashSet;

/// A point closer than this many times its offset from its block's centre gets
/// a precise sprite. Batch vertices are f32 offsets from that centre, so their
/// error grows with the offset; beyond here it stays below a quarter pixel.
const PRECISE: f64 = 2e-3;
/// Keep the batched point while its precise replacement becomes visible. The
/// former is accurate to around a pixel at this cutoff; without overlap a
/// hidden sprite leaves a hole light-years from a bright star.
pub(super) const BATCH_HIDE: f64 = PRECISE * 0.25;
/// Precise candidates are gathered this many times farther out than they
/// show, so a sprite is ready before it is needed.
const PREPARE: f64 = 2.0;
/// Precise candidates are gathered again after the camera moves this far.
pub(super) const RESCAN_PC: f64 = 0.1;
/// A star that may come close enough to need a precise sprite.
struct Candidate {
    cell: Cell,
    star: StarPoint,
    /// Parsecs from its block's centre.
    offset_pc: f64,
}

impl Candidate {
    fn is(&self, (cell, component, index): StarRef) -> bool {
        self.cell == cell && self.star.component == component && u64::from(self.star.index) == index
    }
}

/// A cell star by its ID's parts, without the string.
pub(super) type StarRef = (Cell, space_simulation::galaxy::Component, u64);

/// Stars near the camera, each placed by an f64 subtraction every frame.
#[derive(Resource, Default)]
pub(super) struct Precise {
    scanned_at: Option<DVec3>,
    scanned_magnitude: f64,
    candidates: Vec<Candidate>,
    /// The visited star and, once its cell has loaded, its position, so
    /// batches can hide its point while its body is drawn.
    pub(super) visited: Option<(StarRef, Option<[f64; 3]>)>,
}

/// Where a cell star is, if its cell is loaded.
pub(super) fn star_position(
    field: &Starfield,
    (cell, component, index): StarRef,
) -> Option<[f64; 3]> {
    field.cells.get(&cell)?.stars.iter().find_map(|star| {
        (star.component == component && u64::from(star.index) == index).then_some(star.position_pc)
    })
}

/// Stars that could need a precise sprite before the camera moves
/// `RESCAN_PC`: those that could be bright enough under the `magnitude`
/// limit from anywhere that close.
fn candidates(field: &Starfield, eye: DVec3, magnitude: f64) -> Vec<Candidate> {
    let reach_sq = visible_reach_sq(magnitude);
    let mut out = Vec::new();
    for (&cell, stars) in &field.cells {
        let half_diagonal = cell.size_pc() * 0.5 * 3.0_f64.sqrt();
        let reach = PREPARE * PRECISE * half_diagonal + RESCAN_PC;
        if gap_pc(cell, eye.to_array()) > reach {
            continue;
        }
        for block in stars.blocks(BLOCK) {
            let reach = PREPARE * PRECISE * block.half_diagonal_pc() + RESCAN_PC;
            let centre = DVec3::from_array(block.centre_pc);
            stars.near_in(&block, eye.to_array(), reach, |_, star| {
                let position = DVec3::from_array(star.position_pc);
                let offset_pc = position.distance(centre);
                let distance = position.distance(eye);
                if distance < PREPARE * PRECISE * offset_pc + RESCAN_PC
                    && bright_enough(star, (distance - RESCAN_PC).max(0.0), reach_sq)
                {
                    out.push(Candidate {
                        cell,
                        star: *star,
                        offset_pc,
                    });
                }
            });
        }
    }
    out
}

impl Precise {
    pub(super) fn scan(
        &mut self,
        field: &Starfield,
        eye: DVec3,
        magnitude: f64,
        changed: bool,
        metrics: &mut StreamMetrics,
    ) {
        let precise = self;
        if changed
            || precise.scanned_magnitude != magnitude
            || precise
                .scanned_at
                .is_none_or(|at| at.distance(eye) > RESCAN_PC)
        {
            let scan_start = bevy::platform::time::Instant::now();
            precise.candidates = candidates(field, eye, magnitude);
            precise.scanned_at = Some(eye);
            precise.scanned_magnitude = magnitude;
            metrics.sync_latest.precise_scan_ms = scan_start.elapsed().as_secs_f64() * 1000.0;
            metrics.sync_latest.precise_rescans = 1;
        }
        metrics.sync_latest.precise_candidates = precise.candidates.len();
    }

    pub(super) fn draw(
        &self,
        view: (DVec3, f64, bool),
        body: Option<StarRef>,
        shown_apart: &HashSet<&str>,
        frame: &mut StarFrame,
        metrics: &mut StreamMetrics,
    ) {
        let (eye, magnitude, enabled) = view;
        let precise = self;
        let precise_start = bevy::platform::time::Instant::now();
        let split_sprites = frame.precise.len();
        let reach_sq = visible_reach_sq(magnitude);
        for candidate in &precise.candidates {
            let distance = DVec3::from_array(candidate.star.position_pc).distance(eye);
            if !enabled
                || distance >= PRECISE * candidate.offset_pc
                || body.is_some_and(|key| candidate.is(key))
                || !bright_enough(&candidate.star, distance, reach_sq)
            {
                continue;
            }
            // Only multiple systems are split, so only they need their ID.
            if candidate.star.separation_au > 0.0
                && !shown_apart.is_empty()
                && shown_apart.contains(candidate.star.id(candidate.cell).as_str())
            {
                continue;
            }
            frame
                .precise
                .push(instance(&candidate.star, eye.to_array()));
        }
        metrics.precise_sprites = frame.precise.len();
        metrics.sync_latest.precise_sprites = frame.precise.len() - split_sprites;
        metrics.sync_latest.precise_ms = precise_start.elapsed().as_secs_f64() * 1000.0;
    }
}

#[cfg(test)]
mod tests;
