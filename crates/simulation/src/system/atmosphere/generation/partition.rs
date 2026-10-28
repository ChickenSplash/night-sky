//! Well-mixed reservoir partition, with explicit cold-trap mass accounting.
use super::*;
use crate::system::{
    composition::{Budget, CompositionError, Inventory, Material, Reservoir},
    generation::retention::accessible_fraction,
};
use std::f64::consts::PI;

#[cfg(test)]
pub(super) fn atmosphere(
    body: &mut crate::system::Body,
    temperature: f64,
) -> Result<Option<GasAtmosphere>, AtmosphereError> {
    let facts = body.physical();
    let (budget, air) = solve(
        &body.budget,
        temperature,
        facts.retained_mass_kg,
        facts.reference_radius_m,
        facts.radius_reference == space_shared::RadiusReference::OneBar,
    )?;
    body.budget = budget;
    Ok(air)
}

/// Retains positive trace columns. Presentation thresholds belong to the live
/// projection and must not erase gas inventory or zero a reference pressure.
pub(crate) fn solve(
    budget: &Budget,
    temperature: f64,
    mass_kg: f64,
    radius_m: f64,
    one_bar: bool,
) -> Result<(Budget, Option<GasAtmosphere>), AtmosphereError> {
    let available = Inventory::new(&Gas::ALL.map(|gas| {
        let material = Material::from_gas(gas);
        let retained = budget.retained().mass_kg(material);
        let mass = if matches!(gas, Gas::Hydrogen | Gas::Helium) {
            retained
        } else {
            // Protected material is never offered to the atmosphere. Rebuilding
            // from immutable source also makes repeated population idempotent.
            (retained - budget.source().mass_kg(material) * (1.0 - accessible_fraction(material)))
                .max(0.0)
        };
        (material, mass)
    }))
    .map_err(|_| AtmosphereError::InvalidValue("available gas inventory"))?;
    if available.total_mass_kg() == 0.0 {
        return Ok((*budget, None));
    }
    let weight_per_area =
        crate::units::GRAVITATIONAL_CONSTANT * mass_kg / (4.0 * PI * radius_m.powi(4));
    // p_i = C m_i / mu_i. At a boundary C includes spherical shell mass;
    // at one bar C = p / sum(m_i/mu_i). The latter samples an entire envelope
    // without estimating its mass from a thin one-bar column.
    let allocated = |coefficient: f64| -> Inventory {
        Inventory::new(&Gas::ALL.map(|gas| {
            let material = Material::from_gas(gas);
            let cap = vapour_pressure(gas, temperature) * gas.molecular_mass_u() / coefficient;
            let desired = available.mass_kg(material).min(cap);
            let current = budget.reservoir(Reservoir::Gas).mass_kg(material);
            let condensed = budget
                .reservoir(Reservoir::CondensedVolatiles)
                .mass_kg(material);
            // Bracket the gas that can actually be transferred, not tiny
            // unrepresentable vapour which will be left condensed afterwards.
            let represented = if desired > current {
                let amount = desired - current;
                if condensed - amount == condensed || current + amount == current {
                    current
                } else {
                    current + amount
                }
            } else {
                desired
            };
            (material, represented)
        }))
        .expect("finite nonnegative bounded allocations")
    };
    let mole_weight = |inventory: &Inventory| -> f64 {
        Gas::ALL
            .iter()
            .map(|&gas| inventory.mass_kg(Material::from_gas(gas)) / gas.molecular_mass_u())
            .sum()
    };
    let (mut low, mut high) = if one_bar {
        let light_moles = available.mass_kg(Material::Hydrogen) / Gas::Hydrogen.molecular_mass_u()
            + available.mass_kg(Material::Helium) / Gas::Helium.molecular_mass_u();
        (
            ONE_BAR_PA / mole_weight(&available),
            ONE_BAR_PA / light_moles,
        )
    } else {
        let max = Gas::ALL
            .iter()
            .map(|g| g.molecular_mass_u())
            .fold(0.0, f64::max);
        (
            0.0,
            max * weight_per_area / (1.0 - (-EXTENT_SCALE_HEIGHTS).exp()),
        )
    };
    // Fixed bounded bisection, molecular masses give the boundary-column bracket.
    // No unsolved iterative clipping of mole fractions after condensation.
    for _ in 0..64 {
        let coefficient = (low + high) * 0.5;
        let gas = allocated(coefficient);
        let moles = mole_weight(&gas);
        let below = if one_bar {
            coefficient * moles < ONE_BAR_PA
        } else if moles == 0.0 {
            false
        } else {
            let pressure = coefficient * moles;
            let mean_mass = gas.total_mass_kg() / moles * crate::units::ATOMIC_MASS_KG;
            let height = crate::units::BOLTZMANN_J_K * temperature
                / mean_mass
                / (crate::units::GRAVITATIONAL_CONSTANT * mass_kg / radius_m.powi(2));
            let profile = super::super::profile::Profile::new(radius_m, height, pressure);
            pressure * profile.mass_factor(pressure) < gas.total_mass_kg() * weight_per_area
        };
        if below {
            low = coefficient;
        } else {
            high = coefficient;
        }
    }
    // Upper bracket ensures saturation caps are not exceeded by roundoff.
    let gas = allocated(high);
    let mut partitioned = *budget;
    for species in Gas::ALL {
        let material = Material::from_gas(species);
        let current = partitioned.reservoir(Reservoir::Gas).mass_kg(material);
        let desired = gas.mass_kg(material);
        let (from, to, amount) = if desired >= current {
            (
                Reservoir::CondensedVolatiles,
                Reservoir::Gas,
                desired - current,
            )
        } else {
            (
                Reservoir::Gas,
                Reservoir::CondensedVolatiles,
                current - desired,
            )
        };
        match partitioned.transfer(from, to, material, amount) {
            Ok(()) => {}
            // Below reservoir precision: do not manufacture trace gas.
            Err(CompositionError::TransferPrecision) if desired >= current => {}
            Err(_) => return Err(AtmosphereError::InvalidValue("gas reservoir transfer")),
        }
    }
    let gas = partitioned.reservoir(Reservoir::Gas);
    if gas.total_mass_kg() == 0.0 {
        return Ok((partitioned, None));
    }
    let pressure = if one_bar {
        ONE_BAR_PA
    } else {
        let mean_mass = gas.total_mass_kg() / mole_weight(gas) * crate::units::ATOMIC_MASS_KG;
        super::super::profile::Profile::boundary_pressure(
            gas.total_mass_kg(),
            crate::units::GRAVITATIONAL_CONSTANT * mass_kg,
            radius_m,
            crate::units::BOLTZMANN_J_K * temperature / mean_mass,
        )
    };
    let mixture = gas
        .gas_composition()
        .map_err(|_| AtmosphereError::InvalidValue("partitioned gas mixture"))?;
    if pressure <= 0.0 || mixture.is_none() {
        return Ok((partitioned, None));
    }
    let mixture = mixture.unwrap();
    for species in Gas::ALL {
        if pressure * mixture.fraction(species)
            > vapour_pressure(species, temperature) * (1.0 + 1e-9)
        {
            return Err(AtmosphereError::InvalidValue("cold-trap solve residual"));
        }
    }
    if one_bar && pressure > gas.total_mass_kg() * weight_per_area * (1.0 + 1e-9) {
        return Err(AtmosphereError::InvalidValue("one-bar gas supply"));
    }
    let atmosphere = GasAtmosphere::new(
        mixture,
        if one_bar {
            ReferenceLevel::OneBar
        } else {
            ReferenceLevel::Surface
        },
        pressure,
        temperature,
        None,
    )?;
    Ok((partitioned, Some(atmosphere)))
}
