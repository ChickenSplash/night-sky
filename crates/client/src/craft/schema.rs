//! The ship controls catalogue: every order, derived from the simulation's
//! own types and flattened for clients that do not read JSON Schema. `ns`
//! and `ns-tui` build their ship commands from it, so a new order or field
//! needs no client change. See `docs/client/developer-api.md`.
use super::readout::{CraftReadout, FleetReadout};
use serde_json::{Map, Value, json};
use space_simulation::craft::Order;

/// Order groups in display order: id, label and a note for the whole group.
const GROUPS: [(&str, &str, &str); 3] = [
    (
        "sublight",
        "Sublight drives",
        "Refused while warp is engaged.",
    ),
    (
        "turning",
        "Turning",
        "Turning thrusters, or the warp bubble while warp is engaged.",
    ),
    ("warp", "Warp", ""),
];

/// State display groups in order: id and label.
const STATE_GROUPS: [(&str, &str); 6] = [
    ("ship", "Ship"),
    ("position", "Position"),
    ("attitude", "Attitude"),
    ("drives", "Drives"),
    ("warp", "Warp"),
    ("prediction", "Prediction"),
];

/// Name endings that only repeat the unit, left out of display labels. A
/// schema `title` replaces the label altogether.
const UNIT_SUFFIXES: [&str; 10] = [
    "_xyzw", "_kms", "_ms", "_km", "_m", "_deg", "_dps", "_rads", "_pc", "_seconds",
];

/// The catalogue `GET /craft/schema` serves and `scripts/craft-schema.json`
/// keeps a copy of.
pub fn catalogue() -> Value {
    let schema = serde_json::to_value(schemars::schema_for!(Order)).expect("order schema");
    let mut walk = Walk {
        defs: &schema["$defs"],
        menus: Vec::new(),
        orders: Vec::new(),
    };
    walk.variants(&schema, &[], &json!({}), &[], None);
    let craft = serde_json::to_value(schemars::schema_for!(CraftReadout)).expect("state schema");
    let fleet = serde_json::to_value(schemars::schema_for!(FleetReadout)).expect("fleet schema");
    json!({
        "version": 1,
        "command": "steer_craft",
        "groups": GROUPS.map(|(id, label, note)| json!({"id": id, "label": label, "note": note})),
        "common": [
            {"key": ["id"], "name": "id", "kind": "text", "cli": "flag", "required": false,
             "help": "Craft ID (default: the selected or only ship)."},
            {"key": ["tick"], "name": "tick", "kind": "integer", "cli": "flag", "required": false,
             "help": "World step to apply from (default: the next unsimulated step)."},
        ],
        "menus": walk.menus,
        "orders": walk.orders,
        "state": {
            "groups": STATE_GROUPS.map(|(id, label)| json!({"id": id, "label": label})),
            "craft": state_fields(&craft, None),
            "fleet": state_fields(&fleet, Some("fleet")),
        },
    })
}

/// Display fields of a readout: nested structs are flattened, and each
/// top-level field's `x-group` applies to everything inside it.
fn state_fields(schema: &Value, group: Option<&str>) -> Vec<Value> {
    let walk = Walk {
        defs: &schema["$defs"],
        menus: Vec::new(),
        orders: Vec::new(),
    };
    let mut fields = Vec::new();
    for (name, inner, _) in properties(schema) {
        if name == "craft" && group == Some("fleet") {
            continue;
        }
        let group = inner["x-group"].as_str().or(group);
        walk.state_field(vec![name.to_owned()], inner, group, &mut fields);
    }
    fields
}

struct Walk<'a> {
    defs: &'a Value,
    menus: Vec<Value>,
    orders: Vec<Value>,
}

