//! Cached, smooth Gaussian volume patches for unresolved galactic starlight.
//! No dust or spiral modulation yet. Each component is normalised to its model
//! population, so tessellation changes do not change the represented light.
use crate::{
    galaxy::{Component, GalaxyModel},
    light::blackbody_colour,
    population,
};

pub const DISTANCES: usize = 32;

#[derive(Clone, Debug)]
pub struct Patch {
    pub position_pc: [f64; 3],
    /// Axis-aligned Gaussian standard deviations, in parsecs.
    pub sigma_pc: [f64; 3],
    pub stars: f64,
    pub component: usize,
}

/// Smooth the arms only for the glow prototype, not the generated stars.
pub fn patches(model: &GalaxyModel, detail: f64) -> Vec<Patch> {
    assert!(
        detail.is_finite() && detail > 0.0,
        "positive finite patch detail required"
    );
    let mut smooth = model.clone();
    smooth.thin.arm_strength = 0.0;
    smooth.arms.arm_strength = 0.0;
    let mut out = Vec::new();
    for (component, step, extent) in [
        (
            Component::Bulge,
            [400.0, 250.0, 400.0],
            [4000.0, 1500.0, 4000.0],
        ),
        (
            Component::ThinDisc,
            [1200.0, 300.0, 1200.0],
            [15600.0, 900.0, 15600.0],
        ),
        (
            Component::Arms,
            [1200.0, 100.0, 1200.0],
            [15600.0, 300.0, 15600.0],
        ),
        (
            Component::ThickDisc,
            [1200.0, 900.0, 1200.0],
            [15600.0, 2700.0, 15600.0],
        ),
        (Component::Halo, [6000.0; 3], [48000.0, 30000.0, 48000.0]),
    ] {
        let start = out.len();
        let step = step.map(|s| s / detail);
        let n = std::array::from_fn::<_, 3, _>(|i| (extent[i] / step[i]).floor() as i32);
        for x in -n[0]..=n[0] {
            for y in -n[1]..=n[1] {
                for z in -n[2]..=n[2] {
                    let position_pc = [x as f64 * step[0], y as f64 * step[1], z as f64 * step[2]];
                    let stars = smooth.component_density(component, position_pc)
                        * step.iter().product::<f64>();
                    if stars <= 0.0 {
                        continue;
                    }
                    out.push(Patch {
                        position_pc,
                        sigma_pc: step.map(|s| s * 0.65),
                        stars,
                        component: component as usize,
                    });
                }
            }
        }
        let total: f64 = out[start..].iter().map(|p| p.stars).sum();
        for patch in &mut out[start..] {
            patch.stars *= model.stars_in(component) / total;
        }
    }
    out.push(Patch {
        position_pc: [0.0; 3],
        sigma_pc: [model.nucleus_pc; 3],
        stars: model.stars_in(Component::Nucleus),
        component: 0,
    });
    out
}

/// Log-spaced distances from 0.1 pc to 100 kpc.
pub fn distance(index: usize) -> f64 {
    10f64.powf(-1.0 + index as f64 * 6.0 / (DISTANCES - 1) as f64)
}

/// Mean RGB luminosity per star *below* the point renderer's magnitude limit.
/// Stratified samples include rare bright populations without a huge census.
pub fn unresolved_table() -> Vec<[f32; 4]> {
    let mut table = vec![[0.0; 4]; 6 * DISTANCES];
    let reach = population::reach_sq_per_luminosity(population::VISIBLE_MAGNITUDE);
    for component in Component::ALL {
        for (tier, share) in population::tier_shares(component).into_iter().enumerate() {
            if share == 0.0 {
                continue;
            }
            for sample in 0..64 {
                let star = population::tiered_star(&format!("glow:{sample}"), component, tier);
                let colour = blackbody_colour(star.temperature_k);
                for i in 0..DISTANCES {
                    if star.luminosity_solar * reach >= distance(i).powi(2) {
                        continue;
                    }
                    for (channel, value) in colour.into_iter().enumerate() {
                        table[component as usize * DISTANCES + i][channel] +=
                            (share / 64.0 * star.luminosity_solar * value) as f32;
                    }
                }
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_detail_preserves_component_populations() {
        let model = GalaxyModel::milky_way();
        for detail in [0.75, 1.0] {
            let patches = patches(&model, detail);
            assert!(patches.len() < 40000);
            for component in Component::ALL {
                let total: f64 = patches
                    .iter()
                    .filter(|p| p.component == component as usize)
                    .map(|p| p.stars)
                    .sum();
                assert!((total / model.stars_in(component) - 1.0).abs() < 1e-10);
            }
        }
    }
    #[test]
    fn unresolved_light_is_finite_and_increases_with_distance() {
        let table = unresolved_table();
        for component in table.chunks_exact(DISTANCES) {
            for pair in component.windows(2) {
                for (&near, &far) in pair[0][..3].iter().zip(&pair[1][..3]) {
                    assert!(far.is_finite() && far >= near);
                }
            }
            assert!(component[DISTANCES - 1][0] > 0.0);
        }
    }
}
