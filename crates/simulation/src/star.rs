//! Stars, drawn two ways:
//!
//! - Page stars (`star`) follow the census of the solar neighbourhood (RECONS,
//!   stars within 10 pc): about three in four are red dwarfs.
//! - Galactic stars (`galactic_star`) draw a birth mass from the Kroupa (2001)
//!   initial mass function and an age from their galactic component, then
//!   evolve: main sequence, giant, then white dwarf. Old components have no hot
//!   stars left and many white dwarfs; young spiral arms have O and B stars.
//!
//! - Main-sequence stars pick a temperature within their class, then the mass that
//!   gives it, so the spectral type, mass, radius and luminosity always agree.
//! - Giants and white dwarfs are evolved stars. Their planets formed around the
//!   younger main-sequence star, and inner ones were swallowed as it swelled.
//! - Colours follow temperature along the spectral sequence.
//! - Rotation follows each class's observed range, including the fast rotation of
//!   stars hotter than the Kraft break (about 6,250 K).

pub mod multiple;

use crate::galaxy::Component;
use crate::seed::Rng;
use space_shared::{Spin, Star};
use std::f64::consts::{PI, TAU};

pub(crate) const SUN_TEMPERATURE_K: f64 = 5772.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    O,
    B,
    A,
    F,
    G,
    K,
    M,
    Giant,
    WhiteDwarf,
}

/// Share of stars, in per cent, and main-sequence temperature range in kelvin.
/// O stars (about 0.00003%) are too rare for the census, but galactic stars
/// reach them.
const CENSUS: [(Kind, f64, f64, f64); 9] = [
    (Kind::M, 72.0, 2_600.0, 3_700.0),
    (Kind::K, 12.0, 3_700.0, 5_200.0),
    (Kind::G, 7.0, 5_200.0, 6_000.0),
    (Kind::WhiteDwarf, 5.0, 0.0, 0.0),
    (Kind::F, 3.0, 6_000.0, 7_500.0),
    (Kind::A, 0.6, 7_500.0, 10_000.0),
    (Kind::Giant, 0.3, 0.0, 0.0),
    (Kind::B, 0.1, 10_000.0, 30_000.0),
    (Kind::O, 0.0, 30_000.0, 60_000.0),
];

/// Kroupa (2001) initial mass function: dN/dM ∝ M^-1.3 from the hydrogen-burning
/// limit to 0.5 M☉, and M^-2.3 above. Brown dwarfs are not counted as stars.
const IMF_LOW: f64 = 0.08;
const IMF_BREAK: f64 = 0.5;
const IMF_HIGH: f64 = 100.0;
/// Time after the main sequence spent as a subgiant, giant and on the horizontal
/// and asymptotic branches, as a share of the main-sequence lifetime.
pub(crate) const GIANT_SHARE: f64 = 0.1;
/// Heaviest birth mass that ends as a white dwarf; heavier stars leave neutron
/// stars or black holes, which are not counted as stars.
pub(crate) const WHITE_DWARF_LIMIT: f64 = 8.0;
/// Lightest white dwarf a star can have left in the age of the galaxy.
pub(crate) const WHITE_DWARF_LIGHTEST: f64 = 0.46;
/// Share of supergiants that are red.
const RED_SUPERGIANT_SHARE: f64 = 0.7;

/// How the star shaped its planets, in addition to the star itself.
#[derive(Clone)]
pub(crate) struct Formation {
    pub star: Star,
    /// Distance where water froze in the disc, as the orbits are now.
    pub snow_au: f64,
    /// Innermost orbit that survived, or where the disc began.
    pub inner_au: f64,
}

/// Main-sequence luminosity in solar units, continuous across the branches.
pub fn luminosity(mass_solar: f64) -> f64 {
    if mass_solar < 0.43 {
        // 0.23 · M^2.3, rescaled to meet the next branch at 0.43.
        0.43_f64.powf(1.7) * mass_solar.powf(2.3)
    } else if mass_solar < 2.0 {
        mass_solar.powi(4)
    } else {
        std::f64::consts::SQRT_2 * mass_solar.powf(3.5)
    }
}

pub(crate) fn main_sequence_radius(mass_solar: f64) -> f64 {
    mass_solar.powf(0.8)
}

