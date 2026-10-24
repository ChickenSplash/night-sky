//! Adaptive multiple-system resolution and cached component-star sprites.
use super::{instance, view::Sight};
use crate::stars::{
    render::{StarFrame, StarInstance},
    stream::{Starfield, StreamMetrics},
};
use bevy::{math::DVec3, prelude::*};
use space_simulation::galaxy::{
    PC_METRES,
    stream::{StarPoint, gap_pc},
};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

/// A multiple system is one point until its stars could appear this many
/// pixels apart, well inside the point spread, so splitting never shows.
pub(super) const SPLIT_PX: f64 = 0.5;
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
/// Systems kept built for splitting, beyond those split this frame, before
/// those not split this frame are dropped.
const SPLIT_CACHE: usize = 4_096;

/// Multiple systems that may show their stars apart, gathered now and then,
/// and the stars of those that do, built once.
#[derive(Resource)]
pub(in crate::stars) struct Split {
    enabled: bool,
    /// Systems are split only within this distance, in parsecs; the batches
    /// hide them as points only within it too.
    pub(super) reach_pc: f64,
    /// The reach for the next gather: shrinks where gathers find too many
    /// stars, grows back where they find few.
    next_reach_pc: f64,
    scanned_at: Option<DVec3>,
    /// The angle and movement gathered for (`StarCell::resolved`).
    scanned_angle: f64,
    slack_pc: f64,
    gathered_at: Option<bevy::platform::time::Instant>,
    candidates: Vec<(String, StarPoint)>,
    /// Each split system's stars and pairs, its plane's rotation and the last
    /// frame it was split.
    built: Built,
    frame: u64,
}

type Built = HashMap<String, (space_shared::StarSystem, bevy::math::DQuat, u64)>;

/// Drops the systems not split in frame `now` once `built` holds
/// `SPLIT_CACHE` more than the `shown` in use, never those in use, so a
/// crowded view never rebuilds every frame. Whether it pruned.
fn prune(built: &mut Built, now: u64, shown: usize) -> bool {
    if built.len() <= SPLIT_CACHE + shown {
        return false;
    }
    built.retain(|_, (_, _, used)| *used == now);
    true
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
            frame: 0,
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
    let offsets = space_simulation::star_positions(system, 0, seconds);
    system
        .stars_on(0)
        .zip(offsets)
        .map(move |(member, offset)| {
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

impl Split {
    pub(super) fn update(
        &mut self,
        field: &Starfield,
        eye: DVec3,
        angle: f64,
        changed: bool,
        enabled: bool,
        metrics: &mut StreamMetrics,
    ) {
        let split = self;
        split.set_enabled(enabled);
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
            metrics.split_examined = split.gather(field, eye, angle);
            let search_time = search_start.elapsed();
            metrics.split_search += search_time;
            metrics.split_latest.search_ms = search_time.as_secs_f64() * 1000.0;
            metrics.split_searches += 1;
            metrics.split_checked += metrics.split_examined;
        }
    }

    pub(super) fn prepare<'a>(
        &'a mut self,
        view: (DVec3, f64, bool),
        visited: Option<&str>,
        seconds: f64,
        sight: &Sight,
        frame: &mut StarFrame,
        metrics: &mut StreamMetrics,
    ) -> HashSet<&'a str> {
        let (eye, angle, enabled) = view;
        let split = self;
        let split_start = bevy::platform::time::Instant::now();
        metrics.split_sprites = 0;
        metrics.split_offscreen_systems = 0;
        metrics.split_candidates = split.candidates.len();
        let mut shown_apart = std::collections::HashSet::with_capacity(split.candidates.len());
        split.frame += 1;
        let now = split.frame;
        for (id, star) in &split.candidates {
            let position = DVec3::from_array(star.position_pc);
            let distance = position.distance(eye);
            let separation = instance(star, eye.to_array()).separation_au()
                * space_simulation::galaxy::stream::AU_PC;
            if !enabled
                || distance >= split.reach_pc
                || separation <= angle * distance
                || visited == Some(id.as_str())
            {
                continue;
            }
            // Off screen it is neither a point nor its stars, so it is not built.
            shown_apart.insert(id.as_str());
            let at = position - eye;
            // No star of the system is farther than its widest separation from
            // its barycentre.
            if !sight.sees(at, separation) {
                metrics.split_offscreen_systems += 1;
                continue;
            }
            if !split.built.contains_key(id) {
                metrics.split_rebuilt += 1;
                metrics.split_latest.rebuilt += 1;
                let start = bevy::platform::time::Instant::now();
                let (system, plane) = split_system(id);
                split.built.insert(id.clone(), (system, plane, now));
                metrics.split_latest.generation_ms += start.elapsed().as_secs_f64() * 1000.0;
            }
            let Some((system, plane, used)) = split.built.get_mut(id) else {
                continue;
            };
            *used = now;
            let orbit_start = bevy::platform::time::Instant::now();
            metrics.split_sprites += system.stars.len();
            frame.precise.extend(apart(system, *plane, at, seconds));
            metrics.split_latest.orbit_sprite_ms += orbit_start.elapsed().as_secs_f64() * 1000.0;
        }
        if prune(&mut split.built, now, shown_apart.len()) {
            metrics.split_cache_clears += 1;
            metrics.split_latest.cache_clears += 1;
        }
        metrics.split_systems = shown_apart.len();
        metrics.split_cached = split.built.len();
        let preparation_time = split_start.elapsed();
        metrics.split_preparation += preparation_time;
        metrics.split_latest.preparation_ms = preparation_time.as_secs_f64() * 1000.0;
        shown_apart
    }
}

#[cfg(test)]
mod tests;
