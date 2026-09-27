//! The galaxy's stars, generated cell by cell so none are stored.
//!
//! - An octree of cubic cells covers the galaxy. `Cell` is a locational code:
//!   a leading 1, then three bits per level (x, y, z), so a cell's ID is stable
//!   and its parent is the code shifted down three bits.
//! - The root holds exactly `GalaxyModel::stars` stars, counted per component
//!   and brightness tier. Each cell hands its counts down to its children in
//!   proportion to the stars each child holds (a seeded multinomial draw), so
//!   children always add up to their parent.
//! - A tier's stars belong to the cells of the same level: bright stars to
//!   large cells, which can be loaded from far away, and faint ones to small
//!   cells, loaded only nearby. Coarser cells still count their finer stars.
//! - Everything follows from cell codes, so any cell gives the same stars in any
//!   order, on any machine. `Sky` only caches what it has worked out.

use crate::galaxy::{Component, GalaxyModel};
use crate::population::{TIERS, tiered_formation};
use crate::stream::StarPoint;
use crate::system::{Body, Rng, system_around};
use space_shared::Star;
use std::collections::HashMap;

/// Side of the root cell, in parsecs: the galaxy's diameter, rounded up to a
/// power of two.
pub const ROOT_PC: f64 = 131_072.0;
/// Deepest level cells are divided to when placing a star.
const DEEPEST: usize = 20;
/// Gauss-Legendre nodes and weights on [0, 1].
const NODES: [f64; 3] = [0.112_701_665_379_258_3, 0.5, 0.887_298_334_620_741_7];
const WEIGHTS: [f64; 3] = [5.0 / 18.0, 8.0 / 18.0, 5.0 / 18.0];

/// A cubic cell of the galaxy's octree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cell(pub u64);

impl Cell {
    pub const ROOT: Cell = Cell(1);

    pub fn level(self) -> usize {
        ((63 - self.0.leading_zeros()) / 3) as usize
    }

    pub fn parent(self) -> Option<Cell> {
        (self.0 > 1).then_some(Cell(self.0 >> 3))
    }

    /// Octant bits: 1 for +x, 2 for +y, 4 for +z.
    pub fn child(self, octant: usize) -> Cell {
        Cell(self.0 << 3 | octant as u64)
    }

    pub fn children(self) -> [Cell; 8] {
        std::array::from_fn(|octant| self.child(octant))
    }

    pub fn octant(self) -> usize {
        (self.0 & 7) as usize
    }

    pub fn size_pc(self) -> f64 {
        ROOT_PC / (1u64 << self.level()) as f64
    }

    /// Integer position among the cells of its level, from the root's corner.
    pub fn coords(self) -> [u32; 3] {
        let level = self.level();
        let mut coords = [0u32; 3];
        for i in (0..level).rev() {
            let octant = (self.0 >> (3 * i)) & 7;
            for (axis, c) in coords.iter_mut().enumerate() {
                *c = *c << 1 | ((octant >> axis) & 1) as u32;
            }
        }
        coords
    }

    pub fn from_coords(level: usize, coords: [u32; 3]) -> Option<Cell> {
        if level > DEEPEST || coords.iter().any(|&c| u64::from(c) >> level != 0) {
            return None;
        }
        let mut code = 1u64;
        for i in (0..level).rev() {
            let octant = (0..3).fold(0, |o, axis| o | ((coords[axis] >> i) & 1) << axis);
            code = code << 3 | u64::from(octant);
        }
        Some(Cell(code))
    }

    /// The cell of this level holding a point, in parsecs from the centre.
    pub fn containing(level: usize, p: [f64; 3]) -> Option<Cell> {
        let size = ROOT_PC / (1u64 << level) as f64;
        let mut coords = [0u32; 3];
        for axis in 0..3 {
            let i = ((p[axis] + ROOT_PC / 2.0) / size).floor();
            if !(0.0..(1u64 << level) as f64).contains(&i) {
                return None;
            }
            coords[axis] = i as u32;
        }
        Cell::from_coords(level, coords)
    }

