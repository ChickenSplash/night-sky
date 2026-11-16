//! Total, closed-form structure for every body before and without a layered
//! solve. Radii are read from the layered solver's own table and layers take
//! prescribed density profiles: no iteration or error path. Never counted as
//! a layered solve. See docs/simulation/planet-composition.md#fallback-structure.
use super::{
    coupled::Error,
    table::{CONDENSED_MASS_EARTH, Table},
};
use crate::{
    system::{
        composition::{Budget, Inventory, Material},
        generation::retention::accessible_fraction,
    },
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K, EARTH_MASS_KG, GRAVITATIONAL_CONSTANT},
};
use space_shared::{PlanetEnvironment, RadiusReference};
use std::f64::consts::PI;

pub mod decide;
pub mod partition;
pub use decide::{Decided, decide};

pub const MODEL_ID: &str = "fallback_structure_table_v1";
pub const NOTES: &[&str] = &[
    "Fallback: radius read from the layered solver's table, not a hydrostatic solve of this body",
    "Uniform condensed layers and a linearly thinning gas layer; pressures follow these prescribed profiles",
    "Temperature is the equilibrium estimate, or at a one-bar reference the envelope's prescribed intrinsic-heat profile; no greenhouse or phase",
];
pub const VOLATILE_PROXY_NOTE: &str = "Accessible secondary volatiles use a condensed water-density proxy; protected ones stay in the rock";
pub const CLAMPED_NOTE: &str = "Outside the layered solver's domain: inputs beyond the table take its bounds, and cells it refuses hold a surrogate anchored to their solved neighbours";
pub const CONTINUATION_NOTE: &str =
    "Condensed mass above 20 Earth masses uses a power-law continuation of the table";
pub const REFERENCE_CORE_NOTE: &str = "Envelope thickness is tabulated over a reference rock and metal core; this body's own core radius replaces that core's";

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
    /// Cold-trapped secondaries on a decided surface.
    Frost,
    Envelope,
    Atmosphere,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Mantle => "mantle",
            Self::Water => "water_compression",
            Self::Volatiles => "volatiles",
            Self::Frost => "frost",
            Self::Envelope => "envelope",
            Self::Atmosphere => "atmosphere",
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
    /// Present only after a layered refusal. Initial fallback invents no error.
    pub reason: Option<Error>,
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
    /// Inertia factors of the core and of the gas under a tabulated H/He
    /// envelope, each over its own mass and outer radius squared.
    pub envelope_inertia: Option<[f64; 2]>,
    /// Present only when the synchronous surface decision set the temperature.
    pub optical_depth: Option<f64>,
    /// Subset of `escaped` the surface decision stripped from an unbound column.
    pub blowoff: Inventory,
    /// Table inputs were clamped, continued or read from surrogate cells.
    pub extrapolated: bool,
    pub notes: Vec<&'static str>,
}

impl Fallback {
    pub fn mass_kg(&self) -> f64 {
        self.retained.total_mass_kg()
    }

    /// Spin shape follows inertia, so it must not move when the layers
    /// arrive: a tabulated envelope takes the layered solver's own, core and
    /// gas apart, in place of the display layers' uniform one.
    fn tabulated_envelope_inertia_kg_m2(&self) -> Option<f64> {
        let [core, gas] = self.envelope_inertia?;
        let one_bar = self.one_bar_radius_m?;
        let shell = self.regions.iter().find(|r| r.kind == Kind::Envelope)?;
        let gas_kg = shell.inventory.total_mass_kg();
        // Gas that barely reaches one bar is tabulated against a radius it
        // extends far beyond. No shell exceeds a thin one at its own top.
        let gas = (gas * one_bar.powi(2)).min(2.0 / 3.0 * shell.outer_radius_m.powi(2));
        Some(core * (self.mass_kg() - gas_kg) * self.condensed_radius_m.powi(2) + gas * gas_kg)
    }

    pub fn surface(&self) -> &'static str {
        if self.reference == RadiusReference::OneBar {
            return "envelope";
        }
        match self
            .regions
            .iter()
            .rev()
            .find(|r| !matches!(r.kind, Kind::Envelope | Kind::Atmosphere))
        {
            Some(r) if r.kind == Kind::Water => "unresolved water",
            Some(r) if r.kind == Kind::Volatiles => "unresolved volatiles",
            Some(r) if r.kind == Kind::Frost => "frost",
            _ => "rock",
        }
    }
}

/// Never fails: every retained budget gets finite, conserved, contiguous layers.
pub fn solve(budget: &Budget, environment: &PlanetEnvironment, reason: Error) -> Fallback {
    prescribed(budget, environment, Some(reason))
}

