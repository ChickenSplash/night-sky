//! Binaries, triples and quadruples: a star's companions, how they nest, and
//! where planets can orbit among them.
//!
//! - Multiplicity rises with the primary's mass: about a quarter of red dwarfs,
//!   under half of Sun-like stars and most B stars have companions (Raghavan et
//!   al. 2010; Winters et al. 2019; Moe & Di Stefano 2017). About a quarter of
//!   multiples are triples or more, up to the quadruples drawn here.
//! - Companion masses are a roughly flat share of the primary's, with an excess
//!   of near twins. Separations are log-normal, peaking near 20 AU for red dwarfs
//!   and 50 AU for Sun-like stars, from contact to a tenth of a parsec.
//! - Every star of a system is the same age. A companion born heavier has
//!   evolved first, as Sirius B did. Companions too close to a star that grew
//!   into a giant merged with it or were engulfed, so white dwarfs have fewer.
//! - The primary is the system's brightest star: it alone fixes the system's
//!   brightness tier, and companions add at most a few times its light.
//! - Systems are hierarchical, as real ones must be to last: a close pair with
//!   a distant third, or two pairs orbiting each other. Each pair is a two-body
//!   orbit about its barycentre, spaced to stay stable (Mardling & Aarseth 2001).
//! - Planets orbit one star inside its stable zone, or a close pair from outside
//!   its zone (Holman & Wiegert 1999). Pairs closer than 50 AU suppress planet
//!   formation to about a third (Kraus et al. 2016).

use crate::seed::Rng;
use crate::star::{
    self, Formation, GIANT_SHARE, Origin, Stage, WHITE_DWARF_LIMIT, imf_cumulative, imf_inverse,
    lifetime_gyr,
};
use crate::system::{Body, system_within};
use space_shared::{Barycentre, Orbit, Star};
use std::f64::consts::TAU;

/// Share of companions born heavier than the primary, which may have died.
/// With a flat mass ratio, a star of the steep upper initial mass function is
/// a heavier star's companion about a third as often as it has one of its own.
const BORN_HEAVIER: f64 = 0.35;
/// Share of companions that are near twins of their primary.
const TWINS: f64 = 0.1;
/// Share of triples and higher with a fourth star.
const QUADRUPLES: f64 = 0.27;
const LIGHTEST_STAR: f64 = 0.08;
/// Separations drawn, in AU: from touching to where the galaxy's tides pull
/// pairs apart.
const SEPARATION_AU: (f64, f64) = (0.01, 20_000.0);
/// Absolute post-widening stellar extent. Extremely wide hierarchies are not
/// retained as bound systems in this alpha (galactic tides are not integrated).
/// Also bounds how far a craft looks for systems it may enter.
pub const MAX_STELLAR_SEPARATION_AU: f64 = 100_000.0;
/// Companions closer than this to a star that became a white dwarf shared its
/// giant envelope and merged or spiralled in.
const ENVELOPE_AU: f64 = 3.0;
/// Pairs closer than this form discs around both stars, not each one.
const CIRCUMBINARY_AU: f64 = 1.0;
/// Pairs closer than this truncate their stars' discs.
const CLOSE_AU: f64 = 50.0;
const CLOSE_PLANETS: f64 = 0.34;
/// Pairs closer than this are coplanar with their planets, within a few
/// degrees; wider ones are oriented at random.
const ALIGNED_AU: f64 = 100.0;
/// Orbits with periods under this many days have been made circular by tides.
const CIRCULAR_DAYS: f64 = 10.0;

/// Interpolated linearly in the logarithm of mass.
fn by_mass(table: &[(f64, f64)], mass: f64) -> f64 {
    let x = mass.max(1e-3).log10();
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first.0.log10() {
        return first.1;
    }
    table
        .windows(2)
        .find(|w| x <= w[1].0.log10())
        .map_or(last.1, |w| {
            let (x0, x1) = (w[0].0.log10(), w[1].0.log10());
            w[0].1 + (w[1].1 - w[0].1) * (x - x0) / (x1 - x0)
        })
}

/// Share of stars of this birth mass with at least one companion.
pub(crate) fn multiple_share(mass: f64) -> f64 {
    by_mass(
        &[
            (0.1, 0.22),
            (0.3, 0.27),
            (0.7, 0.42),
            (1.0, 0.46),
            (2.0, 0.55),
            (5.0, 0.7),
            (10.0, 0.8),
            (30.0, 0.9),
        ],
        mass,
    )
}

/// Share of multiples with a third star.
fn higher_order_share(mass: f64) -> f64 {
    by_mass(&[(0.1, 0.2), (1.0, 0.25), (5.0, 0.35), (20.0, 0.45)], mass)
}

fn normal(rng: &mut Rng) -> f64 {
    let (u, v) = (1.0 - rng.unit(), rng.unit());
    (-2.0 * u.ln()).sqrt() * (TAU * v).cos()
}

/// A separation in AU, log-normal: red dwarf pairs are tighter than Sun-like
/// ones, and massive stars have more close companions.
fn separation_au(rng: &mut Rng, mass: f64) -> f64 {
    let mean = by_mass(
        &[(0.1, 0.7), (0.4, 1.1), (1.0, 1.7), (3.0, 1.4), (10.0, 0.9)],
        mass,
    );
    let sigma = by_mass(&[(0.1, 1.0), (0.5, 1.3), (1.0, 1.5), (10.0, 1.7)], mass);
    let (low, high) = SEPARATION_AU;
    let mut au = high;
    for _ in 0..8 {
        au = 10f64.powf(mean + sigma * normal(rng));
        if (low..=high).contains(&au) {
            break;
        }
    }
    au.clamp(low, high)
}

