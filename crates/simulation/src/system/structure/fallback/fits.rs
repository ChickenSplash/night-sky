//! Closed-form radius fits retained by the fallback, not a separate structure model.
use super::super::parameters::*;
use crate::{
    system::composition::Material,
    units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use std::f64::consts::PI;

pub(super) fn pure_radius_earth(material: Material, mass_earth: f64) -> Option<f64> {
    let fit = material.properties().condensed_fit?;
    let bounded_mass = mass_earth.min(CONDENSED_FIT_LIMIT_EARTH);
    let scaled_mass = bounded_mass / fit.mass_scale_earth;
    let radius = fit.radius_scale_earth
        * 10_f64.powf(fit.k1 + scaled_mass.log10() / 3.0 - fit.k2 * scaled_mass.powf(fit.k3));
    Some(radius * (mass_earth / bounded_mass).powf(CONDENSED_CONTINUATION_EXPONENT))
}

/// One-bar radius from the envelope fit, limited by the thin-column support
/// radius. None when the H/He cannot hold one bar above the condensed boundary.
pub(super) fn one_bar(
    mass_kg: f64,
    h_he_kg: f64,
    condensed_radius_m: f64,
    irradiation_w_m2: f64,
    age_gyr: f64,
    clamped: &mut bool,
) -> Option<f64> {
    let radius = envelope(
        mass_kg / EARTH_MASS_KG,
        h_he_kg / mass_kg,
        condensed_radius_m,
        irradiation_w_m2,
        age_gyr,
        clamped,
    );
    // A necessary thin-column supply check, not an estimate of the entire
    // giant envelope. It also gives a continuous bare-radius stripping limit.
    let support_radius = (GRAVITATIONAL_CONSTANT * mass_kg * h_he_kg
        / (4.0 * PI * crate::system::atmosphere::ONE_BAR_PA))
        .sqrt()
        .sqrt();
    if support_radius <= condensed_radius_m {
        return None;
    }
    let capacity = support_radius - condensed_radius_m;
    let thickness = radius - condensed_radius_m;
    Some(condensed_radius_m + thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25))
}

fn envelope(
    mass_earth: f64,
    fraction: f64,
    condensed_radius_m: f64,
    irradiation_w_m2: f64,
    age_gyr: f64,
    clamped: &mut bool,
) -> f64 {
    let mass = bounded(mass_earth, ENVELOPE_FIT_MASS_RANGE, clamped);
    let flux = bounded(
        irradiation_w_m2 / EARTH_REFERENCE_FLUX_W_M2,
        ENVELOPE_FIT_FLUX_RANGE,
        clamped,
    );
    let age = bounded(age_gyr, ENVELOPE_FIT_AGE_RANGE, clamped);
    *clamped |= fraction > ENVELOPE_FIT_MAX_FRACTION;
    // Let thickness vanish continuously at complete stripping; do not floor f.
    let thickness = EARTH_REFERENCE_RADIUS_M
        * ENVELOPE_THICKNESS_EARTH
        * mass.powf(ENVELOPE_MASS_EXPONENT)
        * (fraction.min(ENVELOPE_FIT_MAX_FRACTION) / 0.05).powf(ENVELOPE_FRACTION_EXPONENT)
        * flux.powf(ENVELOPE_FLUX_EXPONENT)
        * (age / 5.0).powf(ENVELOPE_AGE_EXPONENT);
    let giant_radius = EARTH_REFERENCE_RADIUS_M
        * GIANT_RADIUS_EARTH
        * (mass_earth / GIANT_REFERENCE_MASS_EARTH).cbrt()
        * ((1.0 + GIANT_REFERENCE_MASS_EARTH / GIANT_TURNOVER_MASS_EARTH)
            / (1.0 + mass_earth / GIANT_TURNOVER_MASS_EARTH))
            .powf(1.0 / 3.0 - GIANT_HIGH_MASS_EXPONENT);
    let giant_radius = giant_radius.max(condensed_radius_m);
    let capacity = giant_radius - condensed_radius_m;
    // Smooth bounded thickness: no unchecked thin-fit extrapolation to giants.
    let thickness = if capacity > 0.0 {
        thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25)
    } else {
        0.0
    };
    let t = ((fraction - GIANT_BLEND_FRACTION_RANGE[0])
        / (GIANT_BLEND_FRACTION_RANGE[1] - GIANT_BLEND_FRACTION_RANGE[0]))
        .clamp(0.0, 1.0);
    let blend = t * t * (3.0 - 2.0 * t);
    (condensed_radius_m + thickness) * (1.0 - blend) + giant_radius * blend
}

fn bounded(value: f64, range: [f64; 2], clamped: &mut bool) -> f64 {
    let result = value.clamp(range[0], range[1]);
    *clamped |= result != value;
    result
}

#[cfg(test)]
mod tests;
