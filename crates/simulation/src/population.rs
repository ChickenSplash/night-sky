//! Galactic stars sorted into brightness tiers, so a streamed sky can load bright
//! stars from far away and faint ones only nearby.
//!
//! - Tier `t` holds stars no brighter than `tier_limit(t)`, four times dimmer per
//!   tier. Main-sequence stars and giants fall exactly in their band; white dwarfs
//!   are tiered by the brightest they could be at their cooling age, so a few
//!   are up to twice dimmer than their band.
//! - Each tier's stars are drawn directly, not by drawing stars until one fits:
//!   birth mass, age and shape are sampled from the same distributions as
//!   `galactic_star`, restricted to the tier. The mix over all tiers matches it.

use crate::cells::StarKey;
use crate::galaxy::{COMPONENTS, Component};
use crate::multiple::{self, Bodies};
use crate::star::{
    self, Branch, Formation, GIANT_SHARE, IMF_RANGE, Origin, SUN_TEMPERATURE_K, Stage,
    WHITE_DWARF_LIGHTEST, WHITE_DWARF_LIMIT, imf_cumulative, imf_density, imf_inverse,
    lifetime_gyr,
};
use crate::system::Rng;
use space_shared::{Barycentre, Star};
use std::sync::OnceLock;

pub const TIERS: usize = 20;
/// Tier 0 holds stars up to 4^12 (about 17 million) solar luminosities, above
/// the heaviest main-sequence stars.
const TOP: i32 = 12;
/// Mass bins per sampling table.
const BINS: usize = 48;
/// Simpson intervals per bin.
const STEPS: usize = 16;

/// Brightest a star of this tier can be, in solar luminosities.
pub fn tier_limit(tier: usize) -> f64 {
    4f64.powi(TOP - tier as i32)
}

/// Faintest apparent magnitude of the streamed sky, and of stars lists may use.
pub const VISIBLE_MAGNITUDE: f64 = 6.5;
const SUN_ABSOLUTE_MAGNITUDE: f64 = 4.83;

/// Brightness relative to magnitude 0 of one solar luminosity at one parsec.
/// A star's is `luminosity / distance_pc²` times this.
pub fn flux_per_luminosity() -> f64 {
    10f64.powf(-0.4 * (SUN_ABSOLUTE_MAGNITUDE - 5.0))
}

/// Squared parsecs per solar luminosity at which a star fades to `magnitude`.
/// A star is visible when `distance_pc² <= luminosity * this`: no logarithm
/// per star.
pub fn reach_sq_per_luminosity(magnitude: f64) -> f64 {
    flux_per_luminosity() * 10f64.powf(0.4 * magnitude)
}

/// Distance at which the brightest star of a tier fades to this apparent
/// magnitude. Each tier reaches half as far as the one above it.
pub fn reach_pc(tier: usize, magnitude: f64) -> f64 {
    (tier_limit(tier) * reach_sq_per_luminosity(magnitude)).sqrt()
}

/// Tier of a star this bright.
pub fn tier_of(luminosity: f64) -> usize {
    let tier = (f64::from(TOP) - luminosity.log(4.0)).floor();
    tier.clamp(0.0, (TIERS - 1) as f64) as usize
}

/// Share of a component's stars in each tier.
pub fn tier_shares(component: Component) -> [f64; TIERS] {
    let tiers = &census(component).tiers;
    std::array::from_fn(|t| tiers[t].weights.iter().sum())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Main,
    Giant,
    Supergiant,
    WhiteDwarf,
}

const PHASES: [Phase; 4] = [
    Phase::Main,
    Phase::Giant,
    Phase::Supergiant,
    Phase::WhiteDwarf,
];

/// Birth masses in a range, weighted by the chance of each being in a phase.
struct Masses {
    edges: Vec<f64>,
    /// `imf_cumulative` at each edge.
    born: Vec<f64>,
    cumulative: Vec<f64>,
    /// Upper bound of the weight in each bin, for rejection.
    bounds: Vec<f64>,
}

