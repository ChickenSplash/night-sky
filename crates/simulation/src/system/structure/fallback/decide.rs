//! Generation's one synchronous decision of the bulk surface facts: greenhouse
//! temperature, gas and frost split, blow-off, and which layered route applies.
//! The layered solve receives these and adds no escape of its own.
use super::*;
use crate::system::{
    atmosphere::{AtmosphereError, GasAtmosphere, greenhouse},
    structure::{
        coupled::{self, escape, surface},
        prototype::{self, Decision},
    },
};

pub const NOTES: &[&str] = &[
    "Fallback: radius read from the layered solver's table, not a hydrostatic solve of this body",
    "Condensed layer boundaries, their pressures and the condensed body's inertia are interpolated from the same table; the boundary column is finite and isothermal",
    "Surface decision: the layered column's partition, grey greenhouse balance, frost and blow-off rules on the tabulated radius",
    "All water and unprotected secondaries exchange with the column; protected secondaries stay in the rock",
    "Greenhouse: grey Eddington fit to Earth, Mars and Venus; the coolest balance warming up from equilibrium",
    "Blow-off strips the lightest gases of an unbound column entirely, with no escape rate",
    "Frost is one even global layer of fixed solid density",
];

pub const PHASES_NOTE: &str = "Water phases follow the layered solver's water models along the decided isotherm, walked inwards from the tabulated surface; one temperature, not a thermal profile";
pub const UNRESOLVED_NOTE: &str = "Water phase is not decided: the layered solver's water models have no answer on this isotherm, or the body has no rock beneath its water";
pub const SUSPENDED_NOTE: &str = "Suspended condensate is the local saturation split averaged over each gas layer along the envelope's prescribed thermal profile; no settling, rain-out or cloud opacity";

pub const UNSUSPENDED_NOTE: &str = "Secondary gases fall outside the suspended-condensate model on this profile: no condensate split is stated, and the envelope's radius is the table's for H/He alone";

pub const DENSE_NOTE: &str = "Dense column: ideal gas beyond the layered solver's density mask, so no layered result replaces it";

/// Whether the retained H/He alone presses at least one bar on this radius.
/// Display classification applies the same test to the published facts.
fn light_gas_holds_one_bar(retained: &Inventory, condensed_radius_m: f64) -> bool {
    let light = retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium);
    GRAVITATIONAL_CONSTANT * retained.total_mass_kg() * light
        >= 4.0 * PI * ONE_BAR_PA * condensed_radius_m.powi(4)
}

pub struct Decided {
    pub fallback: Fallback,
    pub budget: Budget,
    /// Includes positive trace columns. Optical/population cutoffs are separate.
    pub atmosphere: Option<GasAtmosphere>,
    pub decision: Decision,
}

