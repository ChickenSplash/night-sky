//! Total structure for every body without a layered solve. Radii, layer
//! boundaries and their pressures are read from the layered solver's own
//! table, and water phases from its water models along the decided isotherm.
//! No shooting and no error path. Never counted as a layered solve. See
//! docs/simulation/planet-composition.md#fallback-structure.
use super::{
    coupled::Error,
    table::{Boundaries, CONDENSED_MASS_EARTH, Quality, Solid, Table},
    water::PhaseKnowledge,
};
use crate::system::atmosphere::ONE_BAR_PA;
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
mod phases;
pub use decide::{Decided, decide};

pub const MODEL_ID: &str = "fallback_structure_table_v1";
pub const NOTES: &[&str] = &[
    "Fallback: radius read from the layered solver's table, not a hydrostatic solve of this body",
    "Condensed layer boundaries and pressures are interpolated from the same table; gas follows a linearly thinning profile",
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
/// H/He mass fractions between which the pressure at the base of the gas
/// passes from the gas's own thin-shell weight to the envelope table's.
const THIN_GAS_FRACTIONS: [f64; 2] = [1e-3, 1e-2];
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
    /// Water of undecided phase: under an envelope, before the surface is
    /// decided, or where the phase walk has no answer.
    Water,
    Ocean,
    Ice,
    DeepIce,
    HotWater,
    Coexistence,
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
            Self::Ocean => "ocean",
            Self::Ice => "ice",
            Self::DeepIce => "deep_ice",
            Self::HotWater => "hot_water",
            Self::Coexistence => "water_coexistence",
            Self::Volatiles => "volatiles",
            Self::Frost => "frost",
            Self::Envelope => "envelope",
            Self::Atmosphere => "atmosphere",
        }
    }

    pub fn is_water(self) -> bool {
        matches!(
            self,
            Self::Water
                | Self::Ocean
                | Self::Ice
                | Self::DeepIce
                | Self::HotWater
                | Self::Coexistence
        )
    }

    /// Same names as the layered record gives the solver's water regions.
    fn of(phase: PhaseKnowledge) -> Self {
        use crate::system::materials::water::Phase;
        match phase {
            PhaseKnowledge::Water(Phase::Liquid) => Self::Ocean,
            PhaseKnowledge::Water(Phase::IceIh) => Self::Ice,
            PhaseKnowledge::Water(_) => Self::Coexistence,
            PhaseKnowledge::EffectiveHighPressureIce => Self::DeepIce,
            PhaseKnowledge::UnresolvedHotWater => Self::HotWater,
            PhaseKnowledge::Unresolved | PhaseKnowledge::UnresolvedCompression => Self::Water,
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
    /// Inner and outer pressure read from the table, above whatever presses
    /// on the outermost such region. None: the prescribed density decides.
    pub tabulated_pa: Option<[f64; 2]>,
    /// Suspended condensate within a gas region's inventory.
    pub condensate: Inventory,
}

impl Region {
    fn new(kind: Kind, inventory: Inventory, inner_radius_m: f64, outer_radius_m: f64) -> Self {
        Self {
            kind,
            inventory,
            inner_radius_m,
            outer_radius_m,
            inner_pressure_pa: 0.0,
            outer_pressure_pa: 0.0,
            tabulated_pa: None,
            condensate: Inventory::default(),
        }
    }
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
    /// What the table lookups stood on: filled knots and why.
    pub quality: Quality,
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
        // With the part above one bar, where that has been split off.
        let gas_kg: f64 = self
            .regions
            .iter()
            .filter(|r| matches!(r.kind, Kind::Envelope | Kind::Atmosphere))
            .map(|r| r.inventory.total_mass_kg())
            .sum();
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
            Some(r) if r.kind == Kind::Ocean => "ocean",
            Some(r) if r.kind == Kind::Ice => "ice",
            Some(r) if r.kind == Kind::DeepIce => "deep ice",
            Some(r) if r.kind == Kind::HotWater => "unresolved hot water",
            Some(r) if r.kind == Kind::Coexistence => "water coexistence",
            Some(r) if r.kind == Kind::Volatiles => "unresolved volatiles",
            Some(r) if r.kind == Kind::Frost => "frost",
            _ => "rock",
        }
    }

    /// Bare rock and bare ice Ih are ground. Liquid, frost and water of
    /// undecided phase are not.
    fn condensed_reference(&self) -> RadiusReference {
        if matches!(self.surface(), "rock" | "ice") {
            RadiusReference::SolidSurface
        } else {
            RadiusReference::UnresolvedVolatileBoundary
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
    let clamped = age != environment.age_gyr || !environment.equilibrium_temperature_k.is_finite();
    let shares = condensed_shares(&stack);
    let mut quality = Quality::default();
    let mut regions = Interior::bare(&stack, surface_temperature_k, &mut quality)
        .map_or_else(Vec::new, |interior| lay(stack.clone(), &interior, None));
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
        let mut base = None;
        let one_bar = tabulated_one_bar_m(
            mass_kg,
            h_he,
            shares,
            condensed_radius_m,
            surface_temperature_k,
            age,
        )
        .map(|enveloped| {
            envelope_inertia = Some(enveloped.inertia_factors);
            quality = quality.with(enveloped.quality);
            notes.push(REFERENCE_CORE_NOTE);
            // The gas presses on the layers under it, from one bar down.
            let over = (enveloped.base_pressure_pa - ONE_BAR_PA).max(0.0);
            base = Some([over, 0.0]);
            regions = lay(stack.clone(), &enveloped.interior(over), None);
            condensed_radius_m = regions
                .last()
                .map_or(enveloped.core_radius_m, |r| r.outer_radius_m);
            enveloped.one_bar_radius_m
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
        let mut shell = Region::new(Kind::Envelope, gas, condensed_radius_m, top);
        shell.tabulated_pa = base;
        regions.push(shell);
    }

    if clamped || quality.approximate() {
        extrapolated = true;
        notes.push(CLAMPED_NOTE);
    }

    if regions.iter().any(|r| r.inner_radius_m == r.outer_radius_m) {
        notes
            .push("Sub-resolution condensed shells use zero-thickness pressure and inertia limits");
    }
    let inertia_kg_m2 = settle(
        &mut regions,
        if reference == RadiusReference::OneBar {
            ONE_BAR_PA
        } else {
            0.0
        },
    );
    // The layered envelope reports its prescribed profile's temperature at
    // one bar, warmer than equilibrium by the intrinsic heat. Same closed form
    // here, on the tabulated gravity. Outside its domain the estimate is kept.
    let surface_temperature_k = if reference == RadiusReference::OneBar {
        one_bar_temperature_k(surface_temperature_k, mass_kg, age, radius_m)
    } else {
        surface_temperature_k
    };
    let surface_pressure_pa = match reference {
        RadiusReference::OneBar => ONE_BAR_PA,
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
        quality,
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

/// A tabulated H/He envelope and the body's own core under it.
struct Enveloped {
    core_radius_m: f64,
    one_bar_radius_m: f64,
    /// Of the core and of the gas.
    inertia_factors: [f64; 2],
    /// Where the gas meets the core.
    base_pressure_pa: f64,
    /// The table's core, whose radius this body's own may differ from while
    /// the gas is thin. Pressures include the base pressure.
    tabulated_core_radius_m: f64,
    boundaries: Boundaries,
    quality: Quality,
}

impl Enveloped {
    /// The core's layers, with pressures above the one-bar level: `over_pa`
    /// of gas, then the core's own weight.
    fn interior(&self, over_pa: f64) -> Interior {
        let scale = if self.tabulated_core_radius_m > 0.0 {
            self.core_radius_m / self.tabulated_core_radius_m
        } else {
            1.0
        };
        let b = &self.boundaries;
        let own = |pressure_pa: f64| over_pa + (pressure_pa - self.base_pressure_pa).max(0.0);
        Interior {
            radius_m: self.core_radius_m,
            core_radius_m: b.core_radius_m * scale,
            refractory_radius_m: b.refractory_radius_m * scale,
            top_pa: over_pa,
            central_pa: own(b.central_pressure_pa),
            core_pa: own(b.core_pressure_pa),
            refractory_pa: own(b.refractory_pressure_pa),
        }
    }
}

/// The retained H/He's one-bar radius and the core beneath it, from the
/// layered envelope table. None when the H/He cannot hold one bar above the
/// bare condensed boundary.
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
) -> Option<Enveloped> {
    // A necessary thin-column supply check, not an estimate of the entire
    // giant envelope.
    let support_radius = (GRAVITATIONAL_CONSTANT * mass_kg * h_he_kg / (4.0 * PI * ONE_BAR_PA))
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
    let base_bar = (support_radius / condensed_radius_m).powi(4);
    let enveloped = ((base_bar - 1.0) / 9.0).clamp(0.0, 1.0);
    let core = if condensed_radius_m > 0.0 {
        condensed_radius_m * (envelope.core_radius_m / condensed_radius_m).powf(enveloped)
    } else {
        0.0
    };
    let capacity = support_radius - condensed_radius_m;
    let thickness = envelope.thickness_m;
    let one_bar = core + thickness / (1.0 + (thickness / capacity).powi(4)).powf(0.25);
    Some(Enveloped {
        core_radius_m: core,
        one_bar_radius_m: one_bar,
        inertia_factors: [envelope.core_inertia_factor, envelope.gas_inertia_factor],
        base_pressure_pa: envelope.base_pressure_pa,
        tabulated_core_radius_m: envelope.core_radius_m,
        boundaries: envelope.core_boundaries,
        quality: envelope.quality,
    })
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

/// Where the table puts a condensed body's layers. Pressures are above
/// whatever presses on the outermost tabulated region.
#[derive(Clone, Copy, Debug)]
struct Interior {
    radius_m: f64,
    /// Top of the metal.
    core_radius_m: f64,
    /// Top of the rock.
    refractory_radius_m: f64,
    /// At the body's own surface: the gas above it, where that is tabulated.
    top_pa: f64,
    central_pa: f64,
    core_pa: f64,
    refractory_pa: f64,
}

impl Interior {
    /// A body under at most thin gas, at this water temperature. None for
    /// an empty stack.
    fn bare(
        stack: &[(Kind, Inventory)],
        temperature_k: f64,
        quality: &mut Quality,
    ) -> Option<Self> {
        let body = bare_body(stack, temperature_k)?;
        *quality = quality.with(body.quality);
        let b = &body.boundaries;
        Some(Self {
            radius_m: body.radius_m,
            core_radius_m: b.core_radius_m,
            refractory_radius_m: b.refractory_radius_m,
            top_pa: 0.0,
            central_pa: b.central_pressure_pa,
            core_pa: b.core_pressure_pa,
            refractory_pa: b.refractory_pressure_pa,
        })
    }
}

fn bare_body(stack: &[(Kind, Inventory)], temperature_k: f64) -> Option<Solid> {
    let condensed_kg: f64 = stack.iter().map(|(_, i)| i.total_mass_kg()).sum();
    if condensed_kg <= 0.0 {
        return None;
    }
    let [metal_share, water_share] = condensed_shares(stack);
    Some(Table::embedded().condensed(condensed_kg, metal_share, water_share, temperature_k))
}

/// The stack laid inside the table's boundaries, innermost first: metal to
/// the top of the metal, rock to the top of the rock, water and condensed
/// volatiles above. `phases` replaces the water with its phase layers and
/// moves the top of the rock to where that walk ends; the table's metal
/// boundary and deeper pressures follow it.
fn lay(
    stack: Vec<(Kind, Inventory)>,
    interior: &Interior,
    phases: Option<phases::Phases>,
) -> Vec<Region> {
    let has = |kind: Kind| stack.iter().any(|(k, _)| *k == kind);
    let wet = stack
        .iter()
        .any(|(k, _)| !matches!(k, Kind::Core | Kind::Mantle));
    let dry = has(Kind::Core) || has(Kind::Mantle);
    let i = interior;
    let (mut rock_radius, mut rock_pa) = match (wet, dry) {
        (false, _) => (i.radius_m, i.top_pa),
        (true, false) => (0.0, i.central_pa),
        (true, true) => (
            i.refractory_radius_m.min(i.radius_m),
            i.refractory_pa.max(i.top_pa),
        ),
    };
    let beneath = |pressure_pa: f64| (pressure_pa - i.refractory_pa).max(0.0);
    let mut core_radius = i.core_radius_m;
    let (mut central_pa, mut core_pa) = (i.central_pa, i.core_pa);
    if let Some(walk) = &phases {
        if i.refractory_radius_m > 0.0 {
            core_radius *= walk.rock_radius_m / i.refractory_radius_m;
        }
        central_pa = walk.rock_pa + beneath(i.central_pa);
        core_pa = walk.rock_pa + beneath(i.core_pa);
        (rock_radius, rock_pa) = (walk.rock_radius_m, walk.rock_pa);
    }
    let (core_radius, core_pa) = if has(Kind::Mantle) {
        (core_radius.min(rock_radius), core_pa.max(rock_pa))
    } else {
        (rock_radius, rock_pa)
    };
    let central_pa = central_pa.max(core_pa);

    let mut regions = Vec::new();
    let fluid_kg: f64 = stack
        .iter()
        .filter(|(k, _)| !matches!(k, Kind::Core | Kind::Mantle))
        .map(|(_, inventory)| inventory.total_mass_kg())
        .sum();
    let mut below_kg = 0.0;
    for (kind, inventory) in stack {
        let mass_kg = inventory.total_mass_kg();
        match kind {
            Kind::Core => {
                let mut region = Region::new(kind, inventory, 0.0, core_radius);
                region.tabulated_pa = Some([central_pa, core_pa]);
                regions.push(region);
            }
            Kind::Mantle => {
                let (inner, pressure) = regions
                    .last()
                    .map_or((0.0, central_pa), |_| (core_radius, core_pa));
                let mut region = Region::new(kind, inventory, inner, rock_radius);
                region.tabulated_pa = Some([pressure, rock_pa]);
                regions.push(region);
            }
            _ if kind.is_water() && phases.is_some() => {
                let mut inner = rock_radius;
                let mut pressure = rock_pa;
                for layer in &phases.as_ref().expect("checked").layers {
                    let water = Inventory::new(&[(Material::Water, layer.mass_kg)])
                        .expect("positive phase layer");
                    let mut region = Region::new(layer.kind, water, inner, layer.outer_radius_m);
                    region.tabulated_pa = Some([pressure, layer.outer_pa]);
                    (inner, pressure) = (layer.outer_radius_m, layer.outer_pa);
                    regions.push(region);
                }
            }
            _ => {
                // Water and condensed volatiles share the shell above the
                // rock by mass, as one density proxy.
                let shell = |kg: f64| {
                    f64::cbrt(
                        rock_radius.powi(3)
                            + kg / fluid_kg * (i.radius_m.powi(3) - rock_radius.powi(3)),
                    )
                };
                let inner = regions.last().map_or(rock_radius, |r| r.outer_radius_m);
                below_kg += mass_kg;
                regions.push(Region::new(kind, inventory, inner, shell(below_kg)));
            }
        }
    }
    // Pressure through the proxy shell follows its uniform density, scaled
    // onto the table's value at the top of the rock.
    let first = regions
        .iter()
        .position(|r| r.tabulated_pa.is_none())
        .unwrap_or(regions.len());
    let mut inner_kg: f64 = regions[..first]
        .iter()
        .map(|r| r.inventory.total_mass_kg())
        .sum();
    let drops: Vec<f64> = regions[first..]
        .iter()
        .map(|region| {
            let drop = Shell::new(region, inner_kg).pressure_drop_pa();
            inner_kg += region.inventory.total_mass_kg();
            drop
        })
        .collect();
    let total: f64 = drops.iter().sum();
    let mut pressure = rock_pa;
    for (region, drop) in regions[first..].iter_mut().zip(drops) {
        let outer = if total > 0.0 {
            (pressure - (rock_pa - i.top_pa) * drop / total).max(i.top_pa)
        } else {
            i.top_pa
        };
        region.tabulated_pa = Some([pressure, outer]);
        pressure = outer;
    }
    // The body's surface is the table's radius and carries its load, exactly.
    if let Some(top) = regions.last_mut() {
        top.outer_radius_m = i.radius_m;
        if let Some(tabulated) = &mut top.tabulated_pa {
            tabulated[1] = i.top_pa;
        }
    }
    regions
}

/// The pressure under a one-bar envelope once its gas is decided. The table
/// knows the H/He alone, so thin gas takes its own weight in its place:
/// what one bar does not hold up, spread over the shell between the body and
/// that level. The table takes over as the gas thickens and its self-gravity
/// and compression matter. Everything beneath moves with it.
fn weigh_thin_gas(regions: &mut [Region], mass_kg: f64) {
    let Some(index) = regions
        .iter()
        .position(|r| r.kind == Kind::Envelope && r.tabulated_pa.is_some())
    else {
        return;
    };
    let shell = &regions[index];
    let gas_kg: f64 = regions[index..]
        .iter()
        .map(|r| r.inventory.total_mass_kg())
        .sum();
    let Some([over, _]) = shell.tabulated_pa else {
        return;
    };
    let (inner, outer) = (shell.inner_radius_m, shell.outer_radius_m);
    let weight = GRAVITATIONAL_CONSTANT * (mass_kg - gas_kg) * shell.inventory.total_mass_kg()
        / (4.0 * PI * inner.powi(3) * outer);
    let [thin, thick] = THIN_GAS_FRACTIONS.map(f64::ln);
    let t = (((gas_kg / mass_kg).ln() - thin) / (thick - thin)).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let blended =
        ((ONE_BAR_PA + weight).ln() * (1.0 - t) + (ONE_BAR_PA + over).ln() * t).exp() - ONE_BAR_PA;
    if !(blended.is_finite() && blended >= 0.0) {
        return;
    }
    let change = blended - over;
    for region in &mut regions[..=index] {
        if let Some(tabulated) = &mut region.tabulated_pa {
            tabulated[0] += change;
            if region.kind != Kind::Envelope {
                tabulated[1] += change;
            }
        }
    }
}

/// Pressures from the outside in under `pressure_pa`, and the regions' own
/// moment of inertia. Tabulated regions take the table's pressures above
/// the load on the outermost of them; the rest follow their prescribed
/// density.
fn settle(regions: &mut [Region], mut pressure_pa: f64) -> f64 {
    let mut enclosed: Vec<f64> = regions
        .iter()
        .scan(0.0, |m, r| {
            let inner = *m;
            *m += r.inventory.total_mass_kg();
            Some(inner)
        })
        .collect();
    let mut inertia_kg_m2 = 0.0;
    let mut load = None;
    for (region, inner_mass) in regions.iter_mut().zip(enclosed.drain(..)).rev() {
        let shell = Shell::new(region, inner_mass);
        if let Some([inner, outer]) = region.tabulated_pa {
            let load = *load.get_or_insert(pressure_pa);
            region.outer_pressure_pa = load + outer;
            region.inner_pressure_pa = load + inner;
        } else {
            region.outer_pressure_pa = pressure_pa;
            region.inner_pressure_pa = pressure_pa + shell.pressure_drop_pa();
        }
        pressure_pa = region.inner_pressure_pa;
        inertia_kg_m2 += shell.inertia_kg_m2();
    }
    inertia_kg_m2
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
        if matches!(region.kind, Kind::Core | Kind::Mantle) || region.kind.is_water() {
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
    bare_body(&decided_stack(host), temperature_k).map_or(0.0, |body| body.radius_m)
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