pub(crate) fn temperature(luminosity: f64, radius: f64) -> f64 {
    SUN_TEMPERATURE_K * (luminosity / radius.powi(2)).powf(0.25)
}

/// Main-sequence mass with this surface temperature. Temperature rises with
/// mass on every branch, so bisection converges.
fn mass_for_temperature(kelvin: f64) -> f64 {
    let (mut low, mut high) = (0.075_f64, 30.0_f64);
    for _ in 0..60 {
        let mid = (low * high).sqrt();
        if temperature(luminosity(mid), main_sequence_radius(mid)) < kelvin {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low * high).sqrt()
}

/// Display colour by temperature, from deep red M dwarfs through orange, yellow
/// and white to blue B stars. True blackbody colours are nearly all pale orange
/// below 5,000 K, which would make most stars look alike, so the red-to-yellow
/// range is stretched while keeping the order of the spectral sequence.
const SPECTRAL_SCALE: [(f64, [f64; 3]); 11] = [
    (2_400.0, [255.0, 60.0, 45.0]),
    (3_200.0, [255.0, 95.0, 55.0]),
    (3_700.0, [255.0, 130.0, 60.0]),
    (4_500.0, [255.0, 165.0, 70.0]),
    (5_200.0, [255.0, 200.0, 90.0]),
    (5_800.0, [255.0, 232.0, 140.0]),
    (6_500.0, [255.0, 246.0, 215.0]),
    (7_500.0, [238.0, 240.0, 255.0]),
    (10_000.0, [190.0, 210.0, 255.0]),
    (20_000.0, [145.0, 175.0, 255.0]),
    (30_000.0, [120.0, 150.0, 255.0]),
];

pub fn spectral_colour(kelvin: f64) -> [u8; 3] {
    let t = kelvin.clamp(SPECTRAL_SCALE[0].0, SPECTRAL_SCALE[10].0);
    let i = SPECTRAL_SCALE
        .windows(2)
        .position(|w| t <= w[1].0)
        .unwrap_or(9);
    let ((t0, c0), (t1, c1)) = (SPECTRAL_SCALE[i], SPECTRAL_SCALE[i + 1]);
    let f = (t / t0).ln() / (t1 / t0).ln();
    std::array::from_fn(|k| (c0[k] + (c1[k] - c0[k]) * f).round() as u8)
}

/// Spectral letter for a temperature.
fn letter(kelvin: f64) -> (char, f64, f64) {
    CENSUS
        .iter()
        .filter(|c| c.3 > 0.0 && kelvin >= c.2)
        .map(|c| {
            let name = match c.0 {
                Kind::O => 'O',
                Kind::B => 'B',
                Kind::A => 'A',
                Kind::F => 'F',
                Kind::G => 'G',
                Kind::K => 'K',
                _ => 'M',
            };
            (name, c.2, c.3)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or(('M', 2_600.0, 3_700.0))
}

/// Type such as "K4", with subclass 0 at the hot end of the class.
fn subclass(kelvin: f64) -> String {
    let (name, cool, hot) = letter(kelvin);
    let digit = ((hot - kelvin) / (hot - cool) * 10.0).clamp(0.0, 9.0) as u32;
    format!("{name}{digit}")
}

fn isotropic_tilt(rng: &mut Rng) -> f64 {
    (1.0 - 2.0 * rng.unit()).acos()
}

/// Hot stars in multi-planet systems are mostly aligned. Strong misalignment is
/// common only where a migrating giant has disturbed the system, as for many
/// hot-Jupiter hosts.
fn hot_tilt(rng: &mut Rng) -> f64 {
    if rng.chance(0.1) {
        isotropic_tilt(rng)
    } else {
        rng.rayleigh(0.2)
    }
}

/// Where a star is in its life, for drawing companions of the same age.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Main,
    Giant,
    WhiteDwarf,
}

/// What a star's companions need to know about it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Origin {
    pub stage: Stage,
    /// Mass at birth: the progenitor's, for a white dwarf.
    pub birth_mass: f64,
    /// Age in billions of years, when the draw already knows it.
    pub age_gyr: Option<f64>,
    pub component: Component,
}

/// A page star, in the proportions of the solar neighbourhood.
pub(crate) fn formation(id: &str) -> Formation {
    census(id).0
}

/// A page star, its origin, and its draws so far, to continue with its
/// companions.
pub(crate) fn census(id: &str) -> (Formation, Origin, Rng) {
    let mut rng = Rng::new(&format!("{id}:star"));
    let mut roll = rng.unit() * 100.0;
    let &(kind, _, cool, hot) = CENSUS
        .iter()
        .find(|c| {
            roll -= c.1;
            roll < 0.0
        })
        .unwrap_or(&CENSUS[0]);
    let origin = |stage, birth_mass| Origin {
        stage,
        birth_mass,
        age_gyr: None,
        component: Component::ThinDisc,
    };
    let (formed, origin) = match kind {
        Kind::Giant => {
            let mass = rng.log_range(0.9, 3.0);
            (giant(id, &mut rng, mass), origin(Stage::Giant, mass))
        }
        Kind::WhiteDwarf => {
            let progenitor = rng.log_range(1.0, 4.0);
            (
                white_dwarf(id, &mut rng, progenitor, None),
                origin(Stage::WhiteDwarf, progenitor),
            )
        }
        _ => {
            // Cool stars are far more common within each class too; B stars skew
            // towards their cooler, lighter end.
            let kelvin = if kind == Kind::B {
                cool * (hot / cool).powf(rng.unit().powi(2))
            } else {
                rng.range(cool, hot)
            };
            let mass = mass_for_temperature(kelvin);
            (
                main_sequence(id, &mut rng, kelvin, mass),
                origin(Stage::Main, mass),
            )
        }
    };
    (formed, origin, rng)
}

/// Birth mass for a uniform draw, by inverting the initial mass function.
fn initial_mass(u: f64) -> f64 {
    imf_inverse(u * imf_cumulative(IMF_HIGH))
}

/// Initial mass function per solar mass, unnormalised. Continuous at the break:
/// M^-2.3 carries a factor of 0.5 there.
pub(crate) fn imf_density(mass: f64) -> f64 {
    if mass < IMF_BREAK {
        mass.powf(-1.3)
    } else {
        IMF_BREAK * mass.powf(-2.3)
    }
}

/// Powers of the IMF's limits, worked out once: every star's mass draw uses
/// them.
struct ImfPowers {
    low: f64,
    break_high: f64,
    /// Stars born below the break.
    below_break: f64,
}

fn imf_powers() -> &'static ImfPowers {
    static POWERS: std::sync::LazyLock<ImfPowers> = std::sync::LazyLock::new(|| ImfPowers {
        low: IMF_LOW.powf(-0.3),
        break_high: IMF_BREAK.powf(-1.3),
        below_break: (IMF_LOW.powf(-0.3) - IMF_BREAK.powf(-0.3)) / 0.3,
    });
    &POWERS
}

