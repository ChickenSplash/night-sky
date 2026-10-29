//! Generated physical facts, never the renderer's selected-planet override.
use super::{Info, Section};
use crate::format::number;
use space_shared::{AtmosphereReference, PlanetEnvironment};

pub(super) fn pressure_display(pressure_pa: f64) -> (&'static str, String) {
    if pressure_pa < 1e3 {
        let mbar = pressure_pa / 100.0;
        let value = if mbar > 0.0 && mbar < 0.0001 {
            format!("{mbar:.3e} mbar")
        } else {
            format!("{} mbar", number(mbar, 4))
        };
        ("mbar", value)
    } else {
        ("bar", format!("{} bar", number(pressure_pa / 1e5, 4)))
    }
}

pub(super) fn facts(info: &mut Info, environment: Option<&PlanetEnvironment>) {
    info.section = Section::Atmosphere;
    let Some(environment) = environment else {
        info.unknown("atmosphere.state", "Atmosphere", "Not generated");
        return;
    };
    info.field(
        "atmosphere.heating",
        "Stellar heating",
        Some("W/m²"),
        format!(
            "{} W/m² (mean estimate)",
            number(environment.irradiation_w_m2, 1)
        ),
    );
    info.field(
        "atmosphere.equilibrium_temperature",
        "Equilibrium temperature",
        Some("K"),
        format!(
            "{} K (assumed albedo {:.2})",
            number(environment.equilibrium_temperature_k, 0),
            environment.assumed_bond_albedo
        ),
    );
    let Some(air) = &environment.atmosphere else {
        info.field(
            "atmosphere.state",
            "Atmosphere",
            None,
            "Absent or trace (below model cutoff)",
        );
        return;
    };
    info.field(
        "atmosphere.state",
        "Atmosphere",
        None,
        "Present · approximate generated model",
    );
    let mut gases: Vec<_> = air.gases.iter().collect();
    gases.sort_by(|a, b| b.mole_fraction.total_cmp(&a.mole_fraction));
    info.field(
        "atmosphere.mixture",
        "Gas mixture",
        Some("% by molecule"),
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
    let (pressure_unit, pressure_value) = if air.reference_level == AtmosphereReference::OneBar {
        ("bar", "1 bar (reference level, not ground)".into())
    } else {
        pressure_display(air.pressure_pa)
    };
    info.field(
        "atmosphere.pressure",
        match air.reference_level {
            AtmosphereReference::Surface => "Boundary pressure (approx.)",
            AtmosphereReference::OneBar => "Reference pressure",
        },
        Some(pressure_unit),
        pressure_value,
    );
    if air.reference_level == AtmosphereReference::Surface {
        info.field(
            "atmosphere.one_bar_altitude",
            "One-bar altitude",
            Some("km"),
            if air.pressure_pa >= 1e5 {
                format!(
                    "{} km above boundary (isothermal estimate)",
                    number(
                        space_simulation::system::atmosphere::profile::Profile {
                            radius_m: air.reference_radius_m,
                            scale_height_m: air.scale_height_m,
                            spherical: !air.hydrostatic_unresolved && air.reference_radius_m > 0.0,
                        }
                        .height_for_drop(air.pressure_pa.ln() - 1e5_f64.ln())
                        .unwrap_or(f64::NAN)
                            / 1e3,
                        1
                    )
                )
            } else {
                "Not above boundary (sub-bar column)".into()
            },
        );
    }
    info.field(
        "atmosphere.model_limits",
        "Atmosphere model",
        None,
        "Ideal gas and dilute optics; dense-gas EOS, greenhouse and phases unresolved",
    );
    info.field(
        "atmosphere.column_temperature",
        "Column temperature",
        Some("K"),
        format!(
            "{} K (isothermal estimate, no greenhouse)",
            number(air.temperature_k, 0)
        ),
    );
    info.field(
        "atmosphere.density",
        "Gas density",
        Some("kg/m³"),
        format!(
            "{:.3e} kg/m³ (ideal gas at reference level)",
            air.reference_density_kg_m3
        ),
    );
    info.field(
        "atmosphere.scale_height",
        "Scale height",
        Some("km"),
        format!("{} km", number(air.scale_height_m / 1e3, 1)),
    );
    info.field(
        "atmosphere.extent",
        "Atmosphere extent",
        Some("km"),
        format!("{} km (operational cutoff)", number(air.extent_m / 1e3, 1)),
    );
    if air.hydrostatic_unresolved {
        info.field(
            "atmosphere.validity",
            "Column validity",
            None,
            "Static spherical column unsupported below sonic limit; exponential fallback, wind/thermal structure unresolved",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_uses_bar_or_millibar_without_rounding_trace_columns_to_zero() {
        for (pa, unit, value) in [
            (12.79, "mbar", "0.1279 mbar"),
            (999.0, "mbar", "9.9900 mbar"),
            (1000.0, "bar", "0.0100 bar"),
            (100_000.0, "bar", "1.0000 bar"),
            (0.00001, "mbar", "1.000e-7 mbar"),
            (0.0, "mbar", "0.0000 mbar"),
        ] {
            assert_eq!(pressure_display(pa), (unit, value.into()));
        }
    }

    #[test]
    fn unknown_and_airless_are_distinct_and_temperature_is_labelled_as_an_estimate() {
        let mut info = Info::default();
        facts(&mut info, None);
        assert_eq!(
            info.rows().collect::<Vec<_>>(),
            vec![("Atmosphere", "Not generated")]
        );
        info.fields.clear();
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
            info.rows()
                .any(|(key, value)| key == "Atmosphere" && value.starts_with("Absent"))
        );
        assert!(info.rows().any(
            |(key, value)| key == "Equilibrium temperature" && value.contains("assumed albedo")
        ));
        assert!(
            !info
                .rows()
                .any(|(key, _)| key == "Boundary pressure (approx.)")
        );
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
                assert!(info.rows().any(|(key, _)| key == label));
            }
            match air.reference_level {
                AtmosphereReference::Surface => {
                    surface = true;
                    assert!(
                        info.rows()
                            .any(|(key, _)| key == "Boundary pressure (approx.)")
                    );
                }
                AtmosphereReference::OneBar => {
                    envelope = true;
                    assert!(
                        info.rows().any(|(key, value)| key == "Reference pressure"
                            && value.contains("not ground"))
                    );
                    assert!(
                        !info
                            .rows()
                            .any(|(key, _)| key == "Boundary pressure (approx.)")
                    );
                }
            }
        }
        assert!(surface && envelope);
    }

    #[test]
    fn extended_rocky_boundary_uses_spherical_gravity_without_legacy_warning() {
        let system =
            space_simulation::system::generated_system("g13b6ffffcfeba.2.59", [0.0; 3]).unwrap();
        let world = system
            .planets
            .iter()
            .find(|p| p.id == "g13b6ffffcfeba.2.59/7")
            .unwrap();
        let mut info = Info::default();
        facts(&mut info, world.environment.as_ref());
        super::super::composition::facts(&mut info, world);
        assert!(
            info.rows()
                .any(|(label, _)| label == "Boundary pressure (approx.)")
        );
        assert!(!info.rows().any(|(label, _)| label == "Reference pressure"));
        assert!(!info.rows().any(|(label, _)| label == "Column validity"));
        assert!(
            info.rows()
                .any(|(label, value)| label == "Boundary pressure model"
                    && value.contains("Inventory-normalised"))
        );
        assert!(info.rows().any(|(label, value)| label == "Radius meaning"
            && value.contains("Condensed reference boundary")));
    }

    #[test]
    fn motivating_boundary_exports_pressure_height_and_model_limits() {
        let system =
            space_simulation::system::generated_system("g13b6ffffcfeba.2.339", [0.0; 3]).unwrap();
        let world = system
            .planets
            .iter()
            .find(|p| p.id.ends_with("/7"))
            .unwrap();
        let mut info = Info::default();
        facts(&mut info, world.environment.as_ref());
        assert!(info.rows().any(
            |(label, value)| label == "Boundary pressure (approx.)" && value.ends_with(" bar")
        ));
        assert!(
            info.rows()
                .any(|(label, value)| label == "One-bar altitude"
                    && value.contains("above boundary"))
        );
        assert!(
            info.rows().any(
                |(label, value)| label == "Atmosphere model" && value.contains("dense-gas EOS")
            )
        );
        for id in [
            "atmosphere.pressure",
            "atmosphere.one_bar_altitude",
            "atmosphere.model_limits",
        ] {
            assert!(info.fields.iter().any(|field| field.id == id));
        }
    }
}
