use super::{Info, Section};
use space_shared::{Planet, RadiusReference};

pub(super) fn facts(info: &mut Info, planet: &Planet) {
    info.section = Section::Composition;
    let data = match space_simulation::system::physical::validate(planet) {
        Ok(Some(data)) => data,
        result => {
            let reason = if result.is_err() {
                "Unknown (invalid physical record)"
            } else {
                "Unknown (legacy record)"
            };
            for (id, label) in [
                ("composition.materials", "Material mass fractions"),
                ("composition.retained", "Retained mass"),
                ("composition.density", "Mean density"),
                ("composition.envelope", "H₂/He mass fraction"),
                ("composition.reference", "Radius meaning"),
            ] {
                info.unknown(id, label, reason);
            }
            return;
        }
    };
    info.field(
        "composition.retained",
        "Retained mass",
        Some("kg"),
        format!("{:.5e} kg", data.retained_mass_kg),
    );
    info.field(
        "composition.density",
        "Mean density",
        Some("kg/m³"),
        format!("{:.1} kg/m³", data.mean_density_kg_m3),
    );
    info.field(
        "composition.gravity",
        "Reference gravity",
        Some("m/s²"),
        format!("{:.3} m/s²", data.reference_gravity_m_s2),
    );
    info.field(
        "composition.envelope",
        "H₂/He mass fraction",
        Some("% by mass"),
        percent(data.h_he_mass_fraction),
    );
    info.field(
        "composition.reference",
        "Radius meaning",
        None,
        match data.radius_reference {
            RadiusReference::SolidSurface => "Nominal solid surface (volume-equivalent radius)",
            RadiusReference::UnresolvedVolatileBoundary => {
                "Unresolved volatile boundary (no ocean/phase solution)"
            }
            RadiusReference::OneBar => {
                "One-bar envelope reference (not solid ground or a cloud deck)"
            }
        },
    );
    for p in &data.materials {
        info.field(
            &format!("composition.material.{}", p.material),
            material_label(&p.material),
            Some("% retained mass"),
            percent(p.retained_kg / data.retained_mass_kg),
        );
    }
    let total =
        |f: fn(&space_shared::MaterialBudget) -> f64| data.materials.iter().map(f).sum::<f64>();
    info.field(
        "composition.escaped",
        "Escaped material",
        Some("kg"),
        format!(
            "{:.3e} kg (excluded from planet mass)",
            total(|p| p.escaped_kg)
        ),
    );
    info.field(
        "composition.gas",
        "Gas reservoir",
        Some("kg"),
        format!(
            "{:.3e} kg (whole reservoir, not column)",
            total(|p| p.gas_kg)
        ),
    );
    info.field(
        "composition.condensates",
        "Condensed volatiles",
        Some("kg"),
        format!("{:.3e} kg (unresolved phases)", total(|p| p.condensed_kg)),
    );
    info.field(
        "composition.approximations",
        "Structure caveats",
        None,
        data.approximations
            .iter()
            .map(|a| a.replace('_', " "))
            .collect::<Vec<_>>()
            .join(", "),
    );
    info.field(
        "composition.appearance",
        "Reflectance",
        None,
        "Composition-based appearance proxy, not solved phases/clouds",
    );
}

fn percent(fraction: f64) -> String {
    let p = 100.0 * fraction;
    if p > 0.0 && p < 0.01 {
        format!("{p:.2e}%")
    } else {
        format!("{p:.2}%")
    }
}

fn material_label(id: &str) -> &str {
    match id {
        "iron_nickel" => "Iron/nickel",
        "silicate" => "Silicate rock",
        "water" => "Water (H₂O)",
        "hydrogen" => "Hydrogen (H₂)",
        "helium" => "Helium",
        "nitrogen" => "Nitrogen (N₂)",
        "carbon_dioxide" => "Carbon dioxide",
        "methane" => "Methane",
        "oxygen" => "Oxygen (O₂)",
        "argon" => "Argon",
        _ => id,
    }
}