/// Fast first answer before any layered attempt. Same prescription as refusal
/// fallback, without pretending the layered solver has already failed.
pub fn initial(budget: &Budget, environment: &PlanetEnvironment) -> Fallback {
    prescribed(budget, environment, None)
}

fn prescribed(budget: &Budget, environment: &PlanetEnvironment, reason: Option<Error>) -> Fallback {
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
    // Protected secondaries stay in the rock, as in the layered solve. Only
    // the accessible supply forms the volatile layer the partition draws on.
    let protected = |m: Material| {
        (budget.source().mass_kg(m) * (1.0 - accessible_fraction(m))).min(retained.mass_kg(m))
    };
    let mantle = Inventory::new(
        &std::iter::once((Material::Silicate, retained.mass_kg(Material::Silicate)))
            .chain(SECONDARIES.map(|m| (m, protected(m))))
            .collect::<Vec<_>>(),
    )
    .expect("subset of a valid inventory");
    let volatiles = Inventory::new(&SECONDARIES.map(|m| (m, retained.mass_kg(m) - protected(m))))
        .expect("subset of a valid inventory");
    if volatiles.total_mass_kg() > 0.0 {
        notes.push(VOLATILE_PROXY_NOTE);
    }
    let mut stack = vec![
        (Kind::Core, only(&[Material::Metal])),
        (Kind::Mantle, mantle),
        (Kind::Water, only(&[Material::Water])),
        (Kind::Volatiles, volatiles),
    ];
    stack.retain(|(_, inventory)| inventory.total_mass_kg() > 0.0);
    let gas = (h_he > 0.0).then(|| only(&[Material::Hydrogen, Material::Helium]));

    let condensed_kg: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    if condensed_kg / EARTH_MASS_KG > CONDENSED_MASS_EARTH[1] {
        extrapolated = true;
        notes.push(CONTINUATION_NOTE);
    }
    // An unilluminated body takes the table's cold bound, never Earth's.
    let surface_temperature_k =
        sanitised(environment.equilibrium_temperature_k, MIN_TEMPERATURE_K).max(MIN_TEMPERATURE_K);
    let age = sanitised(environment.age_gyr, 5.0);
    let mut clamped =
        age != environment.age_gyr || !environment.equilibrium_temperature_k.is_finite();
    let shares = condensed_shares(&stack);
    let mut regions = condensed_regions(stack, surface_temperature_k, &mut clamped);
    let mut condensed_radius_m = regions.last().map_or(0.0, |r| r.outer_radius_m);

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
    let mut envelope_inertia = None;
    if let Some(gas) = gas {
        let one_bar = tabulated_one_bar_m(
            mass_kg,
            h_he,
            shares,
            condensed_radius_m,
            surface_temperature_k,
            age,
            &mut clamped,
        )
        .map(|(core, one_bar, factors)| {
            envelope_inertia = Some(factors);
            notes.push(REFERENCE_CORE_NOTE);
            let scale = if condensed_radius_m > 0.0 {
                core / condensed_radius_m
            } else {
                1.0
            };
            for region in &mut regions {
                region.inner_radius_m *= scale;
                region.outer_radius_m *= scale;
            }
            condensed_radius_m = regions.last().map_or(core, |r| r.outer_radius_m);
            one_bar
        });
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
    if regions.iter().any(|r| r.inner_radius_m == r.outer_radius_m) {
        notes
            .push("Sub-resolution condensed shells use zero-thickness pressure and inertia limits");
    }
    for (region, inner_mass) in regions.iter_mut().zip(enclosed.drain(..)).rev() {
        let shell = Shell::new(region, inner_mass);
        region.outer_pressure_pa = pressure;
        pressure += shell.pressure_drop_pa();
        region.inner_pressure_pa = pressure;
        inertia_kg_m2 += shell.inertia_kg_m2();
    }
    // The layered envelope reports its prescribed profile's temperature at
    // one bar, warmer than equilibrium by the intrinsic heat. Same closed form
    // here, on the tabulated gravity. Outside its domain the estimate is kept.
    let surface_temperature_k = if reference == RadiusReference::OneBar {
        one_bar_temperature_k(surface_temperature_k, mass_kg, age, radius_m)
    } else {
        surface_temperature_k
    };
    let surface_pressure_pa = match reference {
        RadiusReference::OneBar => crate::system::atmosphere::ONE_BAR_PA,
        _ => regions
            .iter()
            .find(|r| r.kind == Kind::Envelope)
            .map_or(0.0, |r| r.inner_pressure_pa),
    };

    let mut fallback = Fallback {
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
        envelope_inertia,
        optical_depth: None,
        blowoff: Inventory::default(),
        extrapolated,
        notes,
    };
    if let Some(inertia) = fallback.tabulated_envelope_inertia_kg_m2() {
        fallback.inertia_kg_m2 = inertia;
    }
    fallback
}

/// The envelope's prescribed profile at one bar, over this floored
/// equilibrium temperature. Outside the profile's domain the floor is kept.
pub(crate) fn one_bar_temperature_k(
    equilibrium_k: f64,
    mass_kg: f64,
    age_gyr: f64,
    radius_m: f64,
) -> f64 {
    super::envelope::Thermal::new(
        equilibrium_k,
        super::envelope::intrinsic_k(mass_kg, age_gyr),
        GRAVITATIONAL_CONSTANT * mass_kg / radius_m.powi(2),
    )
    .and_then(|thermal| thermal.temperature_k(crate::system::atmosphere::ONE_BAR_PA))
    .unwrap_or(equilibrium_k)
}

/// One-bar radius of the retained H/He, the condensed radius beneath it and
/// the inertia factors of that core and of the gas, from the layered envelope
/// table. None when the H/He cannot hold one bar
/// above the bare condensed boundary.
///
/// Under an envelope the layered solver compresses water by pressure alone, so
/// a wet core is smaller than the same body bare. The core moves from the
/// bare radius to the enveloped one as the gas at its base goes from one bar
/// to ten, and the gas thickness is limited by the thin-column support
/// radius: both keep the radius continuous down to complete stripping.
fn tabulated_one_bar_m(
    mass_kg: f64,
    h_he_kg: f64,
    [metal_share, water_share]: [f64; 2],
    condensed_radius_m: f64,
    equilibrium_k: f64,
    age_gyr: f64,
    approximate: &mut bool,
) -> Option<(f64, f64, [f64; 2])> {
    // A necessary thin-column supply check, not an estimate of the entire
    // giant envelope.
    let support_radius = (GRAVITATIONAL_CONSTANT * mass_kg * h_he_kg
        / (4.0 * PI * crate::system::atmosphere::ONE_BAR_PA))
        .sqrt()
        .sqrt();
    if support_radius <= condensed_radius_m {
        return None;
    }
    let envelope = Table::embedded().envelope(
        mass_kg,
        h_he_kg / mass_kg,
        metal_share,
        water_share,
        equilibrium_k,
        super::envelope::intrinsic_k(mass_kg, age_gyr),
    );
    *approximate |= envelope.approximate;
    let base_bar = (support_radius / condensed_radius_m).powi(4);
    let enveloped = ((base_bar - 1.0) / 9.0).clamp(0.0, 1.0);
    let core = if condensed_radius_m > 0.0 {
        condensed_radius_m * (envelope.core_radius_m / condensed_radius_m).powf(enveloped)
    } else {
        0.0
    };
    let capacity = support_radius - condensed_radius_m;
    let thickness = envelope.thickness_m;
    Some((
        core,
        core + thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25),
        [envelope.core_inertia_factor, envelope.gas_inertia_factor],
    ))
}

