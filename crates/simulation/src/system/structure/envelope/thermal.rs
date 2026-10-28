use super::Error;
pub const MODEL_ID: &str = "grey_pressure_opacity_fixed_gradient_v1";

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
            || !(20.0..=200.0).contains(&intrinsic_k)
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
    pub fn temperature_k(self, p: f64) -> Result<f64, Error> {
        if !p.is_finite() || !(1e-2..=1e14).contains(&p) {
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
        if !t.is_finite() || !(20.0..=1e5).contains(&t) {
            return Err(Error::InvalidInput("thermal temperature"));
        }
        Ok(t)
    }
}
