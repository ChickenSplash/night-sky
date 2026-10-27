//! Supply-limited partition, without generation's source/exposure priors.
use super::*;
use crate::{
    system::atmosphere::{Gas, ReferenceLevel, condensation::vapour_pressure, profile::Profile},
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};

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

pub(super) fn partition(request: &Request, mass: f64, radius: f64) -> Result<Inventory, Error> {
    if request.exchangeable.total_mass_kg() == 0.0 {
        return Ok(Inventory::default());
    }
    let weight_per_area = GRAVITATIONAL_CONSTANT * mass / (4.0 * PI * radius.powi(4));
    let mut low = 0.0;
    let mut high = Gas::ALL
        .iter()
        .map(|g| g.molecular_mass_u())
        .fold(0.0, f64::max)
        * weight_per_area
        / (1.0 - (-crate::system::atmosphere::EXTENT_SCALE_HEIGHTS).exp());
    let allocated = |c: f64| {
        Inventory::new(&Gas::ALL.map(|g| {
            let m = Material::from_gas(g);
            (
                m,
                request.exchangeable.mass_kg(m).min(
                    vapour_pressure(g, request.column_temperature_k) * g.molecular_mass_u() / c,
                ),
            )
        }))
        .map_err(Error::Composition)
    };
    for _ in 0..64 {
        let c = 0.5 * (low + high);
        let gas = allocated(c)?;
        let moles: f64 = Gas::ALL
            .iter()
            .map(|&g| gas.mass_kg(Material::from_gas(g)) / g.molecular_mass_u())
            .sum();
        if moles <= 0.0 || !moles.is_finite() {
            return Err(Error::InvalidInput("unresolvable gas allocation"));
        }
        let pressure = c * moles;
        let mean_mass = gas.total_mass_kg() / moles * ATOMIC_MASS_KG;
        let height = BOLTZMANN_J_K * request.column_temperature_k
            / mean_mass
            / (GRAVITATIONAL_CONSTANT * mass / radius.powi(2));
        let profile = Profile::new(radius, height, pressure);
        if pressure * profile.mass_factor(pressure) < gas.total_mass_kg() * weight_per_area {
            low = c;
        } else {
            high = c;
        }
    }
    let gas = allocated(high)?;
    if gas.total_mass_kg() / request.retained.total_mass_kg() > MAX_GAS_MASS_FRACTION {
        return Err(Error::GasMassOutsideDomain);
    }
    Ok(gas)
}

pub(super) fn validate(
    gas: &Inventory,
    air: Option<&GasAtmosphere>,
    gm: f64,
    radius: f64,
) -> Result<(), Error> {
    let Some(air) = air else {
        return Ok(());
    };
    debug_assert_eq!(air.reference_level(), ReferenceLevel::Surface);
    let column = air.column(gm, radius).map_err(Error::Atmosphere)?;
    if !column.profile.spherical {
        return Err(Error::WeakBindingUnsupported);
    }
    if column.density_kg_m3 > MAX_GAS_DENSITY_KG_M3 {
        return Err(Error::DenseGasUnsupported);
    }
    for g in Gas::ALL {
        if air.pressure_pa() * air.composition().fraction(g)
            > vapour_pressure(g, air.temperature_k()) * (1.0 + 1e-8)
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

pub(super) fn inertia_kg_m2(s: &CoupledStructure) -> f64 {
    let Some(air) = s.atmosphere() else {
        return 0.0;
    };
    // A successful coupling has already validated this spherical column.
    let c = s.column().unwrap().unwrap();
    let radius = s.surface_radius_m();
    let end = Profile::cutoff_drop(air.pressure_pa()).min(64.0);
    const STEPS: usize = 256;
    let dz = end / STEPS as f64;
    let at = |z: f64| (-z).exp() / (1.0 - z * c.scale_height_m / radius).powi(6);
    let mut sum = at(0.0) + at(end);
    for i in 1..STEPS {
        sum += if i % 2 == 0 { 2.0 } else { 4.0 } * at(i as f64 * dz);
    }
    (2.0 / 3.0) * s.gas.total_mass_kg() * radius.powi(2) * (sum * dz / 3.0)
        / c.profile.mass_factor(air.pressure_pa())
}
