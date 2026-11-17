//! Bulk radii the synchronous stage reads from the layered solver's own
//! answers, instead of from independent published fits. Built offline by
//! `scripts/prepare-structure-table.sh` and embedded, so generation is the
//! same with or without the local H/He resource.
//! See docs/simulation/planet-composition.md#structure-table.
use crate::units::EARTH_MASS_KG;
use std::sync::OnceLock;

#[cfg(test)]
mod build;
#[cfg(test)]
mod tests;

pub const MODEL_ID: &str = "layered_structure_table_v4";

static BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/structure-table.bin"
));
const MAGIC: &[u8; 8] = b"NSSTR003";

/// Condensed bodies: the dry and water solvers' mass domain.
pub(crate) const CONDENSED_MASS_EARTH: [f64; 2] = [1e-4, 20.0];
/// Above the domain the radius continues as a labelled power law.
const CONTINUATION_EXPONENT: f64 = 0.2;
/// Water is tabulated up to the liquid model's ceiling. Hotter condensed
/// water takes this bound and is labelled.
pub(crate) const MAX_WATER_TEMPERATURE_K: f64 = 623.15;
/// Cores under an envelope reach the envelope solver's whole mass domain.
pub(crate) const CORE_MASS_EARTH: [f64; 2] = [1e-4, 1200.0];
/// Load on a core's surface from the gas above it. The first knot is the
/// envelope solver's own outer boundary: a core under no more than a trace.
pub(crate) const CORE_LOADS_PA: [f64; 15] = [
    1e3, 1e7, 1e8, 3e8, 1e9, 3e9, 1e10, 3e10, 1e11, 3e11, 1e12, 3e12, 1e13, 1e14, 1e15,
];
/// A core of another radius than the reference one carries the same gas at
/// another base pressure: as radius to the minus fourth under thin gas (its
/// weight over the core's area), falling to the minus first as the gas
/// becomes the planet. Fitted to the solver, like the next two.
const BASE_PRESSURE_EXPONENTS: [f64; 2] = [4.0, 1.0];
/// Thin gas over a larger core of the same mass is also thicker, for its
/// lower gravity: as this power of the core radius, below the first H/He
/// share. Above the second the thickness is the reference core's.
const THIN_THICKNESS_EXPONENT: f64 = 0.86;
const THIN_FRACTIONS: [f64; 2] = [0.002, 0.05];
/// Envelopes: the envelope solver's total-mass domain.
pub(crate) const ENVELOPE_MASS_EARTH: [f64; 2] = [0.05, 1200.0];
/// Metal share of the reference core every envelope cell is solved over.
pub(crate) const REFERENCE_CORE_METAL: f64 = 0.325;
/// Helium share of the H/He, as generation draws it.
#[cfg(test)]
pub(crate) const REFERENCE_HELIUM: f64 = 0.25;

const WATER_SHARES: [f64; 19] = [
    0.0, 0.001, 0.003, 0.01, 0.02, 0.035, 0.05, 0.075, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7,
    0.8, 0.9, 1.0,
];
/// Dense through the pressure-melting range; the ice to liquid step sits
/// between 272 and 274 K.
const WATER_TEMPERATURES_K: [f64; 21] = [
    20.0,
    150.0,
    250.0,
    253.0,
    256.0,
    259.0,
    262.0,
    265.0,
    268.0,
    270.0,
    272.0,
    274.0,
    285.0,
    300.0,
    350.0,
    400.0,
    450.0,
    500.0,
    550.0,
    600.0,
    MAX_WATER_TEMPERATURE_K,
];
/// Dense towards pure gas: there the core shrinks with the heavy share, not
/// with the logarithm of the gas share the axis is stored in.
const LIGHT_FRACTIONS_HIGH: [f64; 15] = [
    0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.93, 0.95, 0.965, 0.975, 0.983, 0.99,
];

/// Knot coordinates, innermost axis last. Mass and light fraction are stored
/// as natural logarithms, shares and temperatures as they are.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Grid {
    pub knots: [Vec<f64>; 4],
}

fn uniform(min: f64, max: f64, count: usize) -> Vec<f64> {
    (0..count)
        .map(|i| min + (max - min) * i as f64 / (count - 1) as f64)
        .collect()
}

/// Evenly spaced in the logarithm, with exact ends.
fn geometric(min: f64, max: f64, count: usize) -> Vec<f64> {
    let mut knots: Vec<f64> = uniform(min.ln(), max.ln(), count)
        .into_iter()
        .map(f64::exp)
        .collect();
    (knots[0], knots[count - 1]) = (min, max);
    knots
}