/// A companion as it is today.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Main {
        mass: f64,
    },
    WhiteDwarf {
        progenitor: f64,
        mass: f64,
        kelvin: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Companion {
    pub kind: Kind,
    pub luminosity: f64,
    /// Separation from the primary at birth, in AU, and eccentricity.
    pub separation_au: f64,
    pub eccentricity: f64,
}

impl Kind {
    fn mass(self) -> f64 {
        match self {
            Kind::Main { mass } | Kind::WhiteDwarf { mass, .. } => mass,
        }
    }

    /// Radius in solar radii, as the star built from it has.
    fn radius(self) -> f64 {
        match self {
            Kind::Main { mass } => star::main_sequence_radius(mass),
            Kind::WhiteDwarf { mass, .. } => star::white_dwarf_radius(mass),
        }
    }

    fn birth_mass(self) -> f64 {
        match self {
            Kind::Main { mass } => mass,
            Kind::WhiteDwarf { progenitor, .. } => progenitor,
        }
    }

    fn luminosity(self) -> f64 {
        match self {
            Kind::Main { mass } => star::luminosity(mass),
            Kind::WhiteDwarf { mass, kelvin, .. } => {
                star::surface_light(star::white_dwarf_radius(mass), kelvin)
            }
        }
    }
}

/// The primary as companions see it: its origin, and its light and radius now.
pub(crate) struct Primary {
    pub origin: Origin,
    pub mass_solar: f64,
    pub luminosity: f64,
    pub radius_solar: f64,
    pub kelvin: f64,
}

impl Primary {
    pub(crate) fn of(formed: &Formation, origin: Origin) -> Self {
        Self {
            origin,
            mass_solar: formed.star.mass_solar,
            luminosity: formed.star.luminosity_solar,
            radius_solar: formed.star.radius_solar,
            kelvin: formed.star.temperature_k,
        }
    }

    /// Age of the system, drawn from its component when not yet known.
    fn age(&self, rng: &mut Rng) -> f64 {
        let origin = &self.origin;
        let life = lifetime_gyr(origin.birth_mass);
        origin.age_gyr.unwrap_or_else(|| match origin.stage {
            Stage::Main => origin.component.age_between(rng, 0.0, life),
            Stage::Giant => life * (1.0 + GIANT_SHARE * rng.unit()),
            Stage::WhiteDwarf => life * (1.0 + GIANT_SHARE) + star::cooling_gyr(self.kelvin),
        })
    }
}

/// What a companion of this birth mass is today, at the system's age.
fn evolve(rng: &mut Rng, mass: f64, age: f64) -> Option<Kind> {
    let life = lifetime_gyr(mass);
    if age < life {
        Some(Kind::Main { mass })
    } else if age < life * (1.0 + GIANT_SHARE) || mass >= WHITE_DWARF_LIMIT {
        // A giant would outshine the primary; a neutron star or black hole is
        // not a star.
        None
    } else {
        Some(Kind::WhiteDwarf {
            progenitor: mass,
            mass: star::white_dwarf_mass(rng, mass),
            kelvin: star::cooled_kelvin(age - life * (1.0 + GIANT_SHARE)),
        })
    }
}

fn companion_kind(rng: &mut Rng, primary: &Primary, age: f64) -> Option<Kind> {
    let birth = primary.origin.birth_mass;
    if primary.origin.stage != Stage::WhiteDwarf && rng.chance(BORN_HEAVIER) {
        let (low, high) = (imf_cumulative(birth), imf_cumulative(WHITE_DWARF_LIMIT));
        if high > low {
            let progenitor = imf_inverse(low + rng.unit() * (high - low));
            if let Some(kind @ Kind::WhiteDwarf { .. }) = evolve(rng, progenitor, age) {
                return Some(kind);
            }
        }
    }
    let lightest = (LIGHTEST_STAR / birth).clamp(0.1, 1.0);
    let q = if rng.chance(TWINS) {
        rng.range(0.95, 1.0)
    } else {
        rng.range(lightest, 1.0)
    };
    evolve(rng, (q * birth).max(LIGHTEST_STAR), age)
}

fn companion_count(rng: &mut Rng, birth: f64) -> usize {
    if !rng.chance(multiple_share(birth)) {
        0
    } else if !rng.chance(higher_order_share(birth)) {
        1
    } else if rng.chance(QUADRUPLES) {
        3
    } else {
        2
    }
}

/// The system's age: the one `companions` draws from the same `draws`, or for
/// a single star, whose age nothing else needs, one drawn from `own`.
pub(crate) fn system_age(draws: &mut Rng, primary: &Primary, own: &mut Rng) -> f64 {
    if companion_count(draws, primary.origin.birth_mass) == 0 {
        primary.age(own)
    } else {
        primary.age(draws)
    }
}

/// The primary's companions, brightest first. Draws nothing more for a
/// single star than whether it is one.
pub(crate) fn companions(rng: &mut Rng, primary: &Primary) -> Vec<Companion> {
    let birth = primary.origin.birth_mass;
    let extra = companion_count(rng, birth);
    if extra == 0 {
        return Vec::new();
    }
    let age = primary.age(rng);
    let giant_au = 3.0 * primary.radius_solar * crate::SUN_RADIUS_METRES / crate::AU_METRES;
    let mut companions = Vec::with_capacity(extra);
    for _ in 0..extra {
        let kind = companion_kind(rng, primary, age);
        let separation_au = separation_au(rng, birth);
        let eccentricity = rng.range(0.0, 0.8);
        let Some(kind) = kind else {
            continue;
        };
        let luminosity = kind.luminosity();
        let periapsis = separation_au * (1.0 - eccentricity);
        let engulfed = match primary.origin.stage {
            Stage::Giant => periapsis < giant_au,
            Stage::WhiteDwarf => separation_au < ENVELOPE_AU,
            Stage::Main => false,
        } || matches!(kind, Kind::WhiteDwarf { .. }) && separation_au < ENVELOPE_AU;
        // A brighter companion would be the primary of a system drawn elsewhere.
        if !engulfed && luminosity < primary.luminosity {
            companions.push(Companion {
                kind,
                luminosity,
                separation_au,
                eccentricity,
            });
        }
    }
    companions.sort_by(|a, b| b.luminosity.total_cmp(&a.luminosity));
    companions
}

/// A system's members: its stars, primary first, or a pair of members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Node {
    Star(usize),
    Pair(usize),
}

