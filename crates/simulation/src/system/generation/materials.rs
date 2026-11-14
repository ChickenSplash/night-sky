//! Tunable formation priors, not hidden planet-class templates or disc evolution.
use super::*;
use crate::system::composition::{Budget, Inventory, Material};
use crate::units::EARTH_MASS_KG;

/// Draw heavy material first, then a continuous gas-to-heavy acquisition ratio.
/// Compact retries reduce feeding mass, not swap in named planet templates.
pub(super) fn draw(
    rng: &mut Rng,
    distance_au: f64,
    snow_au: f64,
    host_mass: f64,
    compact: bool,
) -> Budget {
    let x = distance_au / snow_au;
    let ice = x.powi(4) / (1.0 + x.powi(4));
    let feeding = (1.0 + 2.0 * ice) / (1.0 + (x / 5.0).powi(2));
    let heavy =
        (rng.log_range(0.0022, 12.0) * feeding * if compact { 0.01 } else { 1.0 }).max(0.0022);
    let water = ice * rng.range(0.05, 0.6);
    let metal = rng.range(0.15, 0.7);
    let secondary = rng.log_range(1e-7, 0.003);
    let co2_share = rng.range(0.05, 0.9);
    let nitrogen_share = 1.0 - co2_share;
    let rock = 1.0 - water - secondary;
    let interior = Inventory::new(&[
        (Material::Metal, heavy * rock * metal * EARTH_MASS_KG),
        (
            Material::Silicate,
            heavy * rock * (1.0 - metal) * EARTH_MASS_KG,
        ),
    ])
    .unwrap();
    let condensate = Inventory::new(&[
        (Material::Water, heavy * water * EARTH_MASS_KG),
        (
            Material::CarbonDioxide,
            heavy * secondary * co2_share * 0.98 * EARTH_MASS_KG,
        ),
        (
            Material::Nitrogen,
            heavy * secondary * nitrogen_share * 0.98 * EARTH_MASS_KG,
        ),
        (Material::Methane, heavy * secondary * 0.01 * EARTH_MASS_KG),
        (Material::Argon, heavy * secondary * 0.01 * EARTH_MASS_KG),
    ])
    .unwrap();
    // Below 0.1 Earth masses the envelope structure model is unsupported.
    // Use the protected heavy mass so later secondary escape cannot take a
    // trace-envelope body below that domain. This is a continuous formation
    // prior, not a post-solve mass clamp or suppression of physical escape.
    let protected_heavy = heavy
        * (1.0
            - water * retention::accessible_fraction(Material::Water)
            - secondary * retention::accessible_fraction(Material::Nitrogen));
    let onset = ((protected_heavy - 0.1) / 0.1).clamp(0.0, 1.0);
    let gas_ratio =
        onset * (heavy / 5.0).powf(2.5) * rng.log_range(1e-4, 30.0) * host_mass.min(1.0)
            / (1.0 + x / 20.0);
    let gas_mass = (heavy * gas_ratio).min(3_900.0 - heavy);
    let gas = Inventory::new(&[
        (Material::Hydrogen, gas_mass * 0.75 * EARTH_MASS_KG),
        (Material::Helium, gas_mass * 0.25 * EARTH_MASS_KG),
    ])
    .unwrap();
    let source = interior
        .checked_add(&condensate)
        .unwrap()
        .checked_add(&gas)
        .unwrap();
    let cap = MAX_MASS_RATIO * host_mass / EARTH_MASS_SOLAR * EARTH_MASS_KG;
    let scale = (cap / source.total_mass_kg()).min(1.0);
    let interior = interior.scaled(scale).unwrap();
    let condensate = condensate.scaled(scale).unwrap();
    let gas = gas.scaled(scale).unwrap();
    // Sum in the same order as Budget, preserving exact source/allocation agreement.
    let source = interior
        .checked_add(&condensate)
        .unwrap()
        .checked_add(&gas)
        .unwrap();
    Budget::new(source, interior, condensate, gas).expect("bounded formation allocations")
}

