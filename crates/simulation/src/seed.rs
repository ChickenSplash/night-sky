//! Stable hashes, random draws and seeded placement shared by generation.

use std::f64::consts::TAU;

/// FNV-1a plus an avalanche finalizer, stable across processes and Rust versions.
pub fn stable_hash(text: &str) -> u64 {
    finish_hash(hash_bytes(HASH_START, text.as_bytes()))
}

/// `stable_hash` in parts: the state before any bytes, adding bytes, and the
/// final mix, so a key can be hashed piece by piece without building a string.
pub(crate) const HASH_START: u64 = 0xcbf29ce484222325;

pub(crate) fn hash_bytes(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

pub(crate) fn finish_hash(mut hash: u64) -> u64 {
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d049bb133111eb);
    hash ^ (hash >> 31)
}

pub fn star_position(id: &str) -> [f64; 3] {
    let coord = |axis| {
        let n = stable_hash(&format!("{id}:{axis}"));
        ((n % 1_000_000) as f64 / 1_000_000.0 - 0.5) * 40.0
    };
    [coord('x'), coord('y'), coord('z')]
}

/// Orientation of a plane in galactic axes, stable for its key: (inclination, node,
/// twist) in radians, applied as Ry(node) · Rx(inclination) · Ry(twist). Poles are
/// uniform over the sphere, as real planetary systems are, so some systems orbit
/// retrograde relative to others.
pub fn plane_tilt(key: &str) -> [f64; 3] {
    let h = stable_hash(&format!("{key}:tilt"));
    let unit = |shift: u32| ((h >> shift) & 0xFFFF) as f64 / 65_535.0;
    [(1.0 - 2.0 * unit(0)).acos(), unit(16) * TAU, unit(32) * TAU]
}

/// SplitMix64: small, fast and identical on every platform.
#[derive(Clone)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(key: &str) -> Self {
        Self(stable_hash(key))
    }

    pub(crate) fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }

    pub(crate) fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    pub(crate) fn log_range(&mut self, low: f64, high: f64) -> f64 {
        (low.ln() + (high.ln() - low.ln()) * self.unit()).exp()
    }

    pub(crate) fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// Eccentricities and inclinations of real systems are close to Rayleigh.
    pub(crate) fn rayleigh(&mut self, sigma: f64) -> f64 {
        sigma * (-2.0 * (1.0 - self.unit()).ln()).sqrt()
    }

    /// A standard normal draw (Box–Muller), using two uniforms.
    pub(crate) fn normal(&mut self) -> f64 {
        let radius = (-2.0 * (1.0 - self.unit()).ln()).sqrt();
        radius * (std::f64::consts::TAU * self.unit()).cos()
    }
}

/// The SplitMix64 finaliser: spreads every input bit over the output.
pub(crate) fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::star_id;
    use std::f64::consts::PI;

    // Fixed values from before the module moves. Keys and random draws must not
    // change when generation code is reorganised.
    #[test]
    fn hashes_and_random_draws_keep_their_bits() {
        for (key, expected) in [
            ("", 0xf52a15e9a9b5e89b),
            ("list:page-3", 0xfe13bcd5a9dada44),
            ("g1abc.2.34", 0xedb9427b0adeaa3a),
        ] {
            assert_eq!(stable_hash(key), expected);
        }
        let mut rng = Rng::new("list:page-3");
        for expected in [
            0x3fcacede27230348,
            0x3febe0aa0a7186e9,
            0x3febc6d785d6e10e,
            0x3f9846e29eabfce0,
        ] {
            assert_eq!(rng.unit().to_bits(), expected);
        }
        assert_eq!(
            star_position("list:page-3").map(f64::to_bits),
            [0xc00f18a86d71f361, 0x3ff67f23cc8de2b4, 0x40222b7fe08aefb2],
        );
    }

    #[test]
    fn hashing_in_parts_matches_the_whole_key() {
        for parts in [
            ["", "", ""],
            ["list", ":page-", "3"],
            ["g1abc", ".2", ".34"],
            ["α", ":", "星"],
        ] {
            let hash = parts
                .iter()
                .fold(HASH_START, |hash, part| hash_bytes(hash, part.as_bytes()));
            assert_eq!(finish_hash(hash), stable_hash(&parts.concat()));
        }
    }

    #[test]
    fn system_poles_are_stable_and_isotropic() {
        let tilt = |page| plane_tilt(&star_id("list", page));
        assert_eq!(tilt(3), tilt(3));
        assert_ne!(tilt(0), tilt(1));
        // Pages of one list are as independent as separate lists.
        let tilts: Vec<_> = (0..4000).map(tilt).collect();
        assert!(tilts.iter().all(|t| (0.0..=PI).contains(&t[0])));
        // Uniform poles have cos(inclination) uniform on [-1, 1].
        let mean = tilts.iter().map(|t| t[0].cos()).sum::<f64>() / 4000.0;
        let square = tilts.iter().map(|t| t[0].cos().powi(2)).sum::<f64>() / 4000.0;
        assert!(mean.abs() < 0.05 && (square - 1.0 / 3.0).abs() < 0.03);
    }
}
