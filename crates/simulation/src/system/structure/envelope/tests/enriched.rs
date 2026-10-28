use super::*;

#[test]
fn enriched_effective_envelopes_conserve_refine_and_locate_the_handoff() {
    check(false);
}

#[test]
#[ignore = "local pinned source resource; run in release"]
fn enriched_effective_envelopes_source_backed() {
    check(true);
}

fn check(source_backed: bool) {
    let table = source_backed.then(|| hhe::Table::local().unwrap());
    let hhe = |p, t, y| match &table {
        Some(table) => table.density_kg_m3(p, t, y),
        None => Ok(p / (3600.0 * t)),
    };
    // 20% secondary and secondary-dominated envelopes, both with water and
    // protected material. No origins are converted into the density surrogate.
    for z in [0.2, 0.9] {
        let secondary = 0.1 * z / (1.0 - z) * EARTH_MASS_KG;
        let protected = Inventory::new(&[(Material::CarbonDioxide, 1e-7 * EARTH_MASS_KG)]).unwrap();
        let request = inventory(5.0, 0.1)
            .checked_add(
                &Inventory::new(&[
                    (Material::Nitrogen, 0.49 * secondary),
                    (
                        Material::CarbonDioxide,
                        0.49 * secondary + protected.total_mass_kg(),
                    ),
                    (Material::Methane, 0.01 * secondary),
                    (Material::Argon, 0.01 * secondary),
                    (Material::Water, 0.03 * EARTH_MASS_KG),
                ])
                .unwrap(),
            )
            .unwrap();
        let run = |controls| {
            solve_with_density(&request, &protected, 350.0, 100.0, controls, &hhe)
                .unwrap_or_else(|e| panic!("source={source_backed} z={z}: {e:?}"))
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
        assert!((fine.one_bar_radius_m / e.one_bar_radius_m - 1.0).abs() < 1e-7);
        assert!((fine.inertia_kg_m2 / e.inertia_kg_m2 - 1.0).abs() < 1e-7);
        assert!(e.uses_effective_components());
        assert!(e.uses_extended_mixture());
        assert_eq!(e.density_model_id(), hhe_mixture::COMPOSITE_MODEL_ID);
        let retained = interior
            .retained_inventory()
            .checked_add(&e.inventory)
            .unwrap();
        for m in Material::ALL {
            assert!((retained.mass_kg(m) - request.mass_kg(m)).abs() <= 1e-10 * request.mass_kg(m));
        }
        assert_eq!(e.inventory.mass_kg(Material::Water), 0.0);
        assert!(
            (interior
                .retained_inventory()
                .mass_kg(Material::CarbonDioxide)
                / protected.total_mass_kg()
                - 1.0)
                .abs()
                < 1e-10
        );
        let boundary = e.effective_boundary_pa.unwrap();
        let pair = e
            .regions
            .windows(2)
            .find(|p| p[0].outer().pressure_pa == boundary)
            .unwrap();
        assert_eq!(pair[0].outer().radius_m, pair[1].inner().radius_m);
        assert_eq!(
            pair[0].outer().enclosed_mass_kg,
            pair[1].inner().enclosed_mass_kg
        );
        assert_eq!(pair[1].inner().pressure_pa, boundary);
        assert!(
            (pair[0].outer().density_kg_m3 / pair[1].inner().density_kg_m3 - 1.0).abs() < 1e-13
        );
        assert_eq!(
            e.region_density_model_id(&pair[0]),
            hhe_mixture::EFFECTIVE_MODEL_ID
        );
        assert_eq!(
            e.region_density_model_id(&pair[1]),
            hhe_mixture::ENRICHED_MODEL_ID
        );

        let route = mixture::ComponentRoute::new(e.thermal, e.inventory).unwrap();
        let mix = hhe_mixture::Mixture::new_enriched(&e.inventory).unwrap();
        let mut independent_inertia = 0.0;
        for r in interior.regions() {
            let (mass, i, dp) = profile_quadrature(r.samples(), |p| {
                if r.inventory().mass_kg(Material::Water) > 0.0 {
                    if r.density_model_id() == loaded::MODEL_ID {
                        loaded::density_kg_m3(p).unwrap()
                    } else {
                        1460.0 + 0.00311 * p.powf(0.513)
                    }
                } else {
                    cold::density_kg_m3(
                        if r.inventory().mass_kg(Material::Metal) > 0.0 {
                            Material::Metal
                        } else {
                            Material::Silicate
                        },
                        p,
                    )
                    .unwrap()
                }
            });
            assert!((mass / r.inventory().total_mass_kg() - 1.0).abs() < 1e-6);
            assert!((dp / (r.inner().pressure_pa - r.outer().pressure_pa) - 1.0).abs() < 2e-4);
            independent_inertia += i;
        }
        for r in e.regions() {
            let effective = e.region_density_model_id(r) == hhe_mixture::EFFECTIVE_MODEL_ID;
            let (mass, i, dp) = profile_quadrature(r.samples(), |p| {
                let t = e.thermal.temperature_k(p).unwrap();
                let light = hhe(p, t, 0.25).unwrap();
                if effective {
                    route
                        .density(mix, p, t, light, &mut dry::Work::default(), 100)
                        .unwrap()
                } else {
                    mix.density_kg_m3(p, t, light).unwrap()
                }
            });
            assert!((mass / r.inventory().total_mass_kg() - 1.0).abs() < 1e-6);
            assert!((dp / (r.inner().pressure_pa - r.outer().pressure_pa) - 1.0).abs() < 2e-4);
            independent_inertia += i;
        }
        assert!((independent_inertia / e.inertia_kg_m2 - 1.0).abs() < 1e-6);
        eprintln!(
            "enriched source={source_backed} z={z}: R={} km base={} bar handoff={} bar",
            e.one_bar_radius_m / 1000.0,
            e.regions[0].inner().pressure_pa / 1e5,
            boundary / 1e5
        );
        let solved = super::super::super::prototype::Solved {
            structure: coupled::CoupledStructure::from_envelope(interior, e),
            source: request,
            escaped: Inventory::default(),
            blowoff: Inventory::default(),
            protected,
        };
        let record = crate::system::physical::layered(&solved);
        assert_eq!(
            record.model_fidelity,
            space_shared::ModelFidelity::Approximate
        );
        assert!(
            record
                .physical
                .approximations
                .iter()
                .any(|m| m == crate::system::materials::effective_secondary::MODEL_ID)
        );
        assert!(
            record
                .physical
                .approximations
                .iter()
                .any(|m| m == crate::system::materials::molecular::MODEL_ID)
        );
        assert!(
            !record
                .physical
                .approximations
                .iter()
                .any(|m| m == TRACE_MODEL_ID)
        );
        let layer = record
            .layers
            .iter()
            .find(|l| l.model == hhe_mixture::EFFECTIVE_MODEL_ID)
            .unwrap();
        assert!(
            layer
                .model_limitations
                .iter()
                .any(|s| s.contains("surrogate"))
        );
        assert!(record.model_limitations.iter().any(|s| s.contains("2/7")));
    }
}
