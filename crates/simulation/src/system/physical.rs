//! Validated transport boundary for inventories and reference structure.
use super::{
    Body,
    composition::{Budget, Inventory, Material, Reservoir},
    structure,
};
use space_shared::{MaterialBudget, Planet, PlanetPhysical, RadiusReference};

mod layered;
pub use layered::layered;

pub fn to_data(body: &Body) -> PlanetPhysical {
    let b = &body.budget;
    let s = &body.structure;
    PlanetPhysical {
        materials: Material::ALL
            .into_iter()
            .map(|m| MaterialBudget {
                material: m.id().into(),
                source_kg: b.source().mass_kg(m),
                retained_kg: b.retained().mass_kg(m),
                escaped_kg: b.escaped().mass_kg(m),
                interior_kg: b.reservoir(Reservoir::Interior).mass_kg(m),
                condensed_kg: b.reservoir(Reservoir::CondensedVolatiles).mass_kg(m),
                gas_kg: b.reservoir(Reservoir::Gas).mass_kg(m),
            })
            .collect(),
        retained_mass_kg: s.mass_kg(),
        reference_radius_m: s.radius_m(),
        condensed_radius_m: s.condensed_radius_m(),
        mean_density_kg_m3: s.mean_density_kg_m3(),
        reference_gravity_m_s2: s.reference_gravity_m_s2(),
        h_he_mass_fraction: s.h_he_mass_fraction(),
        inertia_factor: s.inertia_factor(),
        radius_reference: match s.reference() {
            structure::RadiusReference::SolidSurface => RadiusReference::SolidSurface,
            structure::RadiusReference::UnresolvedVolatileBoundary => {
                RadiusReference::UnresolvedVolatileBoundary
            }
            structure::RadiusReference::OneBar => RadiusReference::OneBar,
        },
        approximations: s.approximations().iter().map(|a| a.id().into()).collect(),
    }
}

