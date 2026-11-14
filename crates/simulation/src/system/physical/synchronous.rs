//! Complete synchronous result and its atmosphere. Generation installs these
//! facts together as the live projections; nothing else may supply them.
use crate::system::{
    atmosphere::{generation::MIN_PRESSURE_PA, optics::boundary::OpticalColumn},
    composition::{Budget, Inventory},
    structure::{fallback, prototype::Decision},
};
use space_shared::{PhysicalRevision, PlanetEnvironment, PlanetResult};

pub struct PreparedInitial {
    pub result: PlanetResult,
    /// The same reservoir split as `result`, for retention re-evaluation.
    pub budget: Budget,
    /// Includes positive trace columns. See `live_environment`.
    pub environment: PlanetEnvironment,
    /// What the layered solve must keep.
    pub decision: Decision,
    /// Stripped from an unbound column; already counted in the budget's escape.
    pub blowoff: Inventory,
}

impl PreparedInitial {
    /// Live projection. Columns below the population's presentation threshold
    /// are omitted for rendering/navigation parity with the previous live
    /// population; their gas and pressure remain in the result record.
    pub fn live_environment(&self) -> PlanetEnvironment {
        let mut environment = self.environment.clone();
        environment
            .atmosphere
            .take_if(|air| air.pressure_pa < MIN_PRESSURE_PA);
        environment
    }
}

/// Decide the surface on the retention fallback: temperature, gas, frost,
/// blow-off and the layered route. No radial structure or EOS solve is used.
pub fn initial_with_atmosphere(
    fallback: &fallback::Fallback,
    forcing: &PlanetEnvironment,
    revision: PhysicalRevision,
) -> Result<PreparedInitial, &'static str> {
    super::validate_record(&super::fallback(fallback))?;
    crate::system::forcing::Forcing::new(
        forcing.irradiation_w_m2,
        0.0,
        forcing.assumed_bond_albedo,
        forcing.age_gyr,
    )?;
    if !forcing.equilibrium_temperature_k.is_finite()
        || forcing.equilibrium_temperature_k < 0.0
        || fallback.surface_temperature_k
            != if fallback.reference == space_shared::RadiusReference::OneBar {
                fallback::one_bar_temperature_k(
                    forcing.equilibrium_temperature_k.max(20.0),
                    fallback.mass_kg(),
                    forcing.age_gyr,
                    fallback.radius_m,
                )
            } else {
                forcing.equilibrium_temperature_k.max(20.0)
            }
    {
        return Err("inconsistent synchronous thermal input");
    }
    let partitioned = fallback::decide(fallback, forcing).map_err(|error| match error {
        crate::system::atmosphere::AtmosphereError::InvalidValue(name) => name,
        _ => "invalid synchronous surface decision",
    })?;
    let mut result = super::initial_from_fallback(&partitioned.fallback, revision)?;
    let approximations = &mut result.record.physical.approximations;
    if matches!(partitioned.decision, Decision::Thin { .. }) {
        approximations.extend(fallback::decide::MODEL_IDS.map(String::from));
    } else {
        approximations.push(fallback::partition::MODEL_ID.into());
    }
    super::validate_result(&result)?;
    let facts = &result.record.physical;
    let mut environment = forcing.clone();
    environment.atmosphere = partitioned
        .atmosphere
        .as_ref()
        .map(|air| {
            OpticalColumn::from_result(&result, air)
                .map_err(|_| "inconsistent synchronous optics")?;
            air.to_data(
                facts.reference_gravity_m_s2 * facts.reference_radius_m.powi(2),
                facts.reference_radius_m,
            )
            .map_err(|_| "invalid synchronous atmosphere")
        })
        .transpose()?;
    Ok(PreparedInitial {
        result,
        budget: partitioned.budget,
        environment,
        decision: partitioned.decision,
        blowoff: partitioned.fallback.blowoff,
    })
}