impl<'a> Walk<'a> {
    fn resolve(&self, schema: &'a Value) -> &'a Value {
        match schema["$ref"].as_str() {
            Some(path) => &self.defs[path.trim_start_matches("#/$defs/")],
            None => schema,
        }
    }

    /// Whether a schema is an enum of tagged objects, like `WarpOrder`.
    fn tagged(&self, schema: &'a Value) -> bool {
        self.resolve(schema)["oneOf"]
            .as_array()
            .is_some_and(|variants| variants.iter().all(|v| tag(v).is_some()))
    }

    /// One order per variant, except that a variant holding only a tagged
    /// enum, like `warp`, becomes a menu of its variants.
    fn variants(
        &mut self,
        schema: &'a Value,
        words: &[String],
        skeleton: &Value,
        at: &[String],
        group: Option<&'a str>,
    ) {
        for variant in self.resolve(schema)["oneOf"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let (key, name) = tag(variant).expect("orders are tagged");
            let group = variant["x-group"].as_str().or(group);
            let words: Vec<String> = words.iter().cloned().chain([kebab(name)]).collect();
            let mut skeleton = skeleton.clone();
            place(&mut skeleton, at)[key] = name.into();
            let (help, about) = describe(variant);
            let fields: Vec<_> = properties(variant)
                .into_iter()
                .filter(|(field, ..)| *field != key)
                .collect();
            if let [(field, inner, true)] = fields[..]
                && self.tagged(inner)
            {
                self.menus.push(json!({
                    "words": words, "group": group, "help": help, "about": about,
                }));
                place(&mut skeleton, at)[field] = json!({});
                let at: Vec<String> = at.iter().cloned().chain([field.to_owned()]).collect();
                self.variants(inner, &words, &skeleton, &at, group);
                continue;
            }
            let fields: Vec<Value> = fields
                .into_iter()
                .flat_map(|(field, inner, required)| {
                    self.field(vec![field.to_owned()], inner, required, true)
                })
                .collect();
            self.orders.push(json!({
                "words": words, "group": group, "help": help, "about": about,
                "order": skeleton, "at": at, "fields": fields,
            }));
        }
    }

    /// A field, or a nested struct's fields as flags. Required top-level
    /// fields are positional on the CLI, in declaration order.
    fn field(&self, key: Vec<String>, schema: &'a Value, required: bool, top: bool) -> Vec<Value> {
        let (inner, nullable) = optional(schema);
        let inner = self.resolve(inner);
        let required = required && !nullable;
        if inner["type"] == "object" && inner["properties"].is_object() {
            return properties(inner)
                .into_iter()
                .flat_map(|(name, sub, needed)| {
                    let key = key.iter().cloned().chain([name.to_owned()]).collect();
                    self.field(key, sub, required && needed, false)
                })
                .collect();
        }
        let mut entry = Map::new();
        entry.insert("key".into(), json!(key));
        entry.insert("name".into(), json!(key.last()));
        let kind = self.kind(inner, &mut entry, &key);
        entry.insert("kind".into(), kind.into());
        let positional = top && required && kind != "boolean";
        entry.insert(
            "cli".into(),
            if positional { "positional" } else { "flag" }.into(),
        );
        entry.insert("required".into(), required.into());
        let (help, about) = describe(if schema["description"].is_string() {
            schema
        } else {
            inner
        });
        for (name, value) in [
            ("help", help),
            ("about", about),
            ("unit", schema["x-unit"].clone()),
        ] {
            if !value.is_null() {
                entry.insert(name.into(), value);
            }
        }
        // A null default still means the field may be left out.
        if let Some(default) = schema.get("default") {
            entry.insert("default".into(), default.clone());
        }
        vec![Value::Object(entry)]
    }

    fn state_field(
        &self,
        key: Vec<String>,
        schema: &'a Value,
        group: Option<&str>,
        out: &mut Vec<Value>,
    ) {
        let (inner, _) = optional(schema);
        let inner = self.resolve(inner);
        if inner["type"] == "object" && inner["properties"].is_object() {
            for (name, sub, _) in properties(inner) {
                let key = key.iter().cloned().chain([name.to_owned()]).collect();
                self.state_field(key, sub, group, out);
            }
            return;
        }
        let kind = if self.tagged(inner) {
            "variant"
        } else if inner["items"]["type"] == "object" || inner["items"]["$ref"].is_string() {
            "list"
        } else {
            let mut scratch = Map::new();
            self.kind(inner, &mut scratch, &key)
        };
        let skip = usize::from(key.len() > 1 && Some(key[0].as_str()) == group);
        let label = schema["title"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                key[skip..]
                    .iter()
                    .map(|part| {
                        let part = UNIT_SUFFIXES
                            .iter()
                            .find_map(|suffix| part.strip_suffix(suffix))
                            .unwrap_or(part);
                        part.replace('_', " ")
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            });
        let mut entry = json!({"key": key, "group": group, "label": label, "kind": kind});
        let (help, _) = describe(if schema["description"].is_string() {
            schema
        } else {
            inner
        });
        for (name, value) in [("help", help), ("unit", schema["x-unit"].clone())] {
            if !value.is_null() {
                entry[name] = value;
            }
        }
        out.push(entry);
    }

    /// The field's kind, adding its range or options to `entry`.
    fn kind(
        &self,
        schema: &'a Value,
        entry: &mut Map<String, Value>,
        key: &[String],
    ) -> &'static str {
        if self.tagged(schema) {
            let variants = schema["oneOf"].as_array().into_iter().flatten();
            let mut content = None;
            let options: Vec<Value> = variants
                .map(|variant| {
                    let (tag_key, name) = tag(variant).expect("tagged");
                    entry.insert("tag".into(), tag_key.into());
                    let value = properties(variant)
                        .into_iter()
                        .find(|(field, ..)| *field != tag_key)
                        .map(|(field, inner, _)| {
                            content = Some(field);
                            let mut value = Map::new();
                            let kind = self.kind(self.resolve(inner), &mut value, key);
                            value.insert("kind".into(), kind.into());
                            Value::Object(value)
                        });
                    let (help, _) = describe(variant);
                    json!({"name": name, "help": help, "value": value})
                })
                .collect();
            entry.insert("content".into(), json!(content));
            entry.insert("options".into(), options.into());
            return "variant";
        }
        let choices: Option<Vec<Value>> = match (&schema["enum"], &schema["oneOf"]) {
            (Value::Array(names), _) => Some(names.iter().map(|n| json!({"name": n})).collect()),
            (_, Value::Array(variants)) => variants
                .iter()
                .map(|v| {
                    let name = v["const"].as_str()?;
                    Some(json!({"name": name, "help": describe(v).0}))
                })
                .collect(),
            _ => None,
        };
        if let Some(options) = choices {
            entry.insert("options".into(), options.into());
            return "choice";
        }
        for bound in ["minimum", "maximum"] {
            if !schema[bound].is_null() {
                entry.insert(bound[..3].into(), schema[bound].clone());
            }
        }
        let kind = match &schema["type"] {
            Value::Array(types) => types.iter().find(|t| *t != "null"),
            kind => Some(kind),
        };
        match kind.and_then(Value::as_str) {
            Some("number") => "number",
            Some("integer") => "integer",
            Some("boolean") => "boolean",
            Some("string") => "text",
            Some("array")
                if schema["items"]["type"] == "number"
                    && schema["minItems"].is_u64()
                    && schema["minItems"] == schema["maxItems"] =>
            {
                entry.insert("length".into(), schema["minItems"].clone());
                "vector"
            }
            _ => panic!("no catalogue kind for field {key:?}: {schema}"),
        }
    }
}

/// A tagged variant's tag key and value, such as `("action", "throttle")`.
fn tag(variant: &Value) -> Option<(&str, &str)> {
    if variant["type"] != "object" {
        return None;
    }
    variant["properties"]
        .as_object()?
        .iter()
        .find_map(|(key, value)| Some((key.as_str(), value["const"].as_str()?)))
}

/// Properties with whether each is required: required ones first, in
/// declaration order, then the rest. Schemas do not keep the rest's order.
fn properties(schema: &Value) -> Vec<(&str, &Value, bool)> {
    let Some(all) = schema["properties"].as_object() else {
        return Vec::new();
    };
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut fields: Vec<_> = required
        .iter()
        .filter_map(|name| all.get_key_value(*name))
        .map(|(name, value)| (name.as_str(), value, true))
        .collect();
    fields.extend(
        all.iter()
            .filter(|(name, _)| !required.contains(&name.as_str()))
            .map(|(name, value)| (name.as_str(), value, false)),
    );
    fields
}

/// The schema without `Option`'s null, and whether it allowed null.
fn optional(schema: &Value) -> (&Value, bool) {
    if let Some(any) = schema["anyOf"].as_array()
        && let [first, second] = &any[..]
    {
        if second["type"] == "null" {
            return (first, true);
        }
        if first["type"] == "null" {
            return (second, true);
        }
    }
    let nullable = schema["type"]
        .as_array()
        .is_some_and(|types| types.iter().any(|t| t == "null"));
    (schema, nullable)
}

/// A doc comment's first paragraph as help, and the rest, on single lines.
fn describe(schema: &Value) -> (Value, Value) {
    let Some(text) = schema["description"].as_str() else {
        return (Value::Null, Value::Null);
    };
    let mut paragraphs = text
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "));
    let help = paragraphs.next().unwrap_or_default();
    let about: Vec<String> = paragraphs.collect();
    (
        help.into(),
        if about.is_empty() {
            Value::Null
        } else {
            about.join("\n\n").into()
        },
    )
}

fn kebab(name: &str) -> String {
    name.replace('_', "-")
}

/// The object at `path` inside `value`, made if missing.
fn place<'v>(value: &'v mut Value, path: &[String]) -> &'v mut Value {
    path.iter()
        .fold(value, |value, key| &mut value[key.as_str()])
}

#[cfg(test)]
mod tests;
