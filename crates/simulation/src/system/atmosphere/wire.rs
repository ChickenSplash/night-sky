//! Validated physical inputs to/from wire snapshots. Derived wire values are
//! informational: consumers recompute them from physical inputs and body GM/R.
use super::*;
use space_shared::{AerosolData, AerosolKind, AtmosphereData, AtmosphereReference, GasFraction};

impl GasAtmosphere {
    pub fn to_data(&self, gm_m3_s2: f64, radius_m: f64) -> Result<AtmosphereData, AtmosphereError> {
        let column = self.column(gm_m3_s2, radius_m)?;
        Ok(AtmosphereData {
            gases: Gas::ALL
                .into_iter()
                .filter_map(|gas| {
                    let mole_fraction = self.composition.fraction(gas);
                    (mole_fraction > 0.0).then(|| GasFraction {
                        gas: gas.formula().into(),
                        mole_fraction,
                    })
                })
                .collect(),
            reference_level: match self.reference_level {
                ReferenceLevel::Surface => AtmosphereReference::Surface,
                ReferenceLevel::OneBar => AtmosphereReference::OneBar,
            },
            pressure_pa: self.pressure_pa,
            temperature_k: self.temperature_k,
            aerosol: self.aerosol.map(|aerosol| AerosolData {
                material: match aerosol.material {
                    AerosolMaterial::WaterDroplets => AerosolKind::WaterDroplets,
                    AerosolMaterial::WaterIce => AerosolKind::WaterIce,
                    AerosolMaterial::SulfuricAcid => AerosolKind::SulfuricAcid,
                    AerosolMaterial::AmmoniaIce => AerosolKind::AmmoniaIce,
                    AerosolMaterial::MineralDust => AerosolKind::MineralDust,
                    AerosolMaterial::OrganicHaze => AerosolKind::OrganicHaze,
                },
                reference_density_kg_m3: aerosol.reference_density_kg_m3,
                scale_height_m: aerosol.scale_height_m,
                particle_radius_m: aerosol.particle_radius_m,
            }),
            mean_molecular_mass_kg: column.molecular_mass_kg,
            reference_density_kg_m3: column.density_kg_m3,
            scale_height_m: column.scale_height_m,
            extent_m: column.extent_m,
            extended: column.validity == ColumnValidity::Extended,
        })
    }
}

impl TryFrom<&AtmosphereData> for GasAtmosphere {
    type Error = AtmosphereError;

    fn try_from(data: &AtmosphereData) -> Result<Self, Self::Error> {
        let parts = data
            .gases
            .iter()
            .map(|part| {
                Gas::from_formula(&part.gas)
                    .map(|gas| (gas, part.mole_fraction))
                    .ok_or(AtmosphereError::InvalidValue("gas formula"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let composition = Composition::new(&parts)?;
        let aerosol = data
            .aerosol
            .as_ref()
            .map(|aerosol| {
                AerosolLayer::new(
                    match aerosol.material {
                        AerosolKind::WaterDroplets => AerosolMaterial::WaterDroplets,
                        AerosolKind::WaterIce => AerosolMaterial::WaterIce,
                        AerosolKind::SulfuricAcid => AerosolMaterial::SulfuricAcid,
                        AerosolKind::AmmoniaIce => AerosolMaterial::AmmoniaIce,
                        AerosolKind::MineralDust => AerosolMaterial::MineralDust,
                        AerosolKind::OrganicHaze => AerosolMaterial::OrganicHaze,
                    },
                    aerosol.reference_density_kg_m3,
                    aerosol.scale_height_m,
                    aerosol.particle_radius_m,
                )
            })
            .transpose()?;
        Self::new(
            composition,
            match data.reference_level {
                AtmosphereReference::Surface => ReferenceLevel::Surface,
                AtmosphereReference::OneBar => ReferenceLevel::OneBar,
            },
            data.pressure_pa,
            data.temperature_k,
            aerosol,
        )
    }
}
