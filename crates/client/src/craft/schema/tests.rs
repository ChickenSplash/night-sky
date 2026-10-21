use super::*;
use space_simulation::craft::warp;

fn order<'c>(catalogue: &'c Value, words: &str) -> &'c Value {
    catalogue["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["words"] == json!(words.split(' ').collect::<Vec<_>>()))
        .unwrap_or_else(|| panic!("no order `{words}`"))
}

fn field<'c>(order: &'c Value, name: &str) -> &'c Value {
    order["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("no field `{name}`"))
}

/// A valid example value for a field, as a client would send it.
fn example(field: &Value) -> Value {
    match field["kind"].as_str().unwrap() {
        "number" | "integer" => field.get("min").cloned().unwrap_or(json!(0.0)),
        "boolean" => json!(true),
        "text" => json!("sun/0"),
        "vector" => json!([0.0, 0.0, 1.0]),
        "choice" => field["options"][0]["name"].clone(),
        "variant" => {
            let last = field["options"].as_array().unwrap().last().unwrap();
            let mut value = json!({field["tag"].as_str().unwrap(): last["name"]});
            if !last["value"].is_null() {
                value[field["content"].as_str().unwrap()] = example(&last["value"]);
            }
            value
        }
        kind => panic!("unknown kind {kind}"),
    }
}

#[test]
fn every_catalogue_order_builds_an_order_the_simulation_accepts() {
    let catalogue = catalogue();
    let orders = catalogue["orders"].as_array().unwrap();
    assert!(orders.len() >= 17, "{}", orders.len());
    let groups: Vec<_> = GROUPS.iter().map(|g| g.0).collect();
    for entry in orders.iter().chain(catalogue["menus"].as_array().unwrap()) {
        let words = &entry["words"];
        assert!(entry["help"].is_string(), "{words} needs a doc comment");
        assert!(
            groups.contains(&entry["group"].as_str().unwrap_or("")),
            "{words} needs an x-group listed in GROUPS"
        );
    }
    for entry in orders {
        let mut built = entry["order"].clone();
        let at: Vec<String> = serde_json::from_value(entry["at"].clone()).unwrap();
        for field in entry["fields"].as_array().unwrap() {
            let key: Vec<String> = serde_json::from_value(field["key"].clone()).unwrap();
            let (last, parents) = key.split_last().unwrap();
            let parents: Vec<String> = at.iter().chain(parents).cloned().collect();
            place(&mut built, &parents)[last.as_str()] = example(field);
        }
        let parsed: Order = serde_json::from_value(built.clone())
            .unwrap_or_else(|e| panic!("{} built {built}: {e}", entry["words"]));
        parsed
            .validate()
            .unwrap_or_else(|e| panic!("{} built {built}: {e}", entry["words"]));
    }
}

#[test]
fn fields_carry_kinds_ranges_units_and_cli_shape() {
    let catalogue = catalogue();
    let throttle = field(order(&catalogue, "throttle"), "value");
    assert_eq!(
        (
            &throttle["kind"],
            &throttle["cli"],
            &throttle["min"],
            &throttle["max"]
        ),
        (
            &json!("number"),
            &json!("positional"),
            &json!(-1.0),
            &json!(1.0)
        )
    );
    let yaw = field(order(&catalogue, "rotate"), "yaw");
    assert_eq!(
        (&yaw["cli"], &yaw["default"]),
        (&json!("flag"), &json!(0.0))
    );
    let set = order(&catalogue, "warp set");
    assert_eq!(
        set["order"],
        json!({"action": "warp", "command": {"command": "set"}})
    );
    assert_eq!(set["at"], json!(["command"]));
    assert_eq!(field(set, "speed_ms")["max"], json!(warp::MAX_SPEED));
    let rate = field(order(&catalogue, "warp turn-rate"), "degrees_per_second");
    assert_eq!(
        (&rate["min"], &rate["max"], &rate["unit"]),
        (
            &json!(warp::MIN_TURN_RATE_DPS),
            &json!(warp::MAX_TURN_RATE_DPS),
            &json!("°/s")
        )
    );
    let matching = order(&catalogue, "warp match");
    assert_eq!(field(matching, "body")["cli"], "positional");
    assert_eq!(field(matching, "orbit")["kind"], "boolean");
    assert_eq!(field(matching, "orbit")["cli"], "flag");
    let gravity = field(order(&catalogue, "warp gravity"), "policy");
    assert_eq!(gravity["kind"], "choice");
    assert_eq!(gravity["options"][1]["name"], "suspended");
    let point = order(&catalogue, "point");
    let target = field(point, "target");
    assert_eq!(
        (&target["tag"], &target["content"]),
        (&json!("target"), &json!("value"))
    );
    let options = target["options"].as_array().unwrap();
    assert_eq!(options[0]["value"], Value::Null);
    assert_eq!(options.last().unwrap()["value"]["kind"], "vector");
    assert!(options.iter().all(|o| o["help"].is_string()));
    assert_eq!(field(point, "up")["required"], false);
    let angular = field(order(&catalogue, "configure"), "angular");
    assert_eq!(angular["key"], json!(["thrusters", "angular"]));
    assert_eq!(
        (&angular["cli"], &angular["required"]),
        (&json!("flag"), &json!(true))
    );
    let warp_menu = &catalogue["menus"][0];
    assert_eq!(warp_menu["words"], json!(["warp"]));
    assert!(
        warp_menu["about"]
            .as_str()
            .unwrap()
            .contains("steer the bubble")
    );
}

/// Every number a client shows or sends must say what it measures.
#[test]
fn every_number_declares_its_unit() {
    fn check(schema: &Value, path: &str, missing: &mut Vec<String>) {
        match schema {
            Value::Object(map) => {
                let numeric = |s: &Value| match &s["type"] {
                    Value::Array(types) => types.iter().any(|t| t == "number"),
                    kind => kind == "number",
                };
                if (numeric(schema) || numeric(&schema["items"])) && !map.contains_key("x-unit") {
                    missing.push(path.to_owned());
                }
                for (key, value) in map {
                    if key != "items" {
                        check(value, &format!("{path}/{key}"), missing);
                    }
                }
            }
            Value::Array(values) => {
                for (i, value) in values.iter().enumerate() {
                    check(value, &format!("{path}/{i}"), missing);
                }
            }
            _ => {}
        }
    }
    let mut missing = Vec::new();
    for schema in [
        schemars::schema_for!(Order),
        schemars::schema_for!(FleetReadout),
    ] {
        check(&serde_json::to_value(schema).unwrap(), "", &mut missing);
    }
    assert!(
        missing.is_empty(),
        "add x-unit (\"\" if none): {missing:#?}"
    );
}

/// `ns` reads this copy, so it works without the app and every change to
/// the controls shows up in review.
#[test]
fn committed_copy_is_current() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/craft-schema.json"
    );
    let text = serde_json::to_string_pretty(&catalogue()).unwrap() + "\n";
    if std::fs::read_to_string(path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(path, &text).unwrap();
        panic!("scripts/craft-schema.json was stale and has been rewritten; check and commit it");
    }
}
