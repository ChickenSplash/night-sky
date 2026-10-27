//! Cold-trapped secondaries above their vapour-pressure cap, held as one even,
//! well-mixed, uncompressed global layer on the condensed surface. Real frost
//! is polar or nightside; placement on top and the solid phase are conventions.
use super::*;

/// Fixed solid densities near each frost point. O2 and H/He have no cold-trap
/// cap, so they never reach the deposit.
fn density_kg_m3(m: Material) -> Option<f64> {
    match m {
        Material::CarbonDioxide => Some(1560.0),
        Material::Nitrogen => Some(1030.0),
        Material::Argon => Some(1620.0),
        Material::Methane => Some(520.0),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Deposit {
    inventory: Inventory,
    volume_m3: f64,
}

impl Deposit {
    pub(super) fn new(inventory: Inventory) -> Result<Self, Error> {
        let mut volume_m3 = 0.0;
        for m in Material::ALL {
            let mass = inventory.mass_kg(m);
            if mass > 0.0 {
                volume_m3 += mass / density_kg_m3(m).ok_or(Error::UnsupportedCondensate(m))?;
            }
        }
        Ok(Self {
            inventory,
            volume_m3,
        })
    }
    pub(super) fn inventory(&self) -> &Inventory {
        &self.inventory
    }
    pub(super) fn mass_kg(&self) -> f64 {
        self.inventory.total_mass_kg()
    }
    pub(super) fn top_radius_m(&self, base: f64) -> f64 {
        (base.powi(3) + 3.0 * self.volume_m3 / (4.0 * PI)).cbrt()
    }
    /// From the volume, not top - base, so thin layers keep their precision.
    pub(super) fn thickness_m(&self, base: f64) -> f64 {
        let top = self.top_radius_m(base);
        3.0 * self.volume_m3 / (4.0 * PI) / (top * top + top * base + base * base)
    }
    /// Exact hydrostatic weight of the uniform self-gravitating shell.
    pub(super) fn load_pa(&self, host_mass: f64, base: f64) -> f64 {
        if self.volume_m3 == 0.0 {
            return 0.0;
        }
        let density = self.mass_kg() / self.volume_m3;
        let top = self.top_radius_m(base);
        let k = 4.0 / 3.0 * PI * density;
        density
            * GRAVITATIONAL_CONSTANT
            * self.thickness_m(base)
            * ((host_mass - k * base.powi(3)) / (base * top) + 0.5 * k * (top + base))
    }
    pub(super) fn inertia_kg_m2(&self, base: f64) -> f64 {
        if self.volume_m3 == 0.0 {
            return 0.0;
        }
        let (t, b) = (self.top_radius_m(base), base);
        0.4 * self.mass_kg()
            * (t.powi(4) + t.powi(3) * b + t * t * b * b + t * b.powi(3) + b.powi(4))
            / (t * t + t * b + b * b)
    }
}