impl Masses {
    fn new(low: f64, high: f64, weight: impl Fn(f64) -> f64) -> Self {
        let edges: Vec<f64> = (0..=BINS)
            .map(|i| (low.ln() + (high / low).ln() * i as f64 / BINS as f64).exp())
            .collect();
        let mut cumulative = vec![0.0];
        let mut bounds = Vec::with_capacity(BINS);
        for bin in edges.windows(2) {
            let (a, b) = (bin[0].ln(), bin[1].ln());
            let step = (b - a) / STEPS as f64;
            let (mut sum, mut bound) = (0.0, 0.0_f64);
            for i in 0..=STEPS {
                let m = (a + i as f64 * step).exp();
                let w = weight(m);
                bound = bound.max(w);
                let simpson = match i {
                    0 => 1.0,
                    i if i == STEPS => 1.0,
                    i if i % 2 == 1 => 4.0,
                    _ => 2.0,
                };
                sum += simpson * w * imf_density(m) * m;
            }
            cumulative.push(cumulative.last().unwrap() + sum * step / 3.0);
            bounds.push((bound * 1.05).min(1.0));
        }
        Self {
            born: edges.iter().map(|&m| imf_cumulative(m)).collect(),
            edges,
            cumulative,
            bounds,
        }
    }

    /// Stars in the range and phase, in the units of `imf_cumulative`.
    fn total(&self) -> f64 {
        *self.cumulative.last().unwrap()
    }

    fn sample(&self, rng: &mut Rng, weight: impl Fn(f64) -> f64) -> f64 {
        let target = rng.unit() * self.total();
        let bin = self
            .cumulative
            .partition_point(|&c| c <= target)
            .clamp(1, BINS)
            - 1;
        let (low, high) = (self.born[bin], self.born[bin + 1]);
        let mut mass = self.edges[bin];
        for _ in 0..256 {
            mass = imf_inverse(low + rng.unit() * (high - low));
            if rng.unit() * self.bounds[bin] <= weight(mass) {
                break;
            }
        }
        mass
    }
}

/// Main-sequence mass with this luminosity.
fn mass_for_luminosity(luminosity: f64) -> f64 {
    let (mut low, mut high) = IMF_RANGE;
    for _ in 0..80 {
        let mid = (low * high).sqrt();
        if star::luminosity(mid) < luminosity {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low * high).sqrt()
}

/// Brightest a white dwarf can be after cooling this long.
fn white_dwarf_limit(cooling_gyr: f64) -> f64 {
    let radius = star::white_dwarf_radius(WHITE_DWARF_LIGHTEST);
    radius.powi(2) * (star::cooled_kelvin(cooling_gyr) / SUN_TEMPERATURE_K).powi(4)
}

/// Cooling age at which a white dwarf can be no brighter than this.
fn cooling_for(luminosity: f64) -> f64 {
    if white_dwarf_limit(0.0) <= luminosity {
        return 0.0;
    }
    // Beyond the coolest white dwarfs, no cooling age reaches it.
    if white_dwarf_limit(1e3) > luminosity {
        return f64::INFINITY;
    }
    let (mut low, mut high) = (0.0, 1e3);
    for _ in 0..80 {
        let mid = 0.5 * (low + high);
        if white_dwarf_limit(mid) > luminosity {
            low = mid;
        } else {
            high = mid;
        }
    }
    high
}

/// Share of an evolved branch's shapes with luminosity in (low, high], and a
/// shape drawn from them.
fn shape_share(branch: Branch, low: f64, high: f64) -> f64 {
    let envelope = branch.envelope();
    let (k0, k1) = envelope.kelvin;
    let steps = 2_000;
    let span = (envelope.radius.1 / envelope.radius.0).ln();
    (0..steps)
        .map(|i| {
            let t = (i as f64 + 0.5) / steps as f64;
            let kelvin = if envelope.log_kelvin {
                k0 * (k1 / k0).powf(t)
            } else {
                k0 + (k1 - k0) * t
            };
            radii(branch, kelvin, low, high).map_or(0.0, |(a, b)| (b / a).ln())
        })
        .sum::<f64>()
        / steps as f64
        / span
}

/// Radii of the branch at this temperature with luminosity in (low, high].
fn radii(branch: Branch, kelvin: f64, low: f64, high: f64) -> Option<(f64, f64)> {
    let (r0, r1) = branch.envelope().radius;
    let flux = (kelvin / SUN_TEMPERATURE_K).powi(4);
    let a = (low / flux).sqrt().max(r0);
    let b = (high / flux).sqrt().min(r1);
    (b > a).then_some((a, b))
}

fn draw_shape(rng: &mut Rng, branch: Branch, low: f64, high: f64) -> (f64, f64) {
    let envelope = branch.envelope();
    // The widest radius range for one temperature is the band's.
    let widest = 0.5 * (high / low).ln();
    let mut shape = (envelope.radius.0, envelope.kelvin.0);
    for _ in 0..10_000 {
        let kelvin = envelope.kelvin(rng);
        let Some((a, b)) = radii(branch, kelvin, low, high) else {
            continue;
        };
        shape = (rng.log_range(a, b), kelvin);
        if rng.unit() * widest <= (b / a).ln() {
            break;
        }
    }
    shape
}

struct Tier {
    /// Share of the component's stars in this tier, per phase.
    weights: [f64; 4],
    main: Option<Masses>,
    main_range: (f64, f64),
    dwarfs: Option<Masses>,
    cooling: (f64, f64),
    /// Share of giants and of red and blue supergiants shaped into this tier.
    shapes: [f64; 3],
}

struct Census {
    tiers: [Tier; TIERS],
    giants: Masses,
    supergiants: Masses,
}

fn life(mass: f64) -> f64 {
    lifetime_gyr(mass)
}

fn main_weight(c: Component, m: f64) -> f64 {
    c.age_cdf(life(m))
}

fn evolved_weight(c: Component, m: f64) -> f64 {
    c.age_cdf(life(m) * (1.0 + GIANT_SHARE)) - c.age_cdf(life(m))
}

fn dwarf_weight(c: Component, m: f64, cooling: (f64, f64)) -> f64 {
    if m >= WHITE_DWARF_LIMIT {
        return 0.0;
    }
    let end = life(m) * (1.0 + GIANT_SHARE);
    c.age_cdf(end + cooling.1) - c.age_cdf(end + cooling.0)
}

fn census(component: Component) -> &'static Census {
    static CENSUS: [OnceLock<Census>; COMPONENTS] = [const { OnceLock::new() }; COMPONENTS];
    CENSUS[component.index()].get_or_init(|| build(component))
}

