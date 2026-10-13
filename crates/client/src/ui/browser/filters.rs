//! The object browser's view of a `Query`: its chips, and the edits the
//! filter picker makes. Everything is read from the search registry, so a
//! new kind or property needs no code here.
use space_simulation::search::query::Relation;
use space_simulation::search::registry::{
    self, KINDS, KindSet, PROPERTIES, Property, RELATIONS, Values,
};
use space_simulation::search::{Filter, Query, Test};

/// Where a filter lives: on the results themselves, or on what one
/// relationship (an index in `RELATIONS`) leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Own,
    Related(usize),
}

impl Slot {
    /// Kinds a filter in this slot can apply to.
    pub fn kinds(self, query: &Query) -> KindSet {
        match self {
            Slot::Own => query.kinds_left().unwrap_or_else(|_| KindSet::available()),
            Slot::Related(relation) => {
                let targets = RELATIONS[relation].target_kinds();
                match relation_of(query, relation) {
                    Some(r) if !r.kinds.is_empty() => targets.and(kind_set(&r.kinds)),
                    _ => targets,
                }
            }
        }
    }
}

/// One chip: what it shows and what it stands for.
#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    /// The query's kinds or family.
    Kinds,
    /// A property filter, by `PROPERTIES` index.
    Filter { slot: Slot, property: usize },
    /// The kinds a relationship leads to.
    RelatedKinds(usize),
}

fn kind_set(ids: &[String]) -> KindSet {
    let mut set = KindSet::default();
    for index in ids.iter().filter_map(|id| registry::kind_index(id)) {
        set.0 |= KindSet::of(index).0;
    }
    set
}

fn kind_label(id: &str) -> &str {
    registry::kind_index(id).map_or(id, |i| KINDS[i].label)
}

fn relation_of(query: &Query, relation: usize) -> Option<&Relation> {
    query
        .relations
        .iter()
        .find(|r| r.relation == RELATIONS[relation].id)
}

fn filters(query: &Query, slot: Slot) -> &[Filter] {
    match slot {
        Slot::Own => &query.filters,
        Slot::Related(relation) => relation_of(query, relation).map_or(&[], |r| &r.filters),
    }
}

/// The filters of `slot`, making its relationship if it has none yet.
fn filters_mut(query: &mut Query, slot: Slot) -> &mut Vec<Filter> {
    match slot {
        Slot::Own => &mut query.filters,
        Slot::Related(relation) => &mut related_mut(query, relation).filters,
    }
}

fn related_mut(query: &mut Query, relation: usize) -> &mut Relation {
    let id = RELATIONS[relation].id;
    let at = match query.relations.iter().position(|r| r.relation == id) {
        Some(at) => at,
        None => {
            query.relations.push(Relation {
                relation: id.into(),
                kinds: Vec::new(),
                filters: Vec::new(),
            });
            query.relations.len() - 1
        }
    };
    &mut query.relations[at]
}

/// The filter on `property` in `slot`, if there is one.
pub fn filter(query: &Query, slot: Slot, property: usize) -> Option<&Filter> {
    filters(query, slot)
        .iter()
        .find(|f| f.property == PROPERTIES[property].id)
}

/// Every chip in the order the query's text gives them.
pub fn chips(query: &Query) -> Vec<(Chip, String)> {
    let mut chips = Vec::new();
    if let Some(family) = &query.family {
        let label = registry::family(family).map_or(family.as_str(), |f| f.label);
        chips.push((Chip::Kinds, format!("Type: {label}")));
    } else if !query.kinds.is_empty() {
        let labels: Vec<_> = query.kinds.iter().map(|k| kind_label(k)).collect();
        chips.push((Chip::Kinds, format!("Type: {}", labels.join(" or "))));
    }
    fn add(chips: &mut Vec<(Chip, String)>, slot: Slot, prefix: &str, list: &[Filter]) {
        for filter in list {
            if let Some(property) = registry::property_index(&filter.property) {
                let label = PROPERTIES[property].label;
                let text = match filter.test {
                    // A flag reads as a statement.
                    Test::Is(true) => format!("{prefix}{label}"),
                    Test::Is(false) => format!("{prefix}{label}: no"),
                    _ => format!(
                        "{prefix}{label}: {}",
                        value_text(&PROPERTIES[property], &filter.test)
                    ),
                };
                chips.push((Chip::Filter { slot, property }, text));
            }
        }
    }
    add(&mut chips, Slot::Own, "", &query.filters);
    for relation in &query.relations {
        let Some(index) = registry::relation_index(&relation.relation) else {
            continue;
        };
        let label = RELATIONS[index].label;
        if !relation.kinds.is_empty() {
            let kinds: Vec<_> = relation.kinds.iter().map(|k| kind_label(k)).collect();
            chips.push((
                Chip::RelatedKinds(index),
                format!("{label}: {}", kinds.join(" or ")),
            ));
        }
        add(
            &mut chips,
            Slot::Related(index),
            &format!("{label} · "),
            &relation.filters,
        );
    }
    chips
}