impl Grid {
    /// Mass, metal share of the refractory, water share, water temperature.
    pub fn condensed() -> Self {
        Self {
            knots: [
                uniform(
                    CONDENSED_MASS_EARTH[0].ln(),
                    CONDENSED_MASS_EARTH[1].ln(),
                    41,
                ),
                uniform(0.0, 1.0, 9),
                WATER_SHARES.to_vec(),
                WATER_TEMPERATURES_K.to_vec(),
            ],
        }
    }

    /// Condensed mass, metal share of the refractory and water share of a
    /// core under an envelope, and the logarithm of the load on its surface.
    pub fn core() -> Self {
        let [_, metal, water, _] = Self::condensed().knots;
        Self {
            knots: [
                uniform(CORE_MASS_EARTH[0].ln(), CORE_MASS_EARTH[1].ln(), 55),
                metal,
                water,
                CORE_LOADS_PA.map(f64::ln).to_vec(),
            ],
        }
    }

    /// Total mass, H/He share, equilibrium and intrinsic temperature.
    pub fn envelope() -> Self {
        let mut fractions: Vec<f64> = (0..25)
            .map(|k| (10_f64.powf(-7.0 + 0.25 * k as f64)).ln())
            .collect();
        fractions.extend(LIGHT_FRACTIONS_HIGH.map(f64::ln));
        let [low, high] = super::envelope::INTRINSIC_RANGE_K;
        Self {
            knots: [
                uniform(ENVELOPE_MASS_EARTH[0].ln(), ENVELOPE_MASS_EARTH[1].ln(), 48),
                fractions,
                geometric(20.0, 2000.0, 17),
                geometric(low, high, 10),
            ],
        }
    }

    pub fn len(&self) -> usize {
        self.knots.iter().map(Vec::len).product()
    }

    pub fn index(&self, cell: [usize; 4]) -> usize {
        cell.iter()
            .zip(&self.knots)
            .fold(0, |index, (i, knots)| index * knots.len() + i)
    }

    #[cfg(test)]
    pub fn cell(&self, mut index: usize) -> [usize; 4] {
        let mut cell = [0; 4];
        for axis in (0..4).rev() {
            cell[axis] = index % self.knots[axis].len();
            index /= self.knots[axis].len();
        }
        cell
    }

    #[cfg(test)]
    pub fn point(&self, cell: [usize; 4]) -> [f64; 4] {
        std::array::from_fn(|axis| self.knots[axis][cell[axis]])
    }

    /// Multilinear weights of the sixteen surrounding knots. Coordinates
    /// outside the grid take its edge.
    fn corners(&self, point: [f64; 4]) -> [(usize, f64); 16] {
        let located: [(usize, f64); 4] = std::array::from_fn(|axis| {
            let knots = &self.knots[axis];
            if knots.len() == 1 {
                return (0, 0.0);
            }
            let x = point[axis].clamp(knots[0], knots[knots.len() - 1]);
            let low = knots.partition_point(|k| *k <= x).clamp(1, knots.len() - 1) - 1;
            (low, (x - knots[low]) / (knots[low + 1] - knots[low]))
        });
        std::array::from_fn(|corner| {
            let mut cell = [0; 4];
            let mut weight = 1.0;
            for axis in 0..4 {
                let high = corner >> axis & 1;
                cell[axis] = (located[axis].0 + high).min(self.knots[axis].len() - 1);
                weight *= if high == 1 {
                    located[axis].1
                } else {
                    1.0 - located[axis].1
                };
            }
            (self.index(cell), weight)
        })
    }

    fn bytes(&self) -> Vec<u8> {
        self.knots
            .iter()
            .flat_map(|knots| {
                (knots.len() as u64)
                    .to_le_bytes()
                    .into_iter()
                    .chain(knots.iter().flat_map(|k| k.to_le_bytes()))
            })
            .collect()
    }
}

fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

/// Changes whenever a grid does, so a stale resource cannot be read.
fn grid_hash() -> u64 {
    fnv(Grid::condensed()
        .bytes()
        .into_iter()
        .chain(Grid::envelope().bytes())
        .chain(Grid::core().bytes()))
}

