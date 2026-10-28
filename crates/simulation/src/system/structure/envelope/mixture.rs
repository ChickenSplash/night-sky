//! Explicit molecular/effective component handoff along the prescribed thermal
//! profile. The event is a model boundary, never a phase transition.
use super::*;
use crate::system::materials::{effective_secondary, molecular};
use std::cell::OnceCell;

pub(super) struct ComponentRoute {
    pub boundary_pa: f64,
    anchor: Option<(f64, f64)>,
    gas: Inventory,
    scales: OnceCell<Result<[f64; 5], SolveError>>,
}

impl ComponentRoute {
    pub fn new(thermal: Thermal, gas: Inventory) -> Result<Self, Error> {
        let maximum = molecular::MAX_PRESSURE_PA;
        let anchor = if thermal.temperature_k(OUTER_PRESSURE_PA)? >= molecular::MAX_TEMPERATURE_K {
            None
        } else {
            let mut low = OUTER_PRESSURE_PA;
            let mut high = maximum;
            if thermal.temperature_k(high)? > molecular::MAX_TEMPERATURE_K {
                for _ in 0..64 {
                    let mid = low + (high - low) * 0.5;
                    if thermal.temperature_k(mid)? <= molecular::MAX_TEMPERATURE_K {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
            } else {
                low = maximum;
            }
            Some((low, thermal.temperature_k(low)?))
        };
        Ok(Self {
            boundary_pa: anchor.map_or(OUTER_PRESSURE_PA, |a| a.0),
            anchor,
            gas,
            scales: OnceCell::new(),
        })
    }

    pub fn interfaces(&self) -> Vec<radial::PressureInterface> {
        let mut pressures = vec![1e5];
        if self.boundary_pa > OUTER_PRESSURE_PA {
            pressures.push(self.boundary_pa);
        }
        pressures.sort_by(|a, b| b.total_cmp(a));
        pressures.dedup();
        pressures
            .into_iter()
            .map(|pressure_pa| radial::PressureInterface {
                material: Material::Hydrogen,
                pressure_pa,
            })
            .collect()
    }

    pub fn effective(&self, p: f64, side: radial::DensitySide) -> bool {
        if self.anchor.is_none() {
            return true;
        }
        match side {
            radial::DensitySide::Equilibrium => p > self.boundary_pa,
            radial::DensitySide::Branch(i) => {
                // A one-bar event above the component event creates a second
                // inner branch. Coincident events are represented only once.
                i < if self.boundary_pa < 1e5 { 2 } else { 1 }
            }
        }
    }

    pub fn density(
        &self,
        mix: hhe_mixture::Mixture,
        p: f64,
        t: f64,
        hhe: f64,
        work: &mut dry::Work,
        limit: usize,
    ) -> Result<f64, SolveError> {
        let scales = (*self.scales.get_or_init(|| self.prepare(work, limit)))?;
        mix.density_with_components(hhe, |m| {
            let index = hhe_mixture::SECONDARIES
                .iter()
                .position(|v| *v == m)
                .unwrap();
            effective_secondary::density_kg_m3(m, p, t)
                .map(|rho| rho * scales[index])
                .map_err(hhe_mixture::Error::Effective)
        })
        .map_err(SolveError::Mixture)
    }

    fn prepare(&self, work: &mut dry::Work, limit: usize) -> Result<[f64; 5], SolveError> {
        let mut scales = [1.0; 5];
        let Some((p, t)) = self.anchor else {
            return Ok(scales);
        };
        for (i, m) in hhe_mixture::SECONDARIES.into_iter().enumerate() {
            if self.gas.mass_kg(m) == 0.0 {
                continue;
            }
            let mut charge = || {
                if work.eos_evaluations >= limit {
                    return Err(SolveError::WorkBudgetExhausted(WorkLimit::EosEvaluations));
                }
                work.eos_evaluations += 1;
                Ok(())
            };
            charge()?;
            let molecular = molecular::density_kg_m3(m, p, t)
                .map_err(|e| SolveError::Mixture(hhe_mixture::Error::Component(e)))?;
            charge()?;
            let effective = effective_secondary::density_kg_m3(m, p, t)
                .map_err(|e| SolveError::Mixture(hhe_mixture::Error::Effective(e)))?;
            scales[i] = molecular / effective;
            if !scales[i].is_finite() || scales[i] <= 0.0 {
                return Err(SolveError::Mixture(hhe_mixture::Error::InvalidDensity));
            }
        }
        Ok(scales)
    }
}
