//! Blow-off of exchangeable gas the static column cannot bind.
use super::*;
use crate::system::atmosphere::Gas;

pub const MODEL_ID: &str = "unbound_column_lightest_first_blowoff_v1";

#[derive(Clone, Debug, PartialEq)]
pub struct Escaping {
    pub structure: CoupledStructure,
    /// Exchangeable supply removed, never part of the returned structure.
    pub escaped: Inventory,
}

/// A column reaching past its sonic radius flows outward, and exposed supply
/// keeps refilling it, so over geological time that species is lost. Strip the
/// lightest exchangeable gas entirely and re-solve, until the column binds.
/// Each attempt carries the full controls; at most one per gas species.
pub fn solve_escaping(request: &Request, controls: Controls) -> Result<Escaping, Error> {
    solve_protected(request, &Inventory::default(), controls)
}

pub(crate) fn solve_protected(
    request: &Request,
    protected: &Inventory,
    controls: Controls,
) -> Result<Escaping, Error> {
    let mut request = *request;
    let mut escaped = Inventory::default();
    let mut order = Gas::ALL;
    order.sort_by(|a, b| a.molecular_mass_u().total_cmp(&b.molecular_mass_u()));
    let mut order = order.into_iter().map(Material::from_gas);
    loop {
        match super::solve_protected(&request, protected, controls) {
            Err(Error::WeakBindingUnsupported) => {
                let lightest = order
                    .find(|&m| request.exchangeable.mass_kg(m) > 0.0)
                    .ok_or(Error::WeakBindingUnsupported)?;
                let lost = request.exchangeable.mass_kg(lightest);
                let without = |i: &Inventory, mass: f64| {
                    Inventory::new(&Material::ALL.map(|m| {
                        (
                            m,
                            if m == lightest {
                                i.mass_kg(m) - mass
                            } else {
                                i.mass_kg(m)
                            },
                        )
                    }))
                    .map_err(Error::Composition)
                };
                request.retained = without(&request.retained, lost)?;
                request.exchangeable = without(&request.exchangeable, lost)?;
                escaped = escaped
                    .checked_add(&Inventory::new(&[(lightest, lost)]).map_err(Error::Composition)?)
                    .map_err(Error::Composition)?;
            }
            result => {
                return result.map(|structure| Escaping { structure, escaped });
            }
        }
    }
}
