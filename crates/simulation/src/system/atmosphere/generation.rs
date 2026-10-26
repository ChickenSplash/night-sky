//! Inventory-backed atmospheric population with approximate cold trapping.
use super::condensation::vapour_pressure;
use super::*;
use crate::{
    light, orbit,
    seed::Rng,
    star::multiple::Bodies,
    units::{EARTH_SOLAR, SUN_GM},
};
use glam::DVec3;
use space_shared::{PlanetEnvironment, Star, StarSystem};
use std::f64::consts::TAU;

pub(crate) mod partition;

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
/// Reconcile bulk structure and spin first using the same heating samples.
/// Neither stage changes orbital placement or formation inventories.
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
        let id = hosted.id();
        crate::system::generation::retention::finalise(
            &mut hosted.body,
            &id,
            system.member_mass(0, &hosted.host),
            heating.flux,
            heating.loss_flux,
            age_gyr,
        );
        let mut rng = Rng::new(&format!("{}:atmosphere:v1", hosted.id()));
        hosted.environment = Some(
            generate(&mut rng, &mut hosted.body, heating, age_gyr)
                .expect("generated atmospheric inputs and columns must be finite and valid"),
        );
        crate::system::generation::retention::refresh_spin(
            &mut hosted.body,
            &id,
            system.member_mass(0, &hosted.host),
        );
    }
    // Formation masses were used during spacing; retained masses can only fall.
    for (host, planets) in bodies.by_host() {
        for pair in planets.windows(2) {
            assert!(
                crate::system::generation::stable(
                    pair[0],
                    pair[1].orbit.semi_major_au,
                    pair[1].orbit.eccentricity,
                    pair[1].mass_earth,
                    bodies.mass(host)
                ),
                "retained planets must remain Hill stable"
            );
        }
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
pub(crate) fn secondary_retention(escape_m_s: f64, loss_flux: f64) -> f64 {
    let threshold = 10.0 * (escape_m_s / 11_200.0).powi(4);
    1.0 / (1.0 + (loss_flux / SOLAR_FLUX_W_M2 / threshold).powf(1.5))
}

fn generate(
    rng: &mut Rng,
    body: &mut crate::system::Body,
    heating: Heating,
    age_gyr: f64,
) -> Result<PlanetEnvironment, AtmosphereError> {
    let albedo = rng.range(0.1, 0.6);
    let equilibrium_temperature_k =
        (heating.flux * (1.0 - albedo) / (4.0 * STEFAN_BOLTZMANN)).powf(0.25);
    // No greenhouse, internal-heat or climate solution yet. Floor the column
    // estimate at 20 K rather than extending an ideal gas to absolute zero.
    let temperature = equilibrium_temperature_k.max(20.0);
    let (structure, budget, atmosphere) = crate::system::structure::boundary::resolve(
        &body.budget,
        heating.flux,
        age_gyr,
        temperature,
    )?;
    body.radius_km = structure.radius_m() / 1e3;
    body.structure = structure;
    body.budget = budget;
    let gm = body.mass_earth * EARTH_SOLAR * SUN_GM;
    let radius = body.radius_km * 1e3;
    let atmosphere = atmosphere.map(|air| air.to_data(gm, radius)).transpose()?;
    Ok(PlanetEnvironment {
        irradiation_w_m2: heating.flux,
        assumed_bond_albedo: albedo,
        equilibrium_temperature_k,
        age_gyr,
        atmosphere,
    })
}

#[cfg(test)]
mod tests;