/// Metal share of the refractory mass and water share of the whole stack.
/// Condensed volatiles count as water, as their density proxy does.
fn condensed_shares(stack: &[(Kind, Inventory)]) -> [f64; 2] {
    let mass_of = |kinds: &[Kind]| -> f64 {
        stack
            .iter()
            .filter(|(kind, _)| kinds.contains(kind))
            .map(|(_, inventory)| inventory.total_mass_kg())
            .sum()
    };
    let total: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    let metal = mass_of(&[Kind::Core]);
    let refractory = metal + mass_of(&[Kind::Mantle]);
    [
        if refractory > 0.0 {
            metal / refractory
        } else {
            0.0
        },
        if total > 0.0 {
            1.0 - refractory / total
        } else {
            0.0
        },
    ]
}

/// The layered solver's tabulated radius for the whole stack, at this water
/// temperature. Layer boundaries inside it divide the volume as the table's
/// pure bodies of the same total mass would. Innermost first; pressures are
/// filled in by the caller.
fn condensed_regions(
    stack: Vec<(Kind, Inventory)>,
    temperature_k: f64,
    approximate: &mut bool,
) -> Vec<Region> {
    let table = Table::embedded();
    let condensed_kg: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    if condensed_kg <= 0.0 {
        return Vec::new();
    }
    let [metal_share, water_share] = condensed_shares(&stack);
    let body = table.condensed(condensed_kg, metal_share, water_share, temperature_k);
    *approximate |= body.approximate;
    let pure = |metal_share: f64, water_share: f64| {
        table
            .condensed(condensed_kg, metal_share, water_share, temperature_k)
            .radius_m
            .powi(3)
    };
    let volumes: Vec<f64> = stack
        .iter()
        .map(|(kind, inventory)| {
            inventory.total_mass_kg() / condensed_kg
                * match kind {
                    Kind::Core => pure(1.0, 0.0),
                    Kind::Mantle => pure(0.0, 0.0),
                    _ => pure(0.0, 1.0),
                }
        })
        .collect();
    let total: f64 = volumes.iter().sum();
    let mut regions = Vec::new();
    let mut volume = 0.0;
    for ((kind, inventory), part) in stack.into_iter().zip(volumes) {
        let inner = body.radius_m * f64::cbrt(volume / total);
        volume += part;
        regions.push(Region {
            kind,
            inventory,
            inner_radius_m: inner,
            outer_radius_m: body.radius_m * f64::cbrt(volume / total),
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
        });
    }
    regions
}

