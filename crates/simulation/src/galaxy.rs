//! A Milky Way-like barred spiral: how densely its stars are packed at every
//! point, and which component they belong to. Galactic stars are drawn from
//! their component (`GalaxyModel::star`), so the mix of types follows the region.
//!
//! - Axes in parsecs, centred on the black hole. The disc is the XZ plane and Y
//!   points to the north galactic pole, as in the orbital planes.
//! - Components, after Bland-Hawthorn & Gerhard (2016): a nuclear star cluster, a
//!   triaxial bar and bulge (Wegg & Gerhard 2013), an exponential thin disc with
//!   spiral arms, young stars concentrated in the arms, a thick disc and a
//!   flattened halo.
//! - Every profile has a closed-form integral, so each component holds exactly
//!   its share of `GalaxyModel::stars`. Arms only move stars around their circle.

use crate::star::galactic_star;
use crate::system::Rng;
use space_shared::Star;
use std::f64::consts::{PI, TAU};

pub const PC_METRES: f64 = 3.085_677_581_491_367e16;
/// Schwarzschild radius per solar mass, 2GM☉/c².
const SCHWARZSCHILD_METRES_PER_SOLAR: f64 = 2_953.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Component {
    Nucleus,
    Bulge,
    ThinDisc,
    /// Stars younger than 200 million years, still near the arms they formed in.
    Arms,
    ThickDisc,
    Halo,
}

impl Component {
    pub const ALL: [Component; 6] = [
        Component::Nucleus,
        Component::Bulge,
        Component::ThinDisc,
        Component::Arms,
        Component::ThickDisc,
        Component::Halo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Component::Nucleus => "Nuclear star cluster",
            Component::Bulge => "Bar and bulge",
            Component::ThinDisc => "Thin disc",
            Component::Arms => "Spiral arm",
            Component::ThickDisc => "Thick disc",
            Component::Halo => "Halo",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }

    /// Star formation history: bursts of (share, youngest, oldest) in billions
    /// of years, each forming stars at a constant rate.
    fn history(self) -> &'static [(f64, f64, f64)] {
        match self {
            // Mostly old, with a young population around the black hole.
            Component::Nucleus => &[(0.85, 5.0, 12.0), (0.15, 0.003, 1.0)],
            Component::Bulge => &[(0.9, 8.0, 12.0), (0.1, 1.0, 5.0)],
            Component::ThinDisc => &[(1.0, 0.2, 10.0)],
            Component::Arms => &[(1.0, 0.0, 0.2)],
            Component::ThickDisc => &[(1.0, 8.0, 12.0)],
            Component::Halo => &[(1.0, 11.0, 13.0)],
        }
    }

    pub(crate) fn age_gyr(self, rng: &mut Rng) -> f64 {
        let mut roll = rng.unit();
        let history = self.history();
        let &(_, young, old) = history
            .iter()
            .find(|b| {
                roll -= b.0;
                roll < 0.0
            })
            .unwrap_or(&history[history.len() - 1]);
        rng.range(young, old)
    }

    /// Share of the component's stars younger than this, in billions of years.
    pub(crate) fn age_cdf(self, age: f64) -> f64 {
        self.history()
            .iter()
            .map(|&(share, young, old)| share * ((age - young) / (old - young)).clamp(0.0, 1.0))
            .sum()
    }

    /// An age between these two, drawn from the formation history.
    pub(crate) fn age_between(self, rng: &mut Rng, young: f64, old: f64) -> f64 {
        let (low, high) = (self.age_cdf(young), self.age_cdf(old));
        let target = low + rng.unit() * (high - low);
        let (mut young, mut old) = (young, old.min(self.oldest()));
        for _ in 0..60 {
            let mid = 0.5 * (young + old);
            if self.age_cdf(mid) < target {
                young = mid;
            } else {
                old = mid;
            }
        }
        0.5 * (young + old)
    }

    fn oldest(self) -> f64 {
        self.history().iter().map(|b| b.2).fold(0.0, f64::max)
    }
}