/// Stars born below this mass, in the units of `imf_density`.
pub(crate) fn imf_cumulative(mass: f64) -> f64 {
    let mass = mass.clamp(IMF_LOW, IMF_HIGH);
    let powers = imf_powers();
    if mass < IMF_BREAK {
        (powers.low - mass.powf(-0.3)) / 0.3
    } else {
        powers.below_break + IMF_BREAK * (powers.break_high - mass.powf(-1.3)) / 1.3
    }
}

pub(crate) fn imf_inverse(x: f64) -> f64 {
    let powers = imf_powers();
    if x < powers.below_break {
        (powers.low - 0.3 * x).powf(-1.0 / 0.3)
    } else {
        (powers.break_high - 1.3 * (x - powers.below_break) / IMF_BREAK).powf(-1.0 / 1.3)
    }
}

/// Birth masses of stars, lightest to heaviest.
pub(crate) const IMF_RANGE: (f64, f64) = (IMF_LOW, IMF_HIGH);

/// Main-sequence lifetime in billions of years: the Sun's ten, shorter for
/// heavier stars, and never under the few million years of the heaviest.
pub fn lifetime_gyr(mass_solar: f64) -> f64 {
    (10.0 * mass_solar.powf(-2.5)).max(0.003)
}

/// White dwarf temperature after cooling this long, after Mestel's law: about
/// 20,000 K at 100 million years, 9,000 K at a billion and 4,000 K at ten.
pub(crate) fn cooled_kelvin(cooling_gyr: f64) -> f64 {
    (9_000.0 * cooling_gyr.max(1e-6).powf(-0.35)).clamp(3_500.0, 100_000.0)
}

