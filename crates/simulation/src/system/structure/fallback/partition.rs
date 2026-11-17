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
    "A fully evaporated volatile layer is removed; its column keeps the pressure normalised before that small shrink",
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
    // A layer whose whole supply evaporated is removed, never kept as an empty
    // shell with invented density. Only the outermost accessible layer can be.
    let mut radius_m = initial.radius_m;
    let mut condensed_radius_m = initial.condensed_radius_m;
    while fallback
        .regions
        .last()
        .is_some_and(|r| r.inventory.total_mass_kg() <= 0.0)
    {
        let removed = fallback.regions.pop().and_then(|r| r.tabulated_pa);
        let top = fallback.regions.last_mut().ok_or_else(invalid)?;
        // The layer beneath is now the surface, under the same load.
        if let (Some(tabulated), Some(removed)) = (&mut top.tabulated_pa, removed) {
            tabulated[1] = removed[1];
        }
        let top = &*top;
        condensed_radius_m = top.outer_radius_m;
        if !one_bar {
            radius_m = condensed_radius_m;
            fallback.reference = if matches!(top.kind, Kind::Core | Kind::Mantle) {
                RadiusReference::SolidSurface
            } else {
                RadiusReference::UnresolvedVolatileBoundary
            };
        }
    }
    if fallback
        .regions
        .iter()
        .any(|r| r.inventory.total_mass_kg() <= 0.0)
    {
        return Err(invalid());
    }
    fallback.radius_m = radius_m;
    fallback.condensed_radius_m = condensed_radius_m;
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
            radius_m
                + air
                    .column(GRAVITATIONAL_CONSTANT * initial.mass_kg(), radius_m)?
                    .extent_m
        };
        if !top.is_finite() || top <= condensed_radius_m {
            return Err(invalid());
        }
        let mut shell = Region::new(
            if one_bar {
                Kind::Envelope
            } else {
                Kind::Atmosphere
            },
            gas,
            condensed_radius_m,
            top,
        );
        if one_bar {
            shell.tabulated_pa = initial
                .regions
                .iter()
                .find(|r| r.kind == Kind::Envelope)
                .and_then(|r| r.tabulated_pa);
            fallback.regions.push(shell);
            split_at_one_bar(
                &mut fallback,
                air,
                GRAVITATIONAL_CONSTANT * initial.mass_kg(),
            )?;
            weigh_thin_gas(&mut fallback.regions, initial.mass_kg());
        } else {
            fallback.regions.push(shell);
        }
    }
    close(
        &mut fallback,
        atmosphere.as_ref(),
        GRAVITATIONAL_CONSTANT * initial.mass_kg(),
        radius_m,
    )?;
    Ok(Partitioned {
        fallback,
        budget,
        atmosphere,
    })
}

/// The gas above a one-bar reference becomes its own layer: the atmosphere
/// record's isothermal column, which holds what one bar supports there. The
/// envelope below it ends at the reference radius.
pub(super) fn split_at_one_bar(
    fallback: &mut Fallback,
    level: &GasAtmosphere,
    gm: f64,
) -> Result<(), AtmosphereError> {
    let invalid = || AtmosphereError::InvalidValue("fallback reference partition");
    let Some(shell) = fallback
        .regions
        .last_mut()
        .filter(|r| r.kind == Kind::Envelope)
    else {
        return Ok(());
    };
    let radius_m = fallback
        .radius_m
        .max(shell.inner_radius_m * (1.0 + MIN_GAS_THICKNESS));
    let column = level.column(gm, radius_m)?;
    let gas = shell.inventory;
    let above_kg = 4.0 * PI * radius_m.powi(2) / column.gravity_m_s2
        * level.pressure_pa()
        * column.profile.mass_factor(level.pressure_pa());
    // Gas that barely holds one bar is tabulated against a radius it cannot
    // fill. The level then takes no more than half of it.
    let share = (above_kg / gas.total_mass_kg()).min(0.5);
    if share.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Ok(());
    }
    // The level takes no more than half, so the second difference is exact
    // and the two parts sum to the decided gas to the bit.
    let below =
        Inventory::new(&Material::ALL.map(|m| (m, gas.mass_kg(m) - gas.mass_kg(m) * share)))
            .map_err(|_| invalid())?;
    let above = Inventory::new(&Material::ALL.map(|m| (m, gas.mass_kg(m) - below.mass_kg(m))))
        .map_err(|_| invalid())?;
    shell.inventory = below;
    shell.outer_radius_m = radius_m;
    fallback.regions.push(Region::new(
        Kind::Atmosphere,
        above,
        radius_m,
        radius_m + column.extent_m,
    ));
    Ok(())
}

/// Pressures and inertia, outermost first: the table's where a region has
/// them, its prescribed profile's elsewhere. A boundary column stands on
/// `radius_m` under `gm`.
pub(super) fn close(
    fallback: &mut Fallback,
    atmosphere: Option<&GasAtmosphere>,
    gm: f64,
    radius_m: f64,
) -> Result<(), AtmosphereError> {
    let invalid = || AtmosphereError::InvalidValue("fallback reference partition");
    fallback.surface_pressure_pa = atmosphere.map_or(0.0, |air| air.pressure_pa());
    fallback.inertia_kg_m2 = 0.0;
    let surface_pressure_pa = fallback.surface_pressure_pa;
    let mut beneath = fallback.regions.len();
    let mut pressure = if fallback.reference == RadiusReference::OneBar {
        ONE_BAR_PA
    } else {
        0.0
    };
    if let Some(region) = fallback
        .regions
        .last_mut()
        .filter(|r| r.kind == Kind::Atmosphere)
    {
        region.outer_pressure_pa = 0.0;
        region.inner_pressure_pa = surface_pressure_pa;
        pressure = surface_pressure_pa;
        beneath -= 1;
        let column = atmosphere.ok_or_else(invalid)?.column(gm, radius_m)?;
        fallback.inertia_kg_m2 += column_inertia(
            &column.profile,
            surface_pressure_pa,
            region.inventory.total_mass_kg(),
        );
    }
    fallback.inertia_kg_m2 += settle(&mut fallback.regions[..beneath], pressure);
    if let Some(inertia) = fallback.tabulated_envelope_inertia_kg_m2() {
        fallback.inertia_kg_m2 = inertia;
    }
    if !fallback.inertia_kg_m2.is_finite() || fallback.inertia_kg_m2 <= 0.0 {
        return Err(invalid());
    }
    Ok(())
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
