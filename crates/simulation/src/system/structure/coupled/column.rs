//! Supply-limited partition, without generation's source/exposure priors.
use super::*;
use crate::{
    system::{
        atmosphere::{Gas, ReferenceLevel, condensation::vapour_pressure, profile::Profile},
        materials::water as eos,
    },
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};

/// Partial-pressure cap. Above its triple point water uses the IF97 saturation
/// curve, so a saturated column leaves the liquid water below it stable.
/// Other caps, and ice, keep the shared cold-trap prescription.
pub(crate) fn cap_pa(gas: Gas, temperature: f64) -> f64 {
    if gas == Gas::Water && temperature >= 273.16 {
        eos::saturation_pressure_pa(temperature).unwrap_or(f64::INFINITY)
    } else {
        vapour_pressure(gas, temperature)
    }
}

pub(super) fn atmosphere(
    gas: &Inventory,
    temperature: f64,
    gm: f64,
    radius: f64,
) -> Result<Option<GasAtmosphere>, Error> {
    let Some(mixture) = gas.gas_composition().map_err(Error::Composition)? else {
        return Ok(None);
    };
    let pressure = Profile::boundary_pressure(
        gas.total_mass_kg(),
        gm,
        radius,
        BOLTZMANN_J_K * temperature / mixture.mean_molecular_mass_kg(),
    );
    GasAtmosphere::new(
        mixture,
        crate::system::atmosphere::ReferenceLevel::Surface,
        pressure,
        temperature,
        None,
    )
    .map(Some)
    .map_err(Error::Atmosphere)
}

pub(super) fn partition(
    request: &Request,
    temperature: f64,
    mass: f64,
    radius: f64,
) -> Result<Inventory, Error> {
    partition_column(request, temperature, mass, radius).map(|(gas, _)| gas)
}

/// The gas and its column on the same geometry. The partition's own balance
/// is the column's mass normalisation, so no second pressure solve is needed.
pub(super) fn partition_column(
    request: &Request,
    temperature: f64,
    mass: f64,
    radius: f64,
) -> Result<(Inventory, Option<GasAtmosphere>), Error> {
    profile::time(profile::Stage::Partition, || {
        partition_unprofiled(request, temperature, mass, radius)
    })
}

fn partition_unprofiled(
    request: &Request,
    temperature: f64,
    mass: f64,
    radius: f64,
) -> Result<(Inventory, Option<GasAtmosphere>), Error> {
    if request.exchangeable.total_mass_kg() == 0.0 {
        return Ok((Inventory::default(), None));
    }
    let weight_per_area = GRAVITATIONAL_CONSTANT * mass / (4.0 * PI * radius.powi(4));
    let bound = Gas::ALL
        .iter()
        .map(|g| g.molecular_mass_u())
        .fold(0.0, f64::max)
        * weight_per_area
        / (1.0 - (-crate::system::atmosphere::EXTENT_SCALE_HEIGHTS).exp());
    // Caps depend on temperature alone: evaluate each once, not per trial.
    let limits = Gas::ALL.map(|g| {
        let m = Material::from_gas(g);
        let offered = request.exchangeable.mass_kg(m);
        let cap = if offered > 0.0 {
            cap_pa(g, temperature) * g.molecular_mass_u()
        } else {
            0.0
        };
        (m, offered, cap)
    });
    let allocated = |c: f64| {
        Inventory::new(&limits.map(|(m, offered, cap)| {
            (
                m,
                if offered > 0.0 {
                    offered.min(cap / c)
                } else {
                    0.0
                },
            )
        }))
        .map_err(Error::Composition)
    };
    let failure = std::cell::Cell::new(None);
    let high = crate::system::atmosphere::profile::upper_root(0.0, bound, |c| {
        let gas = match allocated(c) {
            Ok(gas) => gas,
            Err(e) => {
                failure.set(Some(e));
                return f64::NAN;
            }
        };
        let moles: f64 = Gas::ALL
            .iter()
            .map(|&g| gas.mass_kg(Material::from_gas(g)) / g.molecular_mass_u())
            .sum();
        if moles <= 0.0 || !moles.is_finite() {
            failure.set(Some(Error::InvalidInput("unresolvable gas allocation")));
            return f64::NAN;
        }
        let pressure = c * moles;
        let mean_mass = gas.total_mass_kg() / moles * ATOMIC_MASS_KG;
        let height = BOLTZMANN_J_K * temperature
            / mean_mass
            / (GRAVITATIONAL_CONSTANT * mass / radius.powi(2));
        let profile = Profile::new(radius, height, pressure);
        pressure * profile.mass_factor(pressure) - gas.total_mass_kg() * weight_per_area
    });
    if let Some(e) = failure.take() {
        return Err(e);
    }
    let gas = allocated(high)?;
    if gas.total_mass_kg() / request.retained.total_mass_kg() > MAX_GAS_MASS_FRACTION {
        return Err(Error::GasMassOutsideDomain);
    }
    let Some(mixture) = gas.gas_composition().map_err(Error::Composition)? else {
        return Ok((gas, None));
    };
    let moles: f64 = Gas::ALL
        .iter()
        .map(|&g| gas.mass_kg(Material::from_gas(g)) / g.molecular_mass_u())
        .sum();
    let air = GasAtmosphere::new(
        mixture,
        ReferenceLevel::Surface,
        high * moles,
        temperature,
        None,
    )
    .map_err(Error::Atmosphere)?;
    Ok((gas, Some(air)))
}