/// Cooling time that leaves a white dwarf at this temperature: the inverse of
/// `cooled_kelvin`.
pub(crate) fn cooling_gyr(kelvin: f64) -> f64 {
    (kelvin.clamp(3_500.0, 100_000.0) / 9_000.0).powf(-1.0 / 0.35)
}

/// A star of a galactic component: a birth mass and age, evolved to today.
pub(crate) fn galactic_formation(id: &str, component: Component) -> Formation {
    galactic(id, component).0
}

/// A galactic star, its origin, and its draws so far.
pub(crate) fn galactic(id: &str, component: Component) -> (Formation, Origin, Rng) {
    let mut rng = Rng::new(&format!("{id}:star"));
    let origin = |stage, birth_mass, age| Origin {
        stage,
        birth_mass,
        age_gyr: Some(age),
        component,
    };
    // Neutron stars and black holes are drawn again, so the count is of stars.
    for _ in 0..32 {
        let mass = initial_mass(rng.unit());
        let age = component.age_gyr(&mut rng);
        let life = lifetime_gyr(mass);
        if age < life {
            let kelvin = temperature(luminosity(mass), main_sequence_radius(mass));
            let formed = main_sequence(id, &mut rng, kelvin, mass);
            return (formed, origin(Stage::Main, mass, age), rng);
        }
        if age < life * (1.0 + GIANT_SHARE) {
            let formed = giant(id, &mut rng, mass);
            return (formed, origin(Stage::Giant, mass, age), rng);
        }
        if mass < WHITE_DWARF_LIMIT {
            let kelvin = cooled_kelvin(age - life * (1.0 + GIANT_SHARE));
            let formed = white_dwarf(id, &mut rng, mass, Some(kelvin));
            return (formed, origin(Stage::WhiteDwarf, mass, age), rng);
        }
    }
    let mass = mass_for_temperature(3_200.0);
    let formed = main_sequence(id, &mut rng, 3_200.0, mass);
    let age = component.age_between(&mut rng, 0.0, lifetime_gyr(mass));
    (formed, origin(Stage::Main, mass, age), rng)
}

fn hours(rng: &mut Rng, low_days: f64, high_days: f64) -> f64 {
    rng.log_range(low_days, high_days) * 24.0
}

pub(crate) fn white_dwarf_radius(mass: f64) -> f64 {
    0.0126 * (0.6 / mass).cbrt()
}

/// Evolved stars after the main sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Branch {
    /// The red giant branch, or the red clump after it.
    Giant,
    RedSupergiant,
    BlueSupergiant,
}

/// Ranges of radius (log-uniform) and temperature an evolved branch spans.
pub(crate) struct Envelope {
    pub radius: (f64, f64),
    pub kelvin: (f64, f64),
    pub log_kelvin: bool,
}

impl Branch {
    pub(crate) fn envelope(self) -> Envelope {
        match self {
            Branch::Giant => Envelope {
                radius: (8.0, 40.0),
                kelvin: (3_500.0, 5_400.0),
                log_kelvin: false,
            },
            Branch::RedSupergiant => Envelope {
                radius: (200.0, 1_000.0),
                kelvin: (3_500.0, 4_500.0),
                log_kelvin: false,
            },
            Branch::BlueSupergiant => Envelope {
                radius: (20.0, 80.0),
                kelvin: (10_000.0, 25_000.0),
                log_kelvin: true,
            },
        }
    }

    /// Share of stars leaving the main sequence at this mass that join the branch.
    pub(crate) fn share(self, mass: f64) -> f64 {
        match (self, mass < WHITE_DWARF_LIMIT) {
            (Branch::Giant, true) => 1.0,
            (Branch::RedSupergiant, false) => RED_SUPERGIANT_SHARE,
            (Branch::BlueSupergiant, false) => 1.0 - RED_SUPERGIANT_SHARE,
            _ => 0.0,
        }
    }
}