/// Starts from the unpartitioned retention fallback. Never fails for a
/// generated budget: a refused surface state keeps the cold-trap partition at
/// the equilibrium temperature and leaves the decision open.
pub fn decide(
    initial: &Fallback,
    environment: &PlanetEnvironment,
) -> Result<Decided, AtmosphereError> {
    let invalid = || AtmosphereError::InvalidValue("synchronous surface decision");
    let undecided = |base: &Fallback, blowoff: Inventory, decision| {
        let partitioned = partition::resolve(base)?;
        let mut fallback = partitioned.fallback;
        fallback.blowoff = blowoff;
        let mut atmosphere = partitioned.atmosphere;
        if decision == Decision::Envelope {
            reference_one_bar(&mut fallback, &mut atmosphere)?;
        }
        if fallback.reference == RadiusReference::OneBar {
            suspend(&mut fallback, environment);
        }
        Ok(Decided {
            fallback,
            budget: partitioned.budget,
            atmosphere,
            decision,
        })
    };
    let route = |error: &coupled::Error, retained: &Inventory| {
        if prototype::routes_to_envelope(error, retained) {
            Decision::Envelope
        } else {
            Decision::Refused(*error)
        }
    };
    let (request, protected) =
        match prototype::thin_request(&initial.source, &initial.retained, environment) {
            Ok(request) => request,
            Err(e) => {
                return undecided(initial, Inventory::default(), route(&e, &initial.retained));
            }
        };
    let mut stripped = escape::strip(&request, |request| {
        surface::state(request, &protected, condensed_radius_m, false)
    });
    // A dense column is an envelope only where its H/He alone holds one bar.
    // Otherwise it is a thick secondary or steam atmosphere over a surface:
    // keep the ideal column here, which the layered solve will then refuse.
    let mut dense = false;
    if stripped.result == Err(coupled::Error::DenseGasUnsupported)
        && !light_gas_holds_one_bar(&stripped.request.retained, initial.condensed_radius_m)
    {
        let escaped = stripped.escaped;
        stripped = escape::strip(&stripped.request, |request| {
            surface::state(request, &protected, condensed_radius_m, true)
        });
        stripped.escaped = stripped
            .escaped
            .checked_add(&escaped)
            .map_err(|_| invalid())?;
        dense = true;
    }
    let retained = stripped.request.retained;
    let state = match stripped.result {
        Ok(state) => state,
        Err(e) => {
            let decision = route(&e, &retained);
            if stripped.escaped.total_mass_kg() == 0.0 {
                return undecided(initial, stripped.escaped, decision);
            }
            let budget = Budget::new(
                initial.source,
                retained,
                Inventory::default(),
                Inventory::default(),
            )
            .map_err(|_| invalid())?;
            return undecided(
                &prescribed(&budget, environment, None),
                stripped.escaped,
                decision,
            );
        }
    };

    let water = Inventory::new(&[(Material::Water, state.host.mass_kg(Material::Water))])
        .map_err(|_| invalid())?;
    let interior = Inventory::new(&Material::ALL.map(|m| {
        (
            m,
            if m == Material::Water {
                0.0
            } else {
                state.host.mass_kg(m)
            },
        )
    }))
    .map_err(|_| invalid())?;
    let budget = closed(
        initial.source,
        &retained,
        [
            interior,
            water.checked_add(&state.frost).map_err(|_| invalid())?,
            state.gas,
        ],
    )
    .map_err(|_| invalid())?;

    let stack = decided_stack(&state.host);
    let mut quality = Quality::default();
    let interior = Interior::bare(&stack, state.temperature_k, &mut quality).ok_or_else(invalid)?;
    let condensed_radius_m = interior.radius_m;
    let frost = (state.frost.total_mass_kg() > 0.0).then(|| {
        Region::new(
            Kind::Frost,
            state.frost,
            condensed_radius_m,
            state.surface_radius_m,
        )
    });
    let host_kg: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    // What presses on the water: the column, and any frost under it.
    let load_pa = state
        .atmosphere
        .as_ref()
        .map_or(0.0, |air| air.pressure_pa())
        + frost
            .as_ref()
            .map_or(0.0, |region| Shell::new(region, host_kg).pressure_drop_pa());
    let phases = phases::walk(
        state.temperature_k,
        load_pa,
        host_kg,
        condensed_radius_m,
        state.host.mass_kg(Material::Water),
        interior.refractory_pa,
    );
    let resolved = phases.is_some();
    let mut regions = lay(stack, &interior, phases);
    regions.extend(frost);
    let radius_m = state.surface_radius_m;
    let gm = GRAVITATIONAL_CONSTANT * (state.host.total_mass_kg() + state.frost.total_mass_kg());
    let mut one_bar_radius_m = None;
    if let Some(air) = &state.atmosphere {
        regions.push(Region::new(
            Kind::Atmosphere,
            state.gas,
            radius_m,
            radius_m + air.column(gm, radius_m)?.extent_m,
        ));
        one_bar_radius_m = air
            .pressure_level_altitude_m(ONE_BAR_PA, gm, radius_m)?
            .map(|altitude| radius_m + altitude);
    }
    let extrapolated = quality.approximate();
    let mut fallback = Fallback {
        reason: None,
        source: initial.source,
        retained: *budget.retained(),
        escaped: *budget.escaped(),
        regions,
        radius_m,
        condensed_radius_m,
        reference: RadiusReference::UnresolvedVolatileBoundary,
        one_bar_radius_m,
        surface_temperature_k: state.temperature_k,
        surface_pressure_pa: 0.0,
        inertia_kg_m2: 0.0,
        envelope_inertia: None,
        optical_depth: Some(state.optical_depth),
        blowoff: stripped.escaped,
        extrapolated,
        quality,
        notes: NOTES.to_vec(),
    };
    fallback.reference = fallback.condensed_reference();
    if state.host.mass_kg(Material::Water) > 0.0 {
        fallback.notes.push(if resolved {
            PHASES_NOTE
        } else {
            UNRESOLVED_NOTE
        });
    }
    if extrapolated {
        fallback.notes.push(CLAMPED_NOTE);
    }
    if dense {
        fallback.notes.push(DENSE_NOTE);
    }
    partition::close(&mut fallback, state.atmosphere.as_ref(), gm, radius_m)?;
    // Spin shape follows inertia, so it must not move when the layers arrive.
    let (tabulated, uniform) =
        tabulated_condensed_inertia_kg_m2(&fallback.regions, state.temperature_k);
    fallback.inertia_kg_m2 += tabulated - uniform;
    Ok(Decided {
        fallback,
        budget,
        atmosphere: state.atmosphere,
        decision: Decision::Thin {
            surface_temperature_k: state.temperature_k,
        },
    })
}

