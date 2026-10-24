//! Compact cell storage and Morton-grid spatial queries.
use crate::galaxy::Component;
use crate::galaxy::cells::{Cell, cell_star_id};

/// A cell star, compact enough to hold millions. `id` rebuilds the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StarPoint {
    /// Parsecs from the central black hole.
    pub position_pc: [f64; 3],
    /// All its stars together, for a multiple system, which is one point.
    pub luminosity_solar: f32,
    /// The primary's.
    pub temperature_k: f32,
    pub radius_solar: f32,
    /// Widest its stars get from each other, in AU; zero for a single star.
    pub separation_au: f32,
    pub component: Component,
    pub index: u32,
}

impl StarPoint {
    pub fn id(&self, cell: Cell) -> String {
        cell_star_id(cell, self.component, u64::from(self.index))
    }
}

/// Parsecs per AU.
pub const AU_PC: f64 = crate::AU_METRES / crate::galaxy::PC_METRES;

/// Stars a grid box holds on average, at least: fewer boxes for sparse cells.
const STARS_PER_BOX: f64 = 8.0;
/// Smallest grid box edge, in parsecs.
const MIN_BOX_PC: f64 = 0.1;
/// Grid blocks holding this few stars are measured star by star rather than
/// divided further.
const LEAF_STARS: u32 = 32;

/// A cell's own stars: those of the tier matching its level. Build it with
/// `StarCell::new`, which sorts the stars into a grid of boxes so the stars
/// near a point are found without scanning the whole cell.
#[derive(Clone, Debug, PartialEq)]
pub struct StarCell {
    pub cell: Cell,
    /// Sorted by grid box, boxes in Morton order: any run of stars lies in a
    /// compact region. Changing them means building a new `StarCell`.
    pub stars: Vec<StarPoint>,
    grid: StarGrid,
    /// The widest separation of any multiple system in each grid block, in
    /// AU: level `k` per aligned block of `2^k` boxes per axis, in Morton
    /// order, from single boxes up to the whole grid.
    widest: Vec<Vec<f32>>,
}

/// A uniform grid over a cell, a power of two boxes per axis. Boxes are
/// numbered in Morton order, so every aligned block of boxes, down from the
/// whole grid, holds one contiguous run of stars: box `b` holds
/// `stars[starts[b]..starts[b + 1]]`, and an empty block is skipped at once.
#[derive(Clone, Debug, PartialEq)]
struct StarGrid {
    min_pc: [f64; 3],
    box_pc: f64,
    per_axis: usize,
    starts: Vec<u32>,
}

/// Spreads the low 10 bits of `v` to every third bit.
fn spread(v: usize) -> usize {
    let mut v = v & 0x3ff;
    v = (v | (v << 16)) & 0x0300_00ff;
    v = (v | (v << 8)) & 0x0300_f00f;
    v = (v | (v << 4)) & 0x030c_30c3;
    (v | (v << 2)) & 0x0924_9249
}

impl StarGrid {
    fn new(cell: Cell, stars: usize) -> Self {
        let size = cell.size_pc();
        let wanted = (stars as f64 / STARS_PER_BOX)
            .cbrt()
            .ceil()
            .min((size / MIN_BOX_PC).floor())
            .clamp(1.0, 1024.0) as usize;
        // The largest power of two not above it, for Morton order.
        let per_axis = 1 << wanted.ilog2();
        Self {
            min_pc: cell.min_pc(),
            box_pc: size / per_axis as f64,
            per_axis,
            starts: vec![0; per_axis.pow(3) + 1],
        }
    }

    /// The box containing `p` on each axis, clamped to the grid.
    fn coords(&self, p: [f64; 3]) -> [usize; 3] {
        std::array::from_fn(|axis| {
            ((p[axis] - self.min_pc[axis]) / self.box_pc)
                .floor()
                .clamp(0.0, (self.per_axis - 1) as f64) as usize
        })
    }

    fn index(&self, [x, y, z]: [usize; 3]) -> usize {
        (spread(x) << 2) | (spread(y) << 1) | spread(z)
    }

    /// The stars of the aligned block of `size` boxes per axis at `origin`.
    fn block(&self, origin: [usize; 3], size: usize) -> std::ops::Range<usize> {
        let first = self.index(origin);
        self.starts[first] as usize..self.starts[first + size.pow(3)] as usize
    }

    /// Distance from `p` to the nearest point of a block.
    fn gap_pc(&self, origin: [usize; 3], size: usize, p: [f64; 3]) -> f64 {
        let edge = size as f64 * self.box_pc;
        (0..3)
            .map(|axis| {
                let min = self.min_pc[axis] + origin[axis] as f64 * self.box_pc;
                (min - p[axis]).max(p[axis] - min - edge).max(0.0).powi(2)
            })
            .sum::<f64>()
            .sqrt()
    }