/// An exponential disc, or a ring that peaks at twice its scale length.
#[derive(Clone, Debug, PartialEq)]
pub struct Disc {
    pub scale_length_pc: f64,
    pub scale_height_pc: f64,
    /// Surface density rises as R² from the centre, as for star formation,
    /// which the bar has cleared from the inner disc.
    pub ring: bool,
    /// Arm contrast, from 0 (none) to 1 (no stars left between arms).
    pub arm_strength: f64,
}

impl Disc {
    fn density(&self, stars: f64, radius: f64, height: f64) -> f64 {
        let h = self.scale_height_pc;
        self.surface(stars, radius) * (-height.abs() / h).exp() / (2.0 * h)
    }

    /// Stars per square parsec, seen face on.
    pub(crate) fn surface(&self, stars: f64, radius: f64) -> f64 {
        let l = self.scale_length_pc;
        let x = radius / l;
        let surface = if self.ring {
            x * x * (-x).exp() / (12.0 * PI * l * l)
        } else {
            (-x).exp() / (TAU * l * l)
        };
        stars * surface
    }

    /// Share of the disc's stars at this height or below.
    fn vertical_cdf(&self, y: f64) -> f64 {
        let e = (-y.abs() / self.scale_height_pc).exp();
        if y < 0.0 { e / 2.0 } else { 1.0 - e / 2.0 }
    }

    /// Share of the disc's stars between two heights.
    pub(crate) fn vertical(&self, low: f64, high: f64) -> f64 {
        self.vertical_cdf(high) - self.vertical_cdf(low)
    }

    /// A height between two, drawn from the vertical profile.
    pub(crate) fn height_between(&self, low: f64, high: f64, u: f64) -> f64 {
        let target = self.vertical_cdf(low) + u * self.vertical(low, high);
        let h = self.scale_height_pc;
        let y = if target < 0.5 {
            h * (2.0 * target).ln()
        } else {
            -h * (2.0 * (1.0 - target)).ln()
        };
        y.clamp(low, high)
    }
}

/// Logarithmic spiral arms.
#[derive(Clone, Debug, PartialEq)]
pub struct Spiral {
    pub arms: u32,
    /// Angle between an arm and the circle through it.
    pub pitch_rad: f64,
    /// Arm phase at the reference radius.
    pub phase_rad: f64,
    pub reference_pc: f64,
    /// Arms grow out from the bar's end and fade beyond the edge of the disc.
    pub inner_pc: f64,
    pub outer_pc: f64,
    /// Arm profile ((1 + cos)/2)^sharpness: higher is narrower.
    pub sharpness: i32,
}

impl Spiral {
    /// Mean of the arm profile around any circle.
    fn mean(&self) -> f64 {
        let k = self.sharpness as u64;
        // C(2k, k) / 4^k.
        (0..k).fold(1.0, |m, i| m * (2 * k - i) as f64 / (k - i) as f64) / 4f64.powi(k as i32)
    }

    /// Density factor around the circle for this arm strength. It averages to one
    /// around every circle, so arms never change a disc's total.
    pub(crate) fn modulation(&self, strength: f64, x: f64, z: f64) -> f64 {
        let radius = x.hypot(z);
        if strength <= 0.0 || radius <= 0.0 {
            return 1.0;
        }
        let weight = smoothstep(self.inner_pc * 0.7, self.inner_pc, radius)
            * (1.0 - smoothstep(self.outer_pc * 0.7, self.outer_pc, radius));
        let winding = (radius / self.reference_pc).ln() / self.pitch_rad.tan();
        let angle = f64::from(self.arms) * (z.atan2(x) - winding - self.phase_rad);
        let profile = ((1.0 + angle.cos()) / 2.0).powi(self.sharpness);
        let mean = self.mean();
        1.0 + strength / mean * weight * (profile - mean)
    }
}