/// Model identifiers a decided surface adds to its record's approximations.
pub const MODEL_IDS: [&str; 2] = [surface::MODEL_ID, greenhouse::MODEL_ID];

/// The split's parts can sum one ulp above what was retained. Take that
/// roundoff from the largest allocation, so the budget closes without escape
/// being recaptured.
fn closed(
    source: Inventory,
    retained: &Inventory,
    mut parts: [Inventory; 3],
) -> Result<Budget, crate::system::composition::CompositionError> {
    for m in Material::ALL {
        for _ in 0..4 {
            let masses = parts.map(|p| p.mass_kg(m));
            if masses[0] + masses[1] + masses[2] <= retained.mass_kg(m) {
                break;
            }
            let largest = (0..3)
                .max_by(|&a, &b| masses[a].total_cmp(&masses[b]))
                .expect("three allocations");
            parts[largest] = Inventory::new(&Material::ALL.map(|other| {
                let mass = parts[largest].mass_kg(other);
                (other, if other == m { mass.next_down() } else { mass })
            }))?;
        }
    }
    Budget::new(source, parts[0], parts[1], parts[2])
}

/// The layered envelope route references its radius at one bar whenever its
/// gas reaches it. A boundary column of at least one bar takes the same
/// reference here, at that level of its own isothermal profile.
fn reference_one_bar(
    fallback: &mut Fallback,
    atmosphere: &mut Option<GasAtmosphere>,
) -> Result<(), AtmosphereError> {
    let Some(air) = *atmosphere else {
        return Ok(());
    };
    if fallback.reference == RadiusReference::OneBar || air.pressure_pa() < ONE_BAR_PA {
        return Ok(());
    }
    let gm = GRAVITATIONAL_CONSTANT * fallback.mass_kg();
    let Ok(Some(altitude)) = air.pressure_level_altitude_m(ONE_BAR_PA, gm, fallback.radius_m)
    else {
        return Ok(());
    };
    let radius_m = fallback.radius_m + altitude;
    let Some(gas) = fallback
        .regions
        .iter_mut()
        .find(|r| r.kind == Kind::Atmosphere)
        .filter(|gas| radius_m < gas.outer_radius_m)
    else {
        return Ok(());
    };
    // The whole column presses on the surface: its own pressure, above the
    // one bar at the level its upper part now starts from.
    let over = (air.pressure_pa() - ONE_BAR_PA).max(0.0);
    gas.kind = Kind::Envelope;
    gas.tabulated_pa = Some([over, 0.0]);
    for region in &mut fallback.regions {
        if region.kind != Kind::Envelope
            && let Some(tabulated) = &mut region.tabulated_pa
        {
            *tabulated = tabulated.map(|pressure| pressure + over);
        }
    }
    fallback.radius_m = radius_m;
    fallback.one_bar_radius_m = Some(radius_m);
    fallback.reference = RadiusReference::OneBar;
    let level = GasAtmosphere::new(
        *air.composition(),
        crate::system::atmosphere::ReferenceLevel::OneBar,
        ONE_BAR_PA,
        air.temperature_k(),
        None,
    )?;
    *atmosphere = Some(level);
    partition::split_at_one_bar(fallback, &level, gm)?;
    partition::close(fallback, Some(&level), gm, radius_m)?;
    fallback.surface_pressure_pa = ONE_BAR_PA;
    Ok(())
}