fn build(c: Component) -> Census {
    let (low, high) = IMF_RANGE;
    let giants = Masses::new(low, WHITE_DWARF_LIMIT, |m| evolved_weight(c, m));
    let supergiants = Masses::new(WHITE_DWARF_LIMIT, high, |m| evolved_weight(c, m));
    let born = imf_cumulative(high);
    let mut tiers: [Tier; TIERS] = std::array::from_fn(|t| {
        let (faint, bright) = if t + 1 == TIERS {
            (0.0, tier_limit(t))
        } else {
            (tier_limit(t + 1), tier_limit(t))
        };
        let main_range = (
            mass_for_luminosity(faint).max(low),
            mass_for_luminosity(bright).min(high),
        );
        let main = (main_range.1 > main_range.0 * (1.0 + 1e-9))
            .then(|| Masses::new(main_range.0, main_range.1, |m| main_weight(c, m)));
        let cooling = (
            cooling_for(bright),
            if t + 1 == TIERS {
                f64::INFINITY
            } else {
                cooling_for(faint)
            },
        );
        let dwarfs = (cooling.1 > cooling.0).then(|| {
            let window = (cooling.0, cooling.1.min(1e3));
            Masses::new(low, WHITE_DWARF_LIMIT, |m| dwarf_weight(c, m, window))
        });
        let shapes = [Branch::Giant, Branch::RedSupergiant, Branch::BlueSupergiant]
            .map(|b| shape_share(b, faint, bright));
        let red = Branch::RedSupergiant.share(high);
        let weights = [
            main.as_ref().map_or(0.0, Masses::total),
            giants.total() * shapes[0],
            supergiants.total() * (red * shapes[1] + (1.0 - red) * shapes[2]),
            dwarfs.as_ref().map_or(0.0, Masses::total),
        ]
        .map(|w| w / born);
        Tier {
            weights,
            main,
            main_range,
            dwarfs,
            cooling: (cooling.0, cooling.1.min(1e3)),
            shapes,
        }
    });
    // Neutron stars and black holes are not stars: share among the rest.
    let stars: f64 = tiers.iter().flat_map(|t| t.weights).sum();
    for tier in &mut tiers {
        tier.weights = tier.weights.map(|w| w / stars);
    }
    Census {
        tiers,
        giants,
        supergiants,
    }
}