/// Why a cell is not the solver's own answer. Stored per cell, so a lookup
/// can say which weakness of the model its corners crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Reason {
    Solved,
    /// No central pressure brackets the surface condition.
    Unbracketed,
    NonConvergence,
    /// The integration budget ran out.
    WorkBudget,
    /// Gas hotter or colder than the envelope's thermal model.
    ThermalDomain,
    /// Cold dense gas outside the H/He density resource.
    ColdDenseGas,
    /// Ionised or quantum-screened gas outside the H/He density resource.
    IonisedGas,
    MassDomain,
    /// A layer or boundary too thin to resolve.
    Resolution,
    DensityInversion,
    Other,
}

impl Reason {
    pub const ALL: [Self; 11] = [
        Self::Solved,
        Self::Unbracketed,
        Self::NonConvergence,
        Self::WorkBudget,
        Self::ThermalDomain,
        Self::ColdDenseGas,
        Self::IonisedGas,
        Self::MassDomain,
        Self::Resolution,
        Self::DensityInversion,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solved => "solved",
            Self::Unbracketed => "no central pressure found",
            Self::NonConvergence => "solver did not converge",
            Self::WorkBudget => "integration budget exhausted",
            Self::ThermalDomain => "outside the envelope thermal model",
            Self::ColdDenseGas => "cold dense gas outside the H/He data",
            Self::IonisedGas => "ionised gas outside the H/He data",
            Self::MassDomain => "outside the solver's mass domain",
            Self::Resolution => "layer too thin to resolve",
            Self::DensityInversion => "density inversion",
            Self::Other => "other refusal",
        }
    }

    /// The solver's refusal, as the build reports it.
    #[cfg(test)]
    pub(crate) fn of(refusal: &str) -> Self {
        let has = |text: &str| refusal.contains(text);
        if has("Unbracketed") {
            Self::Unbracketed
        } else if has("WorkBudget") {
            Self::WorkBudget
        } else if has("NonConvergence") {
            Self::NonConvergence
        } else if has("thermal") {
            Self::ThermalDomain
        } else if has("ColdDense") {
            Self::ColdDenseGas
        } else if has("IonQuantum") {
            Self::IonisedGas
        } else if has("mass domain") {
            Self::MassDomain
        } else if has("Resolution") || has("resolution") {
            Self::Resolution
        } else if has("DensityInversion") {
            Self::DensityInversion
        } else {
            Self::Other
        }
    }
}

/// What a lookup stood on: how many of the knots it read were filled, why,
/// and whether it left the grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quality {
    pub corners: u8,
    pub filled: u8,
    /// One bit per `Reason`, of the filled corners.
    reasons: u16,
    /// Past an axis: its edge, or a continuation law, stands in.
    pub outside: bool,
}

impl Quality {
    fn read(&mut self, reason: Reason) {
        self.corners += 1;
        if reason != Reason::Solved {
            self.filled += 1;
            self.reasons |= 1 << reason as u8;
        }
    }

    pub fn approximate(&self) -> bool {
        self.outside || self.filled > 0
    }

    pub fn reasons(&self) -> impl Iterator<Item = Reason> + '_ {
        Reason::ALL
            .into_iter()
            .filter(|r| self.reasons >> *r as u8 & 1 == 1)
    }

    pub(crate) fn with(self, other: Self) -> Self {
        Self {
            corners: self.corners.saturating_add(other.corners),
            filled: self.filled.saturating_add(other.filled),
            reasons: self.reasons | other.reasons,
            outside: self.outside || other.outside,
        }
    }
}

/// Where a solid body's layers meet. Volumes are shares of the body's own,
/// which follow the mass shares almost linearly where a radius would not.
/// Pressures are the body's own weight, above whatever load its surface
/// carries, as shares of the central one. Stored in sixteen bits each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Interior {
    /// Inside the top of the metal.
    pub core_volume: f32,
    /// Inside the top of the rock: all but the water.
    pub refractory_volume: f32,
    pub ln_central_pa: f32,
    /// At the top of the metal.
    pub core_pressure: f32,
    /// At the top of the rock.
    pub refractory_pressure: f32,
}

