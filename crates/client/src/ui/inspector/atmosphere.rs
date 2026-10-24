//! Generated physical facts, never the renderer's selected-planet override.
use super::Info;
use space_shared::{AtmosphereReference, PlanetEnvironment};

pub(super) fn facts(info: &mut Info, environment: Option<&PlanetEnvironment>) {
    let Some(environment) = environment else {
        info.fact("Atmosphere", "Not generated");
        return;
    };
    info.fact(
        "Stellar heating",
        format!("{:.1} W/m² (mean estimate)", environment.irradiation_w_m2),
    );
    info.fact(
        "Equilibrium temperature",
        format!(
            "{:.0} K (assumed albedo {:.2})",
            environment.equilibrium_temperature_k, environment.assumed_bond_albedo
        ),
    );
    let Some(air) = &environment.atmosphere else {
        info.fact("Atmosphere", "Absent or trace (below model cutoff)");
        return;
    };
    info.fact("Atmosphere", "Present · approximate generated model");
    let mut gases: Vec<_> = air.gases.iter().collect();
    gases.sort_by(|a, b| b.mole_fraction.total_cmp(&a.mole_fraction));
    info.fact(
        "Gas mixture",
        gases
            .iter()
            .map(|part| {
                let percent = part.mole_fraction * 100.0;
                if percent < 0.01 {
                    format!("{} {:.2e}%", part.gas, percent)
                } else {
                    format!("{} {:.2}%", part.gas, percent)
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    info.fact(
        match air.reference_level {
            AtmosphereReference::Surface => "Surface pressure",
            AtmosphereReference::OneBar => "Reference pressure",
        },
        if air.reference_level == AtmosphereReference::OneBar {
            "1 bar (reference level, not ground)".into()
        } else if air.pressure_pa < 100.0 {
            format!("{:.2} Pa", air.pressure_pa)
        } else {
            format!("{:.4} bar", air.pressure_pa / 1e5)
        },
    );
    info.fact(
        "Column temperature",
        format!(
            "{:.0} K (isothermal estimate, no greenhouse)",
            air.temperature_k
        ),
    );
    info.fact(
        "Gas density",
        format!(
            "{:.3e} kg/m³ (ideal gas at reference level)",
            air.reference_density_kg_m3
        ),
    );
    info.fact(
        "Scale height",
        format!("{:.1} km", air.scale_height_m / 1e3),
    );
    info.fact(
        "Atmosphere extent",
        format!("{:.1} km (operational cutoff)", air.extent_m / 1e3),
    );
    if air.extended {
        info.fact(
            "Column validity",
            "Extended: constant-gravity estimate unreliable",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_and_airless_are_distinct_and_temperature_is_labelled_as_an_estimate() {
        let mut info = Info::default();
        facts(&mut info, None);
        assert_eq!(
            info.rows,
            vec![("Atmosphere".into(), "Not generated".into())]
        );
        info.rows.clear();
        facts(
            &mut info,
            Some(&PlanetEnvironment {
                irradiation_w_m2: 1361.0,
                assumed_bond_albedo: 0.3,
                equilibrium_temperature_k: 255.0,
                age_gyr: 4.5,
                atmosphere: None,
            }),
        );
        assert!(
            info.rows
                .iter()
                .any(|(key, value)| key == "Atmosphere" && value.starts_with("Absent"))
        );
        assert!(info.rows.iter().any(
            |(key, value)| key == "Equilibrium temperature" && value.contains("assumed albedo")
        ));
        assert!(!info.rows.iter().any(|(key, _)| key == "Surface pressure"));
    }

    #[test]
    fn generated_air_and_reference_levels_are_visible_without_shader_inputs() {
        let galaxy = space_simulation::demo_galaxy();
        let mut surface = false;
        let mut envelope = false;
        for planet in galaxy.systems.iter().flat_map(|system| &system.planets) {
            let Some(environment) = &planet.environment else {
                panic!("missing environment");
            };
            let Some(air) = &environment.atmosphere else {
                continue;
            };
            let mut info = Info::default();
            facts(&mut info, Some(environment));
            for label in [
                "Gas mixture",
                "Column temperature",
                "Gas density",
                "Scale height",
                "Atmosphere extent",
            ] {
                assert!(info.rows.iter().any(|(key, _)| key == label));
            }
            match air.reference_level {
                AtmosphereReference::Surface => {
                    surface = true;
                    assert!(info.rows.iter().any(|(key, _)| key == "Surface pressure"));
                }
                AtmosphereReference::OneBar => {
                    envelope = true;
                    assert!(
                        info.rows
                            .iter()
                            .any(|(key, value)| key == "Reference pressure"
                                && value.contains("not ground"))
                    );
                    assert!(!info.rows.iter().any(|(key, _)| key == "Surface pressure"));
                }
            }
        }
        assert!(surface && envelope);
    }
}
