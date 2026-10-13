//! What to search for, shared by every front end, and its one text syntax:
//! `kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=50`.
use super::registry::{self, KINDS, KindSet, PROPERTIES, RELATIONS, Values};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 1_000;
pub const DEFAULT_RADIUS_LY: f64 = 50.0;
pub const MAX_RADIUS_LY: f64 = 1_000.0;

/// Where a search is centred is not part of the query: the same query can be
/// sent from anywhere (see `SearchRequest`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    #[serde(default = "version")]
    pub version: u32,
    /// An exact catalogue designation or ID; needs no scope.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Kinds by ID; empty, and no family, means any available kind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// A family by ID, in place of kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// All must match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
    /// All must hold, each with its own filters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub sort: Sort,
    #[serde(default = "limit")]
    pub limit: u32,
}

fn version() -> u32 {
    VERSION
}

fn limit() -> u32 {
    DEFAULT_LIMIT
}

impl Default for Query {
    fn default() -> Self {
        Self {
            version: VERSION,
            text: String::new(),
            kinds: Vec::new(),
            family: None,
            filters: Vec::new(),
            relations: Vec::new(),
            scope: Scope::default(),
            sort: Sort::Distance,
            limit: DEFAULT_LIMIT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub property: String,
    #[serde(flatten)]
    pub test: Test,
}

/// Holds when some object the relationship leads to is of one of `kinds`
/// (any it can lead to, if empty) and passes every filter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Registry relation ID, such as `orbits`.
    pub relation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<Filter>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    /// Inclusive; either end may be open.
    Range {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// Is one of these choices.
    OneOf(Vec<String>),
    Is(bool),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Around the request's origin.
    Within { radius_ly: f64 },
    /// One system, by its key.
    System { id: String },
}

impl Default for Scope {
    fn default() -> Self {
        Self::Within {
            radius_ly: DEFAULT_RADIUS_LY,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Nearest first, ties by ID: the only order a radius search can stop
    /// early in.
    #[default]
    Distance,
    /// By a property's value; `System` scope only.
    Property { id: String, descending: bool },
}

/// A checked query, with kinds and properties resolved.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    /// Kinds that can match: the chosen ones less those a filter excludes.
    pub kinds: KindSet,
    /// Property index and test.
    pub filters: Vec<(usize, Test)>,
    pub relations: Vec<RelationPlan>,
    /// Property index and descending.
    pub sort: Option<(usize, bool)>,
}

#[derive(Clone, Debug)]
pub(crate) struct RelationPlan {
    /// Index in `RELATIONS`.
    pub relation: usize,
    /// Kinds the related object can be.
    pub kinds: KindSet,
    pub filters: Vec<(usize, Test)>,
}

/// Kinds by ID, each generated, as a set.
fn kind_set(ids: &[String]) -> Result<KindSet, String> {
    let mut set = KindSet::default();
    for id in ids {
        let index = registry::kind_index(id).ok_or_else(|| format!("unknown kind `{id}`"))?;
        if !KINDS[index].available() {
            return Err(format!("{} are not generated yet", KINDS[index].label));
        }
        set.0 |= KindSet::of(index).0;
    }
    Ok(set)
}

/// Checks filters, narrowing `kinds` to those every filter applies to.
/// `prefix` names a relationship in refusals, such as `orbits.`.
fn plan_filters(
    mut kinds: KindSet,
    given: &[Filter],
    prefix: &str,
) -> Result<(KindSet, Vec<(usize, Test)>), String> {
    let mut filters = Vec::new();
    for filter in given {
        let index = registry::property_index(&filter.property)
            .ok_or_else(|| unknown_property(&filter.property))?;
        if filters.iter().any(|&(i, _)| i == index) {
            return Err(format!("`{prefix}{}` is given twice", filter.property));
        }
        let property = &PROPERTIES[index];
        check_test(property, &filter.test)?;
        let narrowed = kinds.and(property.applies());
        if narrowed.is_empty() {
            let chosen: Vec<_> = kinds.ids().collect();
            return Err(format!(
                "`{prefix}{}` applies to {}, not {}",
                property.id,
                property.kinds.join(", "),
                chosen.join(", ")
            ));
        }
        kinds = narrowed;
        filters.push((index, filter.test.clone()));
    }
    Ok((kinds, filters))
}

impl Query {
    /// Semantic checks, with a reason for each refusal.
    pub fn check(&self) -> Result<(), String> {
        self.plan().map(|_| ())
    }

    pub(crate) fn plan(&self) -> Result<Plan, String> {
        if self.version != VERSION {
            return Err(format!(
                "query version {} is not supported (expected {VERSION})",
                self.version
            ));
        }
        if !(1..=MAX_LIMIT).contains(&self.limit) {
            return Err(format!("limit must be 1 to {MAX_LIMIT}"));
        }
        match &self.scope {
            Scope::Within { radius_ly } => {
                if !(radius_ly.is_finite() && *radius_ly > 0.0 && *radius_ly <= MAX_RADIUS_LY) {
                    return Err(format!(
                        "within_ly must be above 0 and at most {MAX_RADIUS_LY}"
                    ));
                }
            }
            Scope::System { id } => {
                if crate::cells::parse_cell_star(id).is_none() {
                    return Err(format!("`{id}` is not a system ID such as g1abc.2.34"));
                }
            }
        }
        let kinds = if let Some(family) = &self.family {
            if !self.kinds.is_empty() {
                return Err("give kinds or a family, not both".into());
            }
            registry::family(family).ok_or_else(|| format!("unknown family `{family}`"))?;
            KindSet::matching(|k| k.family == family.as_str() && k.available())
        } else if self.kinds.is_empty() {
            KindSet::available()
        } else {
            kind_set(&self.kinds)?
        };
        if kinds.is_empty() {
            return Err("nothing of that family is generated yet".into());
        }
        let (mut kinds, filters) = plan_filters(kinds, &self.filters, "")?;
        let mut relations: Vec<RelationPlan> = Vec::new();
        for given in &self.relations {
            let index = registry::relation_index(&given.relation)
                .ok_or_else(|| unknown_relation(&given.relation))?;
            let relation = &RELATIONS[index];
            let prefix = format!("{}.", relation.id);
            if relations.iter().any(|r| r.relation == index) {
                return Err(format!("`{prefix}` is given twice"));
            }
            if given.kinds.is_empty() && given.filters.is_empty() {
                return Err(format!("`{prefix}` needs a kind or a filter"));
            }
            let narrowed = kinds.and(relation.subject_kinds());
            if narrowed.is_empty() {
                let chosen: Vec<_> = kinds.ids().collect();
                return Err(format!(
                    "`{prefix}` applies to {}, not {}",
                    relation.subjects.join(", "),
                    chosen.join(", ")
                ));
            }
            kinds = narrowed;
            let mut targets = relation.target_kinds();
            if !given.kinds.is_empty() {
                let chosen = kind_set(&given.kinds)?;
                if let Some(id) = chosen.ids().find(|id| !targets.has(id)) {
                    return Err(format!(
                        "`{prefix}kind`: {} leads to {}, not {id}",
                        relation.id,
                        relation.targets.join(", ")
                    ));
                }
                targets = chosen;
            }
            let (targets, filters) = plan_filters(targets, &given.filters, &prefix)?;
            relations.push(RelationPlan {
                relation: index,
                kinds: targets,
                filters,
            });
        }
        let sort = match &self.sort {
            Sort::Distance => None,
            Sort::Property { id, descending } => {
                if !matches!(self.scope, Scope::System { .. }) {
                    return Err(
                        "radius searches sort by distance only; sort by a property within one system"
                            .into(),
                    );
                }
                let index = registry::property_index(id).ok_or_else(|| unknown_property(id))?;
                if kinds.and(PROPERTIES[index].applies()).is_empty() {
                    return Err(format!(
                        "cannot sort by `{id}`: it applies to none of these kinds"
                    ));
                }
                Some((index, *descending))
            }
        };
        Ok(Plan {
            kinds,
            filters,
            relations,
            sort,
        })
    }

    /// The query as `ns search` text, which `parse` turns back into it exactly.
    pub fn to_text(&self) -> String {
        let mut parts = Vec::new();
        if !self.text.is_empty() {
            parts.push(self.text.clone());
        }
        if !self.kinds.is_empty() {
            parts.push(format!("kind={}", self.kinds.join(",")));
        }
        if let Some(family) = &self.family {
            parts.push(format!("family={family}"));
        }
        parts.extend(self.filters.iter().map(|f| filter_text("", f)));
        for relation in &self.relations {
            let prefix = format!("{}.", relation.relation);
            if !relation.kinds.is_empty() {
                parts.push(format!("{prefix}kind={}", relation.kinds.join(",")));
            }
            parts.extend(relation.filters.iter().map(|f| filter_text(&prefix, f)));
        }
        parts.push(match &self.scope {
            Scope::Within { radius_ly } => format!("within_ly={radius_ly}"),
            Scope::System { id } => format!("system={id}"),
        });
        if let Sort::Property { id, descending } = &self.sort {
            parts.push(format!("sort={}{id}", if *descending { "-" } else { "" }));
        }
        if self.limit != DEFAULT_LIMIT {
            parts.push(format!("limit={}", self.limit));
        }
        parts.join(" ")
    }

    /// Reads `to_text`'s syntax. Words without `=` form the designation.
    /// Only the syntax is checked here; `check` does the rest.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut query = Query::default();
        let mut words = Vec::new();
        let mut seen = Vec::new();
        for token in text.split_whitespace() {
            let Some((key, value)) = token.split_once('=') else {
                words.push(token);
                continue;
            };
            if seen.contains(&key) {
                return Err(format!("`{key}` is given twice"));
            }
            seen.push(key);
            if value.is_empty() {
                return Err(format!("`{key}` needs a value"));
            }
            match key {
                "kind" => query.kinds = value.split(',').map(str::to_owned).collect(),
                "family" => query.family = Some(value.to_owned()),
                "within_ly" => {
                    query.scope = Scope::Within {
                        radius_ly: parse_number(key, value)?,
                    };
                }
                "system" => query.scope = Scope::System { id: value.into() },
                "limit" => {
                    query.limit = value
                        .parse()
                        .map_err(|_| format!("limit must be a whole number, not `{value}`"))?;
                }
                "sort" => {
                    query.sort = match value {
                        "distance" => Sort::Distance,
                        _ => {
                            let (descending, id) = match value.strip_prefix('-') {
                                Some(id) => (true, id),
                                None => (false, value),
                            };
                            Sort::Property {
                                id: id.into(),
                                descending,
                            }
                        }
                    };
                }
                _ => match key.split_once('.') {
                    Some((relation, key)) => {
                        registry::relation_index(relation)
                            .ok_or_else(|| unknown_relation(relation))?;
                        let at = match query.relations.iter().position(|r| r.relation == relation) {
                            Some(at) => at,
                            None => {
                                query.relations.push(Relation {
                                    relation: relation.into(),
                                    kinds: Vec::new(),
                                    filters: Vec::new(),
                                });
                                query.relations.len() - 1
                            }
                        };
                        let related = &mut query.relations[at];
                        if key == "kind" {
                            related.kinds = value.split(',').map(str::to_owned).collect();
                        } else {
                            related.filters.push(parse_filter(key, value)?);
                        }
                    }
                    None => query.filters.push(parse_filter(key, value)?),
                },
            }
        }
        if seen.contains(&"within_ly") && seen.contains(&"system") {
            return Err("give within_ly or system, not both".into());
        }
        query.text = words.join(" ");
        Ok(query)
    }
}

fn unknown_property(id: &str) -> String {
    format!("unknown property `{id}`; see `ns catalogue`")
}

fn unknown_relation(id: &str) -> String {
    let all: Vec<_> = RELATIONS.iter().map(|r| format!("{}.", r.id)).collect();
    format!(
        "unknown relationship `{id}.`; relationships are {}",
        all.join(", ")
    )
}

fn filter_text(prefix: &str, filter: &Filter) -> String {
    let value = match &filter.test {
        Test::Range { min, max } if min.is_some() && min == max => number(min),
        Test::Range { min, max } => format!("{}..{}", number(min), number(max)),
        Test::OneOf(values) => values.join(","),
        Test::Is(true) => "yes".into(),
        Test::Is(false) => "no".into(),
    };
    format!("{prefix}{}={value}", filter.property)
}

fn number(n: &Option<f64>) -> String {
    n.map_or_else(String::new, |n| n.to_string())
}

fn parse_number(key: &str, value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("`{key}` needs a number, not `{value}`"))
}

fn parse_filter(key: &str, value: &str) -> Result<Filter, String> {
    let index = registry::property_index(key).ok_or_else(|| unknown_property(key))?;
    let test = match PROPERTIES[index].values {
        Values::Range { .. } => match value.split_once("..") {
            Some((min, max)) => {
                let end = |s: &str| (!s.is_empty()).then(|| parse_number(key, s)).transpose();
                Test::Range {
                    min: end(min)?,
                    max: end(max)?,
                }
            }
            None => {
                let n = parse_number(key, value)?;
                Test::Range {
                    min: Some(n),
                    max: Some(n),
                }
            }
        },
        Values::Choice { .. } => Test::OneOf(value.split(',').map(str::to_owned).collect()),
        Values::Flag => match value {
            "yes" | "true" => Test::Is(true),
            "no" | "false" => Test::Is(false),
            _ => return Err(format!("`{key}` is yes or no, not `{value}`")),
        },
    };
    Ok(Filter {
        property: key.into(),
        test,
    })
}

fn check_test(property: &registry::Property, test: &Test) -> Result<(), String> {
    let id = property.id;
    match (property.values, test) {
        (Values::Range { .. }, Test::Range { min, max }) => {
            if min.is_none() && max.is_none() {
                return Err(format!("`{id}` needs at least one end"));
            }
            if [min, max].iter().any(|n| n.is_some_and(|n| !n.is_finite())) {
                return Err(format!("`{id}` needs finite numbers"));
            }
            if let (Some(min), Some(max)) = (min, max)
                && min > max
            {
                return Err(format!("`{id}`: minimum {min} is above maximum {max}"));
            }
            Ok(())
        }
        (Values::Choice { options }, Test::OneOf(values)) => {
            if values.is_empty() {
                return Err(format!("`{id}` needs at least one choice"));
            }
            for value in values {
                if !options.iter().any(|(o, _)| o == value) {
                    let all: Vec<_> = options.iter().map(|(o, _)| *o).collect();
                    return Err(format!(
                        "`{id}` is one of {}, not `{value}`",
                        all.join(", ")
                    ));
                }
            }
            Ok(())
        }
        (Values::Flag, Test::Is(_)) => Ok(()),
        (Values::Range { .. }, _) => Err(format!("`{id}` needs a range such as 1..5")),
        (Values::Choice { .. }, _) => Err(format!("`{id}` needs a set of choices")),
        (Values::Flag, _) => Err(format!("`{id}` is yes or no")),
    }
}