/// Missing and invalid records are distinct. No label-based physical defaults.
/// Re-derive structure at the boundary, also validating reference/caveat semantics.
pub fn validate(planet: &Planet) -> Result<Option<&PlanetPhysical>, &'static str> {
    let Some(data) = &planet.physical else {
        return Ok(None);
    };
    let env = planet
        .environment
        .as_ref()
        .ok_or("missing structure forcing")?;
    let inventory = |mass: fn(&MaterialBudget) -> f64| -> Result<Inventory, &'static str> {
        let parts = data
            .materials
            .iter()
            .map(|p| {
                Ok((
                    Material::from_id(&p.material).ok_or("unknown material")?,
                    mass(p),
                ))
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        Inventory::new(&parts).map_err(|_| "invalid material inventory")
    };
    if data.materials.len() != Material::ALL.len() {
        return Err("incomplete catalogue");
    }
    let budget = Budget::new(
        inventory(|p| p.source_kg)?,
        inventory(|p| p.interior_kg)?,
        inventory(|p| p.condensed_kg)?,
        inventory(|p| p.gas_kg)?,
    )
    .map_err(|_| "invalid reservoir budget")?;
    let close = |a: f64, b: f64| {
        a.is_finite() && b.is_finite() && (a - b).abs() <= a.abs().max(b.abs()) * 1e-10
    };
    for p in &data.materials {
        let m = Material::from_id(&p.material).unwrap();
        // Reservoir transfers preserve authoritative escaped mass, but summing
        // split retained reservoirs can move one ulp. Re-subtracting a tiny loss
        // from its much larger source magnifies that roundoff relative to loss.
        // Scale only this subtraction allowance to the same species' source,
        // never total planet mass; it cannot create reservoir supply.
        let escaped = budget.escaped().mass_kg(m);
        let loss_roundoff = 8.0 * f64::EPSILON * p.source_kg;
        if !close(p.retained_kg, budget.retained().mass_kg(m))
            || !p.escaped_kg.is_finite()
            || p.escaped_kg < 0.0
            || (p.escaped_kg - escaped).abs() > loss_roundoff + 1e-10 * p.escaped_kg.max(escaped)
        {
            return Err("inconsistent material totals");
        }
    }
    if !env.equilibrium_temperature_k.is_finite() || env.equilibrium_temperature_k < 0.0 {
        return Err("invalid boundary temperature");
    }
    let (s, partitioned, atmosphere) = structure::boundary::resolve(
        &budget,
        env.irradiation_w_m2,
        env.age_gyr,
        env.equilibrium_temperature_k.max(20.0),
    )
    .map_err(|_| "invalid reference structure")?;
    for p in &data.materials {
        let material = Material::from_id(&p.material).unwrap();
        // Repartitioning near a protected reservoir can lose source-scale ulps.
        let allowance = 8.0 * f64::EPSILON * p.source_kg;
        for (actual, expected) in [
            (
                p.gas_kg,
                partitioned.reservoir(Reservoir::Gas).mass_kg(material),
            ),
            (
                p.condensed_kg,
                partitioned
                    .reservoir(Reservoir::CondensedVolatiles)
                    .mass_kg(material),
            ),
        ] {
            if (actual - expected).abs() > allowance + 1e-10 * actual.max(expected) {
                return Err("inconsistent boundary partition");
            }
        }
    }
    // Compare molecular fractions to the actual transported reservoir, not a
    // second subtraction of tiny cold-trapped traces from protected supplies.
    let mixture = budget
        .reservoir(Reservoir::Gas)
        .gas_composition()
        .map_err(|_| "invalid boundary mixture")?;
    match (&env.atmosphere, &atmosphere) {
        (None, None) => {}
        (Some(actual), Some(expected)) => {
            use super::atmosphere::{Gas, GasAtmosphere, ONE_BAR_PA, ReferenceLevel};
            let actual =
                GasAtmosphere::try_from(actual).map_err(|_| "invalid boundary atmosphere")?;
            let mixture = mixture.ok_or("missing boundary gas")?;
            let pressure = if actual.reference_level() == ReferenceLevel::OneBar {
                ONE_BAR_PA
            } else {
                budget.reservoir(Reservoir::Gas).total_mass_kg() * s.reference_gravity_m_s2()
                    / (4.0 * std::f64::consts::PI * s.radius_m().powi(2))
                    / actual
                        .column(
                            s.mass_kg() * crate::units::GRAVITATIONAL_CONSTANT,
                            s.radius_m(),
                        )
                        .map_err(|_| "invalid boundary column")?
                        .profile
                        .mass_factor(actual.pressure_pa())
            };
            if actual.reference_level() != expected.reference_level()
                || !close(actual.pressure_pa(), pressure)
                || !close(actual.temperature_k(), expected.temperature_k())
                || !Gas::ALL
                    .iter()
                    .all(|&gas| close(actual.composition().fraction(gas), mixture.fraction(gas)))
            {
                return Err("inconsistent boundary atmosphere");
            }
        }
        _ => return Err("inconsistent boundary atmosphere"),
    }
    if ![
        (data.retained_mass_kg, s.mass_kg()),
        (data.reference_radius_m, s.radius_m()),
        (data.condensed_radius_m, s.condensed_radius_m()),
        (data.mean_density_kg_m3, s.mean_density_kg_m3()),
        (data.reference_gravity_m_s2, s.reference_gravity_m_s2()),
        (data.h_he_mass_fraction, s.h_he_mass_fraction()),
        (data.inertia_factor, s.inertia_factor()),
        (planet.mass_earth * crate::units::EARTH_MASS_KG, s.mass_kg()),
        (planet.radius_km * 1000.0, s.radius_m()),
        (
            planet.inertia_factor.ok_or("missing physical inertia")?,
            s.inertia_factor(),
        ),
    ]
    .into_iter()
    .all(|(a, b)| close(a, b))
    {
        return Err("inconsistent reference facts");
    }
    let reference = match s.reference() {
        structure::RadiusReference::SolidSurface => RadiusReference::SolidSurface,
        structure::RadiusReference::UnresolvedVolatileBoundary => {
            RadiusReference::UnresolvedVolatileBoundary
        }
        structure::RadiusReference::OneBar => RadiusReference::OneBar,
    };
    let caveats: Vec<_> = s.approximations().iter().map(|a| a.id()).collect();
    if data.radius_reference != reference || data.approximations != caveats {
        return Err("inconsistent reference semantics");
    }
    Ok(Some(data))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        planet.inertia_factor = Some(p.body.structure.inertia_factor());
        planet.physical = Some(to_data(&p.body));
        validate(&planet).unwrap().unwrap();
        let round_trip: Planet =
            serde_json::from_str(&serde_json::to_string(&planet).unwrap()).unwrap();
        validate(&round_trip).unwrap().unwrap();
        for value in [-1.0, f64::NAN, f64::INFINITY, 1e20] {
            let mut corrupt = planet.clone();
            corrupt
                .physical
                .as_mut()
                .unwrap()
                .materials
                .iter_mut()
                .find(|p| p.material == "methane")
                .unwrap()
                .escaped_kg = value;
            assert!(validate(&corrupt).is_err());
        }
        // An absent species cannot borrow tolerance from the massive planet.
        planet
            .physical
            .as_mut()
            .unwrap()
            .materials
            .iter_mut()
            .find(|p| p.material == "oxygen")
            .unwrap()
            .escaped_kg = 1e-100;
        assert!(validate(&planet).is_err());
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
