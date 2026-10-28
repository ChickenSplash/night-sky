//! Total, closed-form structure for every body the layered solver refuses.
//! Published mass-radius fits and prescribed layer density profiles: no
//! iteration, external data or error path. Never counted as a layered solve.
//! See docs/simulation/planet-composition.md#fallback-structure.
use super::{
    Approximation,
    coupled::Error,
    parameters::{CONDENSED_FIT_LIMIT_EARTH, EARTH_REFERENCE_FLUX_W_M2, EARTH_REFERENCE_RADIUS_M},
    radius,
};
use crate::{
    system::composition::{Budget, Inventory, Material},
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K, EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use space_shared::{PlanetEnvironment, RadiusReference};
use std::f64::consts::PI;

pub const MODEL_ID: &str = "fallback_mass_radius_v1";
pub const NOTES: &[&str] = &[
    "Fallback: the layered model refused this body. Published mass-radius fits, not a hydrostatic EOS solve",
    "Uniform condensed layers and a linearly thinning gas layer; pressures follow these prescribed profiles",
    "Temperature is the equilibrium estimate; no greenhouse, phase or thermal structure",
];
pub const VOLATILE_PROXY_NOTE: &str =
    "Secondary volatiles use a condensed water-density proxy; no separate atmosphere";
pub const CLAMPED_NOTE: &str = "Inputs outside the fits' calibration were clamped to their bounds";
pub const CONTINUATION_NOTE: &str =
    "Condensed mass above 20 Earth masses uses a power-law continuation of the fit";

const SECONDARIES: [Material; 5] = [
    Material::Nitrogen,
    Material::CarbonDioxide,
    Material::Methane,
    Material::Oxygen,
    Material::Argon,
];
const MIN_TEMPERATURE_K: f64 = 20.0;
/// Keeps the gas layer's density finite when the fit leaves it no thickness.
const MIN_GAS_THICKNESS: f64 = 1e-6;
const PANELS: usize = 32;
const GAUSS: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Core,
    Mantle,
    Water,
    Volatiles,
    Envelope,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Mantle => "mantle",
            Self::Water => "water_compression",
            Self::Volatiles => "volatiles",
            Self::Envelope => "envelope",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub kind: Kind,
    pub inventory: Inventory,
    pub inner_radius_m: f64,
    pub outer_radius_m: f64,
    pub inner_pressure_pa: f64,
    pub outer_pressure_pa: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fallback {
    /// Why the layered solver refused; always shown with the result.
    pub reason: Error,
    pub source: Inventory,
    pub retained: Inventory,
    pub escaped: Inventory,
    /// Innermost first, contiguous from the centre.
    pub regions: Vec<Region>,
    pub radius_m: f64,
    pub condensed_radius_m: f64,
    pub reference: RadiusReference,
    pub one_bar_radius_m: Option<f64>,
    pub surface_temperature_k: f64,
    pub surface_pressure_pa: f64,
    pub inertia_kg_m2: f64,
    /// Fit inputs were clamped or continued beyond calibration.
    pub extrapolated: bool,
    pub notes: Vec<&'static str>,
}

impl Fallback {
    pub fn mass_kg(&self) -> f64 {
        self.retained.total_mass_kg()
    }

    pub fn surface(&self) -> &'static str {
        if self.reference == RadiusReference::OneBar {
            return "envelope";
        }
        match self.regions.iter().rev().find(|r| r.kind != Kind::Envelope) {
            Some(r) if r.kind == Kind::Water => "unresolved water",
            Some(r) if r.kind == Kind::Volatiles => "unresolved volatiles",
            _ => "rock",
        }
    }
}