/// `dense` accepts a column beyond the density mask: only generation's
/// synchronous decision does, for gas the layered solve cannot represent.
pub(super) fn validate(
    gas: &Inventory,
    air: Option<&GasAtmosphere>,
    gm: f64,
    radius: f64,
    dense: bool,
) -> Result<(), Error> {
    let Some(air) = air else {
        return Ok(());
    };
    debug_assert_eq!(air.reference_level(), ReferenceLevel::Surface);
    let column = air.column(gm, radius).map_err(Error::Atmosphere)?;
    if !column.profile.spherical {
        return Err(Error::WeakBindingUnsupported);
    }
    let limit = if hot_steam(air) {
        MAX_HOT_STEAM_DENSITY_KG_M3
    } else {
        MAX_GAS_DENSITY_KG_M3
    };
    if column.density_kg_m3 > limit && !dense {
        return Err(Error::DenseGasUnsupported);
    }
    for g in Gas::ALL {
        if air.pressure_pa() * air.composition().fraction(g)
            > cap_pa(g, air.temperature_k()) * (1.0 + 1e-8)
        {
            return Err(Error::InvalidClosure(Material::from_gas(g)));
        }
    }
    let represented = 4.0 * PI * radius.powi(4) / gm
        * air.pressure_pa()
        * column.profile.mass_factor(air.pressure_pa());
    if (represented / gas.total_mass_kg() - 1.0).abs() > 1e-9 {
        return Err(Error::InvalidInput("column mass closure"));
    }
    Ok(())
}

/// Composition guard is by mass, not mole fraction, and has no H/He exemption.
/// Small admixtures simply ride along in this declared ideal-mixture proxy.
pub(super) fn hot_steam(air: &GasAtmosphere) -> bool {
    air.temperature_k() >= 1000.0
        && air.composition().fraction(Gas::Water) * Gas::Water.molecular_mass_u()
            / (air.composition().mean_molecular_mass_kg() / ATOMIC_MASS_KG)
            >= 0.9
}

pub(super) fn inertia_kg_m2(s: &CoupledStructure) -> f64 {
    let Some(air) = s.atmosphere() else {
        return 0.0;
    };
    // A successful coupling has already validated this spherical column.
    let c = s.column().unwrap().unwrap();
    let radius = s.surface_radius_m();
    let end = Profile::cutoff_drop(air.pressure_pa()).min(64.0);
    const STEPS: usize = 512;
    let dz = end / STEPS as f64;
    let at = |z: f64| (-z).exp() / (1.0 - z * c.scale_height_m / radius).powi(6);
    let mut sum = at(0.0) + at(end);
    for i in 1..STEPS {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * at(i as f64 * dz);
    }
    (2.0 / 3.0) * s.gas.total_mass_kg() * radius.powi(2) * (sum * dz / 3.0)
        / c.profile.mass_factor(air.pressure_pa())
}