impl Envelope {
    pub(crate) fn kelvin(&self, rng: &mut Rng) -> f64 {
        let (low, high) = self.kelvin;
        if self.log_kelvin {
            rng.log_range(low, high)
        } else {
            rng.range(low, high)
        }
    }
}

/// An evolved star of this mass. Its planets formed around it on the main
/// sequence, and it has since swallowed any within a few radii.
fn giant(id: &str, rng: &mut Rng, mass: f64) -> Formation {
    let branch = if mass < WHITE_DWARF_LIMIT {
        Branch::Giant
    } else if rng.chance(RED_SUPERGIANT_SHARE) {
        Branch::RedSupergiant
    } else {
        Branch::BlueSupergiant
    };
    let envelope = branch.envelope();
    let radius = rng.log_range(envelope.radius.0, envelope.radius.1);
    let kelvin = envelope.kelvin(rng);
    evolved(id, rng, mass, branch, radius, kelvin)
}

/// An evolved star of this shape.
pub(crate) fn evolved(
    id: &str,
    rng: &mut Rng,
    mass: f64,
    branch: Branch,
    radius: f64,
    kelvin: f64,
) -> Formation {
    let (kind, class, period_hours) = match branch {
        Branch::Giant => {
            let kind = match letter(kelvin).0 {
                'M' => "Red giant",
                'K' => "Orange giant",
                _ => "Yellow giant",
            };
            (kind, "III", hours(rng, 100.0, 1_000.0))
        }
        Branch::RedSupergiant => ("Red supergiant", "I", hours(rng, 1_000.0, 10_000.0)),
        Branch::BlueSupergiant => ("Blue supergiant", "I", hours(rng, 5.0, 50.0)),
    };
    let light = surface_light(radius, kelvin);
    let star = Star {
        id: id.into(),
        page: 0,
        spectral: format!("{} {class}", subclass(kelvin)),
        kind: kind.into(),
        mass_solar: mass,
        radius_solar: radius,
        luminosity_solar: light,
        temperature_k: kelvin,
        colour: spectral_colour(kelvin),
        spin: crate::system::shape::star_spin(
            Spin {
                period_hours,
                tilt_rad: rng.rayleigh(0.15),
                azimuth_rad: rng.range(0.0, TAU),
                phase_rad: rng.range(0.0, TAU),
                locked: false,
            },
            mass,
            radius,
        ),
        component: String::new(),
    };
    let swallowed = 3.0 * radius * crate::SUN_RADIUS_METRES / crate::AU_METRES;
    Formation {
        star,
        snow_au: 2.7 * luminosity(mass).sqrt(),
        inner_au: swallowed,
    }
}

/// Luminosity, in solar units, of a surface of this radius and temperature.
pub(crate) fn surface_light(radius_solar: f64, kelvin: f64) -> f64 {
    radius_solar.powi(2) * (kelvin / SUN_TEMPERATURE_K).powi(4)
}

/// The mass of the white dwarf a star of this birth mass leaves: the first
/// draw `white_dwarf` makes.
pub(crate) fn white_dwarf_mass(rng: &mut Rng, progenitor: f64) -> f64 {
    0.5 + 0.3 * (progenitor - 1.0) / 3.0 + rng.range(-0.03, 0.03)
}

/// The white dwarf left by a star of this birth mass, at this temperature or a
/// random one.
pub(crate) fn white_dwarf(
    id: &str,
    rng: &mut Rng,
    progenitor: f64,
    kelvin: Option<f64>,
) -> Formation {
    let mass = white_dwarf_mass(rng, progenitor);
    let kelvin = kelvin.unwrap_or_else(|| rng.log_range(4_500.0, 25_000.0));
    white_dwarf_of_mass(id, rng, progenitor, mass, kelvin)
}

