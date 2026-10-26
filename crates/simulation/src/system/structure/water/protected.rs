//! Protected secondary origins in water-bearing hosts. No solubility or phase law
//! is implied for traces; the caller must not supply exchangeable material here.

use super::{Controls, SolveError, WaterStructure, solve_with_volume_factor, validate};
use crate::{
    system::{composition::Inventory, materials::Material},
    units::EARTH_MASS_KG,
};

pub use super::super::dry::protected::{MAX_TRACE_FRACTION, MODEL_ID};

/// The host contains metal/silicate/protected water and must contain water.
/// Trace supply is separately protected CO₂/N₂/CH₄/O₂/Ar. The imposed load
/// carries no atmosphere. Water phase events describe the host, not the traces.
pub fn solve(
    host: &Inventory,
    protected: &Inventory,
    water_temperature_k: f64,
    outer_pressure_pa: f64,
    controls: Controls,
) -> Result<WaterStructure, SolveError> {
    solve_with_density_ratio(
        host,
        protected,
        water_temperature_k,
        outer_pressure_pa,
        controls,
        1.0,
    )
}

// Lambda is a sensitivity audit, never a runtime material parameter.
fn solve_with_density_ratio(
    host: &Inventory,
    protected: &Inventory,
    water_temperature_k: f64,
    outer_pressure_pa: f64,
    controls: Controls,
    lambda: f64,
) -> Result<WaterStructure, SolveError> {
    controls.validate()?;
    for material in Material::ALL {
        if host.mass_kg(material) > 0.0
            && !matches!(
                material,
                Material::Metal | Material::Silicate | Material::Water
            )
        {
            return Err(SolveError::UnsupportedMaterial(material));
        }
        if protected.mass_kg(material) > 0.0
            && !matches!(
                material,
                Material::CarbonDioxide
                    | Material::Nitrogen
                    | Material::Methane
                    | Material::Oxygen
                    | Material::Argon
            )
        {
            return Err(SolveError::UnsupportedMaterial(material));
        }
    }
    if host.mass_kg(Material::Water) == 0.0 {
        return Err(SolveError::InvalidInput("positive water host required"));
    }
    let requested = host
        .checked_add(protected)
        .map_err(|_| SolveError::InvalidInput("combined inventory"))?;
    let total = requested.total_mass_kg();
    let host_mass = host.total_mass_kg();
    let f = protected.total_mass_kg() / total;
    if f > MAX_TRACE_FRACTION {
        return Err(SolveError::InvalidInput("protected trace fraction"));
    }
    if !super::super::in_domain(total / EARTH_MASS_KG, 0.0001, 20.0) {
        return Err(SolveError::OutsideMassDomain(total));
    }
    // These are full parcel masses labelled by host EOS route, not new origins.
    let scale = total / host_mass;
    let parcels = host
        .scaled(scale)
        .map_err(|_| SolveError::InvalidInput("host parcel mass"))?;
    let mut body = solve_with_volume_factor(
        &parcels,
        water_temperature_k,
        outer_pressure_pa,
        controls,
        1.0 - f + f / lambda,
    )?;
    if protected.total_mass_kg() > 0.0 {
        for region in &mut body.regions {
            // Pressure events already split the FULL water parcel. Decompose each
            // resulting segment, not the original unsplit water shell.
            let parcel_mass = region.inventory().total_mass_kg();
            let weight = parcel_mass / parcels.total_mass_kg();
            let host_allocation = region
                .inventory()
                .scaled(1.0 / scale)
                .map_err(|_| SolveError::InvalidInput("host allocation"))?;
            let traces = protected
                .scaled(weight)
                .map_err(|_| SolveError::InvalidInput("trace allocation"))?;
            for origin in Material::ALL {
                if (protected.mass_kg(origin) > 0.0 && traces.mass_kg(origin) == 0.0)
                    || (region.inventory().mass_kg(origin) > 0.0
                        && host_allocation.mass_kg(origin) == 0.0)
                {
                    return Err(SolveError::InvalidClosure {
                        material: origin,
                        quantity: "unrepresentable protected allocation",
                    });
                }
            }
            region.profile.inventory = host_allocation
                .checked_add(&traces)
                .map_err(|_| SolveError::InvalidInput("region inventory"))?;
        }
    }
    body.model_id = MODEL_ID;
    validate(&body, &requested)?;
    let allocated = body.retained_inventory();
    for origin in Material::ALL {
        let expected = requested.mass_kg(origin);
        if (allocated.mass_kg(origin) - expected).abs() > 1e-12 * expected {
            return Err(SolveError::InvalidClosure {
                material: origin,
                quantity: "protected inventory",
            });
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests;
