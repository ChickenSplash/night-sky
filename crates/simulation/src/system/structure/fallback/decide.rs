//! Generation's one synchronous decision of the bulk surface facts: greenhouse
//! temperature, gas and frost split, blow-off, and which layered route applies.
//! The layered solve receives these and adds no escape of its own.
use super::*;
use crate::system::{
    atmosphere::{AtmosphereError, GasAtmosphere, ONE_BAR_PA, greenhouse},
    structure::{
        coupled::{self, escape, surface},
        prototype::{self, Decision},
    },
};

pub const NOTES: &[&str] = &[
    "Fallback: radius read from the layered solver's table, not a hydrostatic solve of this body",
    "Uniform condensed layers and a finite isothermal boundary column; the condensed body's inertia is the layered solver's tabulated value",
    "Surface decision: the layered column's partition, grey greenhouse balance, frost and blow-off rules on the tabulated radius",
    "All water and unprotected secondaries exchange with the column; protected secondaries stay in the rock",
    "Water phase is not decided here: it follows from the decided temperature in the layered solve",
    "Greenhouse: grey Eddington fit to Earth, Mars and Venus; the coolest balance warming up from equilibrium",
    "Blow-off strips the lightest gases of an unbound column entirely, with no escape rate",
    "Frost is one even global layer of fixed solid density",
];

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

    let mut extrapolated = false;
    let mut regions = condensed_regions(
        decided_stack(&state.host),
        state.temperature_k,
        &mut extrapolated,
    );
    let condensed_radius_m = regions.last().ok_or_else(invalid)?.outer_radius_m;
    let solid = regions
        .last()
        .is_some_and(|r| matches!(r.kind, Kind::Core | Kind::Mantle));
    let frost = state.frost.total_mass_kg() > 0.0;
    if frost {
        regions.push(Region {
            kind: Kind::Frost,
            inventory: state.frost,
            inner_radius_m: condensed_radius_m,
            outer_radius_m: state.surface_radius_m,
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
    }
    let radius_m = state.surface_radius_m;
    let gm = GRAVITATIONAL_CONSTANT * (state.host.total_mass_kg() + state.frost.total_mass_kg());
    let mut one_bar_radius_m = None;
    if let Some(air) = &state.atmosphere {
        regions.push(Region {
            kind: Kind::Atmosphere,
            inventory: state.gas,
            inner_radius_m: radius_m,
            outer_radius_m: radius_m + air.column(gm, radius_m)?.extent_m,
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
        one_bar_radius_m = air
            .pressure_level_altitude_m(ONE_BAR_PA, gm, radius_m)?
            .map(|altitude| radius_m + altitude);
    }
    let mut fallback = Fallback {
        reason: None,
        source: initial.source,
        retained: *budget.retained(),
        escaped: *budget.escaped(),
        regions,
        radius_m,
        condensed_radius_m,
        reference: if solid && !frost {
            RadiusReference::SolidSurface
        } else {
            RadiusReference::UnresolvedVolatileBoundary
        },
        one_bar_radius_m,
        surface_temperature_k: state.temperature_k,
        surface_pressure_pa: 0.0,
        inertia_kg_m2: 0.0,
        optical_depth: Some(state.optical_depth),
        blowoff: stripped.escaped,
        extrapolated,
        notes: NOTES.to_vec(),
    };
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
    let Some(gas) = fallback
        .regions
        .iter_mut()
        .find(|r| r.kind == Kind::Atmosphere)
    else {
        return Ok(());
    };
    let radius_m = fallback.radius_m + altitude;
    if radius_m >= gas.outer_radius_m {
        return Ok(());
    }
    gas.kind = Kind::Envelope;
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
    partition::close(fallback, Some(&level), gm, radius_m)?;
    fallback.surface_pressure_pa = ONE_BAR_PA;
    Ok(())
}
