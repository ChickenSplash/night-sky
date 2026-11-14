//! Greenhouse surface temperature on a fixed partition geometry.
use super::*;

/// Relative scan step. Two balances closer together than this may be merged.
const SCAN_STEP: f64 = 1.005;
/// A supply-limited column thins slightly as it warms, so f can fall: by at
/// most 0.07 K per K in the 399-system probe. Halving the step allows 1 K per K.
const JUMP_FRACTION: f64 = 0.5;
// Keep root error well below the outer coupling's temperature/allocation tests.
const ROOT_RELATIVE_TOLERANCE: f64 = TEMPERATURE_RELATIVE_TOLERANCE * 0.001;

/// Column and water temperatures for this geometry.
pub(super) fn temperatures(
    request: &Request,
    base: (f64, f64),
) -> Result<(f64, Option<f64>), Error> {
    match request.temperature {
        Temperature::Prescribed { column_k, water_k } => Ok((column_k, water_k)),
        Temperature::Greenhouse { equilibrium_k } => {
            let t = profile::time(profile::Stage::Warming, || {
                surface_temperature(request, equilibrium_k, base.0, base.1)
            })?;
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
    let mut low_excess = excess(equilibrium_k)?;
    if low_excess <= 0.0 {
        return Ok(equilibrium_k);
    }
    let mut low = equilibrium_k;
    let (high, high_excess) = loop {
        // While f falls no faster than T rises, no balance lies below the
        // midpoint of low and f(low). Step there when it is beyond the scan step.
        let t = (low * SCAN_STEP)
            .max(low + JUMP_FRACTION * low_excess)
            .min(MAX_COLUMN_TEMPERATURE_K);
        match excess(t) {
            Ok(e) if e <= 0.0 => break (t, e),
            Ok(_) if t == MAX_COLUMN_TEMPERATURE_K => return Err(Error::WarmingOutsideDomain),
            Ok(e) => {
                low = t;
                low_excess = e;
            }
            Err(Error::GasMassOutsideDomain) => return Err(Error::WarmingOutsideDomain),
            Err(e) => return Err(e),
        }
    };
    refine(low, high, low_excess, high_excess, excess)
}

/// Bracket-preserving false position with the Illinois weighting: an endpoint
/// kept twice running has its excess halved, so both ends close in. A central
/// bisection step bounds progress whenever two steps fail to halve the bracket
/// (for example near an evaporation seam).
fn refine(
    mut low: f64,
    mut high: f64,
    mut low_excess: f64,
    mut high_excess: f64,
    excess: impl Fn(f64) -> Result<f64, Error>,
) -> Result<f64, Error> {
    // Weighted copies steer the interpolation only; signs stay those evaluated.
    let (mut low_weight, mut high_weight) = (low_excess, high_excess);
    let mut kept_low = None;
    let mut checkpoint = (0_u8, high - low);
    loop {
        let width = high - low;
        if width <= ROOT_RELATIVE_TOLERANCE * low {
            return Ok(high);
        }
        let interpolated = low + width * (low_weight / (low_weight - high_weight));
        checkpoint.0 += 1;
        let stalled = checkpoint.0 > 2;
        if stalled || width <= 0.5 * checkpoint.1 {
            checkpoint = (0, width);
        }
        let mid = if !stalled && interpolated > low && interpolated < high {
            interpolated
        } else {
            0.5 * (low + high)
        };
        if mid == low || mid == high {
            return Ok(high);
        }
        match excess(mid) {
            Ok(0.0) => return Ok(mid),
            Ok(e) if e < 0.0 => {
                high = mid;
                high_excess = e;
                high_weight = e;
                if kept_low == Some(true) {
                    low_weight *= 0.5;
                } else {
                    low_weight = low_excess;
                }
                kept_low = Some(true);
            }
            Ok(e) => {
                low = mid;
                low_excess = e;
                low_weight = e;
                if kept_low == Some(false) {
                    high_weight *= 0.5;
                } else {
                    high_weight = high_excess;
                }
                kept_low = Some(false);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn interpolated_refinement_keeps_the_bracket_and_matches_bisection() {
        for power in [1, 3, 9] {
            let excess = |t: f64| 0.37_f64.powi(power) - (t - 300.0).powi(power);
            let (mut low, mut high) = (300.0, 301.0);
            loop {
                let mid = 0.5 * (low + high);
                if mid == low || mid == high {
                    break;
                }
                if excess(mid) <= 0.0 {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            let root = refine(300.0, 301.0, excess(300.0), excess(301.0), |t| {
                assert!((300.0..=301.0).contains(&t));
                Ok(excess(t))
            })
            .unwrap();
            assert!(excess(root) <= 0.0);
            assert!((root - high).abs() <= ROOT_RELATIVE_TOLERANCE * high);
        }
    }

    #[test]
    fn interpolation_avoids_full_precision_bisection_for_a_linear_balance() {
        let calls = Cell::new(0);
        let root = refine(300.0, 301.0, 0.25, -0.75, |t| {
            calls.set(calls.get() + 1);
            Ok(300.25 - t)
        })
        .unwrap();
        assert_eq!(root, 300.25);
        assert_eq!(calls.get(), 1);
        assert_eq!(
            refine(300.0, 301.0, 0.25, -0.75, |_| Err(
                Error::DenseGasUnsupported
            )),
            Err(Error::DenseGasUnsupported)
        );
    }
}