/// A white dwarf of this mass and temperature.
pub(crate) fn white_dwarf_of_mass(
    id: &str,
    rng: &mut Rng,
    progenitor: f64,
    mass: f64,
    kelvin: f64,
) -> Formation {
    // The progenitor lost over half its mass, so surviving orbits widened by the
    // ratio of masses. Planets inside its giant phase were engulfed.
    let widening = progenitor / mass;
    let radius = white_dwarf_radius(mass);
    let light = surface_light(radius, kelvin);
    let star = Star {
        id: id.into(),
        page: 0,
        spectral: format!("DA{}", (50_400.0 / kelvin).round().clamp(1.0, 9.0)),
        kind: "White dwarf".into(),
        mass_solar: mass,
        radius_solar: radius,
        luminosity_solar: light,
        temperature_k: kelvin,
        colour: spectral_colour(kelvin),
        spin: crate::system::shape::star_spin(
            Spin {
                period_hours: hours(rng, 0.05, 3.0),
                // The spin axis keeps its progenitor's direction.
                tilt_rad: rng.rayleigh(0.1),
                azimuth_rad: rng.range(0.0, TAU),
                phase_rad: rng.range(0.0, TAU),
                locked: false,
            },
            mass,
            radius,
        ),
        component: String::new(),
    };
    Formation {
        star,
        snow_au: 2.7 * luminosity(progenitor).sqrt() * widening,
        inner_au: 1.5 * progenitor * widening,
    }
}

pub(crate) fn main_sequence(id: &str, rng: &mut Rng, kelvin: f64, mass: f64) -> Formation {
    let kind = CENSUS
        .iter()
        .filter(|c| c.3 > 0.0 && kelvin >= c.2)
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map_or(Kind::M, |c| c.0);
    let radius = main_sequence_radius(mass);
    let light = luminosity(mass);
    let (period_days, tilt) = match kind {
        // Young or fully convective M dwarfs spin fast; most have slowed.
        Kind::M if rng.chance(0.3) => (rng.log_range(0.2, 10.0), rng.rayleigh(0.1)),
        Kind::M => (rng.log_range(40.0, 150.0), rng.rayleigh(0.1)),
        Kind::K => (rng.log_range(15.0, 50.0), rng.rayleigh(0.1)),
        Kind::G => (rng.log_range(15.0, 35.0), rng.rayleigh(0.1)),
        // Hot stars lack the magnetic braking of cool ones, and are more often
        // misaligned with their planets' orbits.
        Kind::F if kelvin < 6_250.0 => (rng.log_range(5.0, 15.0), rng.rayleigh(0.15)),
        Kind::F => (rng.log_range(1.5, 6.0), hot_tilt(rng)),
        _ => (rng.log_range(0.5, 2.5), hot_tilt(rng)),
    };
    let kind_name = match kind {
        Kind::M => "Red dwarf",
        Kind::K => "Orange dwarf",
        Kind::G => "Yellow dwarf",
        Kind::F => "Yellow-white dwarf",
        Kind::A => "White main-sequence star",
        Kind::O => "Blue main-sequence star",
        _ => "Blue-white main-sequence star",
    };
    let star = Star {
        id: id.into(),
        page: 0,
        spectral: format!("{} V", subclass(kelvin)),
        kind: kind_name.into(),
        mass_solar: mass,
        radius_solar: radius,
        luminosity_solar: light,
        temperature_k: kelvin,
        colour: spectral_colour(kelvin),
        spin: crate::system::shape::star_spin(
            Spin {
                period_hours: period_days * 24.0,
                tilt_rad: tilt.min(PI),
                azimuth_rad: rng.range(0.0, TAU),
                phase_rad: rng.range(0.0, TAU),
                locked: false,
            },
            mass,
            radius,
        ),
        component: String::new(),
    };
    // The disc's inner edge, where dust sublimates.
    Formation {
        star,
        snow_au: 2.7 * light.sqrt(),
        inner_au: 0.04 * light.sqrt(),
    }
}

/// The page star with this ID, on page 0.
pub fn star(id: &str) -> Star {
    formation(id).star
}

/// A star of this galactic component, on page 0.
pub fn galactic_star(id: &str, component: Component) -> Star {
    galactic_formation(id, component).star
}