#[cfg(test)]
fn pack(share: f32) -> u16 {
    (share.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn unpack(stored: u16) -> f32 {
    f32::from(stored) / 65535.0
}

impl Interior {
    /// Pressures keep their square root: a thin water layer's base is a small
    /// share of the centre's.
    #[cfg(test)]
    fn bytes(&self) -> [u8; 12] {
        let mut out = [0; 12];
        out[..4].copy_from_slice(&self.ln_central_pa.to_le_bytes());
        for (i, stored) in [
            pack(self.core_volume),
            pack(self.refractory_volume),
            pack(self.core_pressure.sqrt()),
            pack(self.refractory_pressure.sqrt()),
        ]
        .into_iter()
        .enumerate()
        {
            out[4 + 2 * i..6 + 2 * i].copy_from_slice(&stored.to_le_bytes());
        }
        out
    }

    fn from_bytes(data: &[u8]) -> Self {
        let share = |i: usize| unpack(u16::from_le_bytes([data[4 + 2 * i], data[5 + 2 * i]]));
        Self {
            core_volume: share(0),
            refractory_volume: share(1),
            ln_central_pa: f32::from_le_bytes(data[..4].try_into().unwrap()),
            core_pressure: share(2).powi(2),
            refractory_pressure: share(3).powi(2),
        }
    }

    /// As the resource will hold it.
    #[cfg(test)]
    pub(crate) fn stored(&self) -> Self {
        Self::from_bytes(&self.bytes())
    }
}

/// One solid body: the solved one, or a labelled fill from its solved
/// neighbours where the solver refuses. Condensed cells are whole bodies
/// under at most thin gas; core cells sit under an envelope's load, where
/// water takes the envelope solver's pressure-only compression instead of
/// the thin route's phases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SolidCell {
    pub ln_radius_m: f32,
    pub inertia_factor: f32,
    pub interior: Interior,
    pub reason: Reason,
}

/// One envelope cell: the radius of the reference core over the one-bar
/// radius. The inverse radius is the smooth quantity: a hydrostatic column's
/// is linear in temperature and in the logarithm of its gas mass, where the
/// radius itself diverges. Refused cells hold the anchored surrogate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EnvelopeCell {
    pub compactness: f32,
    /// Logarithm of the pressure where the gas meets the core.
    pub ln_base_pa: f32,
    /// The gas's moment of inertia over its mass and the reference radius
    /// squared.
    pub gas_inertia_factor: f32,
    pub reason: Reason,
}

const SOLID_BYTES: usize = 20;
const ENVELOPE_BYTES: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    condensed_grid: Grid,
    envelope_grid: Grid,
    core_grid: Grid,
    pub condensed: Vec<SolidCell>,
    pub envelope: Vec<EnvelopeCell>,
    pub core: Vec<SolidCell>,
}

/// Layer boundaries of a solid body, from its centre outwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boundaries {
    /// Top of the metal. Zero without any.
    pub core_radius_m: f64,
    /// Top of the rock: the body's radius without water.
    pub refractory_radius_m: f64,
    /// Pressures include the load a core lookup was given. A condensed
    /// lookup's are above whatever presses on its surface.
    pub central_pressure_pa: f64,
    pub core_pressure_pa: f64,
    pub refractory_pressure_pa: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub radius_m: f64,
    pub inertia_factor: f64,
    pub boundaries: Boundaries,
    /// Outside the tabulated domain, or read from a filled cell.
    pub approximate: bool,
    pub quality: Quality,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// The core under the gas, compressed by its weight.
    pub core_radius_m: f64,
    /// That core's moment of inertia over its mass and radius squared.
    pub core_inertia_factor: f64,
    /// Gas between that core and the one-bar level.
    pub thickness_m: f64,
    /// The gas's moment of inertia over its mass and the one-bar radius
    /// squared.
    pub gas_inertia_factor: f64,
    /// Pressure where the gas meets the core.
    pub base_pressure_pa: f64,
    /// The body's own core under that pressure.
    pub core_boundaries: Boundaries,
    pub approximate: bool,
    pub quality: Quality,
}

impl Table {
    #[cfg(test)]
    pub(crate) fn new(
        condensed: Vec<SolidCell>,
        envelope: Vec<EnvelopeCell>,
        core: Vec<SolidCell>,
    ) -> Self {
        let table = Self {
            condensed_grid: Grid::condensed(),
            envelope_grid: Grid::envelope(),
            core_grid: Grid::core(),
            condensed,
            envelope,
            core,
        };
        assert_eq!(table.condensed.len(), table.condensed_grid.len());
        assert_eq!(table.envelope.len(), table.envelope_grid.len());
        assert_eq!(table.core.len(), table.core_grid.len());
        table
    }