#[derive(Clone, Debug)]
pub(crate) struct Pair {
    /// Heavier first.
    pub members: [Node; 2],
    /// Orbit of the second member relative to the first.
    pub orbit: Orbit,
}

/// Stability limit, as a share of the pair's separation, for planets orbiting
/// one member, whose partner holds `mu` of the pair's mass (Holman & Wiegert
/// 1999).
pub(crate) fn s_type_limit(mu: f64, e: f64) -> f64 {
    let mu = mu.clamp(0.0, 0.9);
    (0.464 - 0.380 * mu - 0.631 * e + 0.586 * mu * e + 0.150 * e * e - 0.198 * mu * e * e).max(0.01)
}

/// Inner stability limit, as a share of the pair's separation, for planets
/// orbiting both members; the lighter holds `mu` of the mass.
fn p_type_limit(mu: f64, e: f64) -> f64 {
    let mu = mu.clamp(0.0, 0.5);
    1.60 + 5.10 * e - 2.22 * e * e + 4.12 * mu - 4.27 * e * mu - 5.09 * mu * mu
        + 4.61 * e * e * mu * mu
}

/// A system's stars and how they pair up, before planets.
pub(crate) struct Hierarchy {
    /// Primary first, then in letter order. Empty when only the layout is
    /// wanted (`companion_light`).
    pub stars: Vec<Formation>,
    /// Each star's mass, in solar masses, and radius, in AU, in star order.
    masses: Vec<f64>,
    radii_au: Vec<f64>,
    /// Innermost first; the last holds the whole system.
    pub pairs: Vec<Pair>,
}

impl Hierarchy {
    /// Stars of these masses (solar) and radii (solar), not yet paired.
    fn of(stars: Vec<Formation>, bodies: impl Iterator<Item = (f64, f64)>) -> Self {
        let (masses, radii_au) = bodies
            .map(|(mass, radius)| (mass, radius * crate::SUN_RADIUS_METRES / crate::AU_METRES))
            .unzip();
        Self {
            stars,
            masses,
            radii_au,
            pairs: Vec::new(),
        }
    }

    pub(crate) fn mass(&self, node: Node) -> f64 {
        match node {
            Node::Star(i) => self.masses[i],
            Node::Pair(p) => self.pairs[p].members.iter().map(|&m| self.mass(m)).sum(),
        }
    }

    fn letters(&self, node: Node) -> String {
        let mut letters: Vec<char> = Vec::new();
        self.collect(node, &mut letters);
        letters.sort_unstable();
        letters.into_iter().collect()
    }

    fn collect(&self, node: Node, letters: &mut Vec<char>) {
        match node {
            Node::Star(i) => letters.push((b'A' + i as u8) as char),
            Node::Pair(p) => {
                for &member in &self.pairs[p].members {
                    self.collect(member, letters);
                }
            }
        }
    }

    pub(crate) fn root(&self) -> Node {
        if self.pairs.is_empty() {
            Node::Star(0)
        } else {
            Node::Pair(self.pairs.len() - 1)
        }
    }

    /// Furthest a member's stars reach from its barycentre, in AU.
    fn reach_au(&self, node: Node) -> f64 {
        match node {
            Node::Star(i) => self.radii_au[i],
            Node::Pair(p) => {
                let orbit = &self.pairs[p].orbit;
                orbit.semi_major_au * (1.0 + orbit.eccentricity)
            }
        }
    }

    /// Widest any two stars of the system get, in AU: zero for a single star.
    pub(crate) fn separation_au(&self) -> f64 {
        widest(self.pairs.len().checked_sub(1), |p| {
            let pair = &self.pairs[p];
            (
                pair.orbit,
                pair.members.map(|m| match m {
                    Node::Pair(inner) => Some(inner),
                    Node::Star(_) => None,
                }),
            )
        })
    }

