//! Photometry for realistic rendering: what each star emits and what each planet
//! reflects, in physical units.
//!
//! - Colours are blackbody spectra seen through the CIE 1931 observer, white
//!   balanced to the Sun, so the Sun is white and cooler stars are visibly red.
//! - Brightness is visual: a cool star radiates mostly infrared, so it looks far
//!   dimmer than its bolometric luminosity suggests.
//! - Radiance is in W m⁻² sr⁻¹ of visually weighted light, so star surfaces,
//!   lit planets and distant point sources share one scale.

use crate::AU_METRES;
use space_shared::Star;
use std::f64::consts::PI;

pub const SUN_LUMINOSITY_W: f64 = 3.828e26;
/// Apparent visual magnitude of the Sun from 1 AU.
const SUN_MAGNITUDE_AT_AU: f64 = -26.74;
const SUN_TEMPERATURE_K: f64 = 5772.0;

/// Multi-lobe fit to the CIE 1931 2° colour-matching functions (Wyman, Sloan and
/// Shirley 2013): (weight, centre nm, width below, width above) per lobe.
const CMF_X: [(f64, f64, f64, f64); 3] = [
    (1.056, 599.8, 37.9, 31.0),
    (0.362, 442.0, 16.0, 26.7),
    (-0.065, 501.1, 20.4, 26.2),
];
const CMF_Y: [(f64, f64, f64, f64); 2] = [(0.821, 568.8, 46.9, 40.5), (0.286, 530.9, 16.3, 31.1)];
const CMF_Z: [(f64, f64, f64, f64); 2] = [(1.217, 437.0, 11.8, 36.0), (0.681, 459.0, 26.0, 13.8)];

fn cmf(lobes: &[(f64, f64, f64, f64)], nm: f64) -> f64 {
    lobes
        .iter()
        .map(|&(weight, centre, below, above)| {
            let width = if nm < centre { below } else { above };
            weight * (-0.5 * ((nm - centre) / width).powi(2)).exp()
        })
        .sum()
}

/// Planck spectral radiance, up to a constant factor, at a wavelength in nm.
fn planck(nm: f64, kelvin: f64) -> f64 {
    // Second radiation constant hc/k in nm·K.
    const C2: f64 = 1.438_776_877e7;
    1.0 / (nm.powi(5) * ((C2 / (nm * kelvin)).exp() - 1.0))
}

/// CIE XYZ of a blackbody, up to a common factor.
fn xyz(kelvin: f64) -> [f64; 3] {
    let mut sum = [0.0; 3];
    let mut nm = 380.0;
    while nm <= 780.0 {
        let b = planck(nm, kelvin);
        sum[0] += b * cmf(&CMF_X, nm);
        sum[1] += b * cmf(&CMF_Y, nm);
        sum[2] += b * cmf(&CMF_Z, nm);
        nm += 2.0;
    }
    sum
}

fn linear_srgb([x, y, z]: [f64; 3]) -> [f64; 3] {
    [
        3.2406 * x - 1.5372 * y - 0.4986 * z,
        -0.9689 * x + 1.8758 * y + 0.0415 * z,
        0.0557 * x - 0.2040 * y + 1.0570 * z,
    ]
}

pub fn luminance([r, g, b]: [f64; 3]) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Linear sRGB colour of a blackbody with unit luminance, white balanced so the
/// Sun is neutral white.
pub fn blackbody_colour(kelvin: f64) -> [f64; 3] {
    let sun = linear_srgb(xyz(SUN_TEMPERATURE_K));
    let raw = linear_srgb(xyz(kelvin.clamp(1_000.0, 50_000.0)));
    let balanced: [f64; 3] = std::array::from_fn(|i| (raw[i] / sun[i]).max(0.0));
    let y = luminance(balanced);
    balanced.map(|c| c / y)
}

/// Exaggerates a unit-luminance colour's hue away from white, keeping unit
/// luminance. White stays white and hue order is kept; `power` 1 is unchanged.
pub fn saturate(colour: [f64; 3], power: f64) -> [f64; 3] {
    let raised = colour.map(|c| c.max(0.0).powf(power));
    let y = luminance(raised);
    if y <= 0.0 {
        return colour;
    }
    raised.map(|c| c / y)
}

/// Share of a blackbody's output that the eye sees, relative to the Sun's.
pub fn visual_efficiency(kelvin: f64) -> f64 {
    // Total Planck radiance scales as T⁴; the visible share is Y over that.
    let share = |t: f64| xyz(t)[1] / t.powi(4);
    share(kelvin.clamp(1_000.0, 50_000.0)) / share(SUN_TEMPERATURE_K)
}

/// Visual luminosity in solar units.
pub fn visual_luminosity(star: &Star) -> f64 {
    star.luminosity_solar * visual_efficiency(star.temperature_k)
}

/// Visual flux in W m⁻² at a distance from a star of this visual luminosity.
pub fn flux(visual_luminosity_solar: f64, metres: f64) -> f64 {
    visual_luminosity_solar * SUN_LUMINOSITY_W / (4.0 * PI * metres.max(1.0).powi(2))
}

/// Radiance of the star's surface.
pub fn surface_radiance(star: &Star) -> f64 {
    let radius = star.radius_solar * crate::SUN_RADIUS_METRES;
    flux(visual_luminosity(star), radius) / PI
}

