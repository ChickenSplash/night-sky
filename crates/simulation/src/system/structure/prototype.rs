//! Headless generated-budget entry point. Refusals take the labelled
//! `fallback` structure, never the legacy model.
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

/// What generation decided synchronously and the layered solve must keep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    /// A condensed surface under a thin column at this greenhouse temperature.
    /// Blow-off has already been taken from the budget.
    Thin { surface_temperature_k: f64 },
    /// A self-gravitating H/He envelope.
    Envelope,
    /// The synchronous surface model refused: so does the layered solve.
    Refused(Error),
    /// Nothing was decided (headless callers): the thin route solves its own
    /// greenhouse balance, then the envelope.
    Undecided,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub structure: CoupledStructure,
    /// Original formation supply, including generation's prior losses.
    pub source: Inventory,
    /// Generation's losses, unchanged: this solve removes nothing.
    pub escaped: Inventory,
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
        if self
            .structure
            .envelope()
            .is_some_and(|e| e.one_bar_radius_m().is_some())
        {
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

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Layered(Box<Solved>),
    /// The layered solver refused; carries its reason.
    Fallback(Box<super::fallback::Fallback>),
}

/// Every generated body gets a result: layered when supported, else the
/// explicitly labelled fallback structure. Never a legacy substitution.
pub fn resolve(
    budget: &Budget,
    environment: &PlanetEnvironment,
    decision: Decision,
    controls: Controls,
) -> Outcome {
    match solve_decided(budget, environment, decision, controls) {
        Ok(solved) => Outcome::Layered(Box::new(solved)),
        Err(reason) => Outcome::Fallback(Box::new(super::fallback::solve(
            budget,
            environment,
            reason,
        ))),
    }
}

/// Generation's retained budget and surface decision are authoritative. The
/// reservoir split is not: repartition from immutable supply, without
/// replaying retention history. No gas escapes here.
pub fn solve_decided(
    budget: &Budget,
    environment: &PlanetEnvironment,
    decision: Decision,
    controls: Controls,
) -> Result<Solved, Error> {
    match decision {
        Decision::Thin { .. } => return solve_thin(budget, environment, decision, controls),
        Decision::Envelope => return solve_envelope(budget, environment, controls),
        Decision::Refused(error) => return Err(error),
        Decision::Undecided => {}
    }
    let error = match solve_thin(budget, environment, decision, controls) {
        Ok(solved) => return Ok(solved),
        Err(error) => error,
    };
    if routes_to_envelope(&error, budget.retained()) {
        solve_envelope(budget, environment, controls)
    } else {
        Err(error)
    }
}

/// Headless entry with nothing decided: the thin route solves its own
/// greenhouse balance. Still no escape; an unbound column is refused.
pub fn solve(
    budget: &Budget,
    environment: &PlanetEnvironment,
    controls: Controls,
) -> Result<Solved, Error> {
    solve_decided(budget, environment, Decision::Undecided, controls)
}

/// A thin-column refusal that an H/He envelope can represent instead.
pub(crate) fn routes_to_envelope(error: &Error, retained: &Inventory) -> bool {
    matches!(
        error,
        Error::GasMassOutsideDomain
            | Error::DenseGasUnsupported
            | Error::Radial(super::dry::SolveError::OutsideMassDomain(_))
    ) && retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium) > 0.0
}

/// Deep secondaries never offered to the atmosphere or to escape.
fn protected(source: &Inventory, retained: &Inventory) -> Result<Inventory, Error> {
    Inventory::new(&Material::ALL.map(|m| {
        let mass = if matches!(
            m,
            Material::CarbonDioxide
                | Material::Nitrogen
                | Material::Methane
                | Material::Oxygen
                | Material::Argon
        ) {
            (source.mass_kg(m) * (1.0 - accessible_fraction(m))).min(retained.mass_kg(m))
        } else {
            0.0
        };
        (m, mass)
    }))
    .map_err(Error::Composition)
}

fn solve_envelope(
    budget: &Budget,
    environment: &PlanetEnvironment,
    controls: Controls,
) -> Result<Solved, Error> {
    let protected = protected(budget.source(), budget.retained())?;
    let (interior, envelope) = super::profile::time(super::profile::Stage::Envelope, || {
        super::envelope::solve(
            budget.retained(),
            &protected,
            environment.equilibrium_temperature_k,
            super::envelope::intrinsic_k(budget.retained().total_mass_kg(), environment.age_gyr),
            controls,
        )
    })
    .map_err(Error::Envelope)?;
    Ok(Solved {
        structure: CoupledStructure::from_envelope(interior, envelope),
        source: *budget.source(),
        escaped: *budget.escaped(),
        protected,
    })
}

#[cfg(test)]
pub(super) fn thin_route_diagnostic(
    budget: &Budget,
    environment: &PlanetEnvironment,
) -> Result<Solved, Error> {
    solve_thin(
        budget,
        environment,
        Decision::Undecided,
        Controls::default(),
    )
}

/// The one exposure rule, shared by generation's synchronous surface decision
/// and the layered thin route: all water and every unprotected secondary
/// exchange with the column, which takes the greenhouse surface temperature.
pub(crate) fn thin_request(
    source: &Inventory,
    retained: &Inventory,
    environment: &PlanetEnvironment,
) -> Result<(Request, Inventory), Error> {
    let equilibrium = environment.equilibrium_temperature_k;
    if !equilibrium.is_finite() || equilibrium <= 0.0 {
        return Err(Error::InvalidInput("equilibrium temperature"));
    }
    let protected = protected(source, retained)?;
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
    Ok((
        Request {
            retained: *retained,
            exchangeable,
            temperature: Temperature::Greenhouse {
                equilibrium_k: equilibrium.max(coupled::MIN_COLUMN_TEMPERATURE_K),
            },
        },
        protected,
    ))
}

fn solve_thin(
    budget: &Budget,
    environment: &PlanetEnvironment,
    decision: Decision,
    controls: Controls,
) -> Result<Solved, Error> {
    let (mut request, protected) = thin_request(budget.source(), budget.retained(), environment)?;
    if let Decision::Thin {
        surface_temperature_k,
    } = decision
    {
        request.temperature = Temperature::Prescribed {
            column_k: surface_temperature_k,
            water_k: Some(surface_temperature_k),
        };
    }
    let structure = super::profile::time(super::profile::Stage::Coupled, || {
        coupled::solve_protected(&request, &protected, controls)
    })?;
    let allocated = structure
        .retained_inventory()
        .checked_add(budget.escaped())
        .map_err(Error::Composition)?;
    for m in Material::ALL {
        if (allocated.mass_kg(m) - budget.source().mass_kg(m)).abs()
            > 1e-10 * budget.source().mass_kg(m)
        {
            return Err(Error::InvalidClosure(m));
        }
    }
    Ok(Solved {
        structure,
        source: *budget.source(),
        escaped: *budget.escaped(),
        protected,
    })
}

#[cfg(test)]
mod tests;
