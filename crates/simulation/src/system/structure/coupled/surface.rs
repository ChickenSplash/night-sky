//! Surface state on prescribed condensed geometry: the coupling's column side
//! (partition, greenhouse balance, frost, binding) without its radial solve.
//! Generation decides temperature, gas and blow-off here, synchronously; the
//! layered solve then keeps those decisions.
use super::*;

pub(crate) const MODEL_ID: &str = "synchronous_surface_decision_v1";
const MAX_ITERATIONS: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub temperature_k: f64,
    pub optical_depth: f64,
    pub gas: Inventory,
    pub frost: Inventory,
    /// Metal, rock, condensed water and protected traces under the frost.
    pub host: Inventory,
    pub atmosphere: Option<GasAtmosphere>,
    pub condensed_radius_m: f64,
    /// Top of the frost: where the column stands.
    pub surface_radius_m: f64,
}

/// `condensed_radius_m` maps a host inventory to its radius. Every other rule
/// is the coupled solve's own: same supply checks, partition, greenhouse
/// balance, deposit and column validity. `dense` alone lifts the density mask,
/// keeping the ideal column for gas the layered solve will refuse.
pub(crate) fn state(
    request: &Request,
    protected: &Inventory,
    condensed_radius_m: impl Fn(&Inventory) -> f64,
    dense: bool,
) -> Result<State, Error> {
    admit(request, protected)?;
    let exposed = Inventory::new(
        &Material::ALL.map(|m| (m, request.retained.mass_kg(m) - protected.mass_kg(m))),
    )
    .map_err(Error::Composition)?;
    let mut base = (
        request.retained.total_mass_kg(),
        condensed_radius_m(&request.retained),
    );
    let mut temperature = warming::temperatures(request, base, dense)?.0;
    let mut gas = column::partition(request, temperature, base.0, base.1)?;
    for _ in 0..MAX_ITERATIONS {
        let (inventory, deposit) = split(&exposed, &gas)?;
        let host = inventory
            .checked_add(protected)
            .map_err(Error::Composition)?;
        let radius = condensed_radius_m(&host);
        let top = deposit.top_radius_m(radius);
        if !radius.is_finite() || radius <= 0.0 {
            return Err(Error::InvalidInput("condensed radius"));
        }
        base = (host.total_mass_kg() + deposit.mass_kg(), top);
        let next_temperature = warming::temperatures(request, base, dense)?.0;
        let next_gas = column::partition(request, next_temperature, base.0, base.1)?;
        let residual = Material::ALL
            .iter()
            .map(|&m| {
                let (a, b) = (gas.mass_kg(m), next_gas.mass_kg(m));
                if a == b {
                    0.0
                } else {
                    (a - b).abs() / a.max(b)
                }
            })
            .fold(0.0_f64, f64::max);
        if residual <= ALLOCATION_RELATIVE_TOLERANCE
            && (next_temperature - temperature).abs()
                <= TEMPERATURE_RELATIVE_TOLERANCE * temperature
        {
            let gm = GRAVITATIONAL_CONSTANT * base.0;
            let atmosphere = column::atmosphere(&gas, temperature, gm, top)?;
            column::validate(&gas, atmosphere.as_ref(), gm, top, dense)?;
            return Ok(State {
                temperature_k: temperature,
                optical_depth: atmosphere.map_or(0.0, |a| greenhouse::optical_depth(&a)),
                gas,
                frost: *deposit.inventory(),
                host,
                atmosphere,
                condensed_radius_m: radius,
                surface_radius_m: top,
            });
        }
        temperature = next_temperature;
        gas = next_gas;
    }
    Err(Error::NonConvergence)
}
