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

pub const MODEL_ID: &str = "layered_structure_table_v1";

static BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/structure-table.bin"
));
const MAGIC: &[u8; 8] = b"NSSTR001";

/// Condensed bodies: the dry and water solvers' mass domain.
pub(crate) const CONDENSED_MASS_EARTH: [f64; 2] = [1e-4, 20.0];
/// Above the domain the radius continues as a labelled power law.
const CONTINUATION_EXPONENT: f64 = 0.2;
/// Water is tabulated up to the liquid model's ceiling. Hotter condensed
/// water takes this bound and is labelled.
pub(crate) const MAX_WATER_TEMPERATURE_K: f64 = 623.15;
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
const LIGHT_FRACTIONS_HIGH: [f64; 11] = [0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.99];

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
    /// core under an envelope. The last axis is unused.
    pub fn core() -> Self {
        let [mass, metal, water, _] = Self::condensed().knots;
        Self {
            knots: [mass, metal, water, vec![0.0]],
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

/// One condensed cell: the solved body, or a labelled fill from its solved
/// neighbours where the solver refuses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CondensedCell {
    pub ln_radius_m: f32,
    pub inertia_factor: f32,
    pub filled: bool,
}

/// One core under an envelope, where water takes the envelope solver's
/// pressure-only compression instead of the thin route's phases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CoreCell {
    pub ln_radius_m: f32,
    pub filled: bool,
}

/// One envelope cell: the radius of the reference core over the one-bar
/// radius. The inverse radius is the smooth quantity: a hydrostatic column's
/// is linear in temperature and in the logarithm of its gas mass, where the
/// radius itself diverges. Refused cells hold the anchored surrogate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EnvelopeCell {
    pub compactness: f32,
    pub filled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    condensed_grid: Grid,
    envelope_grid: Grid,
    core_grid: Grid,
    pub condensed: Vec<CondensedCell>,
    pub envelope: Vec<EnvelopeCell>,
    pub core: Vec<CoreCell>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Condensed {
    pub radius_m: f64,
    pub inertia_factor: f64,
    /// Outside the tabulated domain, or read from a filled cell.
    pub approximate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    /// The core under the gas.
    pub core_radius_m: f64,
    /// Gas between that core and the one-bar level.
    pub thickness_m: f64,
    pub approximate: bool,
}

impl Table {
    #[cfg(test)]
    pub(crate) fn new(
        condensed: Vec<CondensedCell>,
        envelope: Vec<EnvelopeCell>,
        core: Vec<CoreCell>,
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
        for cell in &self.condensed {
            out.extend(cell.ln_radius_m.to_le_bytes());
            out.extend(cell.inertia_factor.to_le_bytes());
        }
        for cell in &self.envelope {
            out.extend(cell.compactness.to_le_bytes());
        }
        for cell in &self.core {
            out.extend(cell.ln_radius_m.to_le_bytes());
        }
        let flags = self
            .condensed
            .iter()
            .map(|c| c.filled)
            .chain(self.envelope.iter().map(|c| c.filled))
            .chain(self.core.iter().map(|c| c.filled))
            .collect::<Vec<_>>();
        out.extend(flags.chunks(8).map(|bits| {
            bits.iter()
                .enumerate()
                .fold(0_u8, |byte, (i, set)| byte | u8::from(*set) << i)
        }));
        out.extend(fnv(out.iter().copied()).to_le_bytes());
        out
    }

