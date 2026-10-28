use super::{Approximation, StructureError, parameters::*};
use crate::{
    system::composition::{Inventory, Material},
    units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use std::f64::consts::PI;

pub(super) fn condensed(
    inventory: &Inventory,
    warnings: &mut Vec<Approximation>,
) -> Result<f64, StructureError> {
    // Sum heavy constituents directly, not total minus H/He (cancellation).
    let heavy_kg: f64 = Material::ALL
        .into_iter()
        .filter(|m| !matches!(m, Material::Hydrogen | Material::Helium))
        .map(|m| inventory.mass_kg(m))
        .sum();
    if heavy_kg == 0.0 {
        return Ok(0.0);
    }
    let mass_earth = heavy_kg / EARTH_MASS_KG;
    if !super::in_domain(mass_earth, MIN_MASS_EARTH, MAX_CONDENSED_MASS_EARTH) {
        return Err(StructureError::OutsideDomain("condensed mass"));
    }
    if mass_earth > CONDENSED_FIT_LIMIT_EARTH {
        warnings.push(Approximation::CondensedMassContinuation);
    }
    let mut volume = 0.0;
    let mut constituents = 0;
    let mut trace_kg = 0.0;
    for material in Material::ALL {
        let mass = inventory.mass_kg(material);
        if mass == 0.0 || matches!(material, Material::Hydrogen | Material::Helium) {
            continue;
        }
        let proxy = if material.properties().condensed_fit.is_some() {
            material
        } else {
            // Explicit limited proxy, not a generic density for arbitrary materials.
            if material.properties().gas.is_none() {
                return Err(StructureError::UnsupportedMaterial(material));
            }
            trace_kg += mass;
            Material::Water
        };
        volume += mass / heavy_kg * pure_radius_earth(proxy, mass_earth)?.powi(3);
        constituents += 1;
    }
    if trace_kg > MAX_TRACE_VOLATILE_FRACTION * heavy_kg {
        return Err(StructureError::OutsideDomain(
            "secondary volatile fraction of heavy material",
        ));
    }
    if constituents > 1 {
        warnings.push(Approximation::CompressedVolumeMixing);
    }
    if trace_kg > 0.0 {
        warnings.push(Approximation::TraceVolatilesAsWater);
    }
    if inventory.mass_kg(Material::Water) > 0.0 {
        warnings.push(Approximation::ColdWaterProxy);
    }
    Ok(EARTH_REFERENCE_RADIUS_M * volume.cbrt())
}

pub(super) fn pure_radius_earth(
    material: Material,
    mass_earth: f64,
) -> Result<f64, StructureError> {
    let fit = material
        .properties()
        .condensed_fit
        .ok_or(StructureError::UnsupportedMaterial(material))?;
    let bounded_mass = mass_earth.min(CONDENSED_FIT_LIMIT_EARTH);
    let scaled_mass = bounded_mass / fit.mass_scale_earth;
    let radius = fit.radius_scale_earth
        * 10_f64.powf(fit.k1 + scaled_mass.log10() / 3.0 - fit.k2 * scaled_mass.powf(fit.k3));
    Ok(radius * (mass_earth / bounded_mass).powf(CONDENSED_CONTINUATION_EXPONENT))
}

/// One-bar radius from the envelope fit, limited by the thin-column support
/// radius. None when the H/He cannot hold one bar above the condensed boundary.
pub(super) fn one_bar(
    mass_kg: f64,
    h_he_kg: f64,
    condensed_radius_m: f64,
    irradiation_w_m2: f64,
    age_gyr: f64,
    warnings: &mut Vec<Approximation>,
) -> Option<f64> {
    let radius = envelope(
        mass_kg / EARTH_MASS_KG,
        h_he_kg / mass_kg,
        condensed_radius_m,
        irradiation_w_m2,
        age_gyr,
        warnings,
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
    warnings: &mut Vec<Approximation>,
) -> f64 {
    let mass = bounded(mass_earth, ENVELOPE_FIT_MASS_RANGE, warnings);
    let flux = bounded(
        irradiation_w_m2 / EARTH_REFERENCE_FLUX_W_M2,
        ENVELOPE_FIT_FLUX_RANGE,
        warnings,
    );
    let age = bounded(age_gyr, ENVELOPE_FIT_AGE_RANGE, warnings);
    if fraction > ENVELOPE_FIT_MAX_FRACTION
        && !warnings.contains(&Approximation::EnvelopeFitInputsBounded)
    {
        warnings.push(Approximation::EnvelopeFitInputsBounded);
    }
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

fn bounded(value: f64, range: [f64; 2], warnings: &mut Vec<Approximation>) -> f64 {
    let result = value.clamp(range[0], range[1]);
    if result != value && !warnings.contains(&Approximation::EnvelopeFitInputsBounded) {
        warnings.push(Approximation::EnvelopeFitInputsBounded);
    }
    result
}