    /// Corner nearest the root's, in parsecs from the centre.
    pub fn min_pc(self) -> [f64; 3] {
        let size = self.size_pc();
        self.coords().map(|c| f64::from(c) * size - ROOT_PC / 2.0)
    }

    pub fn centre_pc(self) -> [f64; 3] {
        let half = self.size_pc() / 2.0;
        self.min_pc().map(|m| m + half)
    }

    pub fn contains(self, p: [f64; 3]) -> bool {
        let (min, size) = (self.min_pc(), self.size_pc());
        (0..3).all(|axis| (min[axis]..min[axis] + size).contains(&p[axis]))
    }
}

/// Stars of each component (in `Component::ALL` order) and tier within a cell.
/// Tiers brighter than the cell's level belong to its ancestors and count zero.
pub type Counts = [[u64; TIERS]; 6];

#[derive(Clone, Debug, PartialEq)]
pub struct CellStar {
    pub id: String,
    pub cell: Cell,
    pub component: Component,
    pub position_pc: [f64; 3],
    pub star: Star,
}

/// ID of the `index`th star of a component in a cell.
pub fn cell_star_id(cell: Cell, component: Component, index: u64) -> String {
    format!("g{:x}.{}.{index}", cell.0, component.index())
}

/// The cell, component and index a cell star's ID names.
pub fn parse_cell_star(id: &str) -> Option<(Cell, Component, u64)> {
    let mut parts = id.strip_prefix('g')?.split('.');
    let cell = Cell(u64::from_str_radix(parts.next()?, 16).ok()?);
    let component = *Component::ALL.get(parts.next()?.parse::<usize>().ok()?)?;
    let index = parts.next()?.parse().ok()?;
    (parts.next().is_none() && cell.0 != 0 && cell.level() < TIERS)
        .then_some((cell, component, index))
}

/// A cell star's properties, from its ID alone.
pub fn cell_star(id: &str) -> Option<(Component, Star)> {
    let (cell, component, _) = parse_cell_star(id)?;
    Some((
        component,
        tiered_formation(id, component, cell.level()).star,
    ))
}

/// A cell star's planets, innermost first.
pub fn cell_planetary_system(id: &str) -> Option<Vec<Body>> {
    let (cell, component, _) = parse_cell_star(id)?;
    Some(system_around(
        id,
        &tiered_formation(id, component, cell.level()),
    ))
}

/// A seeded binomial draw: exact for small means, normal beyond.
fn binomial(rng: &mut Rng, n: u64, p: f64) -> u64 {
    if n == 0 || p <= 0.0 {
        return 0;
    }
    if p >= 1.0 {
        return n;
    }
    if p > 0.5 {
        return n - binomial(rng, n, 1.0 - p);
    }
    let mean = n as f64 * p;
    if mean < 30.0 {
        let mut u = rng.unit();
        let ratio = p / (1.0 - p);
        let mut pmf = (n as f64 * (-p).ln_1p()).exp();
        let mut k = 0;
        while u > pmf && k < n {
            u -= pmf;
            pmf *= (n - k) as f64 / (k + 1) as f64 * ratio;
            k += 1;
            if pmf <= 0.0 {
                break;
            }
        }
        k
    } else {
        let z =
            (-2.0 * (1.0 - rng.unit()).ln()).sqrt() * (std::f64::consts::TAU * rng.unit()).cos();
        let k = (mean + (mean * (1.0 - p)).sqrt() * z).round();
        k.clamp(0.0, n as f64) as u64
    }
}

/// Split `n` among weights, adding up exactly.
fn multinomial(rng: &mut Rng, n: u64, weights: &[f64; 8]) -> [u64; 8] {
    let mut out = [0; 8];
    let mut left = n;
    let mut rest: f64 = weights.iter().sum();
    let last = weights.iter().rposition(|&w| w > 0.0).unwrap_or(7);
    for (k, &w) in weights.iter().enumerate() {
        if left == 0 {
            break;
        }
        if k == last {
            out[k] = left;
            break;
        }
        let share = if rest > 0.0 { w / rest } else { 0.0 };
        out[k] = binomial(rng, left, share);
        left -= out[k];
        rest -= w;
    }
    out
}

