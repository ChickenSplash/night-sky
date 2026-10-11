//! Fixed unit conversions for galactic light. No view meter or adaptation.
use crate::Navigation;
use space_simulation::{
    galaxy::PC_METRES, light::SUN_LUMINOSITY_W, population::flux_per_luminosity,
};
use std::f64::consts::PI;

/// Legacy dark-space calibration, retained only as the fixed zero-stop reference.
const POINT_LIGHT: f64 = 74.6;

/// Visual irradiance in W/m² per solar luminosity per square parsec.
pub fn irradiance_per_ratio() -> f64 {
    SUN_LUMINOSITY_W / (4.0 * PI * PC_METRES * PC_METRES)
}

/// Visual radiance that maps to scene-linear one at zero stops.
pub fn reference_radiance() -> f64 {
    let pixel_sr = (2.0 * 25f64.to_radians().tan() / 1080.0).powi(2);
    irradiance_per_ratio() / (POINT_LIGHT * flux_per_luminosity() * pixel_sr)
}

/// Shared manual gain for points and glow; the wireframe calibration is unchanged.
pub fn radiance_gain(nav: &Navigation, pixel_sr: f64) -> f64 {
    if nav.realistic {
        irradiance_per_ratio() * crate::render::exposure::gain(nav.exposure)
    } else {
        POINT_LIGHT * flux_per_luminosity() * pixel_sr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn galaxy_and_bodies_share_manual_exposure_without_view_metering() {
        let mut nav = Navigation::default();
        let gain = radiance_gain(&nav, 1e-6);
        assert_eq!(
            gain,
            irradiance_per_ratio() * crate::render::exposure::gain(0.0)
        );
        assert_eq!(gain, radiance_gain(&nav, 1e-8));
        nav.set_exposure(1.0);
        assert_eq!(radiance_gain(&nav, 1e-6), 2.0 * gain);
        nav.realistic = false;
        assert_eq!(
            radiance_gain(&nav, 1e-6),
            POINT_LIGHT * flux_per_luminosity() * 1e-6
        );
    }
}