/// A number without trailing zeros, grouped in thousands when whole.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        let sign = if n < 0.0 { "-" } else { "" };
        format!("{sign}{}", crate::format::grouped(n.abs() as usize))
    } else {
        format!("{n}")
    }
}

/// A property's unit in words: the interface font has no ☉ or ⊕.
pub fn unit_text(property: &Property) -> Option<&'static str> {
    property.unit.map(|unit| match unit {
        "M☉" => "solar masses",
        "R☉" => "solar radii",
        "L☉" => "times the Sun's light",
        "M⊕" => "Earth masses",
        unit => unit,
    })
}

/// A filter's test as a person reads it: `Red dwarf (M) or K`, `1,000–5,000
/// km`, `at least 2`, `yes`.
pub fn value_text(property: &Property, test: &Test) -> String {
    let unit = unit_text(property).map_or(String::new(), |u| format!(" {u}"));
    match test {
        Test::OneOf(ids) => {
            let options = match property.values {
                Values::Choice { options } => options,
                _ => &[],
            };
            let labels: Vec<_> = ids
                .iter()
                .map(|id| {
                    options
                        .iter()
                        .find(|(option, _)| option == id)
                        .map_or(id.as_str(), |(_, label)| label)
                })
                .collect();
            labels.join(" or ")
        }
        Test::Range {
            min: Some(min),
            max: Some(max),
        } if min == max => format!("{}{unit}", number_text(*min)),
        Test::Range {
            min: Some(min),
            max: Some(max),
        } => format!("{}–{}{unit}", number_text(*min), number_text(*max)),
        Test::Range {
            min: Some(min),
            max: None,
        } => format!("at least {}{unit}", number_text(*min)),
        Test::Range {
            min: None,
            max: Some(max),
        } => format!("at most {}{unit}", number_text(*max)),
        Test::Range {
            min: None,
            max: None,
        } => "any".into(),
        Test::Is(true) => "yes".into(),
        Test::Is(false) => "no".into(),
    }
}

/// Reads a range end: blank is open; thousands separators are allowed.
pub fn parse_end(text: &str, integer: bool) -> Result<Option<f64>, String> {
    let text = text.trim().replace([',', '_', ' '], "");
    if text.is_empty() {
        return Ok(None);
    }
    let n: f64 = text
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite())
        .ok_or_else(|| format!("`{text}` is not a number"))?;
    if integer && n.fract() != 0.0 {
        return Err(format!("{text} is not a whole number"));
    }
    Ok(Some(n))
}

/// Sets or replaces the filter on `filter.property` in `slot`.
pub fn set_filter(query: &mut Query, slot: Slot, filter: Filter) {
    let list = filters_mut(query, slot);
    match list.iter_mut().find(|f| f.property == filter.property) {
        Some(existing) => *existing = filter,
        None => list.push(filter),
    }
}

/// Removes what a chip stands for, and any relationship left empty.
pub fn remove(query: &mut Query, chip: &Chip) {
    match chip {
        Chip::Kinds => {
            query.kinds.clear();
            query.family = None;
        }
        Chip::Filter { slot, property } => {
            filters_mut(query, *slot).retain(|f| f.property != PROPERTIES[*property].id);
        }
        Chip::RelatedKinds(relation) => related_mut(query, *relation).kinds.clear(),
    }
    query
        .relations
        .retain(|r| !r.kinds.is_empty() || !r.filters.is_empty());
}

/// Results of one kind only. Filters and relationships that cannot apply to
/// it are dropped rather than left to refuse the query.
pub fn set_kind(query: &mut Query, kind: &str) {
    query.kinds = vec![kind.to_owned()];
    query.family = None;
    query.filters.retain(|f| {
        registry::property_index(&f.property).is_some_and(|i| PROPERTIES[i].kinds.contains(&kind))
    });
    query.relations.retain(|r| {
        registry::relation_index(&r.relation).is_some_and(|i| RELATIONS[i].subjects.contains(&kind))
    });
}

/// What a relationship leads to must be of `kind`; its filters that cannot
/// apply to it are dropped.
pub fn set_related_kind(query: &mut Query, relation: usize, kind: &str) {
    let related = related_mut(query, relation);
    related.kinds = vec![kind.to_owned()];
    related.filters.retain(|f| {
        registry::property_index(&f.property).is_some_and(|i| PROPERTIES[i].kinds.contains(&kind))
    });
}

/// Properties a new filter in `slot` can use: those of the kinds left, not
/// already filtered.
pub fn offered_properties(query: &Query, slot: Slot) -> Vec<usize> {
    let kinds = slot.kinds(query);
    (0..PROPERTIES.len())
        .filter(|&i| !PROPERTIES[i].applies().and(kinds).is_empty())
        .filter(|&i| filter(query, slot, i).is_none())
        .collect()
}

/// Relationships the results can have.
pub fn offered_relations(query: &Query) -> Vec<usize> {
    let kinds = Slot::Own.kinds(query);
    (0..RELATIONS.len())
        .filter(|&i| !RELATIONS[i].subject_kinds().and(kinds).is_empty())
        .collect()
}
