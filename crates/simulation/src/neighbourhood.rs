//! Generated star systems around a point, from seeds alone, so a craft finds
//! the same systems on any machine whatever the camera has loaded.
//!
//! Every star of every tier must be placed to know whether it is nearby, and
//! bright tiers hold millions of stars per cell, so a query costs from about
//! ten milliseconds in the solar neighbourhood to hundreds in the bulge. It is
//! asked once per `Region`, never per step.
use crate::cells::{Cell, Sky, StarKey, cell_star_id};
use crate::galaxy::{Component, PC_METRES};
use crate::population::{TIERS, tiered_light};
use crate::search::engine::{distance, gap_sq};
use crate::stream::{in_parallel, near};
use serde::{Deserialize, Serialize};

/// Furthest a planet's apoapsis is from its host, as generation bounds it.
pub const PLANET_REACH_AU: f64 = crate::system::MAX_SYSTEM_APOAPSIS_AU;
/// Entry range around a system: four times its extent, at least 0.02 ly.
pub const RANGE_EXTENTS: f64 = 4.0;
pub const MIN_RANGE_M: f64 = 0.02 * crate::LY_METRES;
/// Side of a `Region`, in parsecs.
pub const REGION_PC: f64 = 1.0;
/// Stars placed per parallel batch.
const PART: usize = 4_096;

/// A system's entry range in metres from its extent in AU.
pub fn range_m(extent_au: f64) -> f64 {
    MIN_RANGE_M.max(RANGE_EXTENTS * extent_au * crate::AU_METRES)
}

/// Upper bound on a generated system's entry range, in parsecs, from its
/// stars' widest separation alone: planets add at most `PLANET_REACH_AU`.
pub fn reach_pc(separation_au: f64) -> f64 {
    range_m(separation_au + PLANET_REACH_AU) / PC_METRES
}

/// Most any generated system's entry range can be, in parsecs.
pub fn max_reach_pc() -> f64 {
    reach_pc(crate::multiple::MAX_STELLAR_SEPARATION_AU)
}

/// A cube of space whose candidate systems are found together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Region(pub [i64; 3]);

impl Region {
    /// The region holding a point, in parsecs from the galactic centre.
    pub fn of(p: [f64; 3]) -> Self {
        Self(p.map(|x| (x / REGION_PC).floor() as i64))
    }

    pub fn min_pc(self) -> [f64; 3] {
        self.0.map(|i| i as f64 * REGION_PC)
    }
}

/// A system that may hold points of a region: its page star and an upper
/// bound on its entry range. Its bodies decide the exact range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub position_pc: [f64; 3],
    pub reach_pc: f64,
}

/// Every generated system whose entry range may reach into `region`, by ID.
pub fn candidates(sky: &mut Sky, region: Region, threads: usize) -> Vec<Candidate> {
    let min = region.min_pc();
    let centre = min.map(|m| m + REGION_PC / 2.0);
    let radius = REGION_PC * 3f64.sqrt() / 2.0 + max_reach_pc();
    let mut found: Vec<Candidate> = stars_within(sky, centre, radius, threads)
        .into_iter()
        .filter_map(|star| {
            let reach_pc = reach_pc(star.separation_au);
            (gap_sq(min, REGION_PC, star.position_pc) < reach_pc * reach_pc).then_some(Candidate {
                id: star.id,
                position_pc: star.position_pc,
                reach_pc,
            })
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

#[derive(Clone, Debug, PartialEq)]
pub struct NearbyStar {
    pub id: String,
    pub position_pc: [f64; 3],
    pub separation_au: f64,
}

/// Every generated system within `radius_pc` of `p`, in no particular order.
/// The same for any number of threads and any warmth of `sky`'s caches.
pub fn stars_within(sky: &mut Sky, p: [f64; 3], radius_pc: f64, threads: usize) -> Vec<NearbyStar> {
    let mut found = Vec::new();
    for level in 0..TIERS {
        for cell in near(level, p, radius_pc) {
            let counts = sky.counts(cell);
            let mut pending: Vec<(Component, u64)> = Component::ALL
                .into_iter()
                .flat_map(|c| (0..counts[c.index()][level]).map(move |i| (c, i)))
                .collect();
            while !pending.is_empty() {
                let shared = &*sky;
                let (batches, _) = in_parallel(
                    if pending.len() > PART { threads } else { 1 },
                    pending.chunks(PART),
                    |part| batch(shared, cell, part, p, radius_pc),
                );
                let mut needs = std::collections::HashSet::new();
                pending = Vec::new();
                for (stars, waiting) in batches {
                    found.extend(stars);
                    for (component, index, need) in waiting {
                        needs.insert(need);
                        pending.push((component, index));
                    }
                }
                // Caches only hold what follows from cell codes, so completing
                // needs in any order gives the same sky.
                for need in needs {
                    sky.complete(need);
                }
            }
        }
    }
    found
}

type Waiting = Vec<(Component, u64, crate::cells::Need)>;

fn batch(
    sky: &Sky,
    cell: Cell,
    part: &[(Component, u64)],
    p: [f64; 3],
    radius_pc: f64,
) -> (Vec<NearbyStar>, Waiting) {
    let mut found = Vec::new();
    let mut waiting = Vec::new();
    let r2 = radius_pc * radius_pc;
    for &(component, index) in part {
        let key = StarKey::new(cell, component, index);
        match sky.try_place_if(key, cell, component, |min, size| gap_sq(min, size, p) <= r2) {
            Err(need) => waiting.push((component, index, need)),
            Ok(Some(position_pc)) if distance(position_pc, p) <= radius_pc => {
                found.push(NearbyStar {
                    id: cell_star_id(cell, component, index),
                    position_pc,
                    separation_au: tiered_light(key, component, cell.level()).separation_au,
                });
            }
            Ok(_) => {}
        }
    }
    (found, waiting)
}

#[cfg(test)]
mod tests;