fn seeded(parts: [u64; 3]) -> Rng {
    Rng::seeded(parts.iter().fold(0x5eed_ce11_u64, |h, &p| {
        crate::system::mix(h ^ p.wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }))
}

/// Nearest distance from the centre to a box, in 3D and in the disc plane.
fn nearest(min: [f64; 3], size: f64) -> (f64, f64) {
    let gap = |axis: usize| {
        let (a, b) = (min[axis], min[axis] + size);
        if a > 0.0 {
            a
        } else if b < 0.0 {
            -b
        } else {
            0.0
        }
    };
    let [x, y, z] = [gap(0), gap(1), gap(2)];
    ((x * x + y * y + z * z).sqrt(), x.hypot(z))
}

/// Largest box the three-point rule integrates a component over accurately.
fn resolution(model: &GalaxyModel, component: Component, min: [f64; 3], size: f64) -> f64 {
    let (r, radius) = nearest(min, size);
    match component {
        Component::Nucleus => model.nucleus_pc.max(0.3 * r),
        Component::Bulge => model.bar_pc[1].min(model.bar_pc[2]).max(0.3 * r),
        Component::Halo => model.halo_core_pc.max(0.3 * r),
        // Arms are about a tenth of their radius across.
        Component::ThinDisc | Component::Arms => (0.05 * radius).clamp(250.0, 1_000.0),
        Component::ThickDisc => (0.1 * radius).clamp(500.0, 1_000.0),
    }
}

/// Stars of a component in a box. Discs integrate exactly through their height
/// and numerically across the plane; the rest numerically in 3D. Boxes are
/// divided until small enough for the three-point rule.
fn stars_in_box(model: &GalaxyModel, component: Component, min: [f64; 3], size: f64) -> f64 {
    if nearest(min, size).0 > model.radius_pc {
        return 0.0;
    }
    match model.disc(component) {
        Some(disc) => {
            disc.vertical(min[1], min[1] + size) * across(model, component, min[0], min[2], size)
        }
        None => volume(model, component, min, size),
    }
}

/// Face-on stars of a disc component in a square of the plane.
fn across(model: &GalaxyModel, component: Component, x: f64, z: f64, size: f64) -> f64 {
    let min = [x, 0.0, z];
    if nearest(min, size).1 > model.radius_pc {
        return 0.0;
    }
    if size > resolution(model, component, min, size) {
        let half = size / 2.0;
        return [(0.0, 0.0), (half, 0.0), (0.0, half), (half, half)]
            .iter()
            .map(|&(dx, dz)| across(model, component, x + dx, z + dz, half))
            .sum();
    }
    let mut sum = 0.0;
    for (i, wi) in NODES.iter().zip(WEIGHTS) {
        for (k, wk) in NODES.iter().zip(WEIGHTS) {
            sum += wi * wk * model.disc_surface(component, x + i * size, z + k * size);
        }
    }
    sum * size * size
}

fn volume(model: &GalaxyModel, component: Component, min: [f64; 3], size: f64) -> f64 {
    if nearest(min, size).0 > model.radius_pc {
        return 0.0;
    }
    if size > resolution(model, component, min, size) {
        let half = size / 2.0;
        return (0..8)
            .map(|octant| volume(model, component, corner(min, half, octant), half))
            .sum();
    }
    let mut sum = 0.0;
    for (i, wi) in NODES.iter().zip(WEIGHTS) {
        for (j, wj) in NODES.iter().zip(WEIGHTS) {
            for (k, wk) in NODES.iter().zip(WEIGHTS) {
                let p = [min[0] + i * size, min[1] + j * size, min[2] + k * size];
                sum += wi * wj * wk * model.component_density(component, p);
            }
        }
    }
    sum * size.powi(3)
}

/// Corner of the child box in an octant.
fn corner(min: [f64; 3], half: f64, octant: usize) -> [f64; 3] {
    std::array::from_fn(|axis| min[axis] + if octant >> axis & 1 == 1 { half } else { 0.0 })
}

/// Streams the galaxy's stars cell by cell, caching counts and masses.
pub struct Sky {
    pub model: GalaxyModel,
    counts: HashMap<Cell, Box<Counts>>,
    /// Stars of a component in each child of a cell, by octant.
    masses: HashMap<(Cell, Component), [f64; 8]>,
    /// Children integrated so far, for masses not yet complete.
    partial: HashMap<(Cell, Component), Vec<f64>>,
    /// Rejection bound of a component's density in the cells stars end in.
    bounds: HashMap<(Cell, Component), f64>,
}

impl Sky {
    pub fn new(model: GalaxyModel) -> Self {
        Self {
            model,
            counts: HashMap::new(),
            masses: HashMap::new(),
            partial: HashMap::new(),
            bounds: HashMap::new(),
        }
    }

    /// Exact counts at the root: each component's stars, shared among tiers by
    /// largest remainder so nothing is lost to rounding.
    fn root(&self) -> Counts {
        let mut counts = [[0; TIERS]; 6];
        for component in Component::ALL {
            let total = self.model.stars_in(component).round() as u64;
            let shares = crate::population::tier_shares(component);
            let exact = shares.map(|s| s * total as f64);
            let row = &mut counts[component.index()];
            for (t, e) in exact.iter().enumerate() {
                row[t] = e.floor() as u64;
            }
            let mut order: Vec<usize> = (0..TIERS).collect();
            order.sort_by(|&a, &b| {
                (exact[b] - exact[b].floor()).total_cmp(&(exact[a] - exact[a].floor()))
            });
            let short = total - row.iter().sum::<u64>();
            for &t in order.iter().take(short as usize) {
                row[t] += 1;
            }
        }
        counts
    }

    /// Mean stars of a component in each child of a cell, by octant.
    pub fn child_masses(&mut self, cell: Cell, component: Component) -> [f64; 8] {
        loop {
            if let Some(masses) = self.masses.get(&(cell, component)) {
                return *masses;
            }
            self.integrate(cell, component);
        }
    }

    /// Integrates one more child of a cell for `child_masses`: the most work
    /// done in one go while loading.
    pub(crate) fn integrate(&mut self, cell: Cell, component: Component) {
        if self.masses.contains_key(&(cell, component)) {
            return;
        }
        let octant = self.partial.get(&(cell, component)).map_or(0, Vec::len);
        let half = cell.size_pc() / 2.0;
        let stars = stars_in_box(
            &self.model,
            component,
            corner(cell.min_pc(), half, octant),
            half,
        );
        let partial = self.partial.entry((cell, component)).or_default();
        partial.push(stars);
        if let Ok(masses) = <[f64; 8]>::try_from(partial.as_slice()) {
            self.partial.remove(&(cell, component));
            self.masses.insert((cell, component), masses);
        }
    }

    /// Mean stars of a component in a cell, integrated from the model.
    pub fn expected(&self, component: Component, cell: Cell) -> f64 {
        stars_in_box(&self.model, component, cell.min_pc(), cell.size_pc())
    }

    /// Stars of each component and tier in a cell.
    pub fn counts(&mut self, cell: Cell) -> Counts {
        if let Some(counts) = self.counts.get(&cell) {
            return **counts;
        }
        let Some(parent) = cell.parent() else {
            let root = self.root();
            self.counts.insert(cell, Box::new(root));
            return root;
        };
        let above = self.counts(parent);
        let level = cell.level();
        let mut children = [[[0; TIERS]; 6]; 8];
        let finer = above.iter().any(|row| row[level..].iter().any(|&n| n > 0));
        if finer {
            for component in Component::ALL {
                let c = component.index();
                if above[c][level..].iter().all(|&n| n == 0) {
                    continue;
                }
                let weights = self.child_masses(parent, component);
                for tier in level..TIERS {
                    let n = above[c][tier];
                    if n == 0 {
                        continue;
                    }
                    let mut rng = seeded([parent.0, c as u64, tier as u64]);
                    for (octant, k) in multinomial(&mut rng, n, &weights).into_iter().enumerate() {
                        children[octant][c][tier] = k;
                    }
                }
            }
        }
        for (octant, counts) in children.into_iter().enumerate() {
            self.counts.insert(parent.child(octant), Box::new(counts));
        }
        children[cell.octant()]
    }

    /// Masses `counts` still needs for a cell, if any, working out the counts
    /// on the way down that need nothing more.
    pub(crate) fn counts_need(&mut self, cell: Cell) -> Option<(Cell, Component)> {
        let mut path = Vec::new();
        let mut next = Some(cell);
        while let Some(cell) = next.filter(|c| !self.counts.contains_key(c)) {
            path.push(cell);
            next = cell.parent();
        }
        for cell in path.into_iter().rev() {
            if let Some(parent) = cell.parent() {
                let above = self.counts(parent);
                let missing = Component::ALL.into_iter().find(|c| {
                    above[c.index()][cell.level()..].iter().any(|&n| n > 0)
                        && !self.masses.contains_key(&(parent, *c))
                });
                if let Some(component) = missing {
                    return Some((parent, component));
                }
            }
            self.counts(cell);
        }
        None
    }

    /// Stars in each tier within a cell, of every component.
    pub fn tier_counts(&mut self, cell: Cell) -> [u64; TIERS] {
        let counts = self.counts(cell);
        std::array::from_fn(|t| counts.iter().map(|row| row[t]).sum())
    }

    /// The cell's own stars: those of the tier matching its level.
    pub fn cell_stars(&mut self, cell: Cell) -> Vec<CellStar> {
        let tier = cell.level();
        if tier >= TIERS {
            return Vec::new();
        }
        let counts = self.counts(cell);
        let mut stars = Vec::new();
        for component in Component::ALL {
            for index in 0..counts[component.index()][tier] {
                stars.push(self.build(cell, component, index));
            }
        }
        stars
    }

    /// One cell star, by ID, if it exists.
    pub fn star(&mut self, id: &str) -> Option<CellStar> {
        let (cell, component, index) = parse_cell_star(id)?;
        let count = self.counts(cell)[component.index()][cell.level()];
        (index < count).then(|| self.build(cell, component, index))
    }

    fn build(&mut self, cell: Cell, component: Component, index: u64) -> CellStar {
        let id = cell_star_id(cell, component, index);
        let star = tiered_formation(&id, component, cell.level()).star;
        let position_pc = self.place(&id, cell, component);
        CellStar {
            id,
            cell,
            component,
            position_pc,
            star,
        }
    }

    /// One cell star, as `build` makes it, without its names.
    pub(crate) fn point(&mut self, cell: Cell, component: Component, index: u64) -> StarPoint {
        let star = self.build(cell, component, index);
        StarPoint {
            position_pc: star.position_pc,
            luminosity_solar: star.star.luminosity_solar as f32,
            temperature_k: star.star.temperature_k as f32,
            radius_solar: star.star.radius_solar as f32,
            component,
            index: index as u32,
        }
    }

    /// Where a star lies in its cell: down through the children in proportion
    /// to their stars, then by rejection once the density is smooth.
    fn place(&mut self, id: &str, cell: Cell, component: Component) -> [f64; 3] {
        loop {
            let mut rng = Rng::new(&format!("{id}:place"));
            match self.descend(&mut rng, cell, component) {
                Ok(cell) => {
                    let model = &self.model;
                    let bound = *self
                        .bounds
                        .entry((cell, component))
                        .or_insert_with(|| bound(model, component, cell));
                    return within(model, component, cell, bound, &mut rng);
                }
                Err((cell, component)) => {
                    self.child_masses(cell, component);
                }
            }
        }
    }

    /// Masses placing a cell star still needs, if any.
    pub(crate) fn place_need(
        &self,
        cell: Cell,
        component: Component,
        index: u64,
    ) -> Option<(Cell, Component)> {
        let id = cell_star_id(cell, component, index);
        let mut rng = Rng::new(&format!("{id}:place"));
        self.descend(&mut rng, cell, component).err()
    }

    /// The cell a star's descent ends in, or the masses it needs first.
    fn descend(
        &self,
        rng: &mut Rng,
        cell: Cell,
        component: Component,
    ) -> Result<Cell, (Cell, Component)> {
        let mut cell = cell;
        while cell.level() < DEEPEST
            && cell.size_pc() > resolution(&self.model, component, cell.min_pc(), cell.size_pc())
        {
            let weights = self
                .masses
                .get(&(cell, component))
                .ok_or((cell, component))?;
            let total: f64 = weights.iter().sum();
            if total <= 0.0 {
                break;
            }
            let mut roll = rng.unit() * total;
            let octant = weights
                .iter()
                .position(|&w| {
                    roll -= w;
                    roll < 0.0
                })
                .unwrap_or(7);
            cell = cell.child(octant);
        }
        Ok(cell)
    }

    /// Drops cached cells, keeping those asked for. Nothing is lost: dropped
    /// cells are worked out again when needed.
    pub fn retain(&mut self, keep: impl Fn(Cell) -> bool) {
        self.counts.retain(|&cell, _| keep(cell));
        self.masses.retain(|&(cell, _), _| keep(cell));
        self.partial.retain(|&(cell, _), _| keep(cell));
        self.bounds.retain(|&(cell, _), _| keep(cell));
    }

    pub fn cached(&self) -> usize {
        self.counts.len() + self.masses.len() + self.partial.len() + self.bounds.len()
    }
}

impl Default for Sky {
    fn default() -> Self {
        Self::new(GalaxyModel::milky_way())
    }
}

/// Density a star is placed by: face-on for discs, whose heights are drawn
/// exactly.
fn placement_density(model: &GalaxyModel, component: Component, p: [f64; 3]) -> f64 {
    match model.disc(component) {
        Some(_) => model.disc_surface(component, p[0], p[2]),
        None => model.component_density(component, p),
    }
}

/// Rejection bound in a cell: the largest density at its corners, edges and
/// centre, with a margin.
fn bound(model: &GalaxyModel, component: Component, cell: Cell) -> f64 {
    let (min, size) = (cell.min_pc(), cell.size_pc());
    (0..27)
        .map(|i| {
            placement_density(
                model,
                component,
                std::array::from_fn(|axis| {
                    min[axis] + size * ((i / 3usize.pow(axis as u32)) % 3) as f64 / 2.0
                }),
            )
        })
        .fold(0.0, f64::max)
        * 1.5
}

/// A point in a cell with the component's density there, by rejection.
fn within(
    model: &GalaxyModel,
    component: Component,
    cell: Cell,
    bound: f64,
    rng: &mut Rng,
) -> [f64; 3] {
    let (min, size) = (cell.min_pc(), cell.size_pc());
    let disc = model.disc(component);
    let density = |p: [f64; 3]| placement_density(model, component, p);
    let mut p = cell.centre_pc();
    for _ in 0..256 {
        p = std::array::from_fn(|axis| min[axis] + size * rng.unit());
        if let Some(disc) = disc {
            p[1] = disc.height_between(min[1], min[1] + size, rng.unit());
        }
        if rng.unit() * bound <= density(p) {
            break;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::population::{reach_pc, tier_limit};

    #[test]
    fn cells_have_stable_codes_and_coordinates() {
        let home = GalaxyModel::milky_way().home_pc;
        for level in 0..TIERS {
            let cell = Cell::containing(level, home).unwrap();
            assert_eq!(cell.level(), level);
            assert!(cell.contains(home));
            assert_eq!(Cell::from_coords(level, cell.coords()), Some(cell));
            if let Some(parent) = cell.parent() {
                assert_eq!(parent.child(cell.octant()), cell);
                assert!(parent.contains(home));
                assert_eq!(parent.size_pc(), 2.0 * cell.size_pc());
            }
        }
        // The root is centred on the black hole and holds the whole galaxy.
        assert_eq!(Cell::ROOT.centre_pc(), [0.0; 3]);
        assert!(Cell::ROOT.size_pc() >= 2.0 * GalaxyModel::milky_way().radius_pc);
        assert_eq!(Cell::containing(3, [1e6, 0.0, 0.0]), None);
        let id = cell_star_id(Cell::containing(12, home).unwrap(), Component::Halo, 42);
        assert_eq!(
            parse_cell_star(&id),
            Some((Cell::containing(12, home).unwrap(), Component::Halo, 42))
        );
        assert_eq!(parse_cell_star("list:page-0"), None);
    }

    #[test]
    fn integrated_cells_hold_the_model_stars() {
        let mut sky = Sky::default();
        for component in Component::ALL {
            let stars: f64 = sky.child_masses(Cell::ROOT, component).iter().sum();
            let wanted = sky.model.stars_in(component);
            assert!((stars / wanted - 1.0).abs() < 0.002, "{component:?}");
        }
        // Children add up to their parent at every scale, including cells
        // around the black hole and in the disc near home.
        let home = sky.model.home_pc;
        for p in [home, [0.5, 0.2, -0.3], [3_000.0, -400.0, 1_000.0]] {
            for level in [2, 6, 10, 14] {
                let cell = Cell::containing(level, p).unwrap();
                for component in Component::ALL {
                    let whole = sky.expected(component, cell);
                    let parts: f64 = sky.child_masses(cell, component).iter().sum();
                    assert!(
                        (parts - whole).abs() <= 0.01 * whole + 1e-9,
                        "{component:?} at {p:?}, level {level}: {parts} vs {whole}"
                    );
                }
            }
        }
    }

    #[test]
    fn counts_add_up_to_exactly_two_hundred_billion() {
        let mut sky = Sky::default();
        let root = sky.counts(Cell::ROOT);
        let total: u64 = root.iter().flatten().sum();
        assert_eq!(total, 200_000_000_000);
        for component in Component::ALL {
            let stars: u64 = root[component.index()].iter().sum();
            assert_eq!(stars as f64, sky.model.stars_in(component).round());
        }
        // Down to home and to the centre, every cell's children add up to it,
        // less the stars of its own tier, which stay with it.
        for p in [sky.model.home_pc, [0.5, 0.2, -0.3]] {
            for level in 0..TIERS - 1 {
                let cell = Cell::containing(level, p).unwrap();
                let counts = sky.counts(cell);
                let children: Vec<Counts> = cell.children().map(|c| sky.counts(c)).to_vec();
                for c in 0..6 {
                    for tier in 0..TIERS {
                        let below: u64 = children.iter().map(|k| k[c][tier]).sum();
                        let wanted = if tier > level { counts[c][tier] } else { 0 };
                        assert_eq!(below, wanted, "level {level}, component {c}, tier {tier}");
                    }
                }
            }
        }
    }

    #[test]
    fn stars_are_the_same_in_any_order() {
        let home = GalaxyModel::milky_way().home_pc;
        let cell = Cell::containing(11, home).unwrap();
        let mut first = Sky::default();
        let stars = first.cell_stars(cell);
        assert!(!stars.is_empty());
        // Another sky, reaching the cell after its neighbours and a cache flush.
        let mut second = Sky::default();
        let neighbour = Cell::containing(11, [home[0] + 64.0, home[1], home[2]]).unwrap();
        second.cell_stars(neighbour);
        second.retain(|_| false);
        assert_eq!(second.cell_stars(cell), stars);
        for star in &stars {
            assert!(cell.contains(star.position_pc));
            assert_eq!(second.star(&star.id).as_ref(), Some(star));
            assert_eq!(
                cell_star(&star.id),
                Some((star.component, star.star.clone()))
            );
            assert!(star.star.luminosity_solar <= tier_limit(11) * 1.001);
        }
        let missing = cell_star_id(cell, Component::ThinDisc, 1 << 40);
        assert_eq!(second.star(&missing), None);
        let system = cell_planetary_system(&stars[0].id).unwrap();
        assert_eq!(system.len() as u32, crate::planet_count(&stars[0].id));
    }

    #[test]
    fn the_neighbourhood_of_home_is_realistic() {
        let mut sky = Sky::default();
        let home = sky.model.home_pc;
        // Every star within a 32 pc cell around home, whichever tier it is.
        let cell = Cell::containing(12, home).unwrap();
        let total: u64 = sky.tier_counts(cell).iter().sum::<u64>()
            + (0..12)
                .map(|level| {
                    let owner = Cell::containing(level, home).unwrap();
                    sky.cell_stars(owner)
                        .iter()
                        .filter(|s| cell.contains(s.position_pc))
                        .count() as u64
                })
                .sum::<u64>();
        let expected: f64 = Component::ALL.iter().map(|&c| sky.expected(c, cell)).sum();
        let sigma = expected.sqrt();
        assert!(
            (total as f64 - expected).abs() < 5.0 * sigma,
            "{total} stars, expected {expected}"
        );
        // Every star owned by cells within it: mostly red dwarfs, as near the Sun.
        let stars = subtree(&mut sky, cell);
        assert_eq!(
            stars.len() as u64,
            sky.tier_counts(cell).iter().sum::<u64>()
        );
        assert!(stars.iter().all(|s| cell.contains(s.position_pc)));
        let red = stars.iter().filter(|s| s.star.kind == "Red dwarf").count();
        let share = red as f64 / stars.len() as f64;
        assert!((0.65..0.8).contains(&share), "{red} of {}", stars.len());
    }

    fn subtree(sky: &mut Sky, cell: Cell) -> Vec<CellStar> {
        let mut stars = sky.cell_stars(cell);
        if cell.level() + 1 < TIERS
            && sky.tier_counts(cell)[cell.level() + 1..]
                .iter()
                .any(|&n| n > 0)
        {
            for child in cell.children() {
                stars.extend(subtree(sky, child));
            }
        }
        stars
    }

    #[test]
    fn stars_crowd_where_the_model_is_dense() {
        let mut sky = Sky::default();
        // Bright arm stars: near the midplane, and closer to arms than home is.
        let stars = sky.cell_stars(Cell::ROOT);
        assert!(stars.len() > 1_000, "{}", stars.len());
        let arms = stars
            .iter()
            .filter(|s| s.component == Component::Arms)
            .count();
        assert!(arms * 10 > stars.len() * 9);
        let heights: Vec<f64> = stars.iter().map(|s| s.position_pc[1].abs()).collect();
        let mean = heights.iter().sum::<f64>() / heights.len() as f64;
        assert!((60.0..150.0).contains(&mean), "mean height {mean} pc");
        let model = &sky.model;
        let contrast = stars
            .iter()
            .map(|s| {
                model
                    .spiral
                    .modulation(1.0, s.position_pc[0], s.position_pc[2])
            })
            .sum::<f64>()
            / stars.len() as f64;
        assert!(contrast > 1.5, "{contrast}");
    }

    #[test]
    fn a_tier_reaches_a_fixed_number_of_its_cells() {
        // Each tier is twice as far-reaching as the next and its cells twice as
        // large, so loading a few cells around the camera per tier shows every
        // star down to a magnitude limit.
        let cells: Vec<f64> = (0..TIERS)
            .map(|tier| reach_pc(tier, 6.5) / (ROOT_PC / (1u64 << tier) as f64))
            .collect();
        assert!(cells.windows(2).all(|w| (w[0] / w[1] - 1.0).abs() < 1e-9));
        // A star as bright as the Sun reaches naked-eye magnitude at 22 pc.
        assert_eq!(tier_limit(12), 1.0);
        assert!((reach_pc(12, 6.5) - 21.6).abs() < 0.1);
    }

    #[test]
    fn binomial_draws_are_unbiased_and_exact_in_total() {
        let mut rng = Rng::seeded(7);
        for (n, p) in [
            (10u64, 0.3),
            (1_000, 0.01),
            (1_000_000, 0.4),
            (1 << 40, 1e-9),
        ] {
            let draws: Vec<u64> = (0..4_000).map(|_| binomial(&mut rng, n, p)).collect();
            let mean = draws.iter().sum::<u64>() as f64 / draws.len() as f64;
            let sigma = (n as f64 * p * (1.0 - p) / draws.len() as f64).sqrt();
            assert!((mean - n as f64 * p).abs() < 5.0 * sigma, "{n} {p}: {mean}");
            assert!(draws.iter().all(|&k| k <= n));
        }
        let split = multinomial(
            &mut rng,
            1_000_003,
            &[1.0, 0.0, 2.0, 3.0, 0.0, 0.0, 1.0, 0.0],
        );
        assert_eq!(split.iter().sum::<u64>(), 1_000_003);
        assert_eq!(split[1] + split[4] + split[5] + split[7], 0);
    }
}