/// Share of each secondary gas that is condensed and carried in an
/// envelope's layers. The layered envelope reports the same split from its
/// own profile; here it is the local saturation state averaged over each
/// layer's pressures, which is where a hydrostatic layer keeps its mass.
fn suspend(fallback: &mut Fallback, environment: &PlanetEnvironment) {
    use crate::system::{
        materials::{hhe_mixture::SECONDARIES, molecular, suspended},
        structure::{dry, envelope},
    };
    const SAMPLES: usize = 24;
    /// The envelope's own top, a hundredth of a bar.
    const TOP_PA: f64 = 1e3;
    let mass_kg = fallback.mass_kg();
    let equilibrium_k = environment.equilibrium_temperature_k.max(MIN_TEMPERATURE_K);
    let age = if environment.age_gyr.is_finite() && environment.age_gyr > 0.0 {
        environment.age_gyr
    } else {
        return;
    };
    let intrinsic_k = envelope::intrinsic_k(mass_kg, age);
    let Ok(thermal) = envelope::Thermal::new(
        equilibrium_k,
        intrinsic_k,
        GRAVITATIONAL_CONSTANT * mass_kg / fallback.radius_m.powi(2),
    ) else {
        return;
    };
    let coldest_k = (equilibrium_k.powi(4) + 0.5 * intrinsic_k.powi(4)).powf(0.25);
    let mut found = false;
    let mut condensates = Vec::new();
    for region in &fallback.regions {
        let mut condensate = Inventory::default();
        if matches!(region.kind, Kind::Envelope | Kind::Atmosphere) {
            // The envelope's prescribed trace water takes no part in its
            // gas density or this split.
            let Ok(gas) = Inventory::new(&Material::ALL.map(|m| {
                (
                    m,
                    if m == Material::Water {
                        0.0
                    } else {
                        region.inventory.mass_kg(m)
                    },
                )
            })) else {
                return;
            };
            let total = gas.total_mass_kg();
            let light = gas.mass_kg(Material::Hydrogen) + gas.mass_kg(Material::Helium);
            if light.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
                || (total - light) / total <= dry::protected::MAX_TRACE_FRACTION
                || !suspended::cold_composition(&gas, coldest_k)
            {
                return;
            }
            let low = region.outer_pressure_pa.max(TOP_PA);
            let high = region.inner_pressure_pa.min(molecular::MAX_PRESSURE_PA);
            let mut share = [0.0; 5];
            if high > low {
                // Even steps in the logarithm of pressure, each weighted by
                // the pressure it spans.
                let step = (high / low).ln() / SAMPLES as f64;
                for sample in 0..SAMPLES {
                    let pressure = low * ((sample as f64 + 0.5) * step).exp();
                    let Ok(temperature) = thermal.temperature_k(pressure) else {
                        fallback.notes.push(UNSUSPENDED_NOTE);
                        return;
                    };
                    // Above its range every secondary is past condensing.
                    if temperature > 625.0 {
                        continue;
                    }
                    let Ok(state) = suspended::State::new(&gas, pressure, temperature, 1.0) else {
                        fallback.notes.push(UNSUSPENDED_NOTE);
                        return;
                    };
                    for (i, m) in SECONDARIES.into_iter().enumerate() {
                        if gas.mass_kg(m) > 0.0 {
                            share[i] += state.condensate.mass_kg(m) / (gas.mass_kg(m) / total)
                                * pressure
                                * step;
                        }
                    }
                }
            }
            // Mass below `low` or above `high` holds no condensate here.
            let span = region.inner_pressure_pa - region.outer_pressure_pa;
            let Ok(result) = Inventory::new(&std::array::from_fn::<_, 5, _>(|i| {
                let m = SECONDARIES[i];
                (m, gas.mass_kg(m) * (share[i] / span).clamp(0.0, 1.0))
            })) else {
                return;
            };
            found |= result.total_mass_kg() > 0.0;
            condensate = result;
        }
        condensates.push(condensate);
    }
    if !found {
        return;
    }
    for (region, condensate) in fallback.regions.iter_mut().zip(condensates) {
        region.condensate = condensate;
    }
    fallback.notes.push(SUSPENDED_NOTE);
}