    /// ID of a star or pair: a pair by its stars' letters.
    fn id(&self, key: &str, node: Node) -> String {
        match node {
            Node::Star(i) => self.stars[i].star.id.clone(),
            Node::Pair(_) => member_id(key, &self.letters(node)),
        }
    }

    /// The pairs as barycentres, innermost first.
    fn barycentres(&self, key: &str) -> Vec<Barycentre> {
        (0..self.pairs.len())
            .map(|p| Barycentre {
                id: self.id(key, Node::Pair(p)),
                page: 0,
                component: self.letters(Node::Pair(p)),
                members: self.pairs[p].members.map(|m| self.id(key, m)),
                orbit: self.pairs[p].orbit,
            })
            .collect()
    }

    /// Whether a member is or holds this star.
    fn holds(&self, node: Node, star: usize) -> bool {
        match node {
            Node::Star(i) => i == star,
            Node::Pair(p) => self.pairs[p].members.iter().any(|&m| self.holds(m, star)),
        }
    }

    /// Stars in letter order: the primary's side of each pair first, so the
    /// primary is A, its close companion B and a distant third C.
    fn lettered(&self, node: Node, order: &mut Vec<usize>) {
        match node {
            Node::Star(i) => order.push(i),
            Node::Pair(p) => {
                let [first, second] = self.pairs[p].members;
                let (first, second) = if self.holds(second, 0) {
                    (second, first)
                } else {
                    (first, second)
                };
                self.lettered(first, order);
                self.lettered(second, order);
            }
        }
    }

    /// Letters and names the stars as astronomers do, down the hierarchy.
    fn reletter(&mut self, key: &str) {
        let mut order = Vec::new();
        self.lettered(self.root(), &mut order);
        let mut moved = vec![0; order.len()];
        let mut stars: Vec<_> = std::mem::take(&mut self.stars)
            .into_iter()
            .map(Some)
            .collect();
        self.masses = order.iter().map(|&old| self.masses[old]).collect();
        self.radii_au = order.iter().map(|&old| self.radii_au[old]).collect();
        for (index, &old) in order.iter().enumerate() {
            moved[old] = index;
            let mut formed = stars[old].take().expect("each star once");
            let letter = ((b'A' + index as u8) as char).to_string();
            formed.star.id = member_id(key, &letter);
            formed.star.component = letter;
            self.stars.push(formed);
        }
        for pair in &mut self.pairs {
            for member in &mut pair.members {
                if let Node::Star(i) = member {
                    *i = moved[*i];
                }
            }
        }
    }

    /// Pairs the stars up, from the companions' separations: a close pair and
    /// its distant third, or two pairs, or a chain, spaced to stay stable.
    fn arrange(&mut self, rng: &mut Rng, birth_mass: f64, companions: &[Companion]) {
        // Orbits widened as stars that are now white dwarfs lost their envelopes.
        let born: f64 = std::iter::once(birth_mass)
            .chain(companions.iter().map(|c| c.kind.birth_mass()))
            .sum();
        let now: f64 = self.masses.iter().sum();
        let widening = born / now;
        // Companions by separation, innermost first.
        let mut order: Vec<usize> = (1..self.masses.len()).collect();
        order.sort_by(|&a, &b| {
            companions[a - 1]
                .separation_au
                .total_cmp(&companions[b - 1].separation_au)
        });
        let orbit = |i: usize| {
            let c = &companions[i - 1];
            (c.separation_au * widening, c.eccentricity)
        };
        let star = Node::Star;
        match *order.as_slice() {
            [b] => {
                let (a, e) = orbit(b);
                self.join(rng, star(0), star(b), a, e);
            }
            [b, c] => {
                let (inner, e_in) = orbit(b);
                let (outer, e_out) = orbit(c);
                // The primary in the close pair, as Alpha Centauri A is with
                // Proxima far out, or orbited by a close pair of its companions.
                let (pair, third) = if rng.chance(0.5) {
                    (self.join(rng, star(0), star(b), inner, e_in), star(c))
                } else {
                    (self.join(rng, star(b), star(c), inner, e_in), star(0))
                };
                self.join(rng, pair, third, outer, e_out);
            }
            [b, c, d] => {
                let ((ab, e_ab), (cd, e_cd), (outer, e_out)) = (orbit(b), orbit(c), orbit(d));
                if rng.chance(0.5) {
                    // Two pairs orbiting each other.
                    let first = self.join(rng, star(0), star(b), ab, e_ab);
                    let second = self.join(rng, star(c), star(d), cd, e_cd);
                    self.join(rng, first, second, outer, e_out);
                } else {
                    let inner = self.join(rng, star(0), star(b), ab, e_ab);
                    let triple = self.join(rng, inner, star(c), cd, e_cd);
                    self.join(rng, triple, star(d), outer, e_out);
                }
            }
            _ => {}
        }
        // Widening after mass loss and hierarchical stability adjustments can
        // exceed the initial separation draw. Such systems would need disruption
        // modelling; keep only their primary rather than a huge bound hierarchy.
        if self.separation_au() > MAX_STELLAR_SEPARATION_AU {
            self.stars.truncate(1);
            self.masses.truncate(1);
            self.radii_au.truncate(1);
            self.pairs.clear();
        }
    }

