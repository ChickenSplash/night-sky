//! Simulation-owned navigation bounds. Optical extent is not a drag boundary.
//! Bodies are conservatively enclosed in spheres; future ring geometry can
//! supply an additional enclosing bound without changing route steering.
use super::{
    atmosphere::{ColumnValidity, GasAtmosphere, optics::boundary::OpticalColumn},
    shape::{Spheroid, planet_shape, star_shape},
};
use crate::{EARTH_SOLAR, SUN_GM, YEAR_SECONDS};
use space_shared::{Planet, Star};
use std::f64::consts::TAU;

pub const KEEP_CLEAR: f64 = 1.5;

/// Replaceable engineering policy, not a physical edge of an atmosphere.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// m/(Cd A). Smaller values experience more drag. Reference ship only:
    /// craft do not yet have mass/area/Cd or a drag force implementation.
    pub ballistic_coefficient_kg_m2: f64,
    pub loss_m: f64,
    pub duration_s: f64,
    pub hull_radius_m: f64,
    pub safety_m: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            ballistic_coefficient_kg_m2: 10.0,
            loss_m: 1.0,
            duration_s: YEAR_SECONDS,
            hull_radius_m: crate::craft::SHIP_LENGTH_M,
            safety_m: 1_000.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clearance {
    /// Hard navigation floor from the centre, including hull and safety.
    pub exclusion_m: f64,
    /// Comfortable route wrapping radius; may be relaxed, unlike exclusion.
    pub preferred_m: f64,
    /// Modelled atmosphere height, independent of the optical cutoff.
    pub atmosphere_height_m: f64,
    /// Clearance extrapolates constant gravity past the thin-column regime.
    pub extended_column: bool,
}

