//! Validated transport boundary for inventories and reference structure.
use super::{
    Body,
    atmosphere::{GasAtmosphere, generation::MIN_PRESSURE_PA, optics::boundary::OpticalColumn},
};
use crate::units::EARTH_MASS_KG;
use space_shared::{Planet, PlanetPhysical};

mod export;
mod layered;
pub(crate) use export::export_planet;
mod refinement;
mod result;
mod synchronous;
pub use layered::{fallback, layered, outcome};
pub use refinement::{Refinement, prepare_refinement};
pub(crate) use result::initial_from_fallback;
pub use result::{
    initial_result, resolved_result, validate_record, validate_refinement, validate_result,
};
pub use synchronous::{PreparedInitial, initial_with_atmosphere};

/// The live physical record is the authoritative result's record, verbatim.
pub fn to_data(body: &Body) -> PlanetPhysical {
    body.physical().clone()
}

/// Missing and invalid records are distinct. No label-based physical defaults.
/// Checks that every live projection is the transported result's: no
/// structure, EOS or partition solve is re-run.
pub fn validate(planet: &Planet) -> Result<Option<&PlanetPhysical>, &'static str> {
    let Some(data) = &planet.physical else {
        return if planet.initial_result.is_some() {
            Err("result without its physical projection")
        } else {
            Ok(None)
        };
    };
    // Records written before the cutover cannot be checked without re-running
    // the removed legacy model; they are invalid, not silently trusted.
    let result = planet
        .initial_result
        .as_ref()
        .ok_or("physical record without its result")?;
    validate_result(result)?;
    let record = &result.record;
    if *data != record.physical {
        return Err("physical record differs from its result");
    }
    let close = |a: f64, b: f64| {
        a.is_finite() && b.is_finite() && (a - b).abs() <= a.abs().max(b.abs()) * 1e-12
    };
    if !close(planet.mass_earth * EARTH_MASS_KG, data.retained_mass_kg)
        || !close(planet.radius_km * 1000.0, data.reference_radius_m)
        || planet.inertia_factor != Some(data.inertia_factor)
    {
        return Err("inconsistent reference facts");
    }
    let env = planet
        .environment
        .as_ref()
        .ok_or("missing structure forcing")?;
    if !env.equilibrium_temperature_k.is_finite()
        || env.equilibrium_temperature_k < 0.0
        || (result.provenance == space_shared::ResultProvenance::Fallback
            && if record.greenhouse_optical_depth.is_some()
                || data.radius_reference == space_shared::RadiusReference::OneBar
            {
                // A decided surface is greenhouse-warmed and a one-bar level
                // carries intrinsic heat: never cooler than equilibrium.
                record.surface_temperature_k
                    < env.equilibrium_temperature_k.max(20.0) * (1.0 - 1e-12)
            } else {
                !close(
                    env.equilibrium_temperature_k.max(20.0),
                    record.surface_temperature_k,
                )
            })
    {
        return Err("inconsistent boundary temperature");
    }
    match &env.atmosphere {
        None if record.surface_pressure_pa >= MIN_PRESSURE_PA => Err("missing boundary atmosphere"),
        None => Ok(Some(data)),
        Some(air) => {
            let air = GasAtmosphere::try_from(air).map_err(|_| "invalid boundary atmosphere")?;
            OpticalColumn::reference_from_result(result, &air)
                .map_err(|_| "inconsistent boundary atmosphere")?;
            Ok(Some(data))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use space_shared::RadiusReference;

    #[test]
    fn small_escaped_remainders_allow_species_local_subtraction_roundoff_only() {
        // Cold partition splits a retained molecule across gas/condensate. Its
        // recombined mass differs by one ulp, larger than 1e-10 of its tiny loss.
        let system = crate::galactic_bodies(
            "composition-calibration:Halo:239",
            crate::galaxy::Component::Halo,
        );
        let p = system
            .planets
            .iter()
            .find(|p| p.id().ends_with("/8"))
            .unwrap();
        let mut planet = crate::demo_galaxy().systems[0].planets[0].clone();
        planet.mass_earth = p.body.mass_earth;
        planet.radius_km = p.body.radius_km;
        planet.environment = p.environment.clone();
        planet.inertia_factor = Some(p.body.physical().inertia_factor);
        planet.physical = Some(to_data(&p.body));
        planet.initial_result = Some(p.body.initial_result.clone());
        validate(&planet).unwrap().unwrap();
        let round_trip: Planet =
            serde_json::from_str(&serde_json::to_string(&planet).unwrap()).unwrap();
        validate(&round_trip).unwrap().unwrap();
        // Corrupt the result and its live copy together, so the result's own
        // transport checks are what reject it, not a copy mismatch.
        let corrupt = |material: &str, escaped: f64| {
            let mut corrupt = planet.clone();
            let result = corrupt.initial_result.as_mut().unwrap();
            for record in [
                corrupt.physical.as_mut().unwrap(),
                &mut result.record.physical,
            ] {
                record
                    .materials
                    .iter_mut()
                    .find(|p| p.material == material)
                    .unwrap()
                    .escaped_kg = escaped;
            }
            corrupt
        };
        for value in [-1.0, f64::NAN, f64::INFINITY, 1e20] {
            assert!(validate(&corrupt("methane", value)).is_err());
        }
        // An absent species cannot borrow tolerance from the massive planet.
        assert!(validate(&corrupt("oxygen", 1e-100)).is_err());
    }

    #[test]
    fn live_projections_must_all_come_from_the_one_result() {
        let galaxy = crate::demo_galaxy();
        let planets: Vec<_> = galaxy.systems.iter().flat_map(|s| &s.planets).collect();
        let air = planets
            .iter()
            .find(|p| {
                p.environment
                    .as_ref()
                    .is_some_and(|e| e.atmosphere.is_some())
            })
            .unwrap();
        type Corrupt = fn(&mut Planet);
        let corruptions: [Corrupt; 7] = [
            |p| p.radius_km *= 1.0 + 1e-9,
            |p| p.mass_earth *= 1.0 + 1e-9,
            |p| *p.inertia_factor.as_mut().unwrap() *= 1.0 + 1e-9,
            |p| p.initial_result = None,
            |p| p.environment.as_mut().unwrap().equilibrium_temperature_k += 50.0,
            |p| p.environment.as_mut().unwrap().atmosphere = None,
            |p| {
                let air = p.environment.as_mut().unwrap().atmosphere.as_mut().unwrap();
                air.pressure_pa *= 2.0;
            },
        ];
        for (index, corrupt) in corruptions.into_iter().enumerate() {
            let mut bad = (*air).clone();
            corrupt(&mut bad);
            assert!(validate(&bad).is_err(), "corruption {index}");
        }
        // A positive sub-threshold column stays in the result but is omitted
        // from the live projection; an omitted column above it is invalid.
        let mut trace = 0;
        for index in 0..40 {
            let key = format!("g13b6ffffcfeba.2.{index}");
            let Some(system) = super::super::generated_system(&key, [0.0; 3]) else {
                continue;
            };
            for p in &system.planets {
                let record = &p.initial_result.as_ref().unwrap().record;
                let omitted = p.environment.as_ref().unwrap().atmosphere.is_none();
                validate(p).unwrap().unwrap();
                assert_eq!(omitted, record.surface_pressure_pa < MIN_PRESSURE_PA);
                trace += usize::from(omitted && record.surface_pressure_pa > 0.0);
            }
        }
        assert!(trace > 0);
    }

    #[test]
    fn generated_and_overlay_records_round_trip_and_labels_do_not_drive_consumers() {
        let galaxy = crate::demo_galaxy();
        let mut count = 0;
        for system in &galaxy.systems {
            for p in &system.planets {
                validate(p).unwrap().unwrap();
                let encoded = serde_json::to_string(p).unwrap();
                let decoded: Planet = serde_json::from_str(&encoded).unwrap();
                validate(&decoded).unwrap().unwrap();
                let mut renamed = decoded.clone();
                renamed.class = "Arbitrary threshold change".into();
                assert_eq!(
                    crate::light::planet_surface(p),
                    crate::light::planet_surface(&renamed)
                );
                assert_eq!(
                    super::super::planet_shape(p),
                    super::super::planet_shape(&renamed)
                );
                let key = crate::star::multiple::system_key(&p.host);
                if let Some(natural) = super::super::generated_system(key, [0.0; 3]) {
                    let same = natural.planets.iter().find(|n| n.id == p.id).unwrap();
                    assert_eq!(p.physical, same.physical);
                    assert_eq!(p.initial_result, same.initial_result);
                    assert_eq!(p.class, same.class);
                    count += 1;
                }
            }
        }
        assert!(count > 20);
    }

    #[test]
    fn missing_is_unknown_and_corrupted_records_are_rejected() {
        let galaxy = crate::demo_galaxy();
        let p = &galaxy.systems[0].planets[0];
        let mut legacy = serde_json::to_value(p).unwrap();
        legacy.as_object_mut().unwrap().remove("physical");
        assert!(validate(&serde_json::from_value::<Planet>(legacy.clone()).unwrap()).is_err());
        legacy.as_object_mut().unwrap().remove("initial_result");
        let legacy: Planet = serde_json::from_value(legacy).unwrap();
        assert_eq!(validate(&legacy), Ok(None));
        for corrupt in 0..10 {
            let mut bad = p.clone();
            let d = bad.physical.as_mut().unwrap();
            match corrupt {
                0 => d.materials[0].retained_kg *= 0.5,
                1 => d.materials[0].source_kg = -1.0,
                2 => d.materials[0].material = "unmodelled".into(),
                3 => {
                    d.materials.pop();
                }
                4 => d.materials[0].material = d.materials[1].material.clone(),
                5 => d.mean_density_kg_m3 = f64::NAN,
                6 => {
                    d.radius_reference = if d.radius_reference == RadiusReference::OneBar {
                        RadiusReference::SolidSurface
                    } else {
                        RadiusReference::OneBar
                    }
                }
                7 => d.reference_radius_m *= 2.0,
                8 => d.approximations.push("invented".into()),
                _ => d.h_he_mass_fraction = 1.0,
            }
            assert!(validate(&bad).is_err(), "corruption {corrupt}");
        }
    }
}