    /// Joins two members in an orbit drawn from this separation, widened if
    /// needed to keep the hierarchy stable and the stars apart.
    fn join(&mut self, rng: &mut Rng, a: Node, b: Node, separation_au: f64, e: f64) -> Node {
        let (ma, mb) = (self.mass(a), self.mass(b));
        let members = if ma >= mb { [a, b] } else { [b, a] };
        let (inner, outer) = (self.reach_au(a), self.reach_au(b));
        let mut e = e;
        // Mardling & Aarseth (2001), with the larger member's extent as the
        // inner orbit. For stars, keep them a few radii apart.
        let required = |e: f64| {
            let q = ma.min(mb) / ma.max(mb);
            let extent = inner.max(outer);
            let stars_only = matches!((a, b), (Node::Star(_), Node::Star(_)));
            if stars_only {
                3.0 * (inner + outer) / (1.0 - e)
            } else {
                2.8 * (1.0 + q).powf(0.4) * (1.0 + e).powf(0.4) * (1.0 - e).powf(-0.2) * extent
                    / (1.0 - e)
            }
        };
        if separation_au < required(e) {
            e = e.min(0.3);
        }
        let mut semi_major = separation_au.max(required(e));
        let period_days = (semi_major.powi(3) / (ma + mb)).sqrt() * 365.25;
        if period_days < CIRCULAR_DAYS {
            e = rng.rayleigh(0.01).min(0.05);
            semi_major = semi_major.max(required(e));
        } else {
            rng.unit();
        }
        let inclination = if semi_major < ALIGNED_AU {
            rng.rayleigh(0.05)
        } else {
            (1.0 - 2.0 * rng.unit()).acos()
        };
        self.pairs.push(Pair {
            members,
            orbit: Orbit {
                semi_major_au: semi_major,
                eccentricity: e,
                inclination_rad: inclination,
                ascending_node_rad: rng.range(0.0, TAU),
                phase_rad: rng.range(0.0, TAU),
            },
        });
        Node::Pair(self.pairs.len() - 1)
    }
}

/// Where a system's draws come from: one sequence, or, for tiered stars, the
/// tier's sequence for what sets their light and the star's for the rest.
pub(crate) struct Streams<'a> {
    draws: &'a mut Rng,
    stars: Option<&'a mut Rng>,
}

impl Streams<'_> {
    fn stars(&mut self) -> &mut Rng {
        match &mut self.stars {
            Some(stars) => stars,
            None => self.draws,
        }
    }
}

/// Builds the companions' stars and pairs them up with the primary.
fn hierarchy(
    key: &str,
    streams: &mut Streams,
    primary: Formation,
    origin: &Origin,
    companions: &[Companion],
) -> Hierarchy {
    let mut stars = vec![primary];
    for (index, companion) in companions.iter().enumerate() {
        let id = member_id(key, &((b'B' + index as u8) as char).to_string());
        stars.push(match companion.kind {
            Kind::Main { mass } => {
                let kelvin =
                    star::temperature(star::luminosity(mass), star::main_sequence_radius(mass));
                star::main_sequence(&id, streams.stars(), kelvin, mass)
            }
            Kind::WhiteDwarf {
                progenitor,
                mass,
                kelvin,
            } => star::white_dwarf_of_mass(&id, streams.stars(), progenitor, mass, kelvin),
        });
    }
    let bodies: Vec<_> = stars
        .iter()
        .map(|f| (f.star.mass_solar, f.star.radius_solar))
        .collect();
    let mut system = Hierarchy::of(stars, bodies.into_iter());
    if companions.is_empty() {
        return system;
    }
    system.arrange(&mut *streams.draws, origin.birth_mass, companions);
    if !system.pairs.is_empty() {
        system.reletter(key);
    }
    system
}

/// A star or barycentre that planets orbit.
struct Host {
    node: Node,
    /// Innermost and outermost stable orbits, in AU.
    inner_au: f64,
    outer_au: f64,
    /// Tightest pair it belongs to, in AU.
    close_au: f64,
}

fn hosts(system: &Hierarchy, node: Node, outer_au: f64, close_au: f64, out: &mut Vec<Host>) {
    match node {
        Node::Star(i) => out.push(Host {
            node,
            inner_au: system.stars[i].inner_au,
            outer_au,
            close_au,
        }),
        Node::Pair(p) => {
            let pair = &system.pairs[p];
            let (a, e) = (pair.orbit.semi_major_au, pair.orbit.eccentricity);
            let masses = pair.members.map(|m| system.mass(m));
            let total = masses[0] + masses[1];
            if a < CIRCUMBINARY_AU {
                let members = pair
                    .members
                    .map(|m| match m {
                        Node::Star(i) => system.stars[i].inner_au,
                        Node::Pair(_) => 0.0,
                    })
                    .into_iter()
                    .fold(0.0, f64::max);
                out.push(Host {
                    node,
                    inner_au: (p_type_limit(masses[1] / total, e) * a).max(members),
                    outer_au,
                    close_au,
                });
            } else {
                for (k, &member) in pair.members.iter().enumerate() {
                    let limit = s_type_limit(masses[1 - k] / total, e) * a;
                    hosts(system, member, outer_au.min(limit), close_au.min(a), out);
                }
            }
        }
    }
}

/// Everything a star system's key generates: its stars, their pairs and
/// their planets.
#[derive(Clone, Debug)]
pub struct Bodies {
    /// Primary first.
    pub stars: Vec<Star>,
    /// Innermost first; the last holds the whole system.
    pub barycentres: Vec<Barycentre>,
    /// By host, then innermost first.
    pub planets: Vec<Hosted>,
}

