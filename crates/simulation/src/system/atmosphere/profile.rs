//! Isothermal hydrostatic balance in a central gravitational field.
//! All geometry/mass normalisation lives here, independent of chemistry/optics.
use super::{EXTENT_SCALE_HEIGHTS, ONE_BAR_PA};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub radius_m: f64,
    pub scale_height_m: f64,
    /// False explicitly retains the legacy exponential approximation when a
    /// static column cannot reach its optical cutoff below the sonic radius.
    pub spherical: bool,
}

impl Profile {
    /// Normalise the finite gas column to its actual conserved reservoir.
    pub fn boundary_pressure(
        gas_mass_kg: f64,
        gm: f64,
        radius: f64,
        sound_speed_squared: f64,
    ) -> f64 {
        let gravity = gm / radius.powi(2);
        let height = sound_speed_squared / gravity;
        let planar = gas_mass_kg * gravity / (4.0 * std::f64::consts::PI * radius.powi(2));
        // A finite cutoff omits a tiny tail even in the planar limit. Include
        // that bound so very thin spherical columns are bracketed as well.
        let (mut low, mut high) = (0.0, planar / (1.0 - (-EXTENT_SCALE_HEIGHTS).exp()));
        for _ in 0..64 {
            let mid = 0.5 * (low + high);
            if mid * Self::new(radius, height, mid).mass_factor(mid) < planar {
                low = mid;
            } else {
                high = mid;
            }
        }
        high
    }

    pub fn new(radius_m: f64, scale_height_m: f64, pressure_pa: f64) -> Self {
        let drop = Self::cutoff_drop(pressure_pa);
        // r_s = GM/(2 c_s²), the isothermal Parker sonic radius. This is a
        // hydrostatic validity guard, not an exobase or a solved wind model.
        let spherical = radius_m / scale_height_m > drop + 2.0;
        Self {
            radius_m,
            scale_height_m,
            spherical,
        }
    }

    pub fn cutoff_drop(pressure_pa: f64) -> f64 {
        EXTENT_SCALE_HEIGHTS + (pressure_pa.ln() - ONE_BAR_PA.ln()).max(0.0)
    }

    pub fn log_relative_density(self, height_m: f64) -> f64 {
        let potential_height = if self.spherical {
            height_m / (1.0 + height_m / self.radius_m)
        } else {
            height_m
        };
        -potential_height / self.scale_height_m
    }

    pub fn height_for_drop(self, drop: f64) -> Option<f64> {
        if self.spherical {
            let fraction = drop * self.scale_height_m / self.radius_m;
            (fraction < 1.0).then(|| drop * self.scale_height_m / (1.0 - fraction))
        } else {
            Some(drop * self.scale_height_m)
        }
    }

    pub fn sonic_height_m(self) -> f64 {
        (self.radius_m / (2.0 * self.scale_height_m) - 1.0) * self.radius_m
    }

    /// M = (4 pi R² / g) p * mass_factor. Integrate in potential-drop
    /// coordinates z, so even a very thin atmosphere is well resolved:
    /// factor = integral exp(-z) / (1 - z H/R)^4 dz.
    /// The unresolved fallback deliberately preserves legacy inventory pressure.
    pub fn mass_factor(self, pressure_pa: f64) -> f64 {
        if !self.spherical {
            return 1.0;
        }
        let end = Self::cutoff_drop(pressure_pa).min(64.0);
        const STEPS: usize = 512;
        let dz = end / STEPS as f64;
        let at = |z: f64| (-z).exp() / (1.0 - z * self.scale_height_m / self.radius_m).powi(4);
        let mut sum = at(0.0) + at(end);
        for i in 1..STEPS {
            sum += if i % 2 == 0 { 2.0 } else { 4.0 } * at(i as f64 * dz);
        }
        sum * dz / 3.0
    }
}

#[cfg(test)]
mod tests;
