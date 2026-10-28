//! Local, perfectly entrained condensates. Ideal vapour fugacity and independent
//! pure condensates, NOT a non-ideal mixture flash, cloud dynamics or rainout.
use super::{Material, hhe_mixture::SECONDARIES, molecular};
use crate::{
    system::composition::Inventory,
    units::{ATOMIC_MASS_KG, BOLTZMANN_J_K},
};

pub const MODEL_ID: &str = "local_suspended_secondary_saturation_v1";
pub const DEEP_MODEL_ID: &str = "suspended_to_effective_secondary_volume_v1";
pub const NOTES: &[&str] = &[
    "Perfectly suspended local condensate; total constituent abundances uniform, vapour fractions vary with pressure and temperature",
    "Ideal partial-pressure saturation, separate pure condensates; fugacity/activity coefficients and Poynting correction set to one, including dense states",
    "Ideal secondary vapour volume and pressure-compressed condensate proxy; liquid/solid identity and critical mixture phases unresolved",
    "Deep effective component densities anchor to the ideal-vapour endpoint, not Peng-Robinson; handoff requires every present secondary above its critical temperature",
    "No settling, rainout, nucleation, latent-heat feedback, chemistry or condensate opacity; unchanged prescribed H/He thermal profile and fixed 2/7 deep slope",
    "Phase masses use profile quadrature, not additional material; numerical consistency is not a physical accuracy guarantee",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Domain,
    Partition,
    CriticalRegion,
}
impl Error {
    pub fn label(self) -> &'static str {
        match self {
            Self::Domain => "suspended condensate model domain",
            Self::Partition => "suspended phase partition closure",
            Self::CriticalRegion => "suspended near-critical domain",
        }
    }
}

/// Critical constants agree with the bounded molecular model. Condensate
/// reference densities are engineering liquid/solid-like values, not EOS fits.
fn parameters(m: Material) -> Option<(f64, f64, f64)> {
    Some(match m {
        Material::CarbonDioxide => (304.1282, 7_377_300.0, 1560.0),
        Material::Nitrogen => (126.192, 3_395_800.0, 808.0),
        Material::Methane => (190.564, 4_599_200.0, 450.0),
        Material::Oxygen => (154.581, 5_043_000.0, 1140.0),
        Material::Argon => (150.687, 4_863_000.0, 1400.0),
        _ => return None,
    })
}

pub fn cold_composition(gas: &Inventory, minimum_temperature: f64) -> bool {
    SECONDARIES
        .into_iter()
        .any(|m| gas.mass_kg(m) > 0.0 && minimum_temperature < parameters(m).unwrap().0)
}

pub fn gas_constant(m: Material) -> f64 {
    BOLTZMANN_J_K / (ATOMIC_MASS_KG * m.properties().gas.unwrap().molecular_mass_u())
}

pub fn condensate_density_kg_m3(m: Material, p: f64) -> Result<f64, Error> {
    if !p.is_finite() || !(1.0..=molecular::MAX_PRESSURE_PA).contains(&p) {
        return Err(Error::Domain);
    }
    Ok(parameters(m).ok_or(Error::Domain)?.2 * (1.0 + 7.0 * p / 2.0e9).powf(1.0 / 7.0))
}

/// Existing rough cold-trap curves are deliberately not changed for legacy
/// consumers. Here use continuous Clausius-Clapeyron branches through triple
/// and critical anchors, with an independent boiling/frost-point check.
pub fn saturation_pa(m: Material, t: f64) -> Result<f64, Error> {
    let (tc, pc, _) = parameters(m).ok_or(Error::Domain)?;
    if !t.is_finite() || !(80.0..=625.0).contains(&t) {
        return Err(Error::Domain);
    }
    if t >= tc {
        return Ok(f64::INFINITY);
    }
    let (triple, pt, sub) = match m {
        Material::CarbonDioxide => (216.58, 518_500.0, 3135.0),
        Material::Nitrogen => (63.14, 12_520.0, 850.0),
        Material::Methane => (90.694, 11_696.0, 1160.0),
        Material::Oxygen => (54.361, 146.3, 1000.0),
        Material::Argon => (83.806, 68_891.0, 930.0),
        _ => return Err(Error::Domain),
    };
    // O2's wide triple-to-critical interval needs a boiling anchor too: one
    // constant slope misses the independent 90 K vapour-pressure check by 12%.
    // The two joined log-pressure branches remain continuous and monotonic.
    let (anchor, anchor_p, upper, upper_p) = if m == Material::Oxygen && t >= triple {
        if t < 90.188 {
            (triple, pt, 90.188, 101_325.0)
        } else {
            (90.188, 101_325.0, tc, pc)
        }
    } else {
        (triple, pt, tc, pc)
    };
    let slope = if t < triple {
        sub
    } else {
        (upper_p / anchor_p).ln() / (1.0 / anchor - 1.0 / upper)
    };
    Ok(anchor_p * (-slope * (1.0 / t - 1.0 / anchor)).exp())
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    /// Mass fractions per unit TOTAL suspended parcel, not per unit vapour.
    pub vapour: Inventory,
    pub condensate: Inventory,
    pub density_kg_m3: f64,
}