impl Bodies {
    /// Mass of a star or barycentre, in solar masses.
    pub fn mass(&self, id: &str) -> f64 {
        match self.barycentres.iter().find(|b| b.id == id) {
            Some(pair) => pair.members.iter().map(|m| self.mass(m)).sum(),
            None => self
                .stars
                .iter()
                .find(|s| s.id == id)
                .map_or(0.0, |s| s.mass_solar),
        }
    }

    /// Each host's planets, innermost first, in planet order.
    pub fn by_host(&self) -> Vec<(&str, Vec<&Body>)> {
        let mut hosts: Vec<(&str, Vec<&Body>)> = Vec::new();
        for planet in &self.planets {
            match hosts.last_mut() {
                Some((host, bodies)) if *host == planet.host => bodies.push(&planet.body),
                _ => hosts.push((&planet.host, vec![&planet.body])),
            }
        }
        hosts
    }
}

/// A planet and what it orbits.
#[derive(Clone, Debug)]
pub struct Hosted {
    /// Star or barycentre ID.
    pub host: String,
    /// Letters of the host within a multiple system; empty for a single star.
    pub host_letters: String,
    /// Orbit index around the host.
    pub index: usize,
    pub body: Body,
}

impl Hosted {
    pub fn id(&self) -> String {
        format!("{}/{}", self.host, self.index)
    }
}

/// ID of a star or barycentre within a multiple system.
pub fn member_id(key: &str, letters: &str) -> String {
    format!("{key}#{letters}")
}

/// The system key of a star, barycentre or planet ID: the primary's seed.
pub fn system_key(id: &str) -> &str {
    let id = id.rsplit_once('/').map_or(id, |(host, _)| host);
    id.split_once('#').map_or(id, |(key, _)| key)
}

/// Letters of a member ID within its system; empty for a single star.
pub fn member_letters(id: &str) -> &str {
    let id = id.rsplit_once('/').map_or(id, |(host, _)| host);
    id.split_once('#').map_or("", |(_, letters)| letters)
}

/// A planet's catalogue name: exoplanet style, "b" for the innermost, after
/// `separator` around a single star, else with its host star's letter ("Ab")
/// or its pair's in brackets ("(AB) b").
pub fn designation(name: &str, separator: &str, host_letters: &str, index: usize) -> String {
    let letter = (b'b' + index as u8) as char;
    match host_letters.len() {
        0 => format!("{name}{separator}{letter}"),
        1 => format!("{name} {host_letters}{letter}"),
        _ => format!("{name} ({host_letters}) {letter}"),
    }
}

/// The stars, pairs and planets of a system around this primary, continuing
/// its draws. `stars`, if given, draws the companions' details apart from what
/// sets their light, as tiered stars do.
pub(crate) fn assemble(
    key: &str,
    formed: Formation,
    origin: Origin,
    draws: &mut Rng,
    stars: Option<&mut Rng>,
) -> Bodies {
    let primary = Primary::of(&formed, origin);
    let companions = companions(draws, &primary);
    let mut streams = Streams { draws, stars };
    let system = hierarchy(key, &mut streams, formed, &origin, &companions);
    let draws = streams.draws;
    let mut found = Vec::new();
    hosts(
        &system,
        system.root(),
        f64::INFINITY,
        f64::INFINITY,
        &mut found,
    );
    // In letter order: the primary's planets first.
    found.sort_by_key(|host| system.letters(host.node));
    let id = |node: Node| system.id(key, node);
    let mut planets = Vec::new();
    for host in found {
        if (CIRCUMBINARY_AU..CLOSE_AU).contains(&host.close_au) && !draws.chance(CLOSE_PLANETS) {
            continue;
        }
        let host_id = id(host.node);
        let formed = match host.node {
            Node::Star(i) => system.stars[i].clone(),
            Node::Pair(_) => around_pair(&system, host.node, host.inner_au),
        };
        let letters = if system.pairs.is_empty() {
            String::new()
        } else {
            system.letters(host.node)
        };
        let bodies = system_within(&host_id, &formed, host.outer_au);
        planets.extend(bodies.into_iter().enumerate().map(|(index, body)| Hosted {
            host: host_id.clone(),
            host_letters: letters.clone(),
            index,
            body,
        }));
    }
    let barycentres = system.barycentres(key);
    Bodies {
        stars: system.stars.into_iter().map(|f| f.star).collect(),
        barycentres,
        planets,
    }
}

/// The stars and pairs alone of the system `assemble` makes, without its
/// planets.
pub(crate) fn stars_only(
    key: &str,
    formed: Formation,
    origin: Origin,
    draws: &mut Rng,
    stars: Option<&mut Rng>,
) -> (Vec<Star>, Vec<Barycentre>) {
    let primary = Primary::of(&formed, origin);
    let companions = companions(draws, &primary);
    let mut streams = Streams { draws, stars };
    let system = hierarchy(key, &mut streams, formed, &origin, &companions);
    let barycentres = system.barycentres(key);
    (
        system.stars.into_iter().map(|f| f.star).collect(),
        barycentres,
    )
}