/// Layers under a decided surface: metal, then rock holding the protected
/// secondaries, then condensed water. H/He is never part of this stack.
fn decided_stack(host: &Inventory) -> Vec<(Kind, Inventory)> {
    let only = |materials: &[Material]| {
        Inventory::new(
            &materials
                .iter()
                .map(|&m| (m, host.mass_kg(m)))
                .collect::<Vec<_>>(),
        )
        .expect("subset of a valid inventory")
    };
    let mut mantle = vec![Material::Silicate];
    mantle.extend(SECONDARIES);
    let mut stack = vec![
        (Kind::Core, only(&[Material::Metal])),
        (Kind::Mantle, only(&mantle)),
        (Kind::Water, only(&[Material::Water])),
    ];
    stack.retain(|(_, inventory)| inventory.total_mass_kg() > 0.0);
    stack
}

/// Inertia of a decided host as the layered solver finds it, replacing the
/// uniform display layers' own. The table's inertia factor times the host's
/// mass and tabulated radius.
fn tabulated_condensed_inertia_kg_m2(regions: &[Region], temperature_k: f64) -> (f64, f64) {
    let mut enclosed = 0.0;
    let mut uniform = 0.0;
    let mut stack = Vec::new();
    for region in regions {
        if matches!(region.kind, Kind::Core | Kind::Mantle | Kind::Water) {
            uniform += Shell::new(region, enclosed).inertia_kg_m2();
            stack.push((region.kind, region.inventory));
        }
        enclosed += region.inventory.total_mass_kg();
    }
    let mass: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    let [metal_share, water_share] = condensed_shares(&stack);
    let body = Table::embedded().condensed(mass, metal_share, water_share, temperature_k);
    (body.inertia_factor * mass * body.radius_m.powi(2), uniform)
}

/// The radius the synchronous surface decision stands on: the layered
/// solver's, for this host at the decided water temperature.
pub(crate) fn condensed_radius_m(host: &Inventory, temperature_k: f64) -> f64 {
    condensed_regions(decided_stack(host), temperature_k, &mut false)
        .last()
        .map_or(0.0, |r| r.outer_radius_m)
}

/// Non-finite or negative environment values take a neutral table input
/// instead of propagating NaN; the result is then labelled as clamped.
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
    mass_kg: f64,
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
            mass_kg: region.inventory.total_mass_kg(),
            a,
            l,
            rho0: 1.0,
            gas,
        };
        // A positive trace mass can add less than one radius ulp. Its shell
        // limits below stay finite without erasing mass or inflating geometry.
        shell.rho0 = if l == 0.0 {
            0.0
        } else {
            shell.mass_kg / shell.mass_to(1.0)
        };
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
        if self.l == 0.0 {
            return GRAVITATIONAL_CONSTANT
                * (self.inner_mass_kg + 0.5 * self.mass_kg)
                * self.mass_kg
                / (4.0 * PI * self.a.powi(4));
        }
        integrate(|t| {
            let r = self.a + self.l * t;
            GRAVITATIONAL_CONSTANT * (self.inner_mass_kg + self.mass_to(t)) * self.density(t)
                / (r * r)
                * self.l
        })
    }

    fn inertia_kg_m2(&self) -> f64 {
        if self.l == 0.0 {
            return (2.0 / 3.0) * self.mass_kg * self.a.powi(2);
        }
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
