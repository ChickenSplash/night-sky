//! Replaceable, three-channel visible optics, not a greenhouse model.
//! Representative wavelengths are 680, 550 and 440 nm, not an RGB spectrum.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Optics {
    pub column: Column,
    /// Reference coefficients in inverse metres, following the gas profile.
    pub rayleigh_m_inv: [f64; 3],
    pub gas_absorption_m_inv: [f64; 3],
    pub aerosol_scattering_m_inv: [f64; 3],
    pub aerosol_absorption_m_inv: f64,
    pub aerosol_height_m: f64,
    pub aerosol_asymmetry: f64,
}

impl GasAtmosphere {
    pub fn optics(&self, gm: f64, radius: f64) -> Result<Optics, AtmosphereError> {
        let column = self.column(gm, radius)?;
        let number_density = self.pressure_pa() / (BOLTZMANN_J_K * self.temperature_k());
        // Approximate dilute-gas cross sections at 550 nm in m²/molecule.
        // Rounded reference values from refractivity/King-factor Rayleigh
        // estimates (Bodhaine et al. 1999; Sneep & Ubachs 2005). Dispersion is
        // simplified to lambda^-4. Dense gases and ionisation are unsupported.
        let cross_section: f64 = Gas::ALL
            .into_iter()
            .map(|gas| {
                let sigma = match gas {
                    Gas::Nitrogen => 5.1e-31,
                    Gas::Oxygen => 4.6e-31,
                    Gas::CarbonDioxide => 1.24e-30,
                    Gas::Hydrogen => 1.2e-31,
                    Gas::Helium => 8.0e-33,
                    Gas::Water => 4.1e-31,
                    Gas::Methane => 1.25e-30,
                    Gas::Argon => 4.4e-31,
                };
                self.composition().fraction(gas) * sigma
            })
            .sum();
        let rayleigh_m_inv = [680.0_f64, 550.0, 440.0]
            .map(|nm| number_density * cross_section * (550.0 / nm).powi(4));
        // Deliberately rough visible-band methane prescription. These effective
        // cross sections are tunable colour-band estimates, not resolved line
        // data or point samples at the Rayleigh wavelengths. No invented ozone,
        // CO2 infrared opacity, or water greenhouse opacity is added.
        let methane = number_density * self.composition().fraction(Gas::Methane);
        let gas_absorption_m_inv = [2e-29, 4e-30, 0.0].map(|sigma| methane * sigma);
        let mut optics = Optics {
            column,
            rayleigh_m_inv,
            gas_absorption_m_inv,
            aerosol_scattering_m_inv: [0.0; 3],
            aerosol_absorption_m_inv: 0.0,
            // Keep unused shader exponential profiles strictly positive.
            aerosol_height_m: column.scale_height_m,
            aerosol_asymmetry: 0.0,
        };
        if let Some(aerosol) = self.aerosol() {
            // Geometric-optics approximation for micron-sized spheres only.
            // Do not silently apply Q_ext=2 to submicron Rayleigh particles.
            if aerosol.particle_radius_m() < 0.5e-6 {
                return Err(AtmosphereError::UnsupportedOptics(
                    "aerosol radius below 0.5 micrometres",
                ));
            }
            let (density, absorbed, colour, g) = match aerosol.material() {
                AerosolMaterial::WaterDroplets => (1000.0, 0.001, [1.0; 3], 0.85),
                AerosolMaterial::WaterIce => (917.0, 0.001, [1.0; 3], 0.8),
                AerosolMaterial::SulfuricAcid => (1800.0, 0.005, [1.0; 3], 0.7),
                AerosolMaterial::AmmoniaIce => (817.0, 0.01, [1.0; 3], 0.75),
                AerosolMaterial::MineralDust => (2600.0, 0.1, [1.0, 0.6, 0.35], 0.75),
                AerosolMaterial::OrganicHaze => (1000.0, 0.15, [1.0, 0.6, 0.25], 0.6),
            };
            // n * Q_ext*pi*r² = 3*Q_ext*rho_aerosol/(4*rho_particle*r).
            let extinction = 3.0 * 2.0 * aerosol.reference_density_kg_m3()
                / (4.0 * density * aerosol.particle_radius_m());
            optics.aerosol_scattering_m_inv = colour.map(|c| extinction * (1.0 - absorbed) * c);
            optics.aerosol_absorption_m_inv = extinction * absorbed;
            optics.aerosol_height_m = aerosol.scale_height_m();
            optics.aerosol_asymmetry = g;
        }
        if optics
            .rayleigh_m_inv
            .into_iter()
            .chain(optics.gas_absorption_m_inv)
            .chain(optics.aerosol_scattering_m_inv)
            .chain([optics.aerosol_absorption_m_inv])
            .any(|v| !v.is_finite() || v < 0.0)
        {
            return Err(AtmosphereError::InvalidValue("optical coefficient"));
        }
        Ok(optics)
    }
}

#[cfg(test)]
mod tests;