/// Never fails: every retained budget gets finite, conserved, contiguous layers.
pub fn solve(budget: &Budget, environment: &PlanetEnvironment, reason: Error) -> Fallback {
    let retained = *budget.retained();
    let h_he = retained.mass_kg(Material::Hydrogen) + retained.mass_kg(Material::Helium);
    let mut notes = NOTES.to_vec();
    let mut extrapolated = false;

    let only = |materials: &[Material]| {
        Inventory::new(
            &materials
                .iter()
                .map(|&m| (m, retained.mass_kg(m)))
                .collect::<Vec<_>>(),
        )
        .expect("subset of a valid inventory")
    };
    let volatiles = only(&SECONDARIES);
    if volatiles.total_mass_kg() > 0.0 {
        notes.push(VOLATILE_PROXY_NOTE);
    }
    let mut stack = vec![
        (Kind::Core, only(&[Material::Metal])),
        (Kind::Mantle, only(&[Material::Silicate])),
        (Kind::Water, only(&[Material::Water])),
        (Kind::Volatiles, volatiles),
    ];
    stack.retain(|(_, inventory)| inventory.total_mass_kg() > 0.0);
    let gas = (h_he > 0.0).then(|| only(&[Material::Hydrogen, Material::Helium]));

    // Seager-fit volumes at the whole condensed mass, as compressed volume mixing.
    let condensed_kg: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    let condensed_earth = condensed_kg / EARTH_MASS_KG;
    if condensed_earth > CONDENSED_FIT_LIMIT_EARTH {
        extrapolated = true;
        notes.push(CONTINUATION_NOTE);
    }
    let mut regions = Vec::new();
    let mut volume = 0.0;
    for (kind, inventory) in stack {
        let proxy = match kind {
            Kind::Core => Material::Metal,
            Kind::Mantle => Material::Silicate,
            _ => Material::Water,
        };
        let inner = EARTH_REFERENCE_RADIUS_M * f64::cbrt(volume);
        volume += inventory.total_mass_kg() / condensed_kg
            * radius::pure_radius_earth(proxy, condensed_earth)
                .expect("refractory and water fits exist")
                .powi(3);
        regions.push(Region {
            kind,
            inventory,
            inner_radius_m: inner,
            outer_radius_m: EARTH_REFERENCE_RADIUS_M * f64::cbrt(volume),
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
    }
    let condensed_radius_m = regions.last().map_or(0.0, |r| r.outer_radius_m);
    let surface_temperature_k =
        sanitised(environment.equilibrium_temperature_k, MIN_TEMPERATURE_K).max(MIN_TEMPERATURE_K);
    let flux = sanitised(environment.irradiation_w_m2, EARTH_REFERENCE_FLUX_W_M2);
    let age = sanitised(environment.age_gyr, 5.0);
    let mut clamped = flux != environment.irradiation_w_m2
        || age != environment.age_gyr
        || !environment.equilibrium_temperature_k.is_finite();

    let mass_kg = retained.total_mass_kg();
    let mut reference = if regions
        .last()
        .is_some_and(|r| matches!(r.kind, Kind::Core | Kind::Mantle))
    {
        RadiusReference::SolidSurface
    } else {
        RadiusReference::UnresolvedVolatileBoundary
    };
    let mut radius_m = condensed_radius_m;
    let mut one_bar_radius_m = None;
    if let Some(gas) = gas {
        let mut warnings = Vec::new();
        let one_bar = radius::one_bar(mass_kg, h_he, condensed_radius_m, flux, age, &mut warnings);
        clamped |= warnings.contains(&Approximation::EnvelopeFitInputsBounded);
        let mut top = condensed_radius_m;
        if let Some(r) = one_bar {
            (radius_m, reference, one_bar_radius_m) = (r, RadiusReference::OneBar, Some(r));
            top = r.max(condensed_radius_m * (1.0 + MIN_GAS_THICKNESS));
        } else {
            // Sub-bar gas: represent one isothermal scale height, capped at the
            // body's size. Its mass is too small to matter for structure.
            let molar_kg = gas.total_mass_kg()
                / Material::ALL
                    .into_iter()
                    .filter_map(|m| Some(gas.mass_kg(m) / m.properties().gas?.molecular_mass_u()))
                    .sum::<f64>()
                * ATOMIC_MASS_KG;
            let gravity = GRAVITATIONAL_CONSTANT * mass_kg / condensed_radius_m.powi(2);
            let scale_height = BOLTZMANN_J_K * surface_temperature_k / (molar_kg * gravity);
            top = top
                .max(condensed_radius_m + scale_height.min(condensed_radius_m))
                .max(condensed_radius_m * (1.0 + MIN_GAS_THICKNESS));
        }
        regions.push(Region {
            kind: Kind::Envelope,
            inventory: gas,
            inner_radius_m: condensed_radius_m,
            outer_radius_m: top,
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
    }

    if clamped {
        extrapolated = true;
        notes.push(CLAMPED_NOTE);
    }

    // Hydrostatic pressures, outermost first, and inertia from the same profiles.
    let mut enclosed: Vec<f64> = regions
        .iter()
        .scan(0.0, |m, r| {
            let inner = *m;
            *m += r.inventory.total_mass_kg();
            Some(inner)
        })
        .collect();
    let mut pressure = 0.0;
    let mut inertia_kg_m2 = 0.0;
    for (region, inner_mass) in regions.iter_mut().zip(enclosed.drain(..)).rev() {
        let shell = Shell::new(region, inner_mass);
        region.outer_pressure_pa = pressure;
        pressure += shell.pressure_drop_pa();
        region.inner_pressure_pa = pressure;
        inertia_kg_m2 += shell.inertia_kg_m2();
    }
    let surface_pressure_pa = match reference {
        RadiusReference::OneBar => crate::system::atmosphere::ONE_BAR_PA,
        _ => regions
            .iter()
            .find(|r| r.kind == Kind::Envelope)
            .map_or(0.0, |r| r.inner_pressure_pa),
    };

    Fallback {
        reason,
        source: *budget.source(),
        retained,
        escaped: *budget.escaped(),
        regions,
        radius_m,
        condensed_radius_m,
        reference,
        one_bar_radius_m,
        surface_temperature_k,
        surface_pressure_pa,
        inertia_kg_m2,
        extrapolated,
        notes,
    }
}

/// Non-finite or negative environment values take a neutral fit input instead
/// of propagating NaN; the result is then labelled as clamped.
fn sanitised(value: f64, neutral: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        neutral
    }
}

/// One region's prescribed density: uniform when condensed, falling linearly
/// to zero at the top for gas. Integrated in the shell coordinate t = (r - a) / L
/// so thin shells keep their precision.
struct Shell {
    inner_mass_kg: f64,
    a: f64,
    l: f64,
    rho0: f64,
    gas: bool,
}

impl Shell {
    fn new(region: &Region, inner_mass_kg: f64) -> Self {
        let a = region.inner_radius_m;
        let l = region.outer_radius_m - a;
        let gas = region.kind == Kind::Envelope;
        let mut shell = Self {
            inner_mass_kg,
            a,
            l,
            rho0: 1.0,
            gas,
        };
        shell.rho0 = region.inventory.total_mass_kg() / shell.mass_to(1.0);
        shell
    }

    fn density(&self, t: f64) -> f64 {
        if self.gas {
            self.rho0 * (1.0 - t)
        } else {
            self.rho0
        }
    }

    /// Mass between the inner boundary and t. Every grouped term is
    /// non-negative, so nothing cancels.
    fn mass_to(&self, t: f64) -> f64 {
        let (a, l) = (self.a, self.l);
        let q = if self.gas {
            a * a * t * (1.0 - t / 2.0)
                + a * l * t * t * (1.0 - 2.0 * t / 3.0)
                + l * l * t.powi(3) * (1.0 / 3.0 - t / 4.0)
        } else {
            a * a * t + a * l * t * t + l * l * t.powi(3) / 3.0
        };
        4.0 * PI * self.rho0 * l * q
    }

    fn pressure_drop_pa(&self) -> f64 {
        integrate(|t| {
            let r = self.a + self.l * t;
            GRAVITATIONAL_CONSTANT * (self.inner_mass_kg + self.mass_to(t)) * self.density(t)
                / (r * r)
                * self.l
        })
    }

    fn inertia_kg_m2(&self) -> f64 {
        integrate(|t| 8.0 * PI / 3.0 * (self.a + self.l * t).powi(4) * self.density(t) * self.l)
    }
}

/// Composite five-point Gauss-Legendre on [0, 1]. Nodes never touch t = 0.
fn integrate(f: impl Fn(f64) -> f64) -> f64 {
    let width = 1.0 / PANELS as f64;
    (0..PANELS)
        .map(|panel| {
            GAUSS
                .iter()
                .map(|(x, w)| w * f((panel as f64 + x) * width))
                .sum::<f64>()
                * width
        })
        .sum()
}

#[cfg(test)]
mod tests;
