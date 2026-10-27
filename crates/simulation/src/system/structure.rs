//! Planet structure: fast approximate generated radii and a separate headless
//! `dry` and restricted `water` radial solvers. Generation still uses the
//! approximate inventory model.
//! Prescriptions, provenance and domains: docs/simulation/planet-composition.md.

pub(crate) mod boundary;
pub mod coupled;
#[cfg(test)]
mod coverage;
pub mod dry;
pub mod parameters;
mod radial;
mod radius;
pub mod water;

use super::composition::{Budget, Material, Reservoir};
use crate::units::{EARTH_MASS_KG, GRAVITATIONAL_CONSTANT};
use std::{f64::consts::PI, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadiusReference {
    /// Dry condensed body's nominal ground, before terrain/rotation corrections.
    SolidSurface,
    /// Cold water-bearing bulk boundary, with no solved phase or ocean surface.
    UnresolvedVolatileBoundary,
    /// Chosen pressure level inside a continuous envelope, never landable ground.
    OneBar,
}

/// Explicit model caveats, not statistical error bars or physical phase labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approximation {
    CompressedVolumeMixing,
    CondensedMassContinuation,
    TraceVolatilesAsWater,
    ColdWaterProxy,
    EnvelopeSurrogate,
    EnvelopeFitInputsBounded,
    /// Thin gas cannot support a one-bar column at the condensed boundary.
    SubBarEnvelope,
    ThinBoundaryColumn,
    /// Condensed boundary retained outside constant-gravity column geometry.
    ExtendedBoundaryColumn,
    BoundaryReferenceTransition,
    DeepBoundaryUnresolved,
}

