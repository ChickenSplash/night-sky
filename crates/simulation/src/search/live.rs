//! Live objects, such as craft, are not generated from the galaxy's seeds,
//! so the engine never finds them. Whoever holds them (the client) matches
//! them against the same query and merges them into the engine's results.
use super::engine::{Facts, PC_PER_LY, SearchResult, all_pass, sort_value};
use super::query::{Query, Scope};
use super::registry::{Link, PROPERTIES, RELATIONS};
use crate::cells::parse_cell_star;
use crate::multiple::system_key;
use crate::population::tiered_bodies;

/// One live object as search sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveObject {
    /// As it is listed; `distance_ly` is filled in when matched, and
    /// `position_pc` is its system's.
    pub result: SearchResult,
    /// Its system's key, for `system=` scopes.
    pub system_key: String,
    pub altitude_km: f64,
    pub speed_kms: f64,
    /// The planet or star it is around, by ID.
    pub orbits: Option<String>,
}

/// A matched live object, with the value it sorts by (NaN for none).
#[derive(Clone, Debug, PartialEq)]
pub struct LiveHit {
    pub result: SearchResult,
    pub sort: f64,
}

/// The live objects a query from `origin_pc` matches. Text matches an
/// object's ID or, ignoring case, its name, in any scope.
pub fn matching(query: &Query, origin_pc: [f64; 3], objects: &[LiveObject]) -> Vec<LiveHit> {
    let Ok(plan) = query.plan() else {
        return Vec::new();
    };
    let text = query.text.trim();
    objects
        .iter()
        .filter_map(|object| {
            let result = &object.result;
            let distance_pc = super::engine::distance(result.position_pc, origin_pc);
            let in_scope = if !text.is_empty() {
                text == object_id(result) || text.eq_ignore_ascii_case(&result.name)
            } else {
                match &query.scope {
                    Scope::Within { radius_ly } => distance_pc <= radius_ly * PC_PER_LY,
                    Scope::System { id } => *id == object.system_key,
                }
            };
            let kind = result.kind.as_str();
            let facts = Facts {
                craft: Some([object.altitude_km, object.speed_kms]),
                ..Facts::default()
            };
            let related =
                plan.relations
                    .iter()
                    .all(|relation| match RELATIONS[relation.relation].link {
                        Link::Orbits => object.orbits.as_deref().is_some_and(|body| {
                            around(body, |kind, facts| {
                                relation.kinds.has(kind) && all_pass(&relation.filters, kind, facts)
                            })
                        }),
                        Link::System => false,
                    });
            (in_scope && plan.kinds.has(kind) && all_pass(&plan.filters, kind, facts) && related)
                .then(|| LiveHit {
                    result: SearchResult {
                        distance_ly: distance_pc / PC_PER_LY,
                        ..result.clone()
                    },
                    sort: sort_value(&plan, facts),
                })
        })
        .collect()
}

/// Whether the star or planet `id` passes `test`, given its kind and facts.
fn around(id: &str, test: impl Fn(&str, Facts) -> bool) -> bool {
    let key = system_key(id);
    let Some((cell, component, _)) = parse_cell_star(key) else {
        return false;
    };
    let bodies = tiered_bodies(key, component, cell.level());
    let base = Facts {
        stars: bodies.stars.len(),
        planets: Some(bodies.planets.len()),
        ..Facts::default()
    };
    if let Some(star) = bodies.stars.iter().find(|s| s.id == id) {
        return test(
            "star",
            Facts {
                star: Some(star),
                ..base
            },
        );
    }
    bodies
        .planets
        .iter()
        .find(|p| p.id() == id)
        .is_some_and(|planet| {
            test(
                "planet",
                Facts {
                    planet: Some((planet, bodies.mass(&planet.host))),
                    ..base
                },
            )
        })
}

fn object_id(result: &SearchResult) -> &str {
    use crate::objects::ObjectRef;
    match &result.object {
        ObjectRef::Star { id }
        | ObjectRef::Planet { id }
        | ObjectRef::Barycentre { id }
        | ObjectRef::List { id }
        | ObjectRef::Craft { id } => id,
        ObjectRef::Galaxy => "",
    }
}

/// The engine's results with live hits merged in, in the query's order,
/// up to its limit. Nearest first by default; sorted by a property, the
/// side that property belongs to comes first, the other after it by
/// distance (no property applies to both live and generated kinds).
pub fn merge(query: &Query, found: Vec<SearchResult>, mut live: Vec<LiveHit>) -> Vec<SearchResult> {
    if live.is_empty() {
        return found;
    }
    let by_distance = |a: &LiveHit, b: &LiveHit| {
        a.result
            .distance_ly
            .total_cmp(&b.result.distance_ly)
            .then_with(|| object_id(&a.result).cmp(object_id(&b.result)))
    };
    let sort = query.plan().ok().and_then(|plan| plan.sort);
    let live_first = sort.is_some_and(|(index, _)| {
        PROPERTIES[index]
            .kinds
            .iter()
            .any(|kind| live.iter().any(|hit| hit.result.kind == *kind))
    });
    let mut merged = Vec::with_capacity(found.len() + live.len());
    if let (true, Some((_, descending))) = (live_first, sort) {
        live.sort_by(|a, b| {
            a.sort
                .is_nan()
                .cmp(&b.sort.is_nan())
                .then_with(|| {
                    let order = a.sort.total_cmp(&b.sort);
                    if descending { order.reverse() } else { order }
                })
                .then_with(|| by_distance(a, b))
        });
        merged.extend(live.into_iter().map(|hit| hit.result));
        merged.extend(found);
    } else if sort.is_some() {
        live.sort_by(by_distance);
        merged.extend(found);
        merged.extend(live.into_iter().map(|hit| hit.result));
    } else {
        live.sort_by(by_distance);
        let mut live = live.into_iter().peekable();
        for result in found {
            while let Some(hit) = live.next_if(|hit| hit.result.distance_ly < result.distance_ly) {
                merged.push(hit.result);
            }
            merged.push(result);
        }
        merged.extend(live.map(|hit| hit.result));
    }
    merged.truncate(query.limit as usize);
    merged
}
