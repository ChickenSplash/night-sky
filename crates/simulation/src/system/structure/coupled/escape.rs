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
    let stripped = strip(request, |request| {
        profile::time(profile::Stage::Coupled, || {
            super::solve_protected(request, protected, controls)
        })
    });
    stripped.result.map(|structure| Escaping {
        structure,
        escaped: stripped.escaped,
    })
}

pub(crate) struct Stripped<T> {
    /// The first outcome other than an unbound column.
    pub result: Result<T, Error>,
    /// The request that outcome belongs to, without the stripped supply.
    pub request: Request,
    pub escaped: Inventory,
}

/// The lightest-first loop itself, for any solve that reports an unbound
/// column: the layered coupling, or generation's synchronous surface state.
pub(crate) fn strip<T>(
    request: &Request,
    mut solve: impl FnMut(&Request) -> Result<T, Error>,
) -> Stripped<T> {
    let mut request = *request;
    let mut escaped = Inventory::default();
    let mut order = Gas::ALL;
    order.sort_by(|a, b| a.molecular_mass_u().total_cmp(&b.molecular_mass_u()));
    let mut order = order.into_iter().map(Material::from_gas);
    loop {
        let result = solve(&request);
        if !matches!(result, Err(Error::WeakBindingUnsupported)) {
            return Stripped {
                result,
                request,
                escaped,
            };
        }
        let Some(lightest) = order.find(|&m| request.exchangeable.mass_kg(m) > 0.0) else {
            return Stripped {
                result,
                request,
                escaped,
            };
        };
        let lost = request.exchangeable.mass_kg(lightest);
        let without = |i: &Inventory| {
            Inventory::new(&Material::ALL.map(|m| {
                (
                    m,
                    if m == lightest {
                        i.mass_kg(m) - lost
                    } else {
                        i.mass_kg(m)
                    },
                )
            }))
        };
        let next = without(&request.retained).and_then(|retained| {
            Ok((
                retained,
                without(&request.exchangeable)?,
                escaped.checked_add(&Inventory::new(&[(lightest, lost)])?)?,
            ))
        });
        match next {
            Ok((retained, exchangeable, lost)) => {
                request.retained = retained;
                request.exchangeable = exchangeable;
                escaped = lost;
            }
            Err(e) => {
                return Stripped {
                    result: Err(Error::Composition(e)),
                    request,
                    escaped,
                };
            }
        }
    }
}