impl State {
    pub fn new(gas: &Inventory, p: f64, t: f64, hhe: f64) -> Result<Self, Error> {
        if !p.is_finite()
            || !(1.0..=molecular::MAX_PRESSURE_PA).contains(&p)
            || !t.is_finite()
            || !(80.0..=625.0).contains(&t)
            || !hhe.is_finite()
            || hhe <= 0.0
        {
            return Err(Error::Domain);
        }
        // Validate composition even for absent/unsupported species.
        super::hhe_mixture::Mixture::new_enriched(gas).map_err(|_| Error::Domain)?;
        let fractions = gas
            .scaled(1.0 / gas.total_mass_kg())
            .map_err(|_| Error::Domain)?;
        if Material::ALL
            .into_iter()
            .any(|m| gas.mass_kg(m) > 0.0 && fractions.mass_kg(m) == 0.0)
        {
            return Err(Error::Partition);
        }
        let mut available = [0.0; 5];
        let mut k = [0.0; 5];
        let mut capped = [false; 5];
        let light = [Material::Hydrogen, Material::Helium]
            .into_iter()
            .map(|m| fractions.mass_kg(m) / m.properties().gas.unwrap().molecular_mass_u())
            .sum::<f64>();
        for (i, m) in SECONDARIES.into_iter().enumerate() {
            if fractions.mass_kg(m) == 0.0 {
                continue;
            }
            let (tc, pc, _) = parameters(m).unwrap();
            if (t / tc - 1.0).abs() < 0.1 && p > 0.5 * pc {
                return Err(Error::CriticalRegion);
            }
            available[i] = fractions.mass_kg(m) / m.properties().gas.unwrap().molecular_mass_u();
            k[i] = saturation_pa(m, t)? / p;
        }
        // n_i = min(a_i, K_i N); N includes the noncondensing H/He.
        // Removing saturated species decreases N monotonically. At most five
        // removals plus one closure pass, no unbounded nonlinear iteration.
        let mut total = light + available.iter().sum::<f64>();
        for _ in 0..=5 {
            let mut changed = false;
            for i in 0..5 {
                if !capped[i] && available[i] > k[i] * total {
                    capped[i] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            let numerator = light
                + (0..5)
                    .filter(|&i| !capped[i])
                    .map(|i| available[i])
                    .sum::<f64>();
            let denominator = 1.0 - (0..5).filter(|&i| capped[i]).map(|i| k[i]).sum::<f64>();
            if denominator <= 0.0 {
                return Err(Error::Partition);
            }
            total = numerator / denominator;
        }
        let mut condensed = [(Material::CarbonDioxide, 0.0); 5];
        let mut vapour_entries = Material::ALL.map(|m| (m, fractions.mass_kg(m)));
        let mut volume =
            (fractions.mass_kg(Material::Hydrogen) + fractions.mass_kg(Material::Helium)) / hhe;
        for (i, m) in SECONDARIES.into_iter().enumerate() {
            let fraction = fractions.mass_kg(m);
            let mu = m.properties().gas.unwrap().molecular_mass_u();
            let v = if capped[i] {
                k[i] * total * mu
            } else {
                fraction
            };
            let c = fraction - v;
            if c < 0.0 || v < 0.0 || !v.is_finite() {
                return Err(Error::Partition);
            }
            condensed[i] = (m, c);
            vapour_entries.iter_mut().find(|e| e.0 == m).unwrap().1 = v;
            let rho_c = condensate_density_kg_m3(m, p)?;
            volume += v * gas_constant(m) * t / p + c / rho_c;
        }
        let vapour = Inventory::new(&vapour_entries).map_err(|_| Error::Partition)?;
        let condensate = Inventory::new(&condensed).map_err(|_| Error::Partition)?;
        let actual = [Material::Hydrogen, Material::Helium]
            .into_iter()
            .chain(SECONDARIES)
            .map(|m| vapour.mass_kg(m) / m.properties().gas.unwrap().molecular_mass_u())
            .sum::<f64>();
        if !total.is_finite()
            || total <= 0.0
            || !actual.is_finite()
            || actual <= 0.0
            || !volume.is_finite()
            || volume <= 0.0
            || (actual / total - 1.0).abs() > 1e-12
        {
            return Err(Error::Partition);
        }
        Ok(Self {
            vapour,
            condensate,
            density_kg_m3: 1.0 / volume,
        })
    }
}

#[cfg(test)]
mod tests;