    /// The eight blocks within a block, nearest `p` first.
    fn children(&self, origin: [usize; 3], size: usize, p: [f64; 3]) -> [(f64, [usize; 3]); 8] {
        let half = size / 2;
        let mut children = std::array::from_fn(|i| {
            let child = [
                origin[0] + half * (i >> 2 & 1),
                origin[1] + half * (i >> 1 & 1),
                origin[2] + half * (i & 1),
            ];
            (self.gap_pc(child, half, p), child)
        });
        children.sort_by(|a, b| a.0.total_cmp(&b.0));
        children
    }
}

/// An aligned cube of grid boxes and the contiguous run of stars inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    origin: [usize; 3],
    size: usize,
    pub start: usize,
    pub end: usize,
    pub centre_pc: [f64; 3],
    /// Half the cube's edge, in parsecs.
    pub half_pc: f64,
}

impl Block {
    /// Distance from the block's centre to its farthest corner, in parsecs.
    pub fn half_diagonal_pc(&self) -> f64 {
        self.half_pc * 3.0_f64.sqrt()
    }
}

impl StarCell {
    pub fn new(cell: Cell, stars: Vec<StarPoint>) -> Self {
        let mut grid = StarGrid::new(cell, stars.len());
        let boxes: Vec<usize> = stars
            .iter()
            .map(|star| grid.index(grid.coords(star.position_pc)))
            .collect();
        for &b in &boxes {
            grid.starts[b + 1] += 1;
        }
        for b in 1..grid.starts.len() {
            grid.starts[b] += grid.starts[b - 1];
        }
        let mut next = grid.starts.clone();
        let mut sorted = stars.clone();
        for (star, b) in stars.into_iter().zip(boxes) {
            sorted[next[b] as usize] = star;
            next[b] += 1;
        }
        let mut level: Vec<f32> = grid
            .starts
            .windows(2)
            .map(|run| {
                sorted[run[0] as usize..run[1] as usize]
                    .iter()
                    .map(|star| star.separation_au)
                    .fold(0.0, f32::max)
            })
            .collect();
        let mut widest = Vec::new();
        while level.len() > 1 {
            let next = level
                .chunks(8)
                .map(|block| block.iter().copied().fold(0.0, f32::max))
                .collect();
            widest.push(std::mem::replace(&mut level, next));
        }
        widest.push(level);
        Self {
            cell,
            stars: sorted,
            grid,
            widest,
        }
    }

