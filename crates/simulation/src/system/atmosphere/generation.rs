//! Initial, deliberately heuristic atmospheric population. Class affects only
//! the provisional volatile/envelope prior. Retention and physical columns do
//! not depend on class. Replace these priors with bulk inventories later.
use super::*;
use crate::{
    light, orbit,
    seed::Rng,
    star::multiple::Bodies,
    system::Class,
    units::{EARTH_SOLAR, SUN_GM},
};
use glam::DVec3;
use space_shared::{PlanetEnvironment, Star, StarSystem};
use std::f64::consts::TAU;

/// SI Stefan-Boltzmann constant (rounded).
const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;
/// Fixed quadrature budget over independent mean anomalies, not clock time.
const PHASE_SAMPLES: usize = 32;
const MIN_PRESSURE_PA: f64 = 0.1;
const SOLAR_FLUX_W_M2: f64 = 1361.0;

#[derive(Clone, Copy, Debug)]
struct Heating {
    flux: f64,
    /// Flux weighted by an uncalibrated activity/history proxy, not actual XUV.
    loss_flux: f64,
}

/// Fill only after the entire stellar hierarchy and final stable orbits exist.
/// No generation draw here can change masses, radii, spins or orbital placement.
pub(crate) fn populate(bodies: &mut Bodies, age_gyr: f64) {
    let system = StarSystem {
        id: String::new(),
        name: String::new(),
        position_ly: [0.0; 3],
        stars: bodies.stars.clone(),
        barycentres: bodies.barycentres.clone(),
        planets: Vec::new(),
        page_stars: Vec::new(),
    };
    let samples = stellar_samples(&system);
    for hosted in &mut bodies.planets {
        let heating = heating(&system, &samples, &hosted.host, hosted.body.orbit, age_gyr);
        let mut rng = Rng::new(&format!("{}:atmosphere:v1", hosted.id()));
        hosted.environment = Some(
            generate(&mut rng, &hosted.body, heating, age_gyr)
                .expect("generated atmospheric inputs and columns must be finite and valid"),
        );
    }
}

/// Radical inverse gives a deterministic low-discrepancy phase sequence.
/// Each orbit gets a different prime, so long-period companions are included
/// without a finite clock window freezing them at their initial positions.
fn phase(mut index: usize, base: usize) -> f64 {
    let mut unit = 0.0;
    let mut weight = 1.0 / base as f64;
    while index > 0 {
        unit += (index % base) as f64 * weight;
        index /= base;
        weight /= base as f64;
    }
    unit * TAU
}

fn stellar_samples(system: &StarSystem) -> Vec<Vec<(&str, [f64; 3])>> {
    let mut sample = system.clone();
    (1..=PHASE_SAMPLES)
        .map(|n| {
            for (pair, prime) in sample.barycentres.iter_mut().zip([2, 3, 5]) {
                pair.orbit.phase_rad = phase(n, prime);
            }
            // Returned member IDs must borrow the original system, not the sample.
            let placed = orbit::member_positions(&sample, 0, 0.0);
            system
                .stars
                .iter()
                .map(|star| star.id.as_str())
                .chain(system.barycentres.iter().map(|pair| pair.id.as_str()))
                .map(|id| (id, orbit::find(&placed, id).unwrap_or([0.0; 3])))
                .collect()
        })
        .collect()
}

fn heating(
    system: &StarSystem,
    samples: &[Vec<(&str, [f64; 3])>],
    host: &str,
    mut planet_orbit: space_shared::Orbit,
    age_gyr: f64,
) -> Heating {
    let mut heat = Heating {
        flux: 0.0,
        loss_flux: 0.0,
    };
    for (n, placed) in samples.iter().enumerate() {
        // Stratify the planet's full mean-anomaly range. Unlike the first N
        // points of a high-base radical inverse, this never undersamples its
        // periapsis and reproduces the isolated-star eccentric-orbit average.
        planet_orbit.phase_rad = TAU * (n as f64 + 0.5) / samples.len() as f64;
        let host_position = DVec3::from_array(orbit::find(placed, host).unwrap_or([0.0; 3]));
        let local = DVec3::from_array(orbit::position(
            &planet_orbit,
            system.member_mass(0, host),
            0.0,
        ));
        let planet = host_position + local;
        for star in &system.stars {
            let at = DVec3::from_array(orbit::find(placed, &star.id).unwrap_or([0.0; 3]));
            let distance = (at - planet)
                .length()
                .max(star.radius_solar * crate::SUN_RADIUS_METRES);
            let flux = light::flux(star.luminosity_solar, distance);
            heat.flux += flux / samples.len() as f64;
            heat.loss_flux += flux * activity_history(star, age_gyr) / samples.len() as f64;
        }
    }
    heat
}