fn smoothstep(low: f64, high: f64, x: f64) -> f64 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Debug, PartialEq)]
pub struct GalaxyModel {
    /// Stars, including white dwarfs but not brown dwarfs, neutron stars or
    /// stellar black holes.
    pub stars: f64,
    /// Share of the stars in each component, in `Component::ALL` order.
    pub shares: [f64; 6],
    pub black_hole_solar: f64,
    /// Where the program starts: a Sun-like spot in the disc.
    pub home_pc: [f64; 3],
    /// No stars beyond this distance from the centre.
    pub radius_pc: f64,
    /// Plummer radius of the nuclear star cluster.
    pub nucleus_pc: f64,
    /// Scale lengths of the bar along its long axis, vertically and across.
    pub bar_pc: [f64; 3],
    /// Angle of the bar's long axis from the line from home to the centre.
    pub bar_angle_rad: f64,
    pub thin: Disc,
    pub arms: Disc,
    pub thick: Disc,
    pub spiral: Spiral,
    pub halo_core_pc: f64,
    /// Vertical axis of the halo relative to its radial ones.
    pub halo_flattening: f64,
}

impl Default for GalaxyModel {
    fn default() -> Self {
        Self::milky_way()
    }
}

impl GalaxyModel {
    pub fn milky_way() -> Self {
        let home = 8_200.0;
        let spiral = Spiral {
            arms: 4,
            pitch_rad: 12_f64.to_radians(),
            // Home lies midway between two arms.
            phase_rad: PI / 4.0,
            reference_pc: home,
            inner_pc: 4_000.0,
            outer_pc: 18_000.0,
            sharpness: 4,
        };
        Self {
            stars: 2.0e11,
            shares: [0.0005, 0.24, 0.6365, 0.013, 0.09, 0.02],
            black_hole_solar: 4.3e6,
            home_pc: [home, 20.8, 0.0],
            radius_pc: 50_000.0,
            nucleus_pc: 4.0,
            bar_pc: [670.0, 270.0, 290.0],
            bar_angle_rad: 27_f64.to_radians(),
            thin: Disc {
                scale_length_pc: 2_600.0,
                scale_height_pc: 300.0,
                ring: false,
                arm_strength: 0.15,
            },
            arms: Disc {
                scale_length_pc: 2_500.0,
                scale_height_pc: 100.0,
                ring: true,
                arm_strength: 0.97,
            },
            thick: Disc {
                scale_length_pc: 2_000.0,
                scale_height_pc: 900.0,
                ring: false,
                arm_strength: 0.0,
            },
            spiral,
            halo_core_pc: 2_500.0,
            halo_flattening: 0.7,
        }
    }

    pub fn stars_in(&self, component: Component) -> f64 {
        self.stars * self.shares[component.index()]
    }

    /// The disc a component is spread over, if it is one.
    pub(crate) fn disc(&self, component: Component) -> Option<&Disc> {
        match component {
            Component::ThinDisc => Some(&self.thin),
            Component::Arms => Some(&self.arms),
            Component::ThickDisc => Some(&self.thick),
            _ => None,
        }
    }

    /// Stars per square parsec of a disc component, seen face on.
    pub(crate) fn disc_surface(&self, component: Component, x: f64, z: f64) -> f64 {
        self.disc(component).map_or(0.0, |disc| {
            disc.surface(self.stars_in(component), x.hypot(z))
                * self.spiral.modulation(disc.arm_strength, x, z)
        })
    }

    pub fn black_hole_radius_metres(&self) -> f64 {
        self.black_hole_solar * SCHWARZSCHILD_METRES_PER_SOLAR
    }

