//! Explicit renderer inputs, not structural layer edges or navigation limits.
//! The operational cutoff is a declared approximation, not a solved photosphere.
use super::Optics;
use crate::system::{atmosphere::*, composition::Material, physical};
use space_shared::{PlanetResult, RadiusReference};

pub const MODEL_ID: &str = "isothermal_visible_column_v1";
pub const LIMITATIONS: &[&str] = &[
    "Isothermal ideal gas and dilute visible optics; no resolved clouds or deep envelope",
    "Constant enclosed gravitational parameter through the optical column; no gas self-gravity or rotation",
    "Lower boundary is an opaque visual proxy, not evidence of solid or landable ground",
    "Finite density-drop cutoff, not a material edge, exobase or guaranteed optical-error bound",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reference {
    pub radius_m: f64,
    pub pressure_pa: f64,
    pub temperature_k: f64,
    pub gravity_m_s2: f64,
    pub level: ReferenceLevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DensityModel {
    SphericalIsothermal,
    /// The column's explicit weak-binding approximation, not a solved wind.
    UnresolvedExponential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LowerBoundaryKind {
    /// Located reference, with phase/opacity/landing properties still unresolved.
    CondensedReferenceProxy,
    /// One bar is a coordinate, not a detected cloud deck or gas/solid boundary.
    GasReferenceProxy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LowerBoundary {
    pub radius_m: f64,
    pub kind: LowerBoundaryKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OuterCutoff {
    pub radius_m: f64,
    pub height_m: f64,
    /// Gas density drops in the declared profile. Aerosols may extend farther.
    pub gas_density_drops: f64,
    pub aerosol_scale_heights: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpticalColumn {
    pub reference: Reference,
    pub density_model: DensityModel,
    pub lower_boundary: LowerBoundary,
    pub outer_cutoff: OuterCutoff,
    pub optics: Optics,
}

impl OpticalColumn {
    /// Explicit reference inputs. `gm` is the enclosed gravitational parameter
    /// at this reference, not necessarily the mass of the entire planet.
    /// No layer extent or material name is accepted as a visibility boundary.
    pub fn new(gas: &GasAtmosphere, gm: f64, radius_m: f64) -> Result<Self, AtmosphereError> {
        let optics = gas.optics(gm, radius_m)?;
        let outer_radius_m = radius_m + optics.column.extent_m;
        if !outer_radius_m.is_finite() || outer_radius_m <= radius_m {
            return Err(AtmosphereError::InvalidValue("optical outer radius"));
        }
        Ok(Self {
            reference: Reference {
                radius_m,
                pressure_pa: gas.pressure_pa(),
                temperature_k: gas.temperature_k(),
                gravity_m_s2: optics.column.gravity_m_s2,
                level: gas.reference_level(),
            },
            density_model: if optics.column.profile.spherical {
                DensityModel::SphericalIsothermal
            } else {
                DensityModel::UnresolvedExponential
            },
            lower_boundary: LowerBoundary {
                radius_m,
                kind: match gas.reference_level() {
                    ReferenceLevel::Surface => LowerBoundaryKind::CondensedReferenceProxy,
                    ReferenceLevel::OneBar => LowerBoundaryKind::GasReferenceProxy,
                },
            },
            outer_cutoff: OuterCutoff {
                radius_m: outer_radius_m,
                height_m: optics.column.extent_m,
                gas_density_drops: profile::Profile::cutoff_drop(gas.pressure_pa()),
                aerosol_scale_heights: EXTENT_SCALE_HEIGHTS,
            },
            optics,
        })
    }

    /// Prepare optics for a unified-result candidate without adopting it. Gas
    /// must already belong to that result: never repartition or fit a column to
    /// a legacy atmosphere here. A fallback's condensed secondary proxy cannot
    /// acquire a secondary atmosphere just because a live planet has one.
    pub fn from_result(
        result: &PlanetResult,
        gas: &GasAtmosphere,
    ) -> Result<Self, AtmosphereError> {
        let invalid = || AtmosphereError::InvalidValue("result optical reference");
        physical::validate_result(result).map_err(|_| invalid())?;
        let record = &result.record;
        if record.layers.iter().any(|layer| {
            layer
                .phase_allocations
                .iter()
                .any(|phase| phase.condensate_kg > 0.0)
        }) {
            return Err(AtmosphereError::UnsupportedOptics(
                "suspended condensates require local vapour and cloud optical inputs",
            ));
        }
        let facts = &record.physical;
        let one_bar = facts.radius_reference == RadiusReference::OneBar;
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs());
        if one_bar != (gas.reference_level() == ReferenceLevel::OneBar)
            || !close(record.surface_pressure_pa, gas.pressure_pa())
            || !close(record.surface_temperature_k, gas.temperature_k())
        {
            return Err(invalid());
        }
        let mole_weights = Gas::ALL.map(|species| {
            facts
                .materials
                .iter()
                .find(|m| m.material == Material::from_gas(species).id())
                .map_or(0.0, |m| m.gas_kg / species.molecular_mass_u())
        });
        let total: f64 = mole_weights.iter().sum();
        if !total.is_finite()
            || total <= 0.0
            || !Gas::ALL
                .into_iter()
                .zip(mole_weights)
                .all(|(species, weight)| close(weight / total, gas.composition().fraction(species)))
        {
            return Err(invalid());
        }
        // Enclosed-mass gravity is authoritative. Do not substitute total mass.
        Self::new(
            gas,
            facts.reference_gravity_m_s2 * facts.reference_radius_m.powi(2),
            facts.reference_radius_m,
        )
    }
}

#[cfg(test)]
mod tests;