/// Saturation then inverse-age decline integrated over system age. Low-mass
/// stars stay active longer. Hot-star factor loosely proxies UV, not a stellar
/// spectrum/evolution calculation. Normalisation makes a 4.5 Gyr Sun ~1.
fn activity_history(star: &Star, age_gyr: f64) -> f64 {
    let saturation_gyr = (0.1 / star.mass_solar.powi(2)).clamp(0.05, 3.0);
    let integrated = if age_gyr <= saturation_gyr {
        age_gyr
    } else {
        saturation_gyr * (1.0 + (age_gyr / saturation_gyr).ln())
    };
    let hot = (star.temperature_k / 5772.0).powi(4).clamp(1.0, 30.0);
    // A minimum forcing avoids treating newborn systems as loss-free.
    (integrated / 0.48).max(0.05) * hot
}

/// Empirical shoreline-inspired trend, not a universal survival boundary.
/// At Earth's escape speed, the nominal transition is ten present solar fluxes.
fn secondary_retention(escape_m_s: f64, loss_flux: f64) -> f64 {
    let threshold = 10.0 * (escape_m_s / 11_200.0).powi(4);
    1.0 / (1.0 + (loss_flux / SOLAR_FLUX_W_M2 / threshold).powf(1.5))
}

/// Only this initial inventory prior uses labels. No class is forced airless
/// or forced into one chemical mixture. Bulk-composition work replaces this.
fn envelope_prior(class: Class) -> f64 {
    match class {
        Class::GasGiant => 0.99,
        Class::IceGiant => 0.95,
        Class::SubNeptune => 0.8,
        Class::Rocky => 0.03,
        Class::Dwarf => 0.005,
    }
}

fn generate(
    rng: &mut Rng,
    body: &crate::system::Body,
    heating: Heating,
    age_gyr: f64,
) -> Result<PlanetEnvironment, AtmosphereError> {
    let albedo = rng.range(0.1, 0.6);
    let equilibrium_temperature_k =
        (heating.flux * (1.0 - albedo) / (4.0 * STEFAN_BOLTZMANN)).powf(0.25);
    // No greenhouse, internal-heat or climate solution yet. Floor the column
    // estimate at 20 K rather than extending an ideal gas to absolute zero.
    let temperature = equilibrium_temperature_k.max(20.0);
    let gm = body.mass_earth * EARTH_SOLAR * SUN_GM;
    let radius = body.radius_km * 1e3;
    let escape = (2.0 * gm / radius).sqrt();
    let retention = secondary_retention(escape, heating.loss_flux);
    let exosphere_k =
        (1000.0 * (heating.loss_flux / SOLAR_FLUX_W_M2).max(0.01).powf(0.2)).max(2.0 * temperature);
    let jeans_h2 = gm / radius * Gas::Hydrogen.molecular_mass_u() * ATOMIC_MASS_KG
        / (BOLTZMANN_J_K * exosphere_k);
    // Soft retention trend only: not a solved Jeans/hydrodynamic escape rate.
    let hydrogen_survival = 1.0 / (1.0 + (25.0 / jeans_h2).powi(4));
    let atmosphere = if rng.chance(envelope_prior(body.class) * hydrogen_survival) {
        let methane = if temperature < 300.0 {
            rng.log_range(1e-4, 0.02)
        } else {
            1e-5
        };
        let methane = methane.min(vapour_pressure(Gas::Methane, temperature) / ONE_BAR_PA);
        let helium = rng.range(0.10, 0.20);
        let composition = Composition::new(&[
            (Gas::Hydrogen, 1.0 - helium - methane),
            (Gas::Helium, helium),
            (Gas::Methane, methane),
        ])?;
        // Defines a reference level, not the total envelope mass or its bottom.
        Some(GasAtmosphere::new(
            composition,
            ReferenceLevel::OneBar,
            ONE_BAR_PA,
            temperature,
            None,
        )?)
    } else if rng.chance(0.95 * retention) {
        secondary(rng, temperature, retention, body.mass_earth)?
    } else {
        None
    };
    let atmosphere = atmosphere.map(|air| air.to_data(gm, radius)).transpose()?;
    Ok(PlanetEnvironment {
        irradiation_w_m2: heating.flux,
        assumed_bond_albedo: albedo,
        equilibrium_temperature_k,
        age_gyr,
        atmosphere,
    })
}