/// Ordered display rules only, never inputs to any physical prescription.
pub(super) fn classify(physical: &space_shared::PlanetPhysical) -> Class {
    use crate::system::structure::parameters::classification;
    let fraction = physical.h_he_mass_fraction;
    let share = |material: Material| {
        physical
            .materials
            .iter()
            .find(|m| m.material == material.id())
            .map_or(0.0, |m| m.retained_kg / physical.retained_mass_kg)
    };
    if fraction >= classification::H_HE_DOMINATED_MASS_FRACTION {
        Class::GasGiant
    } else if fraction >= classification::ENVELOPE_WORLD_MASS_FRACTION {
        Class::EnvelopeWorld
    } else if fraction >= classification::SIGNIFICANT_H_HE_MASS_FRACTION
        || (physical.radius_reference == space_shared::RadiusReference::OneBar
            && fraction * physical.retained_mass_kg.powi(2) * crate::units::GRAVITATIONAL_CONSTANT
                >= 4.0
                    * std::f64::consts::PI
                    * crate::system::atmosphere::ONE_BAR_PA
                    * physical.condensed_radius_m.powi(4))
    {
        // H/He that alone holds a one-bar level above the ground is an
        // envelope, however little of the mass it is: never labelled as bare
        // rock or water. A thick secondary atmosphere keeps its bulk class.
        Class::SubNeptune
    } else if share(Material::Water) >= classification::WATER_RICH_MASS_FRACTION {
        Class::WaterRich
    } else if share(Material::Metal) >= classification::METAL_RICH_MASS_FRACTION {
        Class::MetalRich
    } else {
        Class::Rocky
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::composition::Reservoir;

    #[test]
    fn ordered_descriptions_do_not_assert_phase_or_dynamical_clearing() {
        let mass = 100.0 * EARTH_MASS_KG;
        let sample = |water: f64, metal: f64, light: f64| {
            let interior = Inventory::new(&[
                (Material::Metal, mass * metal),
                (Material::Silicate, mass * (1.0 - metal - water - light)),
            ])
            .unwrap();
            let condensed = Inventory::new(&[(Material::Water, mass * water)]).unwrap();
            let gas = Inventory::new(&[
                (Material::Hydrogen, mass * light * 0.75),
                (Material::Helium, mass * light * 0.25),
            ])
            .unwrap();
            let source = interior
                .checked_add(&condensed)
                .unwrap()
                .checked_add(&gas)
                .unwrap();
            let b = Budget::new(source, interior, condensed, gas).unwrap();
            let s = crate::system::physical::fallback(&structure::fallback::initial(
                &b,
                &crate::system::forcing::Forcing::new(1361.0, 1361.0, 0.3, 4.5)
                    .unwrap()
                    .environment(),
            ));
            (b, s)
        };
        for (water, metal, light, expected) in [
            (0.0, 0.3, 0.0, Class::Rocky),
            (0.0, 0.5, 0.0, Class::MetalRich),
            (0.25, 0.5, 0.0, Class::WaterRich),
            (0.25, 0.5, 0.01, Class::SubNeptune),
            (0.25, 0.3, 0.2, Class::EnvelopeWorld),
            (0.0, 0.1, 0.6, Class::GasGiant),
        ] {
            let (_, s) = sample(water, metal, light);
            assert_eq!(classify(&s.physical), expected);
        }
        // H/He that alone holds one bar is an envelope below the mass-fraction
        // threshold too; without a one-bar level the bulk class stands.
        let (_, trace) = sample(0.0, 0.3, 1e-5);
        assert_eq!(classify(&trace.physical), Class::SubNeptune);
        let mut ground = trace.physical.clone();
        ground.radius_reference = space_shared::RadiusReference::SolidSurface;
        assert_eq!(classify(&ground), Class::Rocky);
        // Both sides of label boundaries keep the continuous physical solution.
        for (water, metal, light) in [
            (0.25, 0.3, 0.0),
            (0.0, 0.5, 0.0),
            (0.0, 0.3, 0.1),
            (0.0, 0.3, 0.5),
        ] {
            let (_, sa) = sample(
                water * (1.0 - 1e-8),
                metal * (1.0 - 1e-8),
                light * (1.0 - 1e-8),
            );
            let (_, sb) = sample(
                water * (1.0 + 1e-8),
                metal * (1.0 + 1e-8),
                light * (1.0 + 1e-8),
            );
            assert_ne!(classify(&sa.physical), classify(&sb.physical));
            assert!(
                (sa.physical.reference_radius_m / sb.physical.reference_radius_m - 1.0).abs()
                    < 1e-6
            );
        }
    }

    #[test]
    fn continuous_feeding_and_host_mass_priors_not_class_lottery() {
        for seed in 0..100 {
            let sample = |x, mass, compact| {
                draw(
                    &mut Rng::new(&format!("formation:{seed}")),
                    x,
                    1.0,
                    mass,
                    compact,
                )
            };
            let warm = sample(0.1, 1.0, false);
            let cold = sample(2.0, 1.0, false);
            assert!(
                cold.source().mass_kg(Material::Water) > warm.source().mass_kg(Material::Water)
            );
            let compact = sample(2.0, 1.0, true);
            assert!(compact.source().total_mass_kg() <= cold.source().total_mass_kg());
            let light = sample(2.0, 0.08, false);
            assert!(
                light.reservoir(Reservoir::Gas).total_mass_kg()
                    <= cold.reservoir(Reservoir::Gas).total_mass_kg()
            );
            let left = sample(1.0 - 1e-8, 1.0, false);
            let right = sample(1.0 + 1e-8, 1.0, false);
            assert!(
                (left.source().total_mass_kg() / right.source().total_mass_kg() - 1.0).abs() < 1e-6
            );
        }
    }
}
