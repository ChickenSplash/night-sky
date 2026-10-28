//! Immutable thermal/loss inputs, independent of derived atmospheric output.
use space_shared::PlanetEnvironment;

const STEFAN_BOLTZMANN: f64 = 5.670_374_419e-8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Forcing {
    pub irradiation_w_m2: f64,
    /// Bolometric flux weighted by the population activity/history proxy, not XUV.
    pub loss_flux_w_m2: f64,
    pub assumed_bond_albedo: f64,
    pub age_gyr: f64,
}

impl Forcing {
    pub fn new(
        irradiation_w_m2: f64,
        loss_flux_w_m2: f64,
        assumed_bond_albedo: f64,
        age_gyr: f64,
    ) -> Result<Self, &'static str> {
        if !irradiation_w_m2.is_finite() || irradiation_w_m2 < 0.0 {
            return Err("irradiation");
        }
        if !loss_flux_w_m2.is_finite() || loss_flux_w_m2 < 0.0 {
            return Err("loss flux");
        }
        if !assumed_bond_albedo.is_finite() || !(0.0..=1.0).contains(&assumed_bond_albedo) {
            return Err("albedo");
        }
        if !age_gyr.is_finite() || age_gyr <= 0.0 {
            return Err("age");
        }
        Ok(Self {
            irradiation_w_m2,
            loss_flux_w_m2,
            assumed_bond_albedo,
            age_gyr,
        })
    }

    /// Input adapter for structure APIs. No atmospheric partition or column solve.
    pub fn environment(self) -> PlanetEnvironment {
        PlanetEnvironment {
            irradiation_w_m2: self.irradiation_w_m2,
            assumed_bond_albedo: self.assumed_bond_albedo,
            equilibrium_temperature_k: (self.irradiation_w_m2 * (1.0 - self.assumed_bond_albedo)
                / (4.0 * STEFAN_BOLTZMANN))
                .powf(0.25),
            age_gyr: self.age_gyr,
            atmosphere: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forcing_rejects_invalid_inputs_before_retention_or_atmosphere_work() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(Forcing::new(value, 1361.0, 0.3, 4.5).is_err());
            assert!(Forcing::new(1361.0, value, 0.3, 4.5).is_err());
            assert!(Forcing::new(1361.0, 1361.0, value, 4.5).is_err());
            assert!(Forcing::new(1361.0, 1361.0, 0.3, value).is_err());
        }
        assert!(Forcing::new(1361.0, 1361.0, 1.01, 4.5).is_err());
        assert!(Forcing::new(1361.0, 1361.0, 0.3, 0.0).is_err());
    }

    #[test]
    fn thermal_inputs_are_separate_from_output_and_preserve_zero_equilibrium() {
        let input = Forcing::new(1361.0, 1e5, 0.3, 4.5).unwrap();
        let mut output = input.environment();
        assert_eq!(output.atmosphere, None);
        assert!((output.equilibrium_temperature_k - 254.578).abs() < 0.001);
        output.assumed_bond_albedo = 0.9;
        output.equilibrium_temperature_k = 1000.0;
        assert_eq!(input.environment().assumed_bond_albedo, 0.3);
        assert_ne!(input.environment(), output);
        for (flux, albedo) in [(0.0, 0.3), (1361.0, 1.0)] {
            let cold = Forcing::new(flux, 0.0, albedo, 4.5).unwrap();
            assert_eq!(cold.environment().equilibrium_temperature_k, 0.0);
        }
    }
}