    pub(crate) fn decode(data: &[u8]) -> Option<Self> {
        let (condensed_grid, envelope_grid, core_grid) =
            (Grid::condensed(), Grid::envelope(), Grid::core());
        let (c, e, k) = (condensed_grid.len(), envelope_grid.len(), core_grid.len());
        let flags = 16 + 8 * c + 4 * e + 4 * k;
        if data.len() != flags + (c + e + k).div_ceil(8) + 8
            || &data[..8] != MAGIC
            || data[8..16] != grid_hash().to_le_bytes()
            || data[data.len() - 8..] != fnv(data[..data.len() - 8].iter().copied()).to_le_bytes()
        {
            return None;
        }
        let number = |at: usize| f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        let filled = |index: usize| data[flags + index / 8] >> (index % 8) & 1 == 1;
        let condensed: Vec<_> = (0..c)
            .map(|i| CondensedCell {
                ln_radius_m: number(16 + 8 * i),
                inertia_factor: number(20 + 8 * i),
                filled: filled(i),
            })
            .collect();
        let envelope: Vec<_> = (0..e)
            .map(|i| EnvelopeCell {
                compactness: number(16 + 8 * c + 4 * i),
                filled: filled(c + i),
            })
            .collect();
        let core: Vec<_> = (0..k)
            .map(|i| CoreCell {
                ln_radius_m: number(16 + 8 * c + 4 * e + 4 * i),
                filled: filled(c + e + i),
            })
            .collect();
        (condensed
            .iter()
            .all(|c| c.ln_radius_m.is_finite() && c.inertia_factor > 0.0)
            && envelope
                .iter()
                .all(|c| c.compactness.is_finite() && c.compactness > 0.0)
            && core.iter().all(|c| c.ln_radius_m.is_finite()))
        .then_some(Self {
            condensed_grid,
            envelope_grid,
            core_grid,
            condensed,
            envelope,
            core,
        })
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
    ) -> Condensed {
        let mass = mass_kg / EARTH_MASS_KG;
        let [low, high] = CONDENSED_MASS_EARTH;
        let bounded = mass.clamp(low, high);
        let water = water_share.clamp(0.0, 1.0);
        let point = [
            bounded.ln(),
            metal_share.clamp(0.0, 1.0),
            water,
            temperature_k,
        ];
        let mut ln_radius = 0.0;
        let mut inertia_factor = 0.0;
        let mut approximate = bounded != mass
            || (water > 0.0
                && !(WATER_TEMPERATURES_K[0]..=MAX_WATER_TEMPERATURE_K).contains(&temperature_k));
        for (index, weight) in self.condensed_grid.corners(point) {
            if weight == 0.0 {
                continue;
            }
            let cell = &self.condensed[index];
            ln_radius += weight * f64::from(cell.ln_radius_m);
            inertia_factor += weight * f64::from(cell.inertia_factor);
            approximate |= cell.filled;
        }
        // Below the domain bodies are uncompressed; above it the radius
        // continues as the fallback always has.
        let exponent = if mass < low {
            1.0 / 3.0
        } else {
            CONTINUATION_EXPONENT
        };
        Condensed {
            radius_m: ln_radius.exp() * (mass / bounded).powf(exponent),
            inertia_factor,
            approximate,
        }
    }

    /// Radius of a condensed body under an H/He envelope, loaded by no more
    /// than a trace of gas. Same continuations as `condensed`.
    pub fn core(&self, mass_kg: f64, metal_share: f64, water_share: f64) -> (f64, bool) {
        if mass_kg <= 0.0 {
            return (0.0, false);
        }
        let mass = mass_kg / EARTH_MASS_KG;
        let [low, high] = CONDENSED_MASS_EARTH;
        let bounded = mass.clamp(low, high);
        let point = [
            bounded.ln(),
            metal_share.clamp(0.0, 1.0),
            water_share.clamp(0.0, 1.0),
            0.0,
        ];
        let mut ln_radius = 0.0;
        let mut approximate = bounded != mass;
        for (index, weight) in self.core_grid.corners(point) {
            if weight == 0.0 {
                continue;
            }
            let cell = &self.core[index];
            ln_radius += weight * f64::from(cell.ln_radius_m);
            approximate |= cell.filled;
        }
        let exponent = if mass < low {
            1.0 / 3.0
        } else {
            CONTINUATION_EXPONENT
        };
        (
            ln_radius.exp() * (mass / bounded).powf(exponent),
            approximate,
        )
    }

    /// An H/He envelope's core and the gas above it up to one bar. The gas
    /// thickness is the reference core's, which a deep envelope barely
    /// changes with the core beneath it; the core is this body's own.
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
        let mut approximate = point
            .iter()
            .zip(&self.envelope_grid.knots)
            .any(|(x, knots)| !(knots[0]..=knots[knots.len() - 1]).contains(x));
        for (index, weight) in self.envelope_grid.corners(point) {
            if weight == 0.0 {
                continue;
            }
            let cell = &self.envelope[index];
            compactness += weight * f64::from(cell.compactness);
            approximate |= cell.filled;
        }
        // The reference core is the one the cell was solved over, so it takes
        // the table's bounds with the cell. The body's own core does not.
        let [mass, fraction] = [0, 1].map(|axis| {
            let knots = &self.envelope_grid.knots[axis];
            point[axis].clamp(knots[0], knots[knots.len() - 1]).exp()
        });
        let (reference, _) = self.core(
            mass * (1.0 - fraction) * EARTH_MASS_KG,
            REFERENCE_CORE_METAL,
            0.0,
        );
        let (core_radius_m, filled) =
            self.core(mass_kg * (1.0 - light_fraction), metal_share, water_share);
        Envelope {
            core_radius_m,
            thickness_m: (reference / compactness - reference).max(0.0),
            approximate: approximate || filled,
        }
    }
}
