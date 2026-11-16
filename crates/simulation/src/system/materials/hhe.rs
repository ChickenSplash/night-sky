//! Local, pinned CMS19/HG23 density resource. No network or bundled source data.
//! Density-only: phase/quantum screens are conservative engineering restrictions.
use crate::{
    system::atmosphere::Gas,
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};
use std::sync::OnceLock;

pub const MODEL_ID: &str = "cms19_hg23_screened_density_v1";
pub const COLD_MODEL_ID: &str = "cold_hhe_hydrogen_virial_100k_anchor_v2";
pub const MAX_PRESSURE_PA: f64 = 1e15;
pub const MAX_TEMPERATURE_K: f64 = 1e6;
/// Log10 spacing of both table axes. Pressure knots start at 1 Pa and
/// temperature knots at 100 K; interpolation is only continuous across them.
pub(crate) const KNOT_DEX: f64 = 0.05;
/// Knots along temperature (100 K to 1e6 K) and pressure (1 Pa to 1e15 Pa).
pub(crate) const TEMPERATURE_KNOTS: usize = 81;
pub(crate) const PRESSURE_KNOTS: usize = 301;
/// Lowest density the source tabulates, 1e-8 g/cm3.
const MIN_SOURCE_DENSITY_KG_M3: f64 = 1e-5;
/// Above these pure densities the source leaves the molecular and atomic model
/// (SCvH) for the dense-plasma one, whose ion quantum limit is its equation 3.
const PLASMA_HYDROGEN_KG_M3: f64 = 50.0;
const PLASMA_HELIUM_KG_M3: f64 = 100.0;
/// Highest point of the source's hydrogen melting curve (990 K at 0.76 Mbar).
/// Beyond the curve's 3 Mbar range hydrogen is taken as fluid only above it.
const HYDROGEN_MELTING_MAXIMUM_K: f64 = 1000.0;
/// Helium at or below this mass fraction is a solute in hydrogen, so the pure
/// helium melting curve does not apply to it.
pub const SOLUTE_HELIUM_FRACTION: f64 = 0.5;
/// Below this the hydrogen virial fit is an extrapolation and hydrogen nears
/// its critical point (33 K): only the dilute limit is accepted.
const VIRIAL_FLOOR_K: f64 = 40.0;
/// Largest second-virial share of the ideal volume the cold route accepts.
const VIRIAL_LIMIT: f64 = 0.1;
/// Ideal density accepted below the virial floor, kg/m3.
const DILUTE_LIMIT_KG_M3: f64 = 0.1;

/// Second virial coefficient of hydrogen, m3/mol: Hodges et al. (2004,
/// J. Chem. Phys. 120, 710), equation 24 and table IV, fitted to data from
/// about 98 K upward. Used here as a difference from its own 100 K value.
fn hydrogen_virial_m3_mol(t: f64) -> f64 {
    let x = t / 100.0;
    1e-6 * (42.0803 * x.powf(-0.33) - 143.982 * x.powf(-1.4) + 146.918 * x.powf(-1.8)
        - 47.5601 * x.powf(-2.2))
}

/// Pure-helium melting temperature, the source's equation 7.
pub fn helium_melting_k(pressure_pa: f64) -> f64 {
    61.0 * (pressure_pa / 1e8).powf(0.639)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingLocalTable,
    InvalidLocalTable,
    OutsideDomain,
    ColdDenseUnsupported,
    PureMeltingScreen,
    IonQuantumScreen,
    UnstableInterpolation,
}

impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingLocalTable => "H/He local table missing",
            Self::InvalidLocalTable => "H/He local table invalid",
            Self::OutsideDomain => "H/He density domain",
            Self::ColdDenseUnsupported => "H/He cold dense domain",
            Self::PureMeltingScreen => "H/He conservative pure melting screen",
            Self::IonQuantumScreen => "H/He ion quantum screen",
            Self::UnstableInterpolation => "H/He interpolation stability",
        }
    }
}

#[derive(Clone)]
struct Node {
    h: f64,
    he: f64,
    correction: f64,
    flags: u8,
}
pub struct Table {
    nodes: Vec<Node>,
}

