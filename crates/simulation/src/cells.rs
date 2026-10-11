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

use crate::galaxy::{COMPONENTS, Component, GalaxyModel};
use crate::population::{TIERS, tiered_bodies, tiered_formation, tiered_light};
use crate::stream::StarPoint;
use crate::system::{Body, Rng};
use crate::{HASH_START, finish_hash, hash_bytes};
use space_shared::Star;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A fast hasher for `Sky`'s small integer keys, looked up at every level of
/// every star's descent. Only lookup speed depends on it, never a result.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn write_isize(&mut self, n: isize) {
        self.write_u64(n as u64);
    }
}

type Cache<K, V> = HashMap<K, V, BuildHasherDefault<KeyHasher>>;

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
pub type Counts = [[u64; TIERS]; COMPONENTS];

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

/// A cell star's ID hashed as `stable_hash` hashes it, without building the
/// string: `key.rng(":place")` is `Rng::new(&format!("{id}:place"))`.
#[derive(Clone, Copy)]
pub(crate) struct StarKey(u64);

impl StarKey {
    pub(crate) fn new(cell: Cell, component: Component, index: u64) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hash = hash_bytes(HASH_START, b"g");
        let nibbles = 16 - cell.0.leading_zeros() / 4;
        for i in (0..nibbles.max(1)).rev() {
            hash = hash_bytes(hash, &[HEX[(cell.0 >> (4 * i) & 15) as usize]]);
        }
        hash = hash_bytes(hash, &[b'.', b'0' + component.index() as u8, b'.']);
        let mut digits = [0u8; 20];
        let mut start = digits.len();
        let mut n = index;
        loop {
            start -= 1;
            digits[start] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        Self(hash_bytes(hash, &digits[start..]))
    }

    pub(crate) fn rng(self, suffix: &str) -> Rng {
        Rng::seeded(finish_hash(hash_bytes(self.0, suffix.as_bytes())))
    }
}

/// Stable catalogue label, derived from a cell star ID without storing names.
pub fn catalogue_name(id: &str) -> String {
    format!(
        "TS {}",
        id.strip_prefix('g').unwrap_or(id).replace('.', "-")
    )
}

