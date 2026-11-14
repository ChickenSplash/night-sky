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
        upper_root(0.0, planar / (1.0 - (-EXTENT_SCALE_HEIGHTS).exp()), |p| {
            p * Self::new(radius, height, p).mass_factor(p) - planar
        })
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
        let ratio = self.scale_height_m / self.radius_m;
        if ratio * end <= SERIES_LIMIT {
            return series(ratio, end);
        }
        let panels = PANELS.max(end.ceil() as usize);
        let width = end / panels as f64;
        (0..panels)
            .map(|panel| {
                GAUSS
                    .iter()
                    .map(|(x, w)| {
                        let z = (panel as f64 + x) * width;
                        w * (-z).exp() / (1.0 - z * ratio).powi(4)
                    })
                    .sum::<f64>()
                    * width
            })
            .sum()
    }
}

/// Thin columns: expand (1 - zε)^-4 in ε and integrate each power of z exactly
/// with the incomplete-gamma recurrence γ(k+2) = (k+1) γ(k+1) - E^(k+1) e^-E.
/// Converges to roundoff within about 30 terms below this value of εE.
const SERIES_LIMIT: f64 = 0.3;

fn series(ratio: f64, end: f64) -> f64 {
    let decay = (-end).exp();
    let (mut gamma, mut power, mut scale, mut sum) = (1.0 - decay, 1.0, 1.0, 0.0);
    for k in 0..64 {
        let n = k as f64;
        let term = (n + 1.0) * (n + 2.0) * (n + 3.0) / 6.0 * scale * gamma;
        sum += term;
        if term <= f64::EPSILON * 0.1 * sum {
            break;
        }
        power *= end;
        gamma = (n + 1.0) * gamma - power * decay;
        scale *= ratio;
    }
    sum
}

/// Extended columns: composite five-point Gauss-Legendre, panels at most one
/// potential drop wide. The integrand's pole lies at least 2 beyond the
/// cutoff, so the rule holds about 1e-12.
const PANELS: usize = 24;
const GAUSS: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

/// Upper end of a bracket closed to float resolution around the sign change
/// of `residual`, negative at `low`. Illinois false position, with a central
/// step whenever two steps fail to halve the bracket. The returned point never
/// has a negative residual, so callers' caps are not exceeded by roundoff.
pub(crate) fn upper_root(mut low: f64, mut high: f64, residual: impl Fn(f64) -> f64) -> f64 {
    let (mut low_weight, mut high_weight) = (residual(low), residual(high));
    // Degenerate brackets keep the limits plain bisection converged to.
    if low_weight.is_nan() || high_weight.is_nan() || low_weight >= 0.0 {
        return low + (high - low) * f64::EPSILON;
    }
    if high_weight < 0.0 {
        return high;
    }
    let mut kept_low = None;
    let mut checkpoint = (0_u8, high - low);
    loop {
        let width = high - low;
        if width <= 4.0 * f64::EPSILON * high {
            return high;
        }
        let interpolated = low + width * (low_weight / (low_weight - high_weight));
        checkpoint.0 += 1;
        let stalled = checkpoint.0 > 2;
        if stalled || width <= 0.5 * checkpoint.1 {
            checkpoint = (0, width);
        }
        let mid = if !stalled && interpolated > low && interpolated < high {
            interpolated
        } else {
            0.5 * (low + high)
        };
        if mid <= low || mid >= high {
            return high;
        }
        let value = residual(mid);
        if value < 0.0 {
            low = mid;
            low_weight = value;
            if kept_low == Some(false) {
                high_weight *= 0.5;
            }
            kept_low = Some(false);
        } else {
            high = mid;
            high_weight = value;
            if kept_low == Some(true) {
                low_weight *= 0.5;
            }
            kept_low = Some(true);
        }
    }
}

#[cfg(test)]
mod tests;