/// Widest any two stars get, in AU, from the outermost pair (`root`) down:
/// each pair's apoapsis plus how far its members' own stars reach. `pair`
/// gives a pair's orbit and which members are pairs themselves.
fn widest(root: Option<usize>, pair: impl Fn(usize) -> (Orbit, [Option<usize>; 2])) -> f64 {
    let apoapsis = |orbit: Orbit| orbit.semi_major_au * (1.0 + orbit.eccentricity);
    // Furthest a member's stars get from its barycentre, at most.
    fn reach(member: Option<usize>, pair: &impl Fn(usize) -> (Orbit, [Option<usize>; 2])) -> f64 {
        member.map_or(0.0, |p| {
            let (orbit, members) = pair(p);
            let inner = reach(members[0], pair).max(reach(members[1], pair));
            orbit.semi_major_au * (1.0 + orbit.eccentricity) + inner
        })
    }
    root.map_or(0.0, |root| {
        let (orbit, members) = pair(root);
        apoapsis(orbit) + reach(members[0], &pair) + reach(members[1], &pair)
    })
}

/// Widest any two stars of a system's pairs get, in AU, as `Hierarchy`
/// works it out.
pub fn widest_separation_au(barycentres: &[Barycentre]) -> f64 {
    let index = |id: &str| barycentres.iter().position(|b| b.id == id);
    widest(barycentres.len().checked_sub(1), |p| {
        let pair = &barycentres[p];
        (pair.orbit, pair.members.each_ref().map(|m| index(m)))
    })
}

/// A pair's stars as one host: their mass and light together, with the disc
/// starting outside their stable zone.
fn around_pair(system: &Hierarchy, node: Node, inner_au: f64) -> Formation {
    let mut stars = Vec::new();
    let mut letters = Vec::new();
    system.collect(node, &mut letters);
    for letter in letters {
        stars.push(&system.stars[(letter as u8 - b'A') as usize]);
    }
    let primary = stars[0];
    let mut star = primary.star.clone();
    star.mass_solar = system.mass(node);
    star.luminosity_solar = stars.iter().map(|s| s.star.luminosity_solar).sum();
    Formation {
        star,
        snow_au: stars.iter().map(|s| s.snow_au.powi(2)).sum::<f64>().sqrt(),
        inner_au,
    }
}