    /// Stars per cubic parsec of one component.
    pub fn component_density(&self, component: Component, p: [f64; 3]) -> f64 {
        let [x, y, z] = p;
        let r2 = x * x + y * y + z * z;
        if r2 > self.radius_pc * self.radius_pc {
            return 0.0;
        }
        let stars = self.stars_in(component);
        // Only the discs and the halo need the distance in the plane.
        let radius = || x.hypot(z);
        match component {
            Component::Nucleus => {
                let a = self.nucleus_pc;
                3.0 * stars / (4.0 * PI * a.powi(3)) * (1.0 + r2 / (a * a)).powf(-2.5)
            }
            Component::Bulge => {
                let (s, c) = self.bar_angle_rad.sin_cos();
                let [long, high, across] = self.bar_pc;
                let along = (x * c + z * s) / long;
                let side = (z * c - x * s) / across;
                let scaled = (along * along + side * side + (y / high).powi(2)).sqrt();
                stars / (8.0 * PI * long * high * across) * (-scaled).exp()
            }
            Component::ThinDisc | Component::Arms | Component::ThickDisc => {
                let disc = match component {
                    Component::ThinDisc => &self.thin,
                    Component::Arms => &self.arms,
                    _ => &self.thick,
                };
                disc.density(stars, radius(), y) * self.spiral.modulation(disc.arm_strength, x, z)
            }
            Component::Halo => {
                // ρ ∝ (r² + c²)⁻², cored and falling as r⁻⁴, truncated at the
                // galaxy's radius in flattened coordinates.
                let (c, q, edge) = (self.halo_core_pc, self.halo_flattening, self.radius_pc);
                let radius = radius();
                let squared = radius * radius + (y / q).powi(2);
                if squared > edge * edge {
                    return 0.0;
                }
                let total = q
                    * 4.0
                    * PI
                    * ((edge / c).atan() / (2.0 * c) - edge / (2.0 * (edge * edge + c * c)));
                stars / total / (squared + c * c).powi(2)
            }
        }
    }

    /// Stars per cubic parsec.
    pub fn density(&self, p: [f64; 3]) -> f64 {
        Component::ALL
            .iter()
            .map(|&c| self.component_density(c, p))
            .sum()
    }

    /// Share of the stars at a point in each component, in `Component::ALL` order.
    pub fn shares_at(&self, p: [f64; 3]) -> [f64; 6] {
        let densities = Component::ALL.map(|c| self.component_density(c, p));
        let total: f64 = densities.iter().sum();
        if total <= 0.0 {
            return [0.0; 6];
        }
        densities.map(|d| d / total)
    }

    /// The component of a star at a point, for a uniform draw.
    pub fn component_at(&self, p: [f64; 3], u: f64) -> Component {
        let mut roll = u;
        let shares = self.shares_at(p);
        Component::ALL
            .into_iter()
            .zip(shares)
            .find(|&(_, share)| {
                roll -= share;
                roll < 0.0
            })
            .map_or(Component::ThinDisc, |(c, _)| c)
    }

