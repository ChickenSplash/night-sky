//! Local, pinned CMS19/HG23 density resource. No network or bundled source data.
//! Density-only: phase/quantum screens are conservative engineering restrictions.
use crate::{
    system::atmosphere::Gas,
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};
use std::sync::OnceLock;

pub const MODEL_ID: &str = "cms19_hg23_screened_density_v1";
pub const COLD_MODEL_ID: &str = "dilute_hhe_100k_volume_anchor_v1";
pub const MAX_PRESSURE_PA: f64 = 1e13;

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
        if data.len() != 16 + 61 * 261 * 25
            || &data[..8] != b"NSHHE001"
            || data[8..16] != [61, 0, 0, 0, 5, 1, 0, 0]
        {
            return Err(Error::InvalidLocalTable);
        }
        let checksum = data.iter().fold(0xcbf29ce484222325_u64, |a, b| {
            (a ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        if checksum != TABLE_CHECKSUM {
            return Err(Error::InvalidLocalTable);
        }
        let mut nodes = Vec::with_capacity(61 * 261);
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
        let ti = (t.log10() - 2.0) / 0.05;
        let pi = p.log10() / 0.05;
        let i = (ti.floor() as usize).min(59);
        let j = (pi.floor() as usize).min(259);
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
                let node = &self.nodes[(i + di) * 261 + j + dj];
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
        // Equation-7 He melting is a bounded conservative pure-endpoint screen,
        // not a prediction of mixture freezing or a licence for extrapolation.
        if (y > 0.0 && t < 61.0 * (p / 1e8).powf(0.639))
            || (y < 1.0
                && p <= 3e11
                && t < 4.853 * (1.0 + p / 2.3e7).powf(0.748) * (-0.0098 * p / 1e9).exp())
        {
            return Err(Error::PureMeltingScreen);
        }
        // CMS19 eq.3, rho in g/cm³. Beyond the molecular melting interval H is
        // accepted only on a hot path, not via extrapolating its melting curve.
        if (y < 1.0 && (t < 10.0_f64.powf(3.3) * (rh / 1000.0).sqrt() || (p > 3e11 && t < 10000.0)))
            || (y > 0.0
                && t < 10.0_f64.powf(3.3) * (rhe / 1000.0).sqrt() * 2.0 * 4.0_f64.powf(-5.0 / 3.0))
        {
            return Err(Error::IonQuantumScreen);
        }
        let volume = (1.0 - y) / rh + y / rhe + y * (1.0 - y) * correction;
        let pressure_slope = |value: fn(&Node) -> f64| {
            let difference = |row: usize| {
                value(&self.nodes[row * 261 + j + 1]) - value(&self.nodes[row * 261 + j])
            };
            ((1.0 - u) * difference(i) + u * difference(i + 1)) / 0.05
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
            || !(20.0..=1e5).contains(&t)
            || !(0.0..=1.0).contains(&y)
        {
            return Err(Error::OutsideDomain);
        }
        if t < 100.0 {
            let r = BOLTZMANN_J_K / ATOMIC_MASS_KG
                * ((1.0 - y) / Gas::Hydrogen.molecular_mass_u()
                    + y / Gas::Helium.molecular_mass_u());
            if p / (r * t) > 0.1 {
                return Err(Error::ColdDenseUnsupported);
            }
            let v = r * t / p + self.volume(p, 100.0, y)? - r * 100.0 / p;
            if v <= 0.0 || !v.is_finite() {
                return Err(Error::UnstableInterpolation);
            }
            return Ok(1.0 / v);
        }
        Ok(1.0 / self.volume(p, t, y)?)
    }
}

// Filled from prepare-hhe-density.py after SHA-256 verification of all sources.
const TABLE_CHECKSUM: u64 = 0xb4e7a79ecc35d846;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_resources_are_explicit() {
        assert!(matches!(Table::decode(&[]), Err(Error::InvalidLocalTable)));
        assert!(matches!(
            Table::decode(&vec![0; 16 + 61 * 261 * 25]),
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
        assert_eq!(
            table.density_kg_m3(1e11, 10.0_f64.powf(3.7), 0.25),
            Err(Error::PureMeltingScreen)
        );
        assert_eq!(
            table.density_kg_m3(1e5, 20.0, 0.25),
            Err(Error::ColdDenseUnsupported)
        );
        let cold = table.density_kg_m3(1000.0, 100.0 - 1e-6, 0.25).unwrap();
        let warm = table.density_kg_m3(1000.0, 100.0, 0.25).unwrap();
        assert!((cold / warm - 1.0).abs() < 1e-7);
        for (p, t, y) in [
            (0.0, 1000.0, 0.25),
            (1e14, 1000.0, 0.25),
            (1e6, f64::NAN, 0.25),
            (1e6, 1000.0, -0.1),
        ] {
            assert_eq!(table.density_kg_m3(p, t, y), Err(Error::OutsideDomain));
        }
    }
}
