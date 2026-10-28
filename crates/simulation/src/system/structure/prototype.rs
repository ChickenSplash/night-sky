//! Headless generated-budget entry point. No legacy fallback or runtime switch.
use super::coupled::{
    self, CondensedStructure, Controls, CoupledStructure, Error, Request, Temperature,
};
use super::water;
use crate::system::{
    composition::{Budget, Inventory},
    generation::retention::accessible_fraction,
    materials::{Material, water::Phase},
};
use space_shared::PlanetEnvironment;

pub const MODEL_ID: &str = "generated_layered_rock_water_v1";

#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub structure: CoupledStructure,
    /// Original formation supply, including generation's prior losses.
    pub source: Inventory,
    /// All losses: generation's escape plus this solve's blow-off.
    pub escaped: Inventory,
    /// Subset of `escaped`, useful for diagnosing the additional blow-off.
    pub blowoff: Inventory,
    /// Audit-only supply, already owned by the returned interior regions.
    pub protected: Inventory,
}

impl Solved {
    pub fn model_id(&self) -> &'static str {
        if self.structure.envelope().is_some() {
            super::envelope::MODEL_ID
        } else {
            MODEL_ID
        }
    }

    /// What sits on top: a frost deposit, else the outermost condensed region.
    pub fn surface(&self) -> &'static str {
        if self.structure.envelope().is_some() {
            return "envelope";
        }
        if self.structure.deposit_thickness_m() > 0.0 {
            return "frost";
        }
        match self.structure.condensed() {
            CondensedStructure::Dry(_) => "rock",
            CondensedStructure::Water(s) => match s.regions().last().unwrap().phase() {
                water::PhaseKnowledge::Water(Phase::Liquid) => "ocean",
                water::PhaseKnowledge::Water(Phase::IceIh) => "ice",
                water::PhaseKnowledge::Water(_) => "water coexistence",
                water::PhaseKnowledge::EffectiveHighPressureIce => "deep ice",
                water::PhaseKnowledge::Unresolved => "rock",
                water::PhaseKnowledge::UnresolvedHotWater => "unresolved hot water",
                water::PhaseKnowledge::UnresolvedCompression => "unresolved compressed water",
            },
        }
    }
}

/// Generation's existing retained budget is authoritative. Its reservoir split
/// is not: repartition from immutable supply, without replaying retention history.
/// All water exchanges with the atmosphere; only other secondaries are entrained.
pub fn solve(
    budget: &Budget,
    environment: &PlanetEnvironment,
    controls: Controls,
) -> Result<Solved, Error> {
    let error = match solve_thin(budget, environment, controls) {
        Ok(solved) => return Ok(solved),
        Err(error) => error,
    };
    if !matches!(
        error,
        Error::GasMassOutsideDomain
            | Error::DenseGasUnsupported
            | Error::Radial(super::dry::SolveError::OutsideMassDomain(_))
    ) || budget.retained().mass_kg(Material::Hydrogen)
        + budget.retained().mass_kg(Material::Helium)
        == 0.0
    {
        return Err(error);
    }
    let protected = Inventory::new(&Material::ALL.map(|m| {
        let mass = if matches!(
            m,
            Material::CarbonDioxide
                | Material::Nitrogen
                | Material::Methane
                | Material::Oxygen
                | Material::Argon
        ) {
            (budget.source().mass_kg(m) * (1.0 - accessible_fraction(m)))
                .min(budget.retained().mass_kg(m))
        } else {
            0.0
        };
        (m, mass)
    }))
    .map_err(Error::Composition)?;
    let (interior, envelope) = super::envelope::solve(
        budget.retained(),
        &protected,
        environment.equilibrium_temperature_k,
        100.0,
        controls,
    )
    .map_err(Error::Envelope)?;
    Ok(Solved {
        structure: CoupledStructure::from_envelope(interior, envelope),
        source: *budget.source(),
        escaped: *budget.escaped(),
        blowoff: Inventory::default(),
        protected,
    })
}

#[cfg(test)]
pub(super) fn thin_route_diagnostic(
    budget: &Budget,
    environment: &PlanetEnvironment,
) -> Result<Solved, Error> {
    solve_thin(budget, environment, Controls::default())
}

fn solve_thin(
    budget: &Budget,
    environment: &PlanetEnvironment,
    controls: Controls,
) -> Result<Solved, Error> {
    let retained = budget.retained();
    let equilibrium = environment.equilibrium_temperature_k;
    if !equilibrium.is_finite() || equilibrium <= 0.0 {
        return Err(Error::InvalidInput("equilibrium temperature"));
    }
    let protected = Inventory::new(&Material::ALL.map(|m| {
        let secondary = matches!(
            m,
            Material::CarbonDioxide
                | Material::Nitrogen
                | Material::Methane
                | Material::Oxygen
                | Material::Argon
        );
        let mass = if secondary {
            (budget.source().mass_kg(m) * (1.0 - accessible_fraction(m))).min(retained.mass_kg(m))
        } else {
            0.0
        };
        (m, mass)
    }))
    .map_err(Error::Composition)?;
    let exchangeable = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if matches!(m, Material::Metal | Material::Silicate) {
                0.0
            } else {
                retained.mass_kg(m) - protected.mass_kg(m)
            },
        )
    }))
    .map_err(Error::Composition)?;
    // H/He never condenses. This is the column's mass guard, not a new cutoff.
    if (retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium))
        / retained.total_mass_kg()
        > coupled::MAX_GAS_MASS_FRACTION
    {
        return Err(Error::GasMassOutsideDomain);
    }
    let escaping = coupled::escape::solve_protected(
        &Request {
            retained: *retained,
            exchangeable,
            temperature: Temperature::Greenhouse {
                equilibrium_k: equilibrium.max(coupled::MIN_COLUMN_TEMPERATURE_K),
            },
        },
        &protected,
        controls,
    )?;
    let escaped = budget
        .escaped()
        .checked_add(&escaping.escaped)
        .map_err(Error::Composition)?;
    let allocated = escaping
        .structure
        .retained_inventory()
        .checked_add(&escaped)
        .map_err(Error::Composition)?;
    for m in Material::ALL {
        if (allocated.mass_kg(m) - budget.source().mass_kg(m)).abs()
            > 1e-10 * budget.source().mass_kg(m)
        {
            return Err(Error::InvalidClosure(m));
        }
    }
    Ok(Solved {
        structure: escaping.structure,
        source: *budget.source(),
        escaped,
        blowoff: escaping.escaped,
        protected,
    })
}

#[cfg(test)]
mod tests;