    /// The embedded resource. A missing or stale file is a build error to fix
    /// with the preparation script, never a silent change of model.
    pub fn embedded() -> &'static Self {
        static TABLE: OnceLock<Table> = OnceLock::new();
        TABLE.get_or_init(|| {
            Self::decode(BYTES)
                .expect("structure table missing or stale: run scripts/prepare-structure-table.sh")
        })
    }

    #[cfg(test)]
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend(grid_hash().to_le_bytes());
        let solid = |out: &mut Vec<u8>, cells: &[SolidCell]| {
            for cell in cells {
                out.extend(cell.ln_radius_m.to_le_bytes());
                out.extend(cell.inertia_factor.to_le_bytes());
                out.extend(cell.interior.bytes());
            }
        };
        solid(&mut out, &self.condensed);
        for cell in &self.envelope {
            out.extend(cell.compactness.to_le_bytes());
            out.extend(cell.ln_base_pa.to_le_bytes());
            out.extend(cell.gas_inertia_factor.to_le_bytes());
        }
        solid(&mut out, &self.core);
        out.extend(
            self.condensed
                .iter()
                .map(|c| c.reason)
                .chain(self.envelope.iter().map(|c| c.reason))
                .chain(self.core.iter().map(|c| c.reason))
                .map(|reason| reason as u8),
        );
        out.extend(fnv(out.iter().copied()).to_le_bytes());
        out
    }

    pub(crate) fn decode(data: &[u8]) -> Option<Self> {
        let (condensed_grid, envelope_grid, core_grid) =
            (Grid::condensed(), Grid::envelope(), Grid::core());
        let (c, e, k) = (condensed_grid.len(), envelope_grid.len(), core_grid.len());
        let envelopes = 16 + SOLID_BYTES * c;
        let cores = envelopes + ENVELOPE_BYTES * e;
        let reasons = cores + SOLID_BYTES * k;
        if data.len() != reasons + c + e + k + 8
            || &data[..8] != MAGIC
            || data[8..16] != grid_hash().to_le_bytes()
            || data[data.len() - 8..] != fnv(data[..data.len() - 8].iter().copied()).to_le_bytes()
        {
            return None;
        }
        let number = |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let reason = |index: usize| Reason::ALL.get(usize::from(data[reasons + index])).copied();
        let solid = |start: usize, first: usize, count: usize| -> Option<Vec<SolidCell>> {
            (0..count)
                .map(|i| {
                    let at = start + SOLID_BYTES * i;
                    Some(SolidCell {
                        ln_radius_m: number(at),
                        inertia_factor: number(at + 4),
                        interior: Interior::from_bytes(&data[at + 8..at + SOLID_BYTES]),
                        reason: reason(first + i)?,
                    })
                })
                .collect()
        };
        let condensed = solid(16, 0, c)?;
        let envelope: Vec<_> = (0..e)
            .map(|i| {
                let at = envelopes + ENVELOPE_BYTES * i;
                Some(EnvelopeCell {
                    compactness: number(at),
                    ln_base_pa: number(at + 4),
                    gas_inertia_factor: number(at + 8),
                    reason: reason(c + i)?,
                })
            })
            .collect::<Option<_>>()?;
        let core = solid(cores, c + e, k)?;
        let sound = |cell: &SolidCell| {
            cell.ln_radius_m.is_finite()
                && cell.inertia_factor > 0.0
                && cell.interior.ln_central_pa.is_finite()
                && cell.interior.core_volume <= cell.interior.refractory_volume
                && cell.interior.refractory_pressure <= cell.interior.core_pressure
        };
        (condensed.iter().all(sound)
            && envelope.iter().all(|c| {
                c.compactness.is_finite()
                    && c.compactness > 0.0
                    && c.ln_base_pa.is_finite()
                    && c.gas_inertia_factor > 0.0
            })
            && core.iter().all(sound))
        .then_some(Self {
            condensed_grid,
            envelope_grid,
            core_grid,
            condensed,
            envelope,
            core,
        })
    }

    /// One solid body from its grid. Past the mass bounds the layers keep
    /// their shares of the volume and the pressure above `load_pa` follows
    /// the body's own weight: mass squared over radius to the fourth.
    fn solid(
        grid: &Grid,
        cells: &[SolidCell],
        point: [f64; 4],
        mass_beyond: f64,
        load_pa: f64,
        outside: bool,
    ) -> Solid {
        let mut quality = Quality {
            outside,
            ..Quality::default()
        };
        let mut read = |point: [f64; 4], count: bool| {
            let mut sum = [0.0; 7];
            for (index, weight) in grid.corners(point) {
                if weight == 0.0 {
                    continue;
                }
                let cell = &cells[index];
                let interior = &cell.interior;
                for (total, value) in sum.iter_mut().zip([
                    cell.ln_radius_m,
                    cell.inertia_factor,
                    interior.core_volume,
                    interior.refractory_volume,
                    interior.ln_central_pa,
                    interior.core_pressure,
                    interior.refractory_pressure,
                ]) {
                    *total += weight * f64::from(value);
                }
                if count {
                    quality.read(cell.reason);
                }
            }
            sum
        };
        let sum = read(point, true);
        // Pressure across a sphere goes as its radius squared, so where the
        // innermost layer vanishes the pressure beneath its top bends as its
        // share to the two thirds: beneath the rock towards all water, and
        // at the centre towards no metal too. The pressure at a vanishing
        // layer's own top does not.
        let bend = |x: f64, from: f64, to: f64| {
            let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
            from + (to - from) * t.powf(2.0 / 3.0)
        };
        let [metal, water] = [&grid.knots[1], &grid.knots[2]];
        let [all_water, wet] = [water[water.len() - 1], water[water.len() - 2]];
        let mut bent = point;
        if point[2] > wet {
            bent[2] = bend(point[2], all_water, wet);
        }
        let beneath_rock = if bent == point {
            sum
        } else {
            read(bent, false)
        };
        if point[1] < metal[1] {
            bent[1] = bend(point[1], metal[0], metal[1]);
        }
        let centre = if bent[1] == point[1] {
            beneath_rock
        } else {
            read(bent, false)
        };
        // Below the domain bodies are uncompressed; above it the radius
        // continues as the fallback always has.
        let exponent = if mass_beyond < 1.0 {
            1.0 / 3.0
        } else {
            CONTINUATION_EXPONENT
        };
        let radius_m = sum[0].exp() * mass_beyond.powf(exponent);
        let weight = mass_beyond.powf(2.0 - 4.0 * exponent);
        let pressure = |from: &[f64; 7], share: f64| load_pa + share * from[4].exp() * weight;
        Solid {
            radius_m,
            inertia_factor: sum[1],
            boundaries: Boundaries {
                core_radius_m: radius_m * sum[2].cbrt(),
                refractory_radius_m: radius_m * sum[3].cbrt(),
                central_pressure_pa: pressure(&centre, 1.0),
                core_pressure_pa: pressure(&beneath_rock, beneath_rock[5]),
                refractory_pressure_pa: pressure(&sum, sum[6]),
            },
            approximate: quality.approximate(),
            quality,
        }
    }

    /// Radius and inertia factor of a bare condensed body. `metal_share` is of
    /// the refractory mass, `water_share` of the whole, and the temperature is
    /// the water's. Total for every positive mass.
    pub fn condensed(
        &self,
        mass_kg: f64,
        metal_share: f64,
        water_share: f64,
        temperature_k: f64,
    ) -> Solid {
        let mass = mass_kg / EARTH_MASS_KG;
        let [low, high] = CONDENSED_MASS_EARTH;
        let bounded = mass.clamp(low, high);
        let water = water_share.clamp(0.0, 1.0);
        Self::solid(
            &self.condensed_grid,
            &self.condensed,
            [
                bounded.ln(),
                metal_share.clamp(0.0, 1.0),
                water,
                temperature_k,
            ],
            mass / bounded,
            0.0,
            bounded != mass
                || (water > 0.0
                    && !(WATER_TEMPERATURES_K[0]..=MAX_WATER_TEMPERATURE_K)
                        .contains(&temperature_k)),
        )
    }

    /// A condensed body under an H/He envelope whose gas presses on it with
    /// `load_pa`. Same mass continuations as `condensed`, past this grid's
    /// own bounds.
    pub fn core(&self, mass_kg: f64, metal_share: f64, water_share: f64, load_pa: f64) -> Solid {
        if mass_kg <= 0.0 {
            return Solid {
                radius_m: 0.0,
                inertia_factor: 0.4,
                boundaries: Boundaries {
                    core_radius_m: 0.0,
                    refractory_radius_m: 0.0,
                    central_pressure_pa: load_pa,
                    core_pressure_pa: load_pa,
                    refractory_pressure_pa: load_pa,
                },
                approximate: false,
                quality: Quality::default(),
            };
        }
        let mass = mass_kg / EARTH_MASS_KG;
        let [low, high] = CORE_MASS_EARTH;
        let bounded = mass.clamp(low, high);
        Self::solid(
            &self.core_grid,
            &self.core,
            [
                bounded.ln(),
                metal_share.clamp(0.0, 1.0),
                water_share.clamp(0.0, 1.0),
                load_pa.max(CORE_LOADS_PA[0]).ln(),
            ],
            mass / bounded,
            load_pa,
            bounded != mass || load_pa > CORE_LOADS_PA[CORE_LOADS_PA.len() - 1],
        )
    }

    /// An H/He envelope's core and the gas above it up to one bar. The cell
    /// is the layered solve over the reference core; this body's own core
    /// replaces that one under the same load, so a compressible core shrinks
    /// under deep gas as the solver has it.
    pub fn envelope(
        &self,
        mass_kg: f64,
        light_fraction: f64,
        metal_share: f64,
        water_share: f64,
        equilibrium_k: f64,
        intrinsic_k: f64,
    ) -> Envelope {
        let point = [
            (mass_kg / EARTH_MASS_KG).ln(),
            light_fraction.ln(),
            equilibrium_k,
            intrinsic_k,
        ];
        let mut compactness = 0.0;
        let mut ln_base = 0.0;
        let mut gas_inertia_factor = 0.0;
        let mut quality = Quality {
            outside: point
                .iter()
                .zip(&self.envelope_grid.knots)
                .any(|(x, knots)| !(knots[0]..=knots[knots.len() - 1]).contains(x)),
            ..Quality::default()
        };
        for (index, weight) in self.envelope_grid.corners(point) {
            if weight == 0.0 {
                continue;
            }
            let cell = &self.envelope[index];
            compactness += weight * f64::from(cell.compactness);
            ln_base += weight * f64::from(cell.ln_base_pa);
            gas_inertia_factor += weight * f64::from(cell.gas_inertia_factor);
            quality.read(cell.reason);
        }
        // The reference core is the one the cell was solved over, so it takes
        // the table's bounds with the cell. The body's own core does not.
        let [mass, fraction] = [0, 1].map(|axis| {
            let knots = &self.envelope_grid.knots[axis];
            point[axis].clamp(knots[0], knots[knots.len() - 1]).exp()
        });
        let reference_kg = mass * (1.0 - fraction) * EARTH_MASS_KG;
        let load_pa = ln_base.exp();
        // Compactness is stored against the unloaded reference core.
        let one_bar = self
            .core(reference_kg, REFERENCE_CORE_METAL, 0.0, 0.0)
            .radius_m
            / compactness;
        let reference = self.core(reference_kg, REFERENCE_CORE_METAL, 0.0, load_pa);
        let own = |load_pa: f64| {
            self.core(
                mass_kg * (1.0 - light_fraction),
                metal_share,
                water_share,
                load_pa,
            )
        };
        let gas = light_fraction.clamp(0.0, 1.0);
        let [thin, deep] = BASE_PRESSURE_EXPONENTS;
        let exponent = thin - (thin - deep) * gas.powf(1.5);
        let mut base_pressure_pa = load_pa;
        let mut core = own(load_pa);
        let mut wider = 1.0;
        let thickness = (one_bar - reference.radius_m).max(0.0);
        if reference.radius_m > 0.0 && core.radius_m > 0.0 {
            // The load depends on the radius it compresses: a few rounds
            // settle it far below the table's precision.
            for _ in 0..4 {
                base_pressure_pa = load_pa * (reference.radius_m / core.radius_m).powf(exponent);
                core = own(base_pressure_pa);
            }
            let [low, high] = THIN_FRACTIONS.map(f64::ln);
            let t = ((gas.max(f64::MIN_POSITIVE).ln() - low) / (high - low)).clamp(0.0, 1.0);
            let fade = 1.0 - t * t * (3.0 - 2.0 * t);
            wider = (core.radius_m / reference.radius_m).powf(THIN_THICKNESS_EXPONENT * fade);
        }
        Envelope {
            core_radius_m: core.radius_m,
            core_inertia_factor: core.inertia_factor,
            thickness_m: thickness * wider,
            gas_inertia_factor,
            base_pressure_pa,
            core_boundaries: core.boundaries,
            approximate: quality.approximate() || core.approximate,
            quality: quality.with(core.quality),
        }
    }
}