impl Table {
    pub fn local() -> Result<&'static Self, Error> {
        static TABLE: OnceLock<Result<Table, Error>> = OnceLock::new();
        TABLE
            .get_or_init(|| {
                let path = std::env::var_os("NS_HHE_DENSITY_PATH")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../data/composition-sources/hhe-density.bin")
                    });
                let data = std::fs::read(path).map_err(|_| Error::MissingLocalTable)?;
                Self::decode(&data)
            })
            .as_ref()
            .map_err(|e| *e)
    }

    fn decode(data: &[u8]) -> Result<Self, Error> {
        if data.len() != 16 + TEMPERATURE_KNOTS * PRESSURE_KNOTS * 25
            || &data[..8] != b"NSHHE002"
            || data[8..12] != (TEMPERATURE_KNOTS as u32).to_le_bytes()
            || data[12..16] != (PRESSURE_KNOTS as u32).to_le_bytes()
        {
            return Err(Error::InvalidLocalTable);
        }
        let checksum = data.iter().fold(0xcbf29ce484222325_u64, |a, b| {
            (a ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        if checksum != TABLE_CHECKSUM {
            return Err(Error::InvalidLocalTable);
        }
        let mut nodes = Vec::with_capacity(TEMPERATURE_KNOTS * PRESSURE_KNOTS);
        for row in data[16..].chunks_exact(25) {
            let number = |offset| f64::from_le_bytes(row[offset..offset + 8].try_into().unwrap());
            let node = Node {
                h: number(0),
                he: number(8),
                correction: number(16),
                flags: row[24],
            };
            if ![node.h, node.he, node.correction]
                .iter()
                .all(|v| v.is_finite())
                || node.flags > 3
            {
                return Err(Error::InvalidLocalTable);
            }
            nodes.push(node);
        }
        Ok(Self { nodes })
    }

    fn volume(&self, p: f64, t: f64, y: f64) -> Result<f64, Error> {
        let ti = (t.log10() - 2.0) / KNOT_DEX;
        let pi = p.log10() / KNOT_DEX;
        let i = (ti.floor() as usize).min(TEMPERATURE_KNOTS - 2);
        let j = (pi.floor() as usize).min(PRESSURE_KNOTS - 2);
        let u = ti - i as f64;
        let v = pi - j as f64;
        let mut h = 0.0;
        let mut he = 0.0;
        let mut correction = 0.0;
        for (di, wt) in [(0, 1.0 - u), (1, u)] {
            for (dj, wp) in [(0, 1.0 - v), (1, v)] {
                let weight = wt * wp;
                if weight == 0.0 {
                    continue;
                }
                let node = &self.nodes[(i + di) * PRESSURE_KNOTS + j + dj];
                if (y < 1.0 && node.flags & 1 == 0) || (y > 0.0 && node.flags & 2 == 0) {
                    return Err(Error::UnstableInterpolation);
                }
                h += weight * node.h;
                he += weight * node.he;
                correction += weight * node.correction;
            }
        }
        let rh = 1000.0 * 10.0_f64.powf(h);
        let rhe = 1000.0 * 10.0_f64.powf(he);
        if (y < 1.0 && rh < MIN_SOURCE_DENSITY_KG_M3) || (y > 0.0 && rhe < MIN_SOURCE_DENSITY_KG_M3)
        {
            return Err(Error::OutsideDomain);
        }
        // Each pure melting curve screens its own species where that species
        // is the solvent. Helium dissolved in fluid hydrogen keeps its fluid
        // table volume below the pure-helium curve: a labelled approximation,
        // not a mixture phase diagram (no demixing or helium rain).
        let hydrogen_melting_k = if p <= 3e11 {
            4.853 * (1.0 + p / 2.3e7).powf(0.748) * (-0.0098 * p / 1e9).exp()
        } else {
            HYDROGEN_MELTING_MAXIMUM_K
        };
        if (y > SOLUTE_HELIUM_FRACTION && t < helium_melting_k(p))
            || (y < 1.0 && t < hydrogen_melting_k)
        {
            return Err(Error::PureMeltingScreen);
        }
        // CMS19 eq.3, rho in g/cm3: the limit of the dense-plasma model's ion
        // quantum correction. It has no meaning in the molecular gas.
        let quantum_k = |rho: f64| 10.0_f64.powf(3.3) * (rho / 1000.0).sqrt();
        if (y < 1.0 && rh > PLASMA_HYDROGEN_KG_M3 && t < quantum_k(rh))
            || (y > 0.0
                && rhe > PLASMA_HELIUM_KG_M3
                && t < quantum_k(rhe) * 2.0 * 4.0_f64.powf(-5.0 / 3.0))
        {
            return Err(Error::IonQuantumScreen);
        }
        let volume = (1.0 - y) / rh + y / rhe + y * (1.0 - y) * correction;
        let pressure_slope = |value: fn(&Node) -> f64| {
            let difference = |row: usize| {
                value(&self.nodes[row * PRESSURE_KNOTS + j + 1])
                    - value(&self.nodes[row * PRESSURE_KNOTS + j])
            };
            ((1.0 - u) * difference(i) + u * difference(i + 1)) / KNOT_DEX
        };
        let dv_dlnp = -(1.0 - y) / rh * pressure_slope(|n| n.h)
            - y / rhe * pressure_slope(|n| n.he)
            + y * (1.0 - y) * pressure_slope(|n| n.correction) / std::f64::consts::LN_10;
        if !dv_dlnp.is_finite() || dv_dlnp >= 0.0 {
            return Err(Error::UnstableInterpolation);
        }
        if !volume.is_finite() || volume <= 0.0 {
            return Err(Error::UnstableInterpolation);
        }
        Ok(volume)
    }

    pub fn density_kg_m3(&self, p: f64, t: f64, y: f64) -> Result<f64, Error> {
        if ![p, t, y].iter().all(|v| v.is_finite())
            || !(1.0..=MAX_PRESSURE_PA).contains(&p)
            || !(20.0..=MAX_TEMPERATURE_K).contains(&t)
            || !(0.0..=1.0).contains(&y)
        {
            return Err(Error::OutsideDomain);
        }
        if t < 100.0 {
            // Ideal volume at the real temperature, the table's own nonideal
            // residual at 100 K, and the change in hydrogen's second virial
            // coefficient since 100 K. Helium and cross terms stay at 100 K.
            let moles_per_kg = (1.0 - y) / (Gas::Hydrogen.molecular_mass_u() * 1e-3)
                + y / (Gas::Helium.molecular_mass_u() * 1e-3);
            let r = BOLTZMANN_J_K / ATOMIC_MASS_KG * moles_per_kg * 1e-3;
            let hydrogen = (1.0 - y) / (Gas::Hydrogen.molecular_mass_u() * 1e-3) / moles_per_kg;
            let virial = |t: f64| hydrogen * hydrogen * hydrogen_virial_m3_mol(t);
            let ideal = r * t / p;
            let accepted = if t < VIRIAL_FLOOR_K {
                1.0 / ideal <= DILUTE_LIMIT_KG_M3
            } else {
                virial(t).abs() * moles_per_kg <= VIRIAL_LIMIT * ideal
            };
            if !accepted {
                return Err(Error::ColdDenseUnsupported);
            }
            let shift = (virial(t.max(VIRIAL_FLOOR_K)) - virial(100.0)) * moles_per_kg;
            let v = ideal + self.volume(p, 100.0, y)? - r * 100.0 / p + shift;
            if v <= 0.0 || !v.is_finite() {
                return Err(Error::UnstableInterpolation);
            }
            return Ok(1.0 / v);
        }
        Ok(1.0 / self.volume(p, t, y)?)
    }
}

// Filled from prepare-hhe-density.py after SHA-256 verification of all sources.
const TABLE_CHECKSUM: u64 = 0x3fe0293c8a340e38;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_resources_are_explicit() {
        assert!(matches!(Table::decode(&[]), Err(Error::InvalidLocalTable)));
        assert!(matches!(
            Table::decode(&vec![0; 16 + TEMPERATURE_KNOTS * PRESSURE_KNOTS * 25]),
            Err(Error::InvalidLocalTable)
        ));
    }

    #[test]
    #[ignore = "requires separately prepared pinned local EOS resource"]
    fn pinned_density_and_negative_phase_fixtures() {
        let table = Table::local().unwrap();
        for (p, t, expected) in [
            (1e6, 1000.0, 0.2763225683378595),
            (1e11, 10000.0, 653.392513004275),
        ] {
            assert!((table.density_kg_m3(p, t, 0.25).unwrap() / expected - 1.0).abs() < 1e-12);
        }
        // Below the pure-helium curve: refused as helium, fluid as a solute.
        let (p, t) = (1e11, 10.0_f64.powf(3.7));
        assert!(t < helium_melting_k(p));
        assert_eq!(
            table.density_kg_m3(p, t, 1.0),
            Err(Error::PureMeltingScreen)
        );
        assert!((table.density_kg_m3(p, t, 0.25).unwrap() / 761.474 - 1.0).abs() < 1e-6);
        assert_eq!(
            table.density_kg_m3(1e11, 500.0, 0.25),
            Err(Error::PureMeltingScreen)
        );
        assert_eq!(
            table.density_kg_m3(1e12, 2000.0, 0.25),
            Err(Error::IonQuantumScreen)
        );
        // Cold gas: a few bar at 50 K is solved, and denser than ideal.
        let cold = table.density_kg_m3(3e5, 50.0, 0.25).unwrap();
        let ideal = 3e5 / (3614.0 * 50.0);
        assert!(cold > ideal && cold < 1.03 * ideal);
        assert_eq!(
            table.density_kg_m3(3e7, 50.0, 0.25),
            Err(Error::ColdDenseUnsupported)
        );
        assert_eq!(
            table.density_kg_m3(1e5, 35.0, 0.25),
            Err(Error::ColdDenseUnsupported)
        );
        assert!(table.density_kg_m3(1e14, 5e5, 0.25).is_ok());
        assert_eq!(
            table.density_kg_m3(1e5, 20.0, 0.25),
            Err(Error::ColdDenseUnsupported)
        );
        let cold = table.density_kg_m3(1000.0, 100.0 - 1e-6, 0.25).unwrap();
        let warm = table.density_kg_m3(1000.0, 100.0, 0.25).unwrap();
        assert!((cold / warm - 1.0).abs() < 1e-7);
        for (p, t, y) in [
            (0.0, 1000.0, 0.25),
            (1e16, 1000.0, 0.25),
            (1e6, f64::NAN, 0.25),
            (1e6, 1000.0, -0.1),
        ] {
            assert_eq!(table.density_kg_m3(p, t, y), Err(Error::OutsideDomain));
        }
    }
}
