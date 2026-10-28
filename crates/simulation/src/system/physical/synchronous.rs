//! Complete synchronous result and its atmosphere. Generation installs these
//! facts together as the live projections; nothing else may supply them.
use crate::system::{
    atmosphere::{generation::MIN_PRESSURE_PA, optics::boundary::OpticalColumn},
    composition::Budget,
    structure::fallback,
};
use space_shared::{PhysicalRevision, PlanetEnvironment, PlanetResult};

pub struct PreparedInitial {
    pub result: PlanetResult,
    /// The same reservoir split as `result`, for retention re-evaluation.
    pub budget: Budget,
    /// Includes positive trace columns. See `live_environment`.
    pub environment: PlanetEnvironment,
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

/// Partition at the fallback's fixed reference. Neither legacy boundary
/// resolution nor a second structure/retention/EOS solve is used.
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
        || forcing.equilibrium_temperature_k.max(20.0) != fallback.surface_temperature_k
    {
        return Err("inconsistent synchronous thermal input");
    }
    let partitioned = fallback::partition::resolve(fallback).map_err(|error| match error {
        crate::system::atmosphere::AtmosphereError::InvalidValue(name) => name,
        _ => "invalid synchronous fallback partition",
    })?;
    let mut result = super::initial_from_fallback(&partitioned.fallback, revision)?;
    result
        .record
        .physical
        .approximations
        .push(fallback::partition::MODEL_ID.into());
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
    })
}
