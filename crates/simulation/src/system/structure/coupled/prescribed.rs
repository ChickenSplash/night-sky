//! A generated atmosphere is an imposed boundary, not a partition to iterate.
//! The radial solve elaborates the host under the decided gas and frost load.
use super::*;
use crate::system::atmosphere::ReferenceLevel;

pub(crate) struct Boundary {
    pub host: Inventory,
    pub frost: Inventory,
    pub gas: Inventory,
    pub temperature_k: f64,
    pub pressure_pa: f64,
    pub condensed_radius_m: f64,
    pub surface_radius_m: f64,
}

pub(crate) fn solve(
    boundary: &Boundary,
    protected: &Inventory,
    controls: Controls,
) -> Result<CoupledStructure, Error> {
    controls.radial.validate().map_err(Error::Radial)?;
    if controls.max_iterations > 32 || controls.max_total_eos_evaluations > 16_000_000 {
        return Err(Error::InvalidInput("coupling controls"));
    }
    let deposit = deposit::Deposit::new(boundary.frost)?;
    let atmosphere = boundary
        .gas
        .gas_composition()
        .map_err(Error::Composition)?
        .map(|mix| {
            GasAtmosphere::new(
                mix,
                ReferenceLevel::Surface,
                boundary.pressure_pa,
                boundary.temperature_k,
                None,
            )
        })
        .transpose()
        .map_err(Error::Atmosphere)?;
    let gm = GRAVITATIONAL_CONSTANT * (boundary.host.total_mass_kg() + deposit.mass_kg());
    // Validate on the geometry that decided the column, never on the radial
    // solver's slightly different interpolation-check radius.
    column::validate(
        &boundary.gas,
        atmosphere.as_ref(),
        gm,
        boundary.surface_radius_m,
        false,
    )?;
    let load = boundary.pressure_pa
        + deposit.load_pa(boundary.host.total_mass_kg(), boundary.condensed_radius_m);
    let inventory = Inventory::new(
        &Material::ALL.map(|m| (m, boundary.host.mass_kg(m) - protected.mass_kg(m))),
    )
    .map_err(Error::Composition)?;
    let radial = dry::Controls {
        max_eos_evaluations: controls
            .radial
            .max_eos_evaluations
            .min(controls.max_total_eos_evaluations),
        ..controls.radial
    };
    let condensed = if inventory.mass_kg(Material::Water) > 0.0 {
        CondensedStructure::Water(
            water::protected::solve_seeded(
                &inventory,
                protected,
                boundary.temperature_k,
                load,
                radial,
                None,
            )
            .map_err(Error::Radial)?,
        )
    } else {
        CondensedStructure::Dry(
            dry::protected::solve_loaded_seeded(&inventory, protected, load, radial, None)
                .map_err(Error::Radial)?,
        )
    };
    let diagnostics = Diagnostics {
        iterations: 1,
        total_eos_evaluations: condensed.diagnostics().work.eos_evaluations,
        interface_residual_pa: condensed.outer().pressure_pa - load,
        partition_relative_residual: 0.0,
    };
    Ok(CoupledStructure {
        condensed,
        envelope: None,
        deposit,
        gas: boundary.gas,
        atmosphere,
        surface_temperature_k: boundary.temperature_k,
        optical_depth: None,
        diagnostics,
    })
}