fn secondary(
    rng: &mut Rng,
    temperature: f64,
    retention: f64,
    mass_earth: f64,
) -> Result<Option<GasAtmosphere>, AtmosphereError> {
    let supply = (rng.log_range(1.0, 1e7) * mass_earth.powf(0.4)).min(1e8) * retention.powi(2);
    let co2 = rng.range(0.02, 0.98);
    let methane = if temperature < 200.0 {
        rng.log_range(1e-5, 0.03)
    } else {
        1e-6
    };
    let argon = rng.range(0.001, 0.02);
    let water = rng.log_range(1e-5, 0.3);
    let parts = [
        (Gas::CarbonDioxide, co2 * (1.0 - methane - argon - water)),
        (Gas::Nitrogen, (1.0 - co2) * (1.0 - methane - argon - water)),
        (Gas::Methane, methane),
        (Gas::Argon, argon),
        (Gas::Water, water),
    ];
    let partial: Vec<_> = parts
        .into_iter()
        .map(|(gas, fraction)| {
            (
                gas,
                (supply * fraction).min(vapour_pressure(gas, temperature)),
            )
        })
        .collect();
    let pressure: f64 = partial.iter().map(|(_, pressure)| pressure).sum();
    if pressure < MIN_PRESSURE_PA {
        return Ok(None);
    }
    let fractions: Vec<_> = partial
        .into_iter()
        .map(|(gas, p)| (gas, p / pressure))
        .collect();
    let composition = Composition::new(&fractions)?;
    let clear = GasAtmosphere::new(
        composition,
        ReferenceLevel::Surface,
        pressure,
        temperature,
        None,
    )?;
    // Aerosol generation is deliberately deferred: a gas mixture alone is
    // insufficient evidence for dust loading, clouds or photochemical haze.
    Ok(Some(clear))
}

/// Rough Clausius-Clapeyron caps, anchored near a measured phase point.
/// Only a cold-trap heuristic, not a phase diagram or hydrosphere model.
fn vapour_pressure(gas: Gas, temperature: f64) -> f64 {
    let (anchor_k, anchor_pa, latent_over_r, critical_k) = match gas {
        Gas::CarbonDioxide => (194.65, 101_325.0, 3030.0, 304.13),
        Gas::Nitrogen => (77.36, 101_325.0, 670.0, 126.2),
        Gas::Methane => (111.66, 101_325.0, 980.0, 190.56),
        Gas::Argon => (87.30, 101_325.0, 770.0, 150.69),
        Gas::Water if temperature < 273.16 => (273.16, 611.657, 6140.0, 647.1),
        Gas::Water => (273.16, 611.657, 5420.0, 647.1),
        _ => return f64::INFINITY,
    };
    // Do not impose a saturation constraint on supercritical gas. Below the
    // critical temperature, a single latent heat is only an approximation.
    if temperature >= critical_k {
        return f64::INFINITY;
    }
    anchor_pa * (-latent_over_r * (1.0 / temperature - 1.0 / anchor_k)).exp()
}

#[cfg(test)]
mod tests;