impl Approximation {
    pub fn id(self) -> &'static str {
        match self {
            Self::CompressedVolumeMixing => "compressed_volume_mixing",
            Self::CondensedMassContinuation => "condensed_mass_continuation",
            Self::TraceVolatilesAsWater => "trace_volatiles_as_water",
            Self::ColdWaterProxy => "cold_water_proxy",
            Self::EnvelopeSurrogate => "envelope_surrogate",
            Self::EnvelopeFitInputsBounded => "envelope_fit_inputs_bounded",
            Self::SubBarEnvelope => "sub_bar_envelope",
            Self::ThinBoundaryColumn => "thin_boundary_column",
            Self::ExtendedBoundaryColumn => "extended_boundary_column",
            Self::BoundaryReferenceTransition => "boundary_reference_transition",
            Self::DeepBoundaryUnresolved => "deep_boundary_unresolved",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Structure {
    mass_kg: f64,
    radius_m: f64,
    condensed_radius_m: f64,
    h_he_mass_fraction: f64,
    inertia_factor: f64,
    reference: RadiusReference,
    approximations: Vec<Approximation>,
}

impl Structure {
    pub fn mass_kg(&self) -> f64 {
        self.mass_kg
    }
    /// Volume-equivalent reference radius, not equatorial radius.
    pub fn radius_m(&self) -> f64 {
        self.radius_m
    }
    /// Cold heavy-material proxy. The environmental correction promotes this to
    /// an approximate boundary with `ThinBoundaryColumn` or
    /// `ExtendedBoundaryColumn`, never a core EOS.
    pub fn condensed_radius_m(&self) -> f64 {
        self.condensed_radius_m
    }
    pub fn h_he_mass_fraction(&self) -> f64 {
        self.h_he_mass_fraction
    }
    /// Approximate central concentration for Darwin-Radau, not an interior solve.
    pub fn inertia_factor(&self) -> f64 {
        self.inertia_factor
    }
    pub fn reference(&self) -> RadiusReference {
        self.reference
    }
    pub fn approximations(&self) -> &[Approximation] {
        &self.approximations
    }
    pub fn mean_density_kg_m3(&self) -> f64 {
        self.mass_kg / (4.0 * PI / 3.0 * self.radius_m.powi(3))
    }
    pub fn reference_gravity_m_s2(&self) -> f64 {
        GRAVITATIONAL_CONSTANT * self.mass_kg / self.radius_m.powi(2)
    }
}

/// Derive mass and radius from the *retained* inventory. Flux is bolometric
/// W/m² and age is Gyr. Neither a class nor an atmospheric column is an input.
/// No iteration, random draws, global state or silent invalid-input fallback.
pub fn derive(
    budget: &Budget,
    irradiation_w_m2: f64,
    age_gyr: f64,
) -> Result<Structure, StructureError> {
    if !irradiation_w_m2.is_finite() || irradiation_w_m2 < 0.0 {
        return Err(StructureError::InvalidValue("irradiation"));
    }
    if !age_gyr.is_finite() || age_gyr <= 0.0 {
        return Err(StructureError::InvalidValue("age"));
    }
    let inventory = budget.retained();
    let mass_kg = inventory.total_mass_kg();
    let mass_earth = mass_kg / EARTH_MASS_KG;
    if !in_domain(
        mass_earth,
        parameters::MIN_MASS_EARTH,
        parameters::MAX_MASS_EARTH,
    ) {
        return Err(StructureError::OutsideDomain("total mass"));
    }
    let hydrogen = inventory.mass_kg(Material::Hydrogen);
    let helium = inventory.mass_kg(Material::Helium);
    let h_he = hydrogen + helium;
    let fraction = h_he / mass_kg;
    let mut approximations = Vec::new();
    let condensed_radius_m = radius::condensed(inventory, &mut approximations)?;
    let (radius_m, reference) = if h_he > 0.0 {
        if !in_domain(
            mass_earth,
            parameters::MIN_ENVELOPE_MASS_EARTH,
            parameters::MAX_MASS_EARTH,
        ) {
            return Err(StructureError::OutsideDomain("H/He planet mass"));
        }
        if !in_domain(
            helium / h_he,
            parameters::MIN_HELIUM_SHARE,
            parameters::MAX_HELIUM_SHARE,
        ) {
            return Err(StructureError::OutsideDomain("envelope helium share"));
        }
        // Dissolved light gases are not an envelope and need their own physics.
        for reservoir in [Reservoir::Interior, Reservoir::CondensedVolatiles] {
            if budget.reservoir(reservoir).mass_kg(Material::Hydrogen) > 0.0
                || budget.reservoir(reservoir).mass_kg(Material::Helium) > 0.0
            {
                return Err(StructureError::OutsideDomain("dissolved H/He"));
            }
        }
        approximations.push(Approximation::EnvelopeSurrogate);
        let radius = radius::envelope(
            mass_earth,
            fraction,
            condensed_radius_m,
            irradiation_w_m2,
            age_gyr,
            &mut approximations,
        );
        // A necessary thin-column supply check, not an estimate of the entire
        // giant envelope. It also gives a continuous bare-radius stripping limit.
        let support_radius = (GRAVITATIONAL_CONSTANT * mass_kg * h_he
            / (4.0 * PI * super::atmosphere::ONE_BAR_PA))
            .sqrt()
            .sqrt();
        if support_radius <= condensed_radius_m {
            approximations.push(Approximation::SubBarEnvelope);
            (
                condensed_radius_m,
                condensed_reference(inventory.mass_kg(Material::Water)),
            )
        } else {
            let capacity = support_radius - condensed_radius_m;
            let thickness = radius - condensed_radius_m;
            let limited_thickness = thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25);
            (
                condensed_radius_m + limited_thickness,
                RadiusReference::OneBar,
            )
        }
    } else {
        let reference = condensed_reference(inventory.mass_kg(Material::Water));
        (condensed_radius_m, reference)
    };
    let heavy_mass = mass_kg - h_he;
    let metal_share = if heavy_mass > 0.0 {
        inventory.mass_kg(Material::Metal) / heavy_mass
    } else {
        0.0
    };
    let heavy_inertia =
        parameters::HEAVY_INERTIA_BASE - parameters::METAL_INERTIA_REDUCTION * metal_share;
    let inertia_factor =
        ((1.0 - fraction) * heavy_inertia * (condensed_radius_m / radius_m).powi(2)
            + fraction * parameters::ENVELOPE_INERTIA)
            .clamp(parameters::MIN_INERTIA, parameters::HEAVY_INERTIA_BASE);
    let result = Structure {
        mass_kg,
        radius_m,
        condensed_radius_m,
        h_he_mass_fraction: fraction,
        inertia_factor,
        reference,
        approximations,
    };
    for value in [
        result.radius_m,
        result.mean_density_kg_m3(),
        result.reference_gravity_m_s2(),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(StructureError::InvalidValue("derived structure"));
        }
    }
    Ok(result)
}

fn condensed_reference(water_kg: f64) -> RadiusReference {
    if water_kg > 0.0 {
        RadiusReference::UnresolvedVolatileBoundary
    } else {
        RadiusReference::SolidSurface
    }
}

/// Allow only floating-point summation noise at declared inclusive boundaries.
fn in_domain(value: f64, min: f64, max: f64) -> bool {
    value.is_finite()
        && value >= min * (1.0 - 16.0 * f64::EPSILON)
        && value <= max * (1.0 + 16.0 * f64::EPSILON)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureError {
    InvalidValue(&'static str),
    OutsideDomain(&'static str),
    UnsupportedMaterial(Material),
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidValue(name) => write!(f, "invalid structure {name}"),
            Self::OutsideDomain(name) => write!(f, "outside structure model domain: {name}"),
            Self::UnsupportedMaterial(m) => {
                write!(f, "unsupported bulk structure material: {}", m.id())
            }
        }
    }
}

impl std::error::Error for StructureError {}

#[cfg(test)]
mod tests;
