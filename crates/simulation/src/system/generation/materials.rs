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
    // The continuous onset is a formation prior, not a clamp after a failed solve.
    let onset = ((heavy - 0.1) / 0.1).clamp(0.0, 1.0);
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

/// Temporary compatibility description only. Checkpoint 4 replaces these old
/// names and audits consumers. This function never chooses a physical quantity.
pub(super) fn provisional_class(structure: &crate::system::structure::Structure) -> Class {
    use crate::system::structure::parameters::classification;
    let fraction = structure.h_he_mass_fraction();
    if fraction >= classification::H_HE_DOMINATED_MASS_FRACTION {
        Class::GasGiant
    } else if fraction >= 0.1 {
        Class::IceGiant
    } else if fraction >= classification::SIGNIFICANT_H_HE_MASS_FRACTION {
        Class::SubNeptune
    } else {
        Class::Rocky
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::composition::Reservoir;

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