/// A planet's catalogue name, from its host's ID: a cell star, or a star or
/// barycentre of its system.
pub fn planet_designation(host: &str, index: usize) -> String {
    crate::multiple::designation(
        &catalogue_name(crate::multiple::system_key(host)),
        " ",
        crate::multiple::member_letters(host),
        index,
    )
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

/// A cell star's planets, by host and innermost first.
pub fn cell_planetary_system(id: &str) -> Option<Vec<Body>> {
    Some(
        cell_bodies(id)?
            .planets
            .into_iter()
            .map(|p| p.body)
            .collect(),
    )
}

/// Everything a cell star's ID generates: its stars, pairs and planets.
pub fn cell_bodies(id: &str) -> Option<crate::multiple::Bodies> {
    let (cell, component, _) = parse_cell_star(id)?;
    Some(tiered_bodies(id, component, cell.level()))
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
    let [x, y, z] = gaps(min, size);
    ((x * x + y * y + z * z).sqrt(), x.hypot(z))
}

/// Gap from the centre to a box along each axis.
fn gaps(min: [f64; 3], size: f64) -> [f64; 3] {
    std::array::from_fn(|axis| {
        let (a, b) = (min[axis], min[axis] + size);
        if a > 0.0 {
            a
        } else if b < 0.0 {
            -b
        } else {
            0.0
        }
    })
}

/// Largest box the three-point rule integrates a component over accurately.
fn resolution(model: &GalaxyModel, component: Component, min: [f64; 3], size: f64) -> f64 {
    // Each component needs only one of `nearest`'s distances: work out just
    // that one, the same way.
    let [x, y, z] = gaps(min, size);
    let r = || (x * x + y * y + z * z).sqrt();
    let radius = || x.hypot(z);
    match component {
        Component::Nucleus => model.nucleus_pc.max(0.3 * r()),
        Component::Bulge => model.bar_pc[1].min(model.bar_pc[2]).max(0.3 * r()),
        Component::Halo => model.halo_core_pc.max(0.3 * r()),
        // Arms are about a tenth of their radius across.
        Component::ThinDisc | Component::Arms => (0.05 * radius()).clamp(250.0, 1_000.0),
        Component::ThickDisc => (0.1 * radius()).clamp(500.0, 1_000.0),
        Component::NuclearDisc => (0.3 * radius()).max(0.5 * model.nuclear.scale_length_pc),
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

/// What `Sky::try_point` needs worked out before it can place a star.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Need {
    /// A cell's child masses, to descend through it.
    Masses(Cell, Component),
    /// The rejection bound in the cell a descent ended in.
    Bound(Cell, Component),
}

/// Streams the galaxy's stars cell by cell, caching counts and masses.
pub struct Sky {
    pub model: GalaxyModel,
    counts: Cache<Cell, Box<Counts>>,
    /// Stars of a component in each child of a cell, by octant.
    masses: Cache<(Cell, Component), [f64; 8]>,
    /// Children integrated so far, for masses not yet complete.
    partial: Cache<(Cell, Component), Vec<f64>>,
    /// Rejection bound of a component's density in the cells stars end in.
    bounds: Cache<(Cell, Component), f64>,
}

impl Sky {
    pub fn new(model: GalaxyModel) -> Self {
        Self {
            model,
            counts: Cache::default(),
            masses: Cache::default(),
            partial: Cache::default(),
            bounds: Cache::default(),
        }
    }

    /// Exact counts at the root: each component's stars, shared among tiers by
    /// largest remainder so nothing is lost to rounding.
    fn root(&self) -> Counts {
        let mut counts = [[0; TIERS]; COMPONENTS];
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
        let mut children = [[[0; TIERS]; COMPONENTS]; 8];
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

    /// A cell's own star by ordinal, without generating every star in the cell.
    pub fn cell_star_at(&mut self, cell: Cell, mut ordinal: u64) -> Option<CellStar> {
        let tier = cell.level();
        if tier >= TIERS {
            return None;
        }
        let counts = self.counts(cell);
        for component in Component::ALL {
            let count = counts[component.index()][tier];
            if ordinal < count {
                return Some(self.build(cell, component, ordinal));
            }
            ordinal -= count;
        }
        None
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
        let position_pc = self.place(StarKey::new(cell, component, index), cell, component);
        CellStar {
            id,
            cell,
            component,
            position_pc,
            star,
        }
    }

    /// One cell star, as `build` makes it, without its names; or what placing
    /// it still needs, for `supply`. Changes nothing, so threads can share it.
    pub(crate) fn try_point(
        &self,
        cell: Cell,
        component: Component,
        index: u64,
    ) -> Result<StarPoint, Need> {
        let key = StarKey::new(cell, component, index);
        let mut rng = key.rng(":place");
        let end = self
            .descend(&mut rng, cell, component)
            .map_err(|(cell, component)| Need::Masses(cell, component))?;
        let bound = *self
            .bounds
            .get(&(end, component))
            .ok_or(Need::Bound(end, component))?;
        let position_pc = within(&self.model, component, end, bound, &mut rng);
        let (luminosity, kelvin, radius) = tiered_light(key, component, cell.level());
        Ok(StarPoint {
            position_pc,
            luminosity_solar: luminosity as f32,
            temperature_k: kelvin as f32,
            radius_solar: radius as f32,
            component,
            index: index as u32,
        })
    }

    /// Works out part of what `try_point` reported missing: one more child's
    /// masses, or a rejection bound. Repeating a need is harmless.
    pub(crate) fn supply(&mut self, need: Need) {
        match need {
            Need::Masses(cell, component) => self.integrate(cell, component),
            Need::Bound(end, component) => {
                let model = &self.model;
                self.bounds
                    .entry((end, component))
                    .or_insert_with(|| bound(model, component, end));
            }
        }
    }

    /// Works out all of a need at once: every child's masses.
    pub(crate) fn complete(&mut self, need: Need) {
        match need {
            Need::Masses(cell, component) => {
                self.child_masses(cell, component);
            }
            Need::Bound(..) => self.supply(need),
        }
    }

    /// Where a star lies in its cell: down through the children in proportion
    /// to their stars, then by rejection once the density is smooth.
    fn place(&mut self, key: StarKey, cell: Cell, component: Component) -> [f64; 3] {
        loop {
            let mut rng = key.rng(":place");
            match self.descend(&mut rng, cell, component) {
                Ok(end) => return self.within_end(end, component, &mut rng),
                Err((cell, component)) => {
                    self.child_masses(cell, component);
                }
            }
        }
    }

    /// A point by rejection in the cell a descent ended in.
    fn within_end(&mut self, end: Cell, component: Component, rng: &mut Rng) -> [f64; 3] {
        let model = &self.model;
        let bound = *self
            .bounds
            .entry((end, component))
            .or_insert_with(|| bound(model, component, end));
        within(model, component, end, bound, rng)
    }

    /// The cell a star's descent ends in, or the masses it needs first. Tracks
    /// the cell's corner on the way down: exact, as every corner is a sum of
    /// powers of two.
    fn descend(
        &self,
        rng: &mut Rng,
        cell: Cell,
        component: Component,
    ) -> Result<Cell, (Cell, Component)> {
        let mut cell = cell;
        let (mut min, mut size) = (cell.min_pc(), cell.size_pc());
        while cell.level() < DEEPEST && size > resolution(&self.model, component, min, size) {
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
            size /= 2.0;
            min = corner(min, size, octant);
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
    fn star_keys_seed_as_their_id_strings_do() {
        for (cell, index) in [
            (Cell::ROOT, 0),
            (Cell(0xf), 9),
            (Cell(0x10), 10),
            (Cell(0x1234_5678_9abc), 4_294_967_295),
        ] {
            for component in Component::ALL {
                let id = cell_star_id(cell, component, index);
                for suffix in [":place", ":tier", ":star"] {
                    let mut key = StarKey::new(cell, component, index).rng(suffix);
                    let mut text = Rng::new(&format!("{id}{suffix}"));
                    assert_eq!(key.unit(), text.unit(), "{id}{suffix}");
                }
            }
        }
    }

    #[test]
    fn star_points_match_the_full_stars() {
        let mut sky = Sky::default();
        let model = GalaxyModel::milky_way();
        let mut kinds = std::collections::HashSet::new();
        for p in [[3.0, 1.0, -2.0], [200.0, 10.0, 50.0], model.home_pc] {
            for level in [5, 9, 13] {
                let cell = Cell::containing(level, p).unwrap();
                let counts = sky.counts(cell);
                for component in Component::ALL {
                    for index in 0..counts[component.index()][level].min(40) {
                        let point = loop {
                            match sky.try_point(cell, component, index) {
                                Ok(point) => break point,
                                Err(need) => sky.supply(need),
                            }
                        };
                        let full = sky.build(cell, component, index);
                        assert_eq!(point.position_pc, full.position_pc, "{}", full.id);
                        // A point is the whole system's light.
                        let bodies = cell_bodies(&full.id).unwrap();
                        let light: f64 = bodies.stars.iter().map(|s| s.luminosity_solar).sum();
                        let error = (f64::from(point.luminosity_solar) / light - 1.0).abs();
                        assert!(error < 1e-6, "{}: {point:?} {light}", full.id);
                        assert_eq!(bodies.stars[0].luminosity_solar, full.star.luminosity_solar);
                        assert_eq!(point.temperature_k, full.star.temperature_k as f32);
                        assert_eq!(point.radius_solar, full.star.radius_solar as f32);
                        kinds.insert(full.star.kind);
                    }
                }
            }
        }
        // Each way `tiered_light` works a star out: main sequence, evolved
        // and white dwarf.
        for kind in ["Orange dwarf", "giant", "White dwarf"] {
            assert!(kinds.iter().any(|k| k.contains(kind)), "{kinds:?}");
        }
    }

    #[test]
    fn catalogue_names_are_unique_stable_and_planets_are_lettered() {
        assert_eq!(catalogue_name("g78040000.2.854"), "TS 78040000-2-854");
        assert_ne!(
            catalogue_name("g78040000.2.854"),
            catalogue_name("g78040000.2.855")
        );
        assert_eq!(
            planet_designation("g78040000.2.854", 0),
            "TS 78040000-2-854 b"
        );
    }

    #[test]
    fn indexed_stars_match_cell_population() {
        let home = GalaxyModel::milky_way().home_pc;
        let cell = Cell::containing(10, home).unwrap();
        let mut sky = Sky::default();
        let stars = sky.cell_stars(cell);
        assert!(!stars.is_empty());
        for index in [0, stars.len() / 2, stars.len() - 1] {
            assert_eq!(
                sky.cell_star_at(cell, index as u64),
                Some(stars[index].clone())
            );
        }
        assert_eq!(sky.cell_star_at(cell, stars.len() as u64), None);
    }

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
                for c in 0..COMPONENTS {
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