/// A star of this component and tier, from its own ID.
pub(crate) fn tiered_formation(id: &str, component: Component, tier: usize) -> Formation {
    tiered(id, component, tier).0
}

/// A tiered star, its origin, and its two draw sequences so far: `{id}:tier`
/// for what sets its light, and `{id}:star` for the rest.
pub(crate) fn tiered(id: &str, component: Component, tier: usize) -> (Formation, Origin, Rng, Rng) {
    let mut draw = Rng::new(&format!("{id}:tier"));
    let mut rng = Rng::new(&format!("{id}:star"));
    let shape = tiered_shape(&mut draw, component, tier);
    let formed = match shape {
        Shape::Evolved {
            mass,
            branch,
            radius,
            kelvin,
        } => star::evolved(id, &mut rng, mass, branch, radius, kelvin),
        Shape::WhiteDwarf { progenitor, kelvin } => {
            star::white_dwarf(id, &mut rng, progenitor, Some(kelvin))
        }
        Shape::Main { kelvin, mass } => star::main_sequence(id, &mut rng, kelvin, mass),
    };
    (formed, shape.origin(component), draw, rng)
}

/// A tiered star's whole system.
pub(crate) fn tiered_bodies(id: &str, component: Component, tier: usize) -> Bodies {
    let (formed, origin, mut draw, mut rng) = tiered(id, component, tier);
    multiple::assemble(id, formed, origin, &mut draw, Some(&mut rng))
}

/// The stars of a tiered system, primary first, and its pairs, without its
/// planets.
pub fn tiered_stars(id: &str, component: Component, tier: usize) -> (Vec<Star>, Vec<Barycentre>) {
    let (formed, origin, mut draw, mut rng) = tiered(id, component, tier);
    multiple::stars_only(id, formed, origin, &mut draw, Some(&mut rng))
}

/// What a streamed point needs of the system `tiered_bodies` makes for a key.
pub(crate) struct TieredLight {
    /// All its stars together, solar.
    pub luminosity: f64,
    /// The primary's temperature (K) and radius (solar).
    pub kelvin: f64,
    pub radius: f64,
    /// Widest any two of its stars get, in AU; zero for a single star.
    pub separation_au: f64,
}

/// A tiered system's light and size, without building the rest of it.
pub(crate) fn tiered_light(key: StarKey, component: Component, tier: usize) -> TieredLight {
    let mut draw = key.rng(":tier");
    let shape = tiered_shape(&mut draw, component, tier);
    let (mass, light, kelvin, radius) = match shape {
        Shape::Evolved {
            mass,
            radius,
            kelvin,
            ..
        } => (mass, star::surface_light(radius, kelvin), kelvin, radius),
        Shape::WhiteDwarf { progenitor, kelvin } => {
            let mass = star::white_dwarf_mass(&mut key.rng(":star"), progenitor);
            let radius = star::white_dwarf_radius(mass);
            (mass, star::surface_light(radius, kelvin), kelvin, radius)
        }
        Shape::Main { kelvin, mass } => (
            mass,
            star::luminosity(mass),
            kelvin,
            star::main_sequence_radius(mass),
        ),
    };
    let primary = multiple::Primary {
        origin: shape.origin(component),
        mass_solar: mass,
        luminosity: light,
        radius_solar: radius,
        kelvin,
    };
    let (companions, separation_au) = multiple::companion_light(&mut draw, &primary);
    TieredLight {
        luminosity: light + companions,
        kelvin,
        radius,
        separation_au,
    }
}

/// What `tiered_formation` draws from `{id}:tier`: the kind of star and its
/// shape. The rest comes from `{id}:star`.
#[derive(Clone, Copy)]
enum Shape {
    Evolved {
        mass: f64,
        branch: Branch,
        radius: f64,
        kelvin: f64,
    },
    WhiteDwarf {
        progenitor: f64,
        kelvin: f64,
    },
    Main {
        kelvin: f64,
        mass: f64,
    },
}

impl Shape {
    fn origin(self, component: Component) -> Origin {
        let (stage, birth_mass) = match self {
            Shape::Evolved { mass, .. } => (Stage::Giant, mass),
            Shape::WhiteDwarf { progenitor, .. } => (Stage::WhiteDwarf, progenitor),
            Shape::Main { mass, .. } => (Stage::Main, mass),
        };
        Origin {
            stage,
            birth_mass,
            age_gyr: None,
            component,
        }
    }
}

