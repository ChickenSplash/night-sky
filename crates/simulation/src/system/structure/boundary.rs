//! Inventory-bounded condensed-boundary correction, after escape.
//! Never feeds a class label or an inferred column mass back into formation.
use super::*;
use crate::system::atmosphere::{
    AtmosphereError, GasAtmosphere, MAX_THIN_EXTENT_RATIO, ReferenceLevel, generation::partition,
};

use super::parameters::{MAX_BOUNDARY_GAS_FRACTION, MAX_BOUNDARY_WATER_FRACTION};

/// Resolve using the actual generated column temperature. The original algebraic
/// structure remains the escape/formation surrogate; this is its environmental
/// reference correction. Rebuilding supply from source makes this idempotent.
pub(crate) fn resolve(
    budget: &Budget,
    flux: f64,
    age: f64,
    temperature: f64,
) -> Result<(Structure, Budget, Option<GasAtmosphere>), AtmosphereError> {
    if !temperature.is_finite() || temperature <= 0.0 {
        return Err(AtmosphereError::InvalidValue("boundary column temperature"));
    }
    let mut structure = derive(budget, flux, age)
        .map_err(|_| AtmosphereError::InvalidValue("boundary reference structure"))?;
    let mass = structure.mass_kg();
    let radius = structure.condensed_radius_m();
    // A pure gas planet has no condensed proxy to test.
    if radius <= 0.0 {
        return unresolved(structure, budget, temperature);
    }
    let water_fraction = budget.retained().mass_kg(Material::Water) / mass;
    let heavy_mass = mass * (1.0 - structure.h_he_mass_fraction());
    // These bounds do not depend on cold trapping. Do not run a fictional
    // boundary-column solve at the heavy proxy inside an obvious deep envelope.
    let inventory_score = (structure.h_he_mass_fraction() / MAX_BOUNDARY_GAS_FRACTION)
        .max(water_fraction / MAX_BOUNDARY_WATER_FRACTION)
        .max(heavy_mass / EARTH_MASS_KG / parameters::CONDENSED_FIT_LIMIT_EARTH);
    if inventory_score >= 2.0 {
        return unresolved(structure, budget, temperature);
    }
    let (boundary_budget, boundary_air) =
        partition::solve(budget, temperature, mass, radius, false)?;
    let extent = boundary_air
        .map(|air| air.column(GRAVITATIONAL_CONSTANT * mass, radius))
        .transpose()?
        .map_or(0.0, |column| column.extent_m);
    let gas_fraction = boundary_budget.reservoir(Reservoir::Gas).total_mass_kg() / mass;
    // Geometric thickness limits the accuracy of the column, not the existence
    // of the condensed body. An extended light atmosphere must not turn a
    // rock-dominated inventory into a groundless one-bar envelope.
    let score = (gas_fraction / MAX_BOUNDARY_GAS_FRACTION).max(inventory_score);
    if score <= 1.0 {
        set_radius(&mut structure, radius, budget);
        structure.reference = condensed_reference(budget.retained().mass_kg(Material::Water));
        structure
            .approximations
            .push(if extent / radius <= MAX_THIN_EXTENT_RATIO {
                Approximation::ThinBoundaryColumn
            } else {
                Approximation::ExtendedBoundaryColumn
            });
        return Ok((structure, boundary_budget, boundary_air));
    }
    // Non-envelope legacy proxies cannot acquire a fictional one-bar level.
    // Retain their existing reference, explicitly outside the supported domain.
    if score >= 2.0 || structure.reference != RadiusReference::OneBar {
        return unresolved(structure, budget, temperature);
    }
    let (legacy_budget, legacy_air) =
        partition::solve(budget, temperature, mass, structure.radius_m(), true)?;
    let Some(boundary_air) = boundary_air else {
        return unresolved(structure, budget, temperature);
    };
    let Some(height) = boundary_air.pressure_level_altitude_m(
        super::super::atmosphere::ONE_BAR_PA,
        GRAVITATIONAL_CONSTANT * mass,
        radius,
    )?
    else {
        return unresolved(structure, budget, temperature);
    };
    // Change reference from boundary to one bar at score=1, then smoothly blend
    // the SAME physical pressure level and reservoir allocations to the deep
    // surrogate by score=2. Never interpolate a ground radius with a one-bar
    // radius as though they described the same level.
    let t = score - 1.0;
    let weight = t * t * (3.0 - 2.0 * t);
    let one_bar_radius = radius + height;
    let blended_radius = one_bar_radius + weight * (structure.radius_m() - one_bar_radius);
    let mut blended = boundary_budget;
    for material in Material::ALL {
        let from_mass = boundary_budget.reservoir(Reservoir::Gas).mass_kg(material);
        let to_mass = legacy_budget.reservoir(Reservoir::Gas).mass_kg(material);
        let desired = from_mass + weight * (to_mass - from_mass);
        let (from, to, amount) = if desired >= from_mass {
            (
                Reservoir::CondensedVolatiles,
                Reservoir::Gas,
                desired - from_mass,
            )
        } else {
            (
                Reservoir::Gas,
                Reservoir::CondensedVolatiles,
                from_mass - desired,
            )
        };
        match blended.transfer(from, to, material, amount) {
            Ok(()) | Err(super::super::composition::CompositionError::TransferPrecision) => {}
            Err(_) => {
                return Err(AtmosphereError::InvalidValue(
                    "boundary transition inventory",
                ));
            }
        }
    }
    let composition = blended
        .reservoir(Reservoir::Gas)
        .gas_composition()
        .map_err(|_| AtmosphereError::InvalidValue("boundary transition mixture"))?
        .ok_or(AtmosphereError::InvalidValue("boundary transition supply"))?;
    let air = GasAtmosphere::new(
        composition,
        ReferenceLevel::OneBar,
        super::super::atmosphere::ONE_BAR_PA,
        temperature,
        None,
    )?;
    debug_assert!(legacy_air.is_some());
    set_radius(&mut structure, blended_radius, budget);
    structure
        .approximations
        .push(Approximation::BoundaryReferenceTransition);
    structure
        .approximations
        .push(Approximation::DeepBoundaryUnresolved);
    Ok((structure, blended, Some(air)))
}

fn unresolved(
    mut structure: Structure,
    budget: &Budget,
    temperature: f64,
) -> Result<(Structure, Budget, Option<GasAtmosphere>), AtmosphereError> {
    let (budget, air) = partition::solve(
        budget,
        temperature,
        structure.mass_kg(),
        structure.radius_m(),
        structure.reference == RadiusReference::OneBar,
    )?;
    structure
        .approximations
        .push(Approximation::DeepBoundaryUnresolved);
    Ok((structure, budget, air))
}

fn set_radius(structure: &mut Structure, radius: f64, budget: &Budget) {
    structure.radius_m = radius;
    let fraction = structure.h_he_mass_fraction();
    let heavy = structure.mass_kg() * (1.0 - fraction);
    let metal_share = budget.retained().mass_kg(Material::Metal) / heavy;
    let heavy_inertia =
        parameters::HEAVY_INERTIA_BASE - parameters::METAL_INERTIA_REDUCTION * metal_share;
    structure.inertia_factor =
        ((1.0 - fraction) * heavy_inertia * (structure.condensed_radius_m() / radius).powi(2)
            + fraction * parameters::ENVELOPE_INERTIA)
            .clamp(parameters::MIN_INERTIA, parameters::HEAVY_INERTIA_BASE);
}

#[cfg(test)]
mod tests;
