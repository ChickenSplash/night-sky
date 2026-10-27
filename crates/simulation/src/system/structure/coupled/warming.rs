//! Greenhouse surface temperature on a fixed partition geometry.
use super::*;

/// Relative scan step. Two balances closer together than this may be merged.
const SCAN_STEP: f64 = 1.005;

/// Column and water temperatures for this geometry.
pub(super) fn temperatures(
    request: &Request,
    base: (f64, f64),
) -> Result<(f64, Option<f64>), Error> {
    match request.temperature {
        Temperature::Prescribed { column_k, water_k } => Ok((column_k, water_k)),
        Temperature::Greenhouse { equilibrium_k } => {
            let t = surface_temperature(request, equilibrium_k, base.0, base.1)?;
            Ok((t, Some(t)))
        }
    }
}

/// Warmed temperature f(T) never falls as T rises: caps only rise, so gas only
/// evaporates. Starting from T_eq <= f(T_eq), the first crossing upwards is the
/// coolest stable balance reached by warming from equilibrium. Hotter balances
/// that a hot start (a runaway) would keep are not explored.
fn surface_temperature(
    request: &Request,
    equilibrium_k: f64,
    mass: f64,
    radius: f64,
) -> Result<f64, Error> {
    let gm = GRAVITATIONAL_CONSTANT * mass;
    let excess = |t: f64| -> Result<f64, Error> {
        let gas = column::partition(request, t, mass, radius)?;
        let tau = column::atmosphere(&gas, t, gm, radius)?
            .map_or(0.0, |air| greenhouse::optical_depth(&air));
        Ok(greenhouse::surface_temperature_k(equilibrium_k, tau) - t)
    };
    // A column already outside the domain unwarmed (a dense H/He column, say)
    // is reported as such, not as a runaway its broadening would drive.
    let gas = column::partition(request, equilibrium_k, mass, radius)?;
    let air = column::atmosphere(&gas, equilibrium_k, gm, radius)?;
    column::validate(&gas, air.as_ref(), gm, radius)?;
    if excess(equilibrium_k)? <= 0.0 {
        return Ok(equilibrium_k);
    }
    let mut low = equilibrium_k;
    let mut high = loop {
        let t = (low * SCAN_STEP).min(MAX_COLUMN_TEMPERATURE_K);
        match excess(t) {
            Ok(e) if e <= 0.0 => break t,
            Ok(_) if t == MAX_COLUMN_TEMPERATURE_K => return Err(Error::WarmingOutsideDomain),
            Ok(_) => low = t,
            Err(Error::GasMassOutsideDomain) => return Err(Error::WarmingOutsideDomain),
            Err(e) => return Err(e),
        }
    };
    // Full precision: the saturated water partition follows T steeply.
    loop {
        let mid = 0.5 * (low + high);
        if mid == low || mid == high {
            return Ok(high);
        }
        match excess(mid) {
            Ok(e) if e <= 0.0 => high = mid,
            Ok(_) => low = mid,
            Err(e) => return Err(e),
        }
    }
}
