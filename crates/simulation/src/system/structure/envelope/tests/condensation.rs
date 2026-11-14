use super::*;

#[test]
fn suspended_envelope_conserves_partitions_and_refines() {
    check(false);
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn suspended_envelope_source_backed() {
    check(true);
}

fn check(source: bool) {
    let table = source.then(|| hhe::Table::local().unwrap());
    let eos = |p, t, y| match &table {
        Some(tables) => tables.density_kg_m3(p, t, y),
        None => Ok(p / (3600.0 * t)),
    };
    let protected = Inventory::new(&[(Material::CarbonDioxide, 1e-7 * EARTH_MASS_KG)]).unwrap();
    for z in [0.02, 0.2] {
        let secondary = 0.01 * z / (1.0 - z) * EARTH_MASS_KG;
        let request = inventory(1.0, 0.01)
            .checked_add(
                &Inventory::new(&[
                    (
                        Material::CarbonDioxide,
                        secondary + protected.total_mass_kg(),
                    ),
                    (Material::Water, 0.03 * EARTH_MASS_KG),
                ])
                .unwrap(),
            )
            .unwrap();
        let run = |controls| {
            solve_with_density(&request, &protected, 100.0, 100.0, controls, &eos)
                .unwrap_or_else(|e| panic!("source={source} z={z}: {e:?}"))
        };
        let (interior, e) = run(Default::default());
        let (_, fine) = run(coupled::Controls {
            radial: dry::Controls {
                relative_tolerance: 2e-11,
                max_step: 1.0 / 256.0,
                ..Default::default()
            },
            ..Default::default()
        });
        assert!(e.uses_suspended_condensates());
        assert!((fine.reference_radius_m / e.reference_radius_m - 1.0).abs() < 1e-7);
        assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
        let condensate = e
            .condensates
            .iter()
            .map(|c| c.mass_kg(Material::CarbonDioxide))
            .sum::<f64>();
        let fine_condensate = fine
            .condensates
            .iter()
            .map(|c| c.mass_kg(Material::CarbonDioxide))
            .sum::<f64>();
        assert!(condensate > 0.0);
        assert!(e.regions.last().unwrap().outer().pressure_pa >= OUTER_PRESSURE_PA);
        assert!(e.regions.iter().any(|r| r.outer().pressure_pa == 1e5));
        let mut work = 0;
        assert!(matches!(
            phase_allocations(
                &e.regions,
                &e.inventory,
                e.thermal,
                e.effective_boundary_pa.unwrap(),
                &mut work,
                0
            ),
            Err(Error::WorkBudget)
        ));
        assert!((fine_condensate / condensate - 1.0).abs() < 1e-3);
        let restored = interior
            .retained_inventory()
            .checked_add(&e.inventory)
            .unwrap();
        for m in Material::ALL {
            assert!((restored.mass_kg(m) - request.mass_kg(m)).abs() <= 1e-10 * request.mass_kg(m));
        }
        assert_eq!(e.inventory.mass_kg(Material::Water), 0.0);
        let boundary = e.effective_boundary_pa.unwrap();
        let pair = e
            .regions
            .windows(2)
            .find(|p| p[0].outer().pressure_pa == boundary)
            .unwrap();
        assert!(
            (pair[0].outer().density_kg_m3 / pair[1].inner().density_kg_m3 - 1.0).abs() < 1e-13
        );
        let route = mixture::ComponentRoute::new(e.thermal, e.inventory).unwrap();
        let mix = hhe_mixture::Mixture::new_enriched(&e.inventory).unwrap();
        for r in &e.regions {
            let effective = r.outer().pressure_pa >= boundary;
            let (mass, _, dp) = profile_quadrature(r.samples(), |p| {
                let t = e.thermal.temperature_k(p).unwrap();
                let light = eos(p, t, 0.25).unwrap();
                if effective {
                    route
                        .density(mix, p, t, light, &mut dry::Work::default(), 100)
                        .unwrap()
                } else {
                    suspended::State::new(&e.inventory, p, t, light)
                        .unwrap()
                        .density_kg_m3
                }
            });
            assert!((mass / r.inventory().total_mass_kg() - 1.0).abs() < 1e-6);
            assert!((dp / (r.inner().pressure_pa - r.outer().pressure_pa) - 1.0).abs() < 2e-4);
        }
        let solved = super::super::super::prototype::Solved {
            structure: coupled::CoupledStructure::from_envelope(interior, e),
            source: request,
            escaped: Inventory::default(),
            protected,
        };
        let record = crate::system::physical::layered(&solved);
        for m in &record.physical.materials {
            assert!(m.gas_kg >= 0.0 && m.condensed_kg >= 0.0);
            assert!(
                (m.interior_kg + m.condensed_kg + m.gas_kg - m.retained_kg).abs()
                    <= 1e-10 * m.retained_kg
            );
        }
        assert!(
            record
                .model_limitations
                .iter()
                .any(|s| s.contains("settling"))
        );
        let phase_mass = record
            .layers
            .iter()
            .flat_map(|l| &l.phase_allocations)
            .map(|p| p.vapour_kg + p.condensate_kg)
            .sum::<f64>();
        assert!(
            (phase_mass / solved.structure.gas_inventory().total_mass_kg() - 1.0).abs() < 1e-10
        );
        assert!(
            record
                .layers
                .iter()
                .flat_map(|l| &l.phase_allocations)
                .any(|p| p.condensate_kg > 0.0)
        );
        // A supported physical transport is not a supported cloud/drag model.
        // This supplies only the declared global gas-mixture reference proxy,
        // not invented local vapour/cloud optics for the resolved condensate.
        use crate::system::{
            atmosphere::{
                Composition, Gas, GasAtmosphere, ReferenceLevel, optics::boundary::OpticalColumn,
            },
            physical,
        };
        let weights = Gas::ALL.map(|gas| {
            let mass = record
                .physical
                .materials
                .iter()
                .find(|m| m.material == Material::from_gas(gas).id())
                .unwrap()
                .gas_kg;
            (gas, mass / gas.molecular_mass_u())
        });
        let total: f64 = weights.iter().map(|(_, w)| w).sum();
        let gas = GasAtmosphere::new(
            Composition::new(&weights.map(|(g, w)| (g, w / total))).unwrap(),
            ReferenceLevel::OneBar,
            record.surface_pressure_pa,
            record.surface_temperature_k,
            None,
        )
        .unwrap();
        let result = physical::resolved_result(
            &super::super::super::prototype::Outcome::Layered(Box::new(solved)),
            space_shared::PhysicalRevision {
                inputs: "suspended-consumer".into(),
                model: "fixture".into(),
                sequence: 1,
            },
        )
        .unwrap();
        let reference = OpticalColumn::reference_from_result(&result, &gas).unwrap();
        assert!(OpticalColumn::from_result(&result, &gas).is_err());
        let planet = space_shared::Planet {
            mass_earth: record.physical.retained_mass_kg / EARTH_MASS_KG,
            radius_km: record.physical.reference_radius_m / 1000.0,
            inertia_factor: Some(record.physical.inertia_factor),
            physical: Some(record.physical.clone()),
            environment: Some(space_shared::PlanetEnvironment {
                irradiation_w_m2: 1361.0,
                equilibrium_temperature_k: 100.0,
                assumed_bond_albedo: 0.3,
                age_gyr: 4.5,
                atmosphere: Some(
                    gas.to_data(
                        reference.gravity_m_s2 * reference.radius_m.powi(2),
                        reference.radius_m,
                    )
                    .unwrap(),
                ),
            }),
            initial_result: Some(result),
            ..Default::default()
        };
        physical::validate(&planet).unwrap().unwrap();
        assert!(
            crate::system::surface::geometry(&planet)
                .unwrap()
                .has_gas_depth()
        );
        assert!(
            crate::system::clearance::Policy::default()
                .planet(&planet)
                .unwrap_err()
                .contains("Suspended-condensate")
        );
        eprintln!(
            "suspended source={source} z={z}: R={} km condensate={} kg phase refinement={}",
            record.physical.reference_radius_m / 1000.0,
            condensate,
            (fine_condensate / condensate - 1.0).abs()
        );
    }
}

#[test]
fn suspended_density_inversion_stays_a_refusal() {
    let protected = Inventory::new(&[(Material::CarbonDioxide, 1e-7 * EARTH_MASS_KG)]).unwrap();
    let request = inventory(5.0, 0.1)
        .checked_add(
            &Inventory::new(&[
                (
                    Material::CarbonDioxide,
                    0.9 * EARTH_MASS_KG + protected.total_mass_kg(),
                ),
                (Material::Water, 0.03 * EARTH_MASS_KG),
            ])
            .unwrap(),
        )
        .unwrap();
    let result = solve_with_density(
        &request,
        &protected,
        100.0,
        100.0,
        Default::default(),
        &|p, t, _| Ok(p / (3600.0 * t)),
    );
    assert!(matches!(result, Err(Error::DensityInversion)));
}