/// Radiance of a Lambertian surface lit face on.
pub fn reflected_radiance(albedo: f64, flux: f64) -> f64 {
    albedo * flux / PI
}

/// Apparent visual magnitude of a star from a distance.
pub fn apparent_magnitude(visual_luminosity_solar: f64, metres: f64) -> f64 {
    SUN_MAGNITUDE_AT_AU - 2.5 * visual_luminosity_solar.log10()
        + 5.0 * (metres.max(1.0) / AU_METRES).log10()
}

/// How a planet reflects light: overall albedo and a reflectance colour with
/// unit luminance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub albedo: f64,
    pub tint: [f64; 3],
}

/// Representative geometric albedos and colours by planet class.
pub fn surface(class: &str) -> Surface {
    let (albedo, tint) = match class {
        "Dwarf planet" => (0.15, [0.55, 0.5, 0.45]),
        "Sub-Neptune" => (0.35, [0.55, 0.65, 0.75]),
        "Ice giant" => (0.45, [0.45, 0.7, 0.8]),
        "Gas giant" => (0.5, [0.8, 0.7, 0.55]),
        _ => (0.3, [0.6, 0.55, 0.5]),
    };
    let y = luminance(tint);
    Surface {
        albedo,
        tint: tint.map(|c| c / y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sun() -> Star {
        Star {
            id: "sun".into(),
            page: 0,
            spectral: "G2 V".into(),
            kind: "Yellow dwarf".into(),
            mass_solar: 1.0,
            radius_solar: 1.0,
            luminosity_solar: 1.0,
            temperature_k: SUN_TEMPERATURE_K,
            colour: [255, 255, 255],
            spin: Default::default(),
            component: String::new(),
        }
    }

    #[test]
    fn the_sun_is_white_and_cool_stars_are_red() {
        for c in blackbody_colour(SUN_TEMPERATURE_K) {
            assert!((c - 1.0).abs() < 1e-9);
        }
        // True colours: red dwarfs are orange rather than deep red.
        let red = blackbody_colour(3_000.0);
        assert!(red[0] > 1.5 * red[1] && red[1] > 2.0 * red[2], "{red:?}");
        let blue = blackbody_colour(20_000.0);
        assert!(blue[2] > blue[0], "{blue:?}");
        for kelvin in [2_600.0, 4_000.0, 8_000.0, 30_000.0] {
            let colour = blackbody_colour(kelvin);
            assert!((luminance(colour) - 1.0).abs() < 1e-9);
            assert!(colour.iter().all(|&c| c >= 0.0));
        }
        // Hotter is never redder.
        let ratios: Vec<_> = (26..300)
            .map(|t| {
                let c = blackbody_colour(t as f64 * 100.0);
                c[2] / c[0]
            })
            .collect();
        assert!(ratios.windows(2).all(|r| r[1] >= r[0]));
    }

    #[test]
    fn saturation_makes_red_dwarfs_red_and_keeps_the_sun_white() {
        for c in saturate(blackbody_colour(SUN_TEMPERATURE_K), 2.5) {
            assert!((c - 1.0).abs() < 1e-9);
        }
        let red = saturate(blackbody_colour(3_000.0), 2.5);
        assert!((luminance(red) - 1.0).abs() < 1e-9);
        assert!(red[0] > 4.0 * red[1] && red[1] > 5.0 * red[2], "{red:?}");
        for (a, b) in saturate(red, 1.0).into_iter().zip(red) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn cool_stars_look_dimmer_than_they_radiate() {
        assert!((visual_efficiency(SUN_TEMPERATURE_K) - 1.0).abs() < 1e-12);
        // A 3,000 K red dwarf emits most of its power in the infrared.
        let red = visual_efficiency(3_000.0);
        assert!(red > 0.05 && red < 0.35, "{red}");
        assert!(visual_efficiency(10_000.0) < 1.0);
    }

    #[test]
    fn solar_values_match_the_real_sun() {
        let sun = sun();
        let constant = flux(1.0, AU_METRES);
        assert!((constant - 1_361.0).abs() < 5.0, "{constant}");
        assert!((apparent_magnitude(1.0, AU_METRES) - SUN_MAGNITUDE_AT_AU).abs() < 1e-12);
        // The Sun from 10 parsecs: absolute magnitude 4.83.
        let parsec = 3.085_677_581e16;
        assert!((apparent_magnitude(1.0, 10.0 * parsec) - 4.83).abs() < 0.02);
        // Around 17 stops between the Sun's surface and Earth's sunlit ground.
        let ratio = surface_radiance(&sun) / reflected_radiance(0.3, constant);
        assert!(ratio.log2() > 16.0 && ratio.log2() < 18.0, "{ratio}");
    }

    #[test]
    fn surfaces_have_unit_luminance_tints() {
        for class in [
            "Dwarf planet",
            "Rocky planet",
            "Sub-Neptune",
            "Ice giant",
            "Gas giant",
        ] {
            let s = surface(class);
            assert!((luminance(s.tint) - 1.0).abs() < 1e-12);
            assert!(s.albedo > 0.0 && s.albedo < 1.0);
        }
    }
}