fn tiered_shape(draw: &mut Rng, component: Component, tier: usize) -> Shape {
    let census = census(component);
    let tier = tier.min(TIERS - 1);
    let row = &census.tiers[tier];
    let total: f64 = row.weights.iter().sum();
    let mut roll = draw.unit() * total;
    let phase = PHASES
        .into_iter()
        .zip(row.weights)
        .find(|&(_, w)| {
            roll -= w;
            roll < 0.0 && w > 0.0
        })
        .map_or(Phase::Main, |(p, _)| p);
    let (faint, bright) = if tier + 1 == TIERS {
        (0.0, tier_limit(tier))
    } else {
        (tier_limit(tier + 1), tier_limit(tier))
    };
    match (phase, &row.main, &row.dwarfs) {
        (Phase::Giant, ..) => {
            let mass = census.giants.sample(draw, |m| evolved_weight(component, m));
            let (radius, kelvin) = draw_shape(draw, Branch::Giant, faint, bright);
            Shape::Evolved {
                mass,
                branch: Branch::Giant,
                radius,
                kelvin,
            }
        }
        (Phase::Supergiant, ..) => {
            let mass = census
                .supergiants
                .sample(draw, |m| evolved_weight(component, m));
            let red = Branch::RedSupergiant.share(mass) * row.shapes[1];
            let blue = Branch::BlueSupergiant.share(mass) * row.shapes[2];
            let branch = if draw.unit() * (red + blue) < red {
                Branch::RedSupergiant
            } else {
                Branch::BlueSupergiant
            };
            let (radius, kelvin) = draw_shape(draw, branch, faint, bright);
            Shape::Evolved {
                mass,
                branch,
                radius,
                kelvin,
            }
        }
        (Phase::WhiteDwarf, _, Some(dwarfs)) => {
            let window = row.cooling;
            let progenitor = dwarfs.sample(draw, |m| dwarf_weight(component, m, window));
            let end = life(progenitor) * (1.0 + GIANT_SHARE);
            let age = component.age_between(draw, end + window.0, end + window.1);
            Shape::WhiteDwarf {
                progenitor,
                kelvin: star::cooled_kelvin((age - end).max(window.0)),
            }
        }
        (_, Some(main), _) => {
            let mass = main
                .sample(draw, |m| main_weight(component, m))
                .clamp(row.main_range.0, row.main_range.1);
            Shape::Main {
                kelvin: star::temperature(star::luminosity(mass), star::main_sequence_radius(mass)),
                mass,
            }
        }
        // Only reached for a tier without stars of this component.
        _ => Shape::Main {
            kelvin: 3_200.0,
            mass: 0.2,
        },
    }
}

