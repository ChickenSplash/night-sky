//! Synchronous partition on fixed fallback reference geometry, not another
//! structure solve. Retention and the standalone refusal fallback are unchanged.
use super::*;
use crate::system::{
    atmosphere::{AtmosphereError, GasAtmosphere, generation::partition, profile::Profile},
    composition::Reservoir,
};

pub const MODEL_ID: &str = "fallback_reference_partition_v1";
pub const NOTES: &[&str] = &[
    "Uniform condensed layers, prescribed thinning bulk envelope or finite isothermal boundary gas",
    "Secondary gas is cold-trapped at the fallback reference; not a coupled EOS or climate solve",
    "Sub-resolution vapour transfers remain condensed and are excluded from the trace pressure solve",
    "Condensed proxy volumes are held fixed when accessible volatiles enter gas; densities and inertia are updated",
    "Boundary gas inertia uses a mass-normalised finite column; weak-binding reference pressure retains the planar approximation",
    "Bulk envelope inertia and deep pressure retain the prescribed thinning profile, not the outer optical column",
];

pub struct Partitioned {
    pub fallback: Fallback,
    pub budget: Budget,
    /// Includes positive trace columns. Optical/population cutoffs are separate.
    pub atmosphere: Option<GasAtmosphere>,
}

pub fn resolve(initial: &Fallback) -> Result<Partitioned, AtmosphereError> {
    let invalid = || AtmosphereError::InvalidValue("fallback reference partition");
    // Canonicalise the supplied allocations to the fallback's represented stack.
    // No source supply or escape is redrawn, and no prior partition is compounded.
    let inventory = |kinds: &[Kind]| -> Result<Inventory, AtmosphereError> {
        initial
            .regions
            .iter()
            .filter(|r| kinds.contains(&r.kind))
            .try_fold(Inventory::default(), |sum, r| sum.checked_add(&r.inventory))
            .map_err(|_| invalid())
    };
    if initial.regions.iter().any(|r| r.kind == Kind::Atmosphere)
        || initial.notes.contains(&NOTES[0])
    {
        return Err(invalid()); // Must start with the unpartitioned retention fallback.
    }
    let canonical = Budget::new(
        initial.source,
        inventory(&[Kind::Core, Kind::Mantle])?,
        inventory(&[Kind::Water, Kind::Volatiles])?,
        inventory(&[Kind::Envelope])?,
    )
    .map_err(|_| invalid())?;
    let one_bar = initial.reference == RadiusReference::OneBar;
    let (budget, atmosphere) = partition::solve(
        &canonical,
        initial.surface_temperature_k,
        initial.mass_kg(),
        initial.radius_m,
        one_bar,
    )?;
    let gas = *budget.reservoir(Reservoir::Gas);
    let mut fallback = initial.clone();
    fallback
        .notes
        .retain(|note| *note != VOLATILE_PROXY_NOTE && *note != super::NOTES[1]);
    fallback.notes.extend_from_slice(NOTES);
    fallback.regions.retain(|r| r.kind != Kind::Envelope);
    for region in &mut fallback.regions {
        if matches!(region.kind, Kind::Water | Kind::Volatiles) {
            region.inventory = Inventory::new(&Material::ALL.map(|m| {
                (
                    m,
                    if region.inventory.mass_kg(m) > 0.0 {
                        budget.reservoir(Reservoir::CondensedVolatiles).mass_kg(m)
                    } else {
                        0.0
                    },
                )
            }))
            .map_err(|_| invalid())?;
        }
    }
    // Retention's protected secondary supply leaves every existing condensed
    // layer positive. Do not retain an empty shell with invented density.
    if fallback
        .regions
        .iter()
        .any(|r| r.inventory.total_mass_kg() <= 0.0)
    {
        return Err(invalid());
    }
    if gas.total_mass_kg() > 0.0 {
        let air = atmosphere.as_ref().ok_or_else(invalid)?;
        let top = if one_bar {
            initial
                .regions
                .iter()
                .find(|r| r.kind == Kind::Envelope)
                .ok_or_else(invalid)?
                .outer_radius_m
        } else {
            initial.radius_m
                + air
                    .column(GRAVITATIONAL_CONSTANT * initial.mass_kg(), initial.radius_m)?
                    .extent_m
        };
        if !top.is_finite() || top <= initial.condensed_radius_m {
            return Err(invalid());
        }
        fallback.regions.push(Region {
            kind: if one_bar {
                Kind::Envelope
            } else {
                Kind::Atmosphere
            },
            inventory: gas,
            inner_radius_m: initial.condensed_radius_m,
            outer_radius_m: top,
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
    }
    fallback.surface_pressure_pa = atmosphere.as_ref().map_or(0.0, |air| air.pressure_pa());
    fallback.inertia_kg_m2 = 0.0;
    let mut enclosed = Vec::new();
    let mut mass = 0.0;
    for r in &fallback.regions {
        enclosed.push(mass);
        mass += r.inventory.total_mass_kg();
    }
    let mut pressure = 0.0;
    for (region, inner_mass) in fallback.regions.iter_mut().zip(enclosed).rev() {
        if region.kind == Kind::Atmosphere {
            region.outer_pressure_pa = 0.0;
            pressure = fallback.surface_pressure_pa;
            region.inner_pressure_pa = pressure;
            let column = atmosphere
                .as_ref()
                .ok_or_else(invalid)?
                .column(GRAVITATIONAL_CONSTANT * initial.mass_kg(), initial.radius_m)?;
            fallback.inertia_kg_m2 += column_inertia(
                &column.profile,
                fallback.surface_pressure_pa,
                region.inventory.total_mass_kg(),
            );
        } else {
            let shell = Shell::new(region, inner_mass);
            region.outer_pressure_pa = pressure;
            pressure += shell.pressure_drop_pa();
            region.inner_pressure_pa = pressure;
            fallback.inertia_kg_m2 += shell.inertia_kg_m2();
        }
    }
    if !fallback.inertia_kg_m2.is_finite() || fallback.inertia_kg_m2 <= 0.0 {
        return Err(invalid());
    }
    Ok(Partitioned {
        fallback,
        budget,
        atmosphere,
    })
}

/// Same finite-column coordinates as the atmosphere, normalised to its actual
/// gas inventory. No atmospheric mass is added to the retained total twice.
fn column_inertia(profile: &Profile, pressure: f64, gas_mass: f64) -> f64 {
    let end = Profile::cutoff_drop(pressure).min(64.0);
    let relative_radius = |z: f64| {
        if profile.spherical {
            1.0 / (1.0 - z * profile.scale_height_m / profile.radius_m)
        } else {
            1.0 + z * profile.scale_height_m / profile.radius_m
        }
    };
    let weight =
        |z: f64| (-z).exp() * relative_radius(z).powi(if profile.spherical { 4 } else { 2 });
    let mass = integrate(|t| weight(t * end));
    let moment = integrate(|t| weight(t * end) * relative_radius(t * end).powi(2));
    (2.0 / 3.0) * gas_mass * profile.radius_m.powi(2) * moment / mass
}

#[cfg(test)]
mod tests;
