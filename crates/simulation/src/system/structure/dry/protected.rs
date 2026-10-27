//! Protected secondary origins entrained in cold dry hosts, never exposed shells.
//! This mechanical approximation supplies no chemistry, temperature or phase law.

use super::{Controls, DryStructure, SolveError, radial, solve_with_policy, validate};
use crate::{
    system::{
        composition::Inventory,
        materials::{Material, cold},
    },
    units::EARTH_MASS_KG,
};

pub const MODEL_ID: &str = "entrained_secondary_host_volume_v1";
pub const MAX_TRACE_FRACTION: f64 = 0.00301;

/// Only `protected` secondary supply is accepted. Exchangeable supply has no
/// allocation in this API and must not be passed as protected material. The load
/// carries no atmosphere. All returned region inventories retain origin identity.
pub fn solve(
    host: &Inventory,
    protected: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
) -> Result<DryStructure, SolveError> {
    solve_with_density_ratio(host, protected, outer_pressure_pa, controls, 1.0)
}

// Lambda is an audit variation, not a caller-selectable generated parameter.
fn solve_with_density_ratio(
    host: &Inventory,
    protected: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    lambda: f64,
) -> Result<DryStructure, SolveError> {
    solve_internal(
        host,
        protected,
        outer_pressure_pa,
        controls,
        lambda,
        radial::Policy::default(),
        None,
    )
}

pub(crate) fn solve_loaded_seeded(
    host: &Inventory,
    protected: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    seed: Option<f64>,
) -> Result<DryStructure, SolveError> {
    solve_internal(
        host,
        protected,
        outer_pressure_pa,
        controls,
        1.0,
        radial::Policy {
            boundary_tolerance_pa: Some(super::super::water::BOUNDARY_TOLERANCE_PA),
            pressure_error_scale_pa: Some(1e8),
            ..radial::Policy::default()
        },
        seed,
    )
}

fn solve_internal(
    host: &Inventory,
    protected: &Inventory,
    outer_pressure_pa: f64,
    controls: Controls,
    lambda: f64,
    policy: radial::Policy,
    seed: Option<f64>,
) -> Result<DryStructure, SolveError> {
    controls.validate()?;
    for material in Material::ALL {
        if host.mass_kg(material) > 0.0 && !matches!(material, Material::Metal | Material::Silicate)
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
    let host_mass = host.total_mass_kg();
    if host_mass <= 0.0 {
        return Err(SolveError::InvalidInput("positive dry host required"));
    }
    let requested = host
        .checked_add(protected)
        .map_err(|_| SolveError::InvalidInput("combined inventory"))?;
    let total = requested.total_mass_kg();
    let f = protected.total_mass_kg() / total;
    if f > MAX_TRACE_FRACTION {
        return Err(SolveError::InvalidInput("protected trace fraction"));
    }
    if !super::super::in_domain(total / EARTH_MASS_KG, 0.0001, 20.0) {
        return Err(SolveError::OutsideMassDomain(total));
    }
    // Internal coordinates label host EOS routes, not constituent origins.
    // Each route integrates its FULL parcel mass with the parcel density, so
    // volume, pressure, enclosed gravity and inertia include the trace supply.
    let parcels = host
        .scaled(total / host_mass)
        .map_err(|_| SolveError::InvalidInput("host parcel mass"))?;
    let volume_factor = 1.0 - f + f / lambda;
    let mut body = solve_with_policy(
        &parcels,
        outer_pressure_pa,
        controls,
        |material, pressure| cold::density_kg_m3(material, pressure).map(|rho| rho / volume_factor),
        policy,
        seed,
    )?;
    for region in &mut body.regions {
        let material = if region.inventory.mass_kg(Material::Metal) > 0.0 {
            Material::Metal
        } else {
            Material::Silicate
        };
        let weight = host.mass_kg(material) / host_mass;
        let traces = protected
            .scaled(weight)
            .map_err(|_| SolveError::InvalidInput("trace allocation"))?;
        for origin in Material::ALL {
            if protected.mass_kg(origin) > 0.0 && traces.mass_kg(origin) == 0.0 {
                return Err(SolveError::InvalidClosure {
                    material: origin,
                    quantity: "unrepresentable trace allocation",
                });
            }
        }
        region.inventory = Inventory::new(&[(material, host.mass_kg(material))])
            .unwrap()
            .checked_add(&traces)
            .map_err(|_| SolveError::InvalidInput("region inventory"))?;
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