    /// The star with this ID at a point, drawn from the components there.
    pub fn star(&self, id: &str, p: [f64; 3]) -> (Component, Star) {
        let mut rng = Rng::new(&format!("{id}:component"));
        let component = self.component_at(p, rng.unit());
        (component, galactic_star(id, component))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stars in each component, integrated over all space in cylindrical
    /// coordinates with log-spaced radius and height, so both the parsec-wide
    /// nucleus and the 50 kpc halo resolve.
    fn integrate(galaxy: &GalaxyModel) -> [f64; 6] {
        let f = |p| Component::ALL.map(|c| galaxy.component_density(c, p));
        let (low, high) = (1e-2_f64.ln(), 5e4_f64.ln());
        let steps = 240;
        let ds = (high - low) / steps as f64;
        let angles = 48;
        let simpson = |i: usize| match i {
            0 => 1.0,
            i if i == steps => 1.0,
            i if i % 2 == 1 => 4.0,
            _ => 2.0,
        };
        let mut sum = [0.0; 6];
        for i in 0..=steps {
            let radius = (low + i as f64 * ds).exp();
            for j in 0..=steps {
                let height = (low + j as f64 * ds).exp();
                let weight = simpson(i) * simpson(j) * radius * radius * height;
                for k in 0..angles {
                    let phi = TAU * k as f64 / angles as f64;
                    let (s, c) = phi.sin_cos();
                    let [x, z] = [radius * c, radius * s];
                    let (above, below) = (f([x, height, z]), f([x, -height, z]));
                    for c in 0..6 {
                        sum[c] += weight * (above[c] + below[c]);
                    }
                }
            }
        }
        sum.map(|s| s * (ds / 3.0).powi(2) * TAU / angles as f64)
    }

    #[test]
    fn components_hold_their_shares_of_two_hundred_billion_stars() {
        let galaxy = GalaxyModel::milky_way();
        assert!((galaxy.shares.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        let stars = integrate(&galaxy);
        let total: f64 = stars.iter().sum();
        assert!((total / 2.0e11 - 1.0).abs() < 0.01, "{total:e} stars");
        for (component, stars) in Component::ALL.into_iter().zip(stars) {
            let wanted = galaxy.stars_in(component);
            assert!(
                (stars / wanted - 1.0).abs() < 0.01,
                "{component:?}: {stars:e} of {wanted:e}"
            );
        }
    }

    /// Stars per square parsec through the disc at a point in the plane.
    fn surface(galaxy: &GalaxyModel, x: f64, z: f64) -> f64 {
        (0..4_000)
            .map(|i| {
                let y = (i as f64 + 0.5) * 2.0;
                2.0 * 2.0 * galaxy.density([x, y, z])
            })
            .sum()
    }

    #[test]
    fn density_falls_from_the_centre_and_the_disc_is_exponential() {
        let galaxy = GalaxyModel::milky_way();
        let at = |r: f64| galaxy.density([r, 0.0, 0.0]);
        let radii = [
            1.0, 10.0, 100.0, 1_000.0, 4_000.0, 8_000.0, 16_000.0, 40_000.0,
        ];
        assert!(radii.windows(2).all(|w| at(w[0]) > at(w[1])));
        // Around the black hole, the nuclear cluster packs 10⁵ stars per pc³.
        assert!(at(1.0) > 1e5, "{}", at(1.0));
        // Near home, a few tenths per pc³, as in the solar neighbourhood.
        let home = galaxy.density(galaxy.home_pc);
        assert!((0.05..0.5).contains(&home), "{home}");
        // Averaged around the circle, the outer disc thins by e every scale length.
        let ring = |r: f64| {
            (0..64)
                .map(|k| {
                    let (s, c) = (TAU * k as f64 / 64.0).sin_cos();
                    surface(&galaxy, r * c, r * s)
                })
                .sum::<f64>()
                / 64.0
        };
        let length = 2_000.0 / (ring(8_000.0) / ring(10_000.0)).ln();
        assert!((2_200.0..2_800.0).contains(&length), "{length} pc");
    }

    #[test]
    fn the_bar_arms_and_halo_shape_the_galaxy() {
        let galaxy = GalaxyModel::milky_way();
        // The bar is longer along its axis than across it.
        let (s, c) = galaxy.bar_angle_rad.sin_cos();
        let along = galaxy.component_density(Component::Bulge, [800.0 * c, 0.0, 800.0 * s]);
        let across = galaxy.component_density(Component::Bulge, [-800.0 * s, 0.0, 800.0 * c]);
        assert!(along > 2.0 * across);
        // Young stars crowd into the arms; old ones barely notice them. Home is
        // between arms.
        let circle = |component| -> Vec<f64> {
            (0..720)
                .map(|k| {
                    let (s, c) = (TAU * k as f64 / 720.0).sin_cos();
                    galaxy.component_density(component, [8_200.0 * c, 0.0, 8_200.0 * s])
                })
                .collect()
        };
        let contrast = |component| {
            let values = circle(component);
            let max = values.iter().copied().fold(0.0, f64::max);
            let min = values.iter().copied().fold(f64::MAX, f64::min);
            max / min
        };
        assert!(contrast(Component::Arms) > 20.0);
        let old = contrast(Component::ThinDisc);
        assert!((1.2..2.5).contains(&old), "{old}");
        let home = galaxy.component_density(Component::Arms, [8_200.0, 0.0, 0.0]);
        assert!(home < circle(Component::Arms).iter().sum::<f64>() / 720.0 * 0.3);
        // The halo takes over far above the disc and is rare near home.
        assert!(galaxy.shares_at([0.0, 20_000.0, 0.0])[5] > 0.9);
        let halo = galaxy.shares_at(galaxy.home_pc)[5];
        assert!((1e-4..1e-2).contains(&halo), "{halo}");
        // The nucleus owns the centre; the bulge the inner kiloparsec.
        assert!(galaxy.shares_at([1.0, 0.0, 0.0])[0] > 0.9);
        assert!(galaxy.shares_at([500.0, 0.0, 0.0])[1] > 0.5);
    }

    fn census(galaxy: &GalaxyModel, p: [f64; 3], count: u32) -> Vec<Star> {
        (0..count)
            .map(|i| galaxy.star(&format!("census:{p:?}:{i}"), p).1)
            .collect()
    }

    fn share(stars: &[Star], test: impl Fn(&Star) -> bool) -> f64 {
        stars.iter().filter(|s| test(s)).count() as f64 / stars.len() as f64
    }

    fn hot(star: &Star) -> bool {
        star.spectral.ends_with(" V") && star.temperature_k > 6_000.0
    }

    #[test]
    fn star_types_follow_the_region() {
        let galaxy = GalaxyModel::milky_way();
        // Near home, the solar-neighbourhood mix.
        let home = census(&galaxy, galaxy.home_pc, 40_000);
        let red = share(&home, |s| s.kind == "Red dwarf");
        let white = share(&home, |s| s.kind == "White dwarf");
        let giants = share(&home, |s| s.kind.contains("giant"));
        assert!((0.65..0.8).contains(&red), "red dwarfs {red}");
        assert!((0.03..0.12).contains(&white), "white dwarfs {white}");
        assert!((0.001..0.02).contains(&giants), "giants {giants}");
        // The halo is old: nothing hotter than the Sun is left on the main
        // sequence, and its white dwarfs have cooled.
        let halo = census(&galaxy, [0.0, 20_000.0, 0.0], 20_000);
        assert_eq!(share(&halo, hot), 0.0);
        assert!(share(&halo, |s| s.kind == "White dwarf") > white);
        let cool = |stars: &[Star]| {
            let dwarfs: Vec<_> = stars.iter().filter(|s| s.kind == "White dwarf").collect();
            dwarfs.iter().map(|s| s.temperature_k).sum::<f64>() / dwarfs.len() as f64
        };
        assert!(cool(&halo) < cool(&home));
        // Young arm stars include O and B stars, and have made few white dwarfs.
        let arms: Vec<Star> = (0..40_000)
            .map(|i| galactic_star(&format!("arm:{i}"), Component::Arms))
            .collect();
        assert!(
            share(&arms, |s| s.spectral.starts_with('O')
                || s.spectral.starts_with('B'))
                > 0.001
        );
        assert!(share(&arms, |s| s.kind == "White dwarf") < 0.01);
        assert!(share(&arms, hot) > share(&home, hot));
        // The bulge is old too, with a few younger stars.
        let bulge: Vec<Star> = (0..20_000)
            .map(|i| galactic_star(&format!("bulge:{i}"), Component::Bulge))
            .collect();
        assert!(share(&bulge, |s| s.spectral.starts_with('O')) == 0.0);
        assert!(share(&bulge, |s| s.kind == "White dwarf") > white);
    }

    #[test]
    fn galactic_stars_are_repeatable() {
        let galaxy = GalaxyModel::milky_way();
        let a = galaxy.star("cell:7", galaxy.home_pc);
        assert_eq!(a, galaxy.star("cell:7", galaxy.home_pc));
        assert_ne!(a.1, galaxy.star("cell:8", galaxy.home_pc).1);
        // Sgr A*: about 0.085 AU across its event horizon.
        let horizon = galaxy.black_hole_radius_metres() / crate::AU_METRES;
        assert!((horizon - 0.085).abs() < 0.005, "{horizon}");
    }
}