impl Policy {
    fn validate(self) -> Result<(), String> {
        if [
            self.ballistic_coefficient_kg_m2,
            self.loss_m,
            self.duration_s,
            self.hull_radius_m,
            self.safety_m,
        ]
        .into_iter()
        .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err("Invalid navigation clearance policy".into());
        }
        Ok(())
    }

    pub fn bound(
        self,
        shape: Spheroid,
        atmosphere_height_m: f64,
        extended_column: bool,
    ) -> Result<Clearance, String> {
        self.validate()?;
        if !shape.equatorial.is_finite()
            || !shape.polar.is_finite()
            || shape.polar <= 0.0
            || shape.equatorial < shape.polar
            || !atmosphere_height_m.is_finite()
            || atmosphere_height_m < 0.0
        {
            return Err("Invalid navigation clearance geometry".into());
        }
        let exclusion_m =
            shape.equatorial + atmosphere_height_m + self.hull_radius_m + self.safety_m;
        // Wrap comfortably outside the atmosphere as well as the ground. A
        // thick column must not collapse the preferred/hard gap to just 1 km.
        let preferred_m = (KEEP_CLEAR * (shape.equatorial + atmosphere_height_m))
            .max(exclusion_m + self.safety_m);
        if !preferred_m.is_finite() {
            return Err("Navigation clearance overflow".into());
        }
        Ok(Clearance {
            exclusion_m,
            preferred_m,
            atmosphere_height_m,
            extended_column,
        })
    }

    pub fn star(self, star: &Star) -> Result<Clearance, String> {
        if !star.radius_solar.is_finite()
            || star.radius_solar <= 0.0
            || !star.mass_solar.is_finite()
            || star.mass_solar <= 0.0
            || !star.spin.period_hours.is_finite()
            || star.spin.period_hours < 0.0
        {
            return Err("Invalid stellar navigation geometry".into());
        }
        self.bound(star_shape(star), 0.0, false)
    }

    pub fn planet(self, planet: &Planet) -> Result<Clearance, String> {
        self.validate()?;
        super::physical::validate(planet).map_err(str::to_owned)?;
        let geometry = super::surface::geometry(planet).map_err(str::to_owned)?;
        let gm = planet.mass_earth * EARTH_SOLAR * SUN_GM;
        let radius = planet.radius_km * 1_000.0;
        if !gm.is_finite()
            || gm <= 0.0
            || !radius.is_finite()
            || radius <= 0.0
            || !planet.spin.period_hours.is_finite()
            || planet.spin.period_hours < 0.0
        {
            return Err("Invalid planetary navigation geometry".into());
        }
        let environment = planet
            .environment
            .as_ref()
            .ok_or("Atmosphere state is unknown")?;
        let shape = planet_shape(planet);
        let Some(data) = environment.atmosphere.as_ref() else {
            return self.bound(shape, 0.0, false);
        };
        let gas = GasAtmosphere::try_from(data).map_err(|e| format!("Invalid atmosphere: {e}"))?;
        if planet.initial_result.as_ref().is_some_and(|result| {
            result.record.layers.iter().any(|layer| {
                layer
                    .phase_allocations
                    .iter()
                    .any(|p| p.condensate_kg > 0.0)
            })
        }) {
            return Err("Suspended-condensate drag clearance is unresolved".into());
        }
        let reference_gm = if let Some(result) = &planet.initial_result {
            let reference = OpticalColumn::reference_from_result(result, &gas)
                .map_err(|e| format!("Invalid atmosphere reference: {e}"))?;
            reference.gravity_m_s2 * reference.radius_m.powi(2)
        } else {
            gm
        };
        let column = gas
            .column(reference_gm, radius)
            .map_err(|e| format!("Invalid atmosphere column: {e}"))?;
        let omega = if planet.spin.period_hours == 0.0 {
            0.0
        } else {
            TAU / (planet.spin.period_hours * 3_600.0)
        };
        // At least the visible shell, and four scale heights so exponential
        // falloff dominates the polynomial speed/radius factors monotonically.
        let mut high = column.extent_m.max(4.0 * column.scale_height_m);
        let log_loss = |h: f64| {
            let r = shape.equatorial + h;
            let vc = (gm / r).sqrt();
            // Retrograde circular orbit is worst relative to co-rotating air.
            // |dr/dt| <= rho/B * sqrt(GM*r) * (1 + omega*r/vc)^2.
            let factor = self.duration_s / self.ballistic_coefficient_kg_m2
                * (gm * r).sqrt()
                * (1.0 + omega * r / vc).powi(2);
            let profile = column.profile;
            // Beyond the sonic radius the adopted hydrostatic guard is exceeded.
            // Preserve a bounded navigation-only exponential extrapolation,
            // matched in value/slope, and flag the resulting clearance below.
            // This is not exported as a physical atmospheric profile.
            let limit_height = profile.sonic_height_m();
            let gas_drop = if profile.spherical && h > limit_height {
                let local_height = column.scale_height_m * (1.0 + limit_height / radius).powi(2);
                profile.log_relative_density(limit_height) - (h - limit_height) / local_height
            } else {
                profile.log_relative_density(h)
            };
            let gas_log = column.density_kg_m3.ln() + gas_drop;
            let aerosol_log = gas.aerosol().map_or(f64::NEG_INFINITY, |a| {
                a.reference_density_kg_m3().ln() - h / a.scale_height_m()
            });
            // Count aerosol mass conservatively as drag-coupled material.
            let max = gas_log.max(aerosol_log);
            max + ((gas_log - max).exp() + (aerosol_log - max).exp()).ln() + factor.ln()
        };
        let limit = self.loss_m.ln();
        for _ in 0..128 {
            if log_loss(high) <= limit {
                break;
            }
            high *= 2.0;
            if !high.is_finite() {
                return Err("Atmospheric clearance overflow".into());
            }
        }
        if !log_loss(high).is_finite() || log_loss(high) > limit {
            return Err("Cannot resolve maintenance-free atmospheric clearance".into());
        }
        let mut low = column.extent_m;
        for _ in 0..80 {
            let mid = (low + high) * 0.5;
            if log_loss(mid) > limit {
                low = mid;
            } else {
                high = mid;
            }
        }
        let mut bound = self.bound(
            shape,
            high,
            column.validity == ColumnValidity::Extended
                || !column.profile.spherical
                || high > column.profile.sonic_height_m()
                || high / radius > super::atmosphere::MAX_THIN_EXTENT_RATIO,
        )?;
        // The reference is not ground, but routes must enclose both it and
        // the actual condensed top, even if a future model references inside it.
        let condensed = geometry.condensed_shape(shape);
        bound.exclusion_m = bound
            .exclusion_m
            .max(condensed.equatorial + self.hull_radius_m + self.safety_m);
        bound.preferred_m = bound.preferred_m.max(KEEP_CLEAR * condensed.equatorial);
        Ok(bound)
    }
}

#[cfg(test)]
mod tests;
