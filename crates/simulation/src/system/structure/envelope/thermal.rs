use super::Error;
use crate::system::materials::hhe;
pub const MODEL_ID: &str = "grey_pressure_opacity_fixed_gradient_mass_age_intrinsic_v2";

/// Intrinsic temperature range the profile accepts.
pub const INTRINSIC_RANGE_K: [f64; 2] = [20.0, 200.0];
const JUPITER_INTRINSIC_K: f64 = 100.0;
const JUPITER_MASS_EARTH: f64 = 317.83;
const SOLAR_SYSTEM_AGE_GYR: f64 = 4.57;
/// Jupiter (about 100 K) to Neptune (about 53 K) today; Saturn then falls at
/// its measured 77 K. Below Neptune's mass this is an extrapolation.
const INTRINSIC_MASS_EXPONENT: f64 = 0.215;
/// Much gentler than giant cooling luminosity (about age^-1.3, so -0.3 here),
/// which balloons young low-mass envelopes to several times the published
/// radius trend. Chosen so layered radii follow the retained fit's age trend.
const INTRINSIC_AGE_EXPONENT: f64 = -0.1;

/// Leftover formation heat still leaving a planet, as a temperature: a
/// prescribed trend in mass and age, not a cooling calculation. Heavier planets
/// hold more heat and lose it more slowly; all cool with age.
pub fn intrinsic_k(mass_kg: f64, age_gyr: f64) -> f64 {
    let mass = mass_kg / crate::units::EARTH_MASS_KG / JUPITER_MASS_EARTH;
    let age = age_gyr / SOLAR_SYSTEM_AGE_GYR;
    let t =
        JUPITER_INTRINSIC_K * mass.powf(INTRINSIC_MASS_EXPONENT) * age.powf(INTRINSIC_AGE_EXPONENT);
    if t.is_nan() {
        return JUPITER_INTRINSIC_K;
    }
    t.clamp(INTRINSIC_RANGE_K[0], INTRINSIC_RANGE_K[1])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thermal {
    pub equilibrium_k: f64,
    pub intrinsic_k: f64,
    pub gravity_m_s2: f64,
}

impl Thermal {
    pub fn new(equilibrium_k: f64, intrinsic_k: f64, gravity_m_s2: f64) -> Result<Self, Error> {
        if ![equilibrium_k, intrinsic_k, gravity_m_s2]
            .iter()
            .all(|v| v.is_finite())
            || !(20.0..=2000.0).contains(&equilibrium_k)
            || !(INTRINSIC_RANGE_K[0]..=INTRINSIC_RANGE_K[1]).contains(&intrinsic_k)
            || !(0.1..=1000.0).contains(&gravity_m_s2)
        {
            return Err(Error::InvalidInput("thermal inputs"));
        }
        Ok(Self {
            equilibrium_k,
            intrinsic_k,
            gravity_m_s2,
        })
    }
    pub fn boundary_pressure_pa(self) -> f64 {
        let a = (self.equilibrium_k / self.intrinsic_k).powi(4) + 0.5;
        let tau = (4.0 * (2.0 / 7.0) * a) / (0.75 * (2.0 - 4.0 * (2.0 / 7.0)));
        (2.0 * self.gravity_m_s2 * 1e5 * tau / 0.001).sqrt()
    }
    /// Pressure at which the profile reaches `t`; `None` where it never does.
    /// Temperature rises monotonically with pressure, so this is unique.
    pub fn pressure_at_pa(self, t: f64) -> Option<f64> {
        let join = self.boundary_pressure_pa();
        let radiative = (t.powi(4) - self.equilibrium_k.powi(4))
            / (0.75 * self.intrinsic_k.powi(4))
            - 2.0 / 3.0;
        let p = (radiative * 2.0 * self.gravity_m_s2 * 1e5 / 0.001).sqrt();
        if p <= join {
            return Some(p);
        }
        let joined = self.temperature_k(join).ok()?;
        let p = join * (t / joined).powf(3.5);
        (t > joined && p.is_finite()).then_some(p)
    }
    pub fn temperature_k(self, p: f64) -> Result<f64, Error> {
        if !p.is_finite() || !(1e-2..=hhe::MAX_PRESSURE_PA).contains(&p) {
            return Err(Error::InvalidInput("thermal pressure"));
        }
        let join = self.boundary_pressure_pa();
        let radiative = |p: f64| {
            (self.equilibrium_k.powi(4)
                + 0.75
                    * self.intrinsic_k.powi(4)
                    * (0.001 * p * p / (2.0 * self.gravity_m_s2 * 1e5) + 2.0 / 3.0))
                .powf(0.25)
        };
        let t = if p <= join {
            radiative(p)
        } else {
            radiative(join) * (p / join).powf(2.0 / 7.0)
        };
        if !t.is_finite() || !(20.0..=hhe::MAX_TEMPERATURE_K).contains(&t) {
            return Err(Error::InvalidInput("thermal temperature"));
        }
        Ok(t)
    }
}