    /// Calls `f` with every multiple system within `reach_pc` of `p` whose
    /// stars could appear more than `angle` radians apart from somewhere
    /// within `slack_pc` of `p`: a superset, for callers to filter. Stops
    /// once `examined` stars measured exceed `limit`.
    pub fn resolved(
        &self,
        p: [f64; 3],
        (reach_pc, angle, slack_pc): (f64, f64, f64),
        limit: usize,
        examined: &mut usize,
        f: &mut impl FnMut(&StarPoint),
    ) {
        self.resolved_in(
            [0; 3],
            self.grid.per_axis,
            p,
            (reach_pc, angle, slack_pc),
            limit,
            examined,
            f,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn resolved_in(
        &self,
        origin: [usize; 3],
        size: usize,
        p: [f64; 3],
        (reach_pc, angle, slack_pc): (f64, f64, f64),
        limit: usize,
        examined: &mut usize,
        f: &mut impl FnMut(&StarPoint),
    ) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() || *examined > limit {
            return;
        }
        let gap = self.grid.gap_pc(origin, size, p);
        let widest = self.widest[size.trailing_zeros() as usize]
            [self.grid.index(origin) >> (3 * size.trailing_zeros())];
        let shows = |separation_au: f32, distance: f64| {
            separation_au > 0.0
                && distance <= reach_pc
                && f64::from(separation_au) * AU_PC >= angle * (distance - slack_pc)
        };
        if !shows(widest, gap) {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for star in &self.stars[stars] {
                *examined += 1;
                if *examined > limit {
                    return;
                }
                let distance = (0..3)
                    .map(|axis| (star.position_pc[axis] - p[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if shows(star.separation_au, distance) {
                    f(star);
                }
            }
            return;
        }
        for (_, child) in self.grid.children(origin, size, p) {
            self.resolved_in(
                child,
                size / 2,
                p,
                (reach_pc, angle, slack_pc),
                limit,
                examined,
                f,
            );
        }
    }

    /// The star nearest `p` and closer than `cap_pc`, with its distance in
    /// parsecs; equal distances resolve by ID. Descends the grid's blocks
    /// nearest first, skipping empty ones and those beyond the best so far.
    /// `checked` counts the stars measured.
    pub fn nearest(
        &self,
        p: [f64; 3],
        cap_pc: f64,
        checked: &mut usize,
    ) -> Option<(f64, StarPoint)> {
        let mut best = None;
        self.search([0; 3], self.grid.per_axis, p, cap_pc, &mut best, checked);
        best
    }

    fn search(
        &self,
        origin: [usize; 3],
        size: usize,
        p: [f64; 3],
        cap_pc: f64,
        best: &mut Option<(f64, StarPoint)>,
        checked: &mut usize,
    ) {
        let stars = self.grid.block(origin, size);
        let limit = best.map_or(cap_pc, |(distance, _)| distance);
        if stars.is_empty() || self.grid.gap_pc(origin, size, p) > limit {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for star in &self.stars[stars] {
                *checked += 1;
                let distance = (0..3)
                    .map(|axis| (star.position_pc[axis] - p[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                if distance >= cap_pc {
                    continue;
                }
                if best.is_none_or(|(old, old_star)| {
                    distance < old
                        || (distance == old && star.id(self.cell) < old_star.id(self.cell))
                }) {
                    *best = Some((distance, *star));
                }
            }
            return;
        }
        for (_, child) in self.grid.children(origin, size, p) {
            self.search(child, size / 2, p, cap_pc, best, checked);
        }
    }

    /// Calls `f` with the index and star of every star in the grid blocks
    /// within `radius_pc` of `p`: a superset of the stars within that
    /// distance, for callers to filter.
    pub fn near(&self, p: [f64; 3], radius_pc: f64, mut f: impl FnMut(usize, &StarPoint)) {
        self.within([0; 3], self.grid.per_axis, p, radius_pc, &mut f);
    }

    /// As `near`, within one of this cell's `blocks`.
    pub fn near_in(
        &self,
        block: &Block,
        p: [f64; 3],
        radius_pc: f64,
        mut f: impl FnMut(usize, &StarPoint),
    ) {
        self.within(block.origin, block.size, p, radius_pc, &mut f);
    }

    /// Visits only grid blocks intersecting `intersects`, then their stars.
    /// The predicate must conservatively accept cubes touching the query.
    pub fn query_blocks(
        &self,
        intersects: impl Fn([f64; 3], f64) -> bool,
        mut visit: impl FnMut(&StarPoint),
    ) {
        self.query_block([0; 3], self.grid.per_axis, &intersects, &mut visit);
    }

    fn query_block(
        &self,
        origin: [usize; 3],
        size: usize,
        intersects: &impl Fn([f64; 3], f64) -> bool,
        visit: &mut impl FnMut(&StarPoint),
    ) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() {
            return;
        }
        let half = size as f64 * self.grid.box_pc / 2.0;
        let centre = std::array::from_fn(|axis| {
            self.grid.min_pc[axis] + origin[axis] as f64 * self.grid.box_pc + half
        });
        if !intersects(centre, half * 3f64.sqrt()) {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for star in &self.stars[stars] {
                visit(star);
            }
            return;
        }
        let half = size / 2;
        for i in 0..8 {
            self.query_block(
                [
                    origin[0] + half * (i >> 2 & 1),
                    origin[1] + half * (i >> 1 & 1),
                    origin[2] + half * (i & 1),
                ],
                half,
                intersects,
                visit,
            );
        }
    }

    /// Boxes in the grid, empty ones included.
    pub fn boxes(&self) -> usize {
        self.grid.starts.len() - 1
    }

    /// Non-empty aligned cubes of the grid holding at most `most` stars each,
    /// unless a single box holds more, in star order: together they cover
    /// `stars` from start to end. Each is a compact region with its own centre.
    pub fn blocks(&self, most: usize) -> Vec<Block> {
        let mut out = Vec::new();
        self.split([0; 3], self.grid.per_axis, most, &mut out);
        out
    }

    fn split(&self, origin: [usize; 3], size: usize, most: usize, out: &mut Vec<Block>) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() {
            return;
        }
        if size == 1 || stars.len() <= most {
            let half_pc = size as f64 * self.grid.box_pc / 2.0;
            out.push(Block {
                origin,
                size,
                start: stars.start,
                end: stars.end,
                centre_pc: std::array::from_fn(|axis| {
                    self.grid.min_pc[axis] + origin[axis] as f64 * self.grid.box_pc + half_pc
                }),
                half_pc,
            });
            return;
        }
        // Children in Morton order, so blocks come out in star order.
        let half = size / 2;
        for i in 0..8 {
            let child = [
                origin[0] + half * (i >> 2 & 1),
                origin[1] + half * (i >> 1 & 1),
                origin[2] + half * (i & 1),
            ];
            self.split(child, half, most, out);
        }
    }

    fn within(
        &self,
        origin: [usize; 3],
        size: usize,
        p: [f64; 3],
        radius_pc: f64,
        f: &mut impl FnMut(usize, &StarPoint),
    ) {
        let stars = self.grid.block(origin, size);
        if stars.is_empty() || self.grid.gap_pc(origin, size, p) > radius_pc {
            return;
        }
        if size == 1 || stars.len() <= LEAF_STARS as usize {
            for index in stars {
                f(index, &self.stars[index]);
            }
            return;
        }
        for (_, child) in self.grid.children(origin, size, p) {
            self.within(child, size / 2, p, radius_pc, f);
        }
    }
}

#[cfg(test)]
mod tests;