/// The light of a primary's companions, and the widest its stars get (AU),
/// without building them: what `assemble` draws from the same sequence.
pub(crate) fn companion_light(draws: &mut Rng, primary: &Primary) -> (f64, f64) {
    let companions = companions(draws, primary);
    if companions.is_empty() {
        return (0.0, 0.0);
    }
    let bodies = std::iter::once((primary.mass_solar, primary.radius_solar))
        .chain(companions.iter().map(|c| (c.kind.mass(), c.kind.radius())));
    let mut system = Hierarchy::of(Vec::new(), bodies);
    system.arrange(draws, primary.origin.birth_mass, &companions);
    (
        if system.pairs.is_empty() {
            0.0
        } else {
            companions.iter().map(|c| c.luminosity).sum()
        },
        system.separation_au(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galaxy::Component;

    #[test]
    fn post_widening_hierarchies_have_a_finite_spatial_bound() {
        let mut hierarchy = Hierarchy::of(Vec::new(), [(1.0, 1.0), (0.5, 0.5)].into_iter());
        let companion = Companion {
            kind: Kind::Main { mass: 0.5 },
            luminosity: 0.1,
            separation_au: MAX_STELLAR_SEPARATION_AU * 2.0,
            eccentricity: 0.5,
        };
        let mut rng = Rng::new("warp-bound");
        hierarchy.arrange(&mut rng, 1.0, &[companion]);
        assert!(hierarchy.pairs.is_empty());
        assert_eq!(hierarchy.masses.len(), 1);
        for component in Component::ALL {
            for i in 0..1_000 {
                let bodies = crate::galactic_bodies(&format!("warp-bound:{i}"), component);
                assert!(widest_separation_au(&bodies.barycentres) <= MAX_STELLAR_SEPARATION_AU);
            }
        }
    }

    fn sample(count: usize) -> Vec<Bodies> {
        (0..count)
            .map(|i| crate::galactic_bodies(&format!("multiple:{i}"), Component::ThinDisc))
            .collect()
    }

    fn share(systems: &[&Bodies], test: impl Fn(&Bodies) -> bool) -> f64 {
        systems.iter().filter(|b| test(b)).count() as f64 / systems.len() as f64
    }

    #[test]
    fn multiplicity_follows_observed_shares() {
        let systems = sample(20_000);
        let of_kind = |kind: &str| -> Vec<&Bodies> {
            systems.iter().filter(|b| b.stars[0].kind == kind).collect()
        };
        for (kind, low, high) in [
            ("Red dwarf", 0.2, 0.32),
            ("Orange dwarf", 0.35, 0.5),
            ("Yellow dwarf", 0.38, 0.56),
        ] {
            let multiple = share(&of_kind(kind), |b| b.stars.len() > 1);
            assert!((low..high).contains(&multiple), "{kind}: {multiple}");
        }
        let multiples: Vec<_> = systems.iter().filter(|b| b.stars.len() > 1).collect();
        let higher = share(&multiples, |b| b.stars.len() > 2);
        assert!((0.15..0.35).contains(&higher), "triples and up: {higher}");
        assert!(multiples.iter().any(|b| b.stars.len() == 4));
        // About a quarter of white dwarfs have companions, some of them
        // brighter stars, as Sirius B has.
        let dwarfs: Vec<_> = systems
            .iter()
            .flat_map(|b| b.stars.iter().map(move |s| (s, b.stars.len() > 1)))
            .filter(|(s, _)| s.kind == "White dwarf")
            .collect();
        let paired = dwarfs.iter().filter(|(_, m)| *m).count() as f64 / dwarfs.len() as f64;
        assert!(
            (0.18..0.34).contains(&paired),
            "white dwarfs paired: {paired}"
        );
        assert!(systems.iter().any(|b| b.stars[0].kind != "White dwarf"
            && b.stars[1..].iter().any(|s| s.kind == "White dwarf")));
    }

    #[test]
    fn primaries_are_brightest_and_systems_nest() {
        for bodies in sample(5_000) {
            let primary = bodies.stars[0].luminosity_solar;
            let light: f64 = bodies.stars.iter().map(|s| s.luminosity_solar).sum();
            assert!(
                bodies.stars[1..]
                    .iter()
                    .all(|s| s.luminosity_solar < primary)
            );
            assert!(light < 4.0 * primary, "{bodies:?}");
            if bodies.stars.len() == 1 {
                assert!(bodies.barycentres.is_empty());
                assert!(bodies.stars[0].component.is_empty());
                continue;
            }
            assert_eq!(bodies.barycentres.len(), bodies.stars.len() - 1);
            let root = bodies.barycentres.last().unwrap();
            let letters: String = ('A'..='D').take(bodies.stars.len()).collect();
            assert_eq!(root.component, letters);
            for pair in &bodies.barycentres {
                let orbit = &pair.orbit;
                let [first, second] = pair.members.each_ref().map(|m| bodies.mass(m));
                assert!(first >= second && second > 0.0);
                assert!((0.0..0.8).contains(&orbit.eccentricity));
                let periapsis = orbit.semi_major_au * (1.0 - orbit.eccentricity);
                // Members are far enough apart for their own orbits to last.
                for member in &pair.members {
                    if let Some(inner) = bodies.barycentres.iter().find(|b| b.id == *member) {
                        let apoapsis = inner.orbit.semi_major_au * (1.0 + inner.orbit.eccentricity);
                        assert!(periapsis > 2.8 * apoapsis, "{pair:?} around {inner:?}");
                    } else {
                        let star = bodies.stars.iter().find(|s| s.id == *member).unwrap();
                        let radius =
                            star.radius_solar * crate::SUN_RADIUS_METRES / crate::AU_METRES;
                        assert!(periapsis > 3.0 * radius, "{pair:?}");
                    }
                }
                let days = (orbit.semi_major_au.powi(3) / (first + second)).sqrt() * 365.25;
                if days < CIRCULAR_DAYS {
                    assert!(orbit.eccentricity <= 0.05);
                }
            }
        }
    }

    #[test]
    fn planets_orbit_inside_stable_zones() {
        let (mut around_one, mut around_both) = (0, 0);
        for bodies in sample(5_000) {
            for hosted in &bodies.planets {
                let orbit = &hosted.body.orbit;
                let apoapsis = orbit.semi_major_au * (1.0 + orbit.eccentricity);
                if let Some(own) = bodies.barycentres.iter().find(|b| b.id == hosted.host) {
                    around_both += 1;
                    let [first, second] = own.members.each_ref().map(|m| bodies.mass(m));
                    let limit = p_type_limit(second / (first + second), own.orbit.eccentricity)
                        * own.orbit.semi_major_au;
                    assert!(orbit.semi_major_au >= limit * 0.999, "{hosted:?} {own:?}");
                } else if bodies.stars.len() > 1 {
                    around_one += 1;
                }
                // Inside the stable zone of every pair it sits in.
                let mut member = hosted.host.as_str();
                while let Some(pair) = bodies
                    .barycentres
                    .iter()
                    .find(|b| b.members.iter().any(|m| m == member))
                {
                    let k = usize::from(pair.members[1] == member);
                    let masses = pair.members.each_ref().map(|m| bodies.mass(m));
                    let limit = s_type_limit(
                        masses[1 - k] / (masses[0] + masses[1]),
                        pair.orbit.eccentricity,
                    ) * pair.orbit.semi_major_au;
                    assert!(apoapsis <= limit * 1.001, "{hosted:?} {pair:?}");
                    member = &pair.id;
                }
            }
        }
        assert!(
            around_one > 100 && around_both > 20,
            "{around_one} {around_both}"
        );
    }

    #[test]
    fn designations_follow_exoplanet_style() {
        assert_eq!(designation("TS 1-2-3", " ", "", 0), "TS 1-2-3 b");
        assert_eq!(
            designation("Flight Operations 2", "", "", 1),
            "Flight Operations 2c"
        );
        assert_eq!(designation("TS 1-2-3", " ", "B", 0), "TS 1-2-3 Bb");
        assert_eq!(designation("TS 1-2-3", " ", "AB", 2), "TS 1-2-3 (AB) d");
        assert_eq!(system_key("g1.2.3#AB/4"), "g1.2.3");
        assert_eq!(system_key("list:page-3"), "list:page-3");
        assert_eq!(member_letters("g1.2.3#B/0"), "B");
        assert_eq!(member_letters("g1.2.3/0"), "");
    }
}