/// A star of this component and tier.
pub fn tiered_star(id: &str, component: Component, tier: usize) -> Star {
    tiered_formation(id, component, tier).star
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galactic_star;

    #[test]
    fn tiers_are_bands_four_times_apart() {
        assert_eq!(tier_of(tier_limit(3)), 3);
        assert_eq!(tier_of(tier_limit(3) * 0.99), 3);
        assert_eq!(tier_of(tier_limit(3) * 1.01), 2);
        assert_eq!(tier_of(1.0), 12);
        assert_eq!(tier_of(1e-9), TIERS - 1);
        for component in Component::ALL {
            let shares = tier_shares(component);
            assert!((shares.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        }
        // Old populations have no bright tiers left; young arms do.
        let bright = |c| tier_shares(c)[..5].iter().sum::<f64>();
        assert_eq!(bright(Component::Halo), 0.0);
        assert!(bright(Component::Arms) > 1e-6);
    }

    #[test]
    fn reach_matches_the_distance_modulus() {
        // The Sun is at its absolute magnitude from ten parsecs.
        assert!((reach_sq_per_luminosity(SUN_ABSOLUTE_MAGNITUDE) - 100.0).abs() < 1e-9);
        for (luminosity, distance_pc) in [(1.0, 5.0), (40.0, 90.0), (1e-3, 0.7)] {
            let magnitude: f64 = SUN_ABSOLUTE_MAGNITUDE - 2.5 * f64::log10(luminosity)
                + 5.0 * f64::log10(distance_pc)
                - 5.0;
            let squared = distance_pc * distance_pc;
            assert!((luminosity * reach_sq_per_luminosity(magnitude) / squared - 1.0).abs() < 1e-9);
            let flux = flux_per_luminosity() * luminosity / squared;
            assert!((flux / 10f64.powf(-0.4 * magnitude) - 1.0).abs() < 1e-9);
        }
        assert!((reach_pc(tier_of(1.0), SUN_ABSOLUTE_MAGNITUDE) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn tiered_stars_stay_in_their_band() {
        for component in Component::ALL {
            let shares = tier_shares(component);
            for tier in (0..TIERS).filter(|&t| shares[t] > 0.0) {
                for i in 0..200 {
                    let star = tiered_star(&format!("band:{i}"), component, tier);
                    let light = star.luminosity_solar;
                    assert!(
                        light <= tier_limit(tier) * 1.001,
                        "{component:?} tier {tier}: {star:?}"
                    );
                    // White dwarfs may be up to twice dimmer than their band.
                    let floor = if tier + 1 == TIERS {
                        0.0
                    } else if star.kind == "White dwarf" {
                        tier_limit(tier + 1) / 2.0
                    } else {
                        tier_limit(tier + 1) * 0.999
                    };
                    assert!(light > floor, "{component:?} tier {tier}: {star:?}");
                }
            }
        }
    }

    #[test]
    fn companions_add_at_most_a_tier_of_light() {
        let mut brighter = 0;
        for component in [Component::ThinDisc, Component::Halo, Component::Arms] {
            let shares = tier_shares(component);
            for tier in (0..TIERS).filter(|&t| shares[t] > 0.0) {
                for i in 0..200 {
                    let id = format!("band:{i}");
                    let bodies = tiered_bodies(&id, component, tier);
                    let light: f64 = bodies.stars.iter().map(|s| s.luminosity_solar).sum();
                    assert!(light < 4.0 * bodies.stars[0].luminosity_solar);
                    brighter += usize::from(light > tier_limit(tier));
                }
            }
        }
        assert!(brighter > 0);
    }

    type Test<'a> = (&'a str, &'a dyn Fn(&Star) -> bool);

    /// Shares of each kind among stars drawn tier by tier.
    fn tiered_census(component: Component, count: usize) -> Vec<Star> {
        let shares = tier_shares(component);
        let mut rng = Rng::new("census");
        (0..count)
            .map(|i| {
                let mut roll = rng.unit();
                let tier = (0..TIERS)
                    .find(|&t| {
                        roll -= shares[t];
                        roll < 0.0
                    })
                    .unwrap_or(TIERS - 1);
                tiered_star(&format!("mix:{i}"), component, tier)
            })
            .collect()
    }

    #[test]
    fn tiers_together_match_the_galactic_mix() {
        for component in [Component::ThinDisc, Component::Halo, Component::Arms] {
            let tiered = tiered_census(component, 40_000);
            let direct: Vec<Star> = (0..40_000)
                .map(|i| galactic_star(&format!("mix:{i}"), component))
                .collect();
            let share = |stars: &[Star], test: &dyn Fn(&Star) -> bool| {
                stars.iter().filter(|s| test(s)).count() as f64 / stars.len() as f64
            };
            let tests: [Test; 5] = [
                ("red dwarfs", &|s| s.kind == "Red dwarf"),
                ("orange dwarfs", &|s| s.kind == "Orange dwarf"),
                ("white dwarfs", &|s| s.kind == "White dwarf"),
                ("giants", &|s| s.kind.contains("giant")),
                ("sun-like", &|s| s.luminosity_solar > 0.5),
            ];
            for (name, test) in tests {
                let (a, b) = (share(&tiered, test), share(&direct, test));
                // Within about four standard errors.
                let error = 4.0 * (b.max(1e-4) / 40_000.0).sqrt() + 1e-4;
                assert!((a - b).abs() < error, "{component:?} {name}: {a} vs {b}");
            }
            let light = |stars: &[Star]| {
                stars.iter().map(|s| s.luminosity_solar.ln()).sum::<f64>() / stars.len() as f64
            };
            assert!(
                (light(&tiered) - light(&direct)).abs() < 0.05,
                "{component:?}"
            );
        }
    }
}