/// Rotation angle about the spin axis at a simulation time.
pub fn spin_angle(spin: &Spin, seconds: f64) -> f64 {
    if spin.period_hours <= 0.0 {
        return 0.0;
    }
    (spin.phase_rad + TAU * (seconds / (spin.period_hours * 3_600.0)).fract()).rem_euclid(TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stars() -> Vec<Star> {
        (0..20_000)
            .map(|page| star(&crate::star_id("list", page)))
            .collect()
    }

    #[test]
    fn census_matches_the_solar_neighbourhood() {
        let stars = stars();
        let share = |kind: &str| {
            stars.iter().filter(|s| s.kind == kind).count() as f64 / stars.len() as f64 * 100.0
        };
        assert!((share("Red dwarf") - 72.0).abs() < 1.5);
        assert!((share("Orange dwarf") - 12.0).abs() < 1.0);
        assert!((share("Yellow dwarf") - 7.0).abs() < 1.0);
        assert!((share("White dwarf") - 5.0).abs() < 1.0);
        let giants = share("Red giant") + share("Orange giant") + share("Yellow giant");
        assert!(giants > 0.1 && giants < 0.6, "giants {giants}%");
    }

    #[test]
    fn main_sequence_properties_agree() {
        for star in stars().iter().filter(|s| s.spectral.ends_with(" V")) {
            let first = star.spectral.chars().next().unwrap();
            assert_eq!(first, letter(star.temperature_k).0, "{star:?}");
            let kelvin = temperature(star.luminosity_solar, star.radius_solar);
            assert!(
                (kelvin / star.temperature_k - 1.0).abs() < 1e-6,
                "{kelvin} {star:?}"
            );
        }
        // The Sun: a G2 V star of one solar mass.
        let sun = mass_for_temperature(SUN_TEMPERATURE_K);
        assert!((sun - 1.0).abs() < 0.03, "{sun}");
        assert_eq!(subclass(SUN_TEMPERATURE_K), "G2");
    }

    #[test]
    fn rotation_is_realistic_per_class() {
        for star in stars() {
            let days = star.spin.period_hours / 24.0;
            match star.kind.as_str() {
                "Yellow dwarf" => assert!((15.0..=35.0).contains(&days)),
                "White dwarf" => assert!(days <= 3.0),
                "Red giant" | "Orange giant" | "Yellow giant" => assert!(days >= 100.0),
                _ => assert!((0.2..=150.0).contains(&days)),
            }
        }
    }

    #[test]
    fn spins_keep_stars_round_enough() {
        use crate::system::shape::{MAX_FLATTENING, fastest_star_hours, star_flattening};
        let stars = stars();
        let mut slowed = std::collections::BTreeMap::<&str, usize>::new();
        for star in &stars {
            let f = star_flattening(star.mass_solar, star.radius_solar, star.spin.period_hours);
            assert!(f <= MAX_FLATTENING, "{star:?}");
            if star.spin.period_hours == fastest_star_hours(star.mass_solar, star.radius_solar) {
                *slowed.entry(star.kind.as_str()).or_default() += 1;
            }
        }
        eprintln!("{} stars, slowed to the limit: {slowed:?}", stars.len());
    }

    #[test]
    fn stars_are_mostly_aligned_with_their_planets() {
        let stars = stars();
        let tilted = stars
            .iter()
            .filter(|s| s.spin.tilt_rad > 30_f64.to_radians())
            .count();
        assert!(tilted < stars.len() / 100, "{tilted} tilted past 30°");
        for star in stars.iter().filter(|s| {
            ["Red dwarf", "Orange dwarf", "Yellow dwarf", "White dwarf"].contains(&s.kind.as_str())
        }) {
            assert!(star.spin.tilt_rad < 40_f64.to_radians(), "{star:?}");
        }
    }

    #[test]
    fn colours_run_from_red_to_blue() {
        let [red, sun, blue] = [3_000.0, 5_772.0, 20_000.0].map(spectral_colour);
        assert!(red[0] == 255 && red[1] < 100 && red[2] < 60);
        assert!(sun[0] == 255 && sun[1] > 220 && sun[2] < 160);
        assert!(blue[2] == 255 && blue[0] < 150);
        // Hotter is never redder.
        let greens: Vec<_> = (26..60)
            .map(|t| spectral_colour(t as f64 * 100.0)[1])
            .collect();
        assert!(greens.windows(2).all(|g| g[1] >= g[0]));
    }
}
