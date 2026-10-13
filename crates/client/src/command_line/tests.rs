use super::*;
use crate::actions::{Display, GalaxyReference};
use crate::presentation::PresentationMode;

#[test]
fn removed_readable_sizes_option_is_rejected() {
    assert!(parse("set_display exaggerate true").is_err());
    assert!(parse(r#"{"type":"set_display","option":"exaggerate","on":true}"#).is_err());
}

/// Every `UiCommand` name, as serde lists them when it meets an unknown one.
fn variants() -> Vec<String> {
    let err = serde_json::from_str::<UiCommand>(r#"{"type":"?"}"#).unwrap_err();
    let err = err.to_string();
    let (_, list) = err.split_once("expected one of").unwrap();
    list.split('`')
        .skip(1)
        .step_by(2)
        .map(String::from)
        .collect()
}

#[test]
fn every_command_has_usage() {
    let variants = variants();
    assert!(variants.len() >= USAGE.len(), "{variants:?}");
    let missing: Vec<_> = variants
        .iter()
        .filter(|v| !USAGE.iter().any(|u| u.name == *v))
        .collect();
    let unknown: Vec<_> = USAGE
        .iter()
        .map(|u| u.name)
        .filter(|name| !variants.iter().any(|v| v == name))
        .collect();
    assert!(
        missing.is_empty() && unknown.is_empty(),
        "command_line::USAGE is out of step with UiCommand. \
         Add a USAGE entry for: {missing:?}. Remove or rename: {unknown:?}."
    );
}

#[test]
fn every_usage_parses() {
    for usage in &USAGE {
        if usage.name == "inspect_object" {
            parse(r#"{"type":"inspect_object","inspection":{"source":"location","object":{"kind":"galaxy"}}}"#).expect("nested inspection uses the documented JSON command route");
            continue;
        }
        let args: Vec<_> = usage
            .fields
            .iter()
            .map(|field| match *field {
                "page" => "1",
                "rate" | "degrees" | "speed" => "60",
                "magnitude" => "5.5",
                "stops" => "-2",
                "amount" => "0.5",
                "paused" | "on" => "true",
                "option" => "orbits",
                "switch" => "bloom",
                "presentation_mode" => "table",
                "view" => "inside",
                "threads" => "4",
                "work_cap_millions" => "100",
                "query" => "kind=planet within_ly=20",
                _ => "id",
            })
            .collect();
        let line = format!("{} {}", usage.name, args.join(" "));
        if let Err(err) = parse(&line) {
            panic!(
                "USAGE entry `{}` does not match UiCommand's fields: {line}: {err}",
                usage.name
            );
        }
    }
}

#[test]
fn arguments_fill_fields_in_order_or_by_name() {
    assert_eq!(
        parse("set_display orbits false").unwrap(),
        UiCommand::SetDisplay {
            option: Display::Orbits,
            on: false
        }
    );
    assert_eq!(
        parse("  set_display on=true option=galaxy_glow ").unwrap(),
        UiCommand::SetDisplay {
            option: Display::GalaxyGlow,
            on: true
        }
    );
    assert_eq!(
        parse("set_presentation_mode constellation").unwrap(),
        UiCommand::SetPresentationMode {
            presentation_mode: PresentationMode::Constellation
        }
    );
    assert_eq!(
        parse("set_presentation_mode presentation_mode=simulation").unwrap(),
        UiCommand::SetPresentationMode {
            presentation_mode: PresentationMode::Simulation
        }
    );
    assert_eq!(
        parse("galaxy_reference face_on").unwrap(),
        UiCommand::GalaxyReference {
            view: GalaxyReference::FaceOn
        }
    );
    assert_eq!(
        parse("set_fov 72.5").unwrap(),
        UiCommand::SetFov { degrees: 72.5 }
    );
    assert_eq!(
        parse("set_magnitude 5.5").unwrap(),
        UiCommand::SetMagnitude { magnitude: 5.5 }
    );
}

#[test]
fn quotes_keep_text_and_json_still_works() {
    assert_eq!(
        parse(r#"find_task list="123" task="a b=c""#).unwrap(),
        UiCommand::FindTask {
            list: "123".into(),
            task: "a b=c".into()
        }
    );
    assert_eq!(
        parse(r#"{"type":"pick_star","id":"g1.2.3"}"#).unwrap(),
        UiCommand::PickStar {
            id: "g1.2.3".into()
        }
    );
}

#[test]
fn mistakes_explain_themselves() {
    assert!(parse("").unwrap_err().contains("Empty"));
    assert!(
        parse("warp 9")
            .unwrap_err()
            .contains("Unknown command `warp`")
    );
    assert!(parse("follow now").unwrap_err().contains("Too many"));
    assert!(
        parse("set_presentation_mode colour=red")
            .unwrap_err()
            .contains("no field")
    );
    assert!(
        parse("set_presentation_mode sideways")
            .unwrap_err()
            .contains("Usage: set_presentation_mode <presentation_mode>")
    );
    assert!(parse("jump \"open").unwrap_err().contains("Unclosed"));
}

#[test]
fn out_of_range_values_are_refused_not_clamped() {
    for (line, message) in [
        ("set_fov 500", "degrees must be 30 to 130, not 500"),
        ("set_fov 29.9", "degrees must be 30 to 130"),
        (
            "set_magnitude 10.6",
            "magnitude must be 3 to 10.5, not 10.6",
        ),
        ("set_exposure 17", "stops must be -64 to 16, not 17"),
        ("set_compression -0.1", "amount must be 0 to 1, not -0.1"),
        ("set_rate 0", "rate must be 1 to"),
        ("set_move_speed 0.5", "speed must be 1 to"),
        (
            r#"{"type":"set_fov","degrees":131}"#,
            "degrees must be 30 to 130",
        ),
    ] {
        let err = parse(line).unwrap_err();
        assert!(err.contains(message), "{line}: {err}");
    }
    for line in [
        "set_fov 30",
        "set_fov 130",
        "set_magnitude 8.5",
        "set_magnitude 9",
        "set_magnitude 10.5",
        "set_exposure -64",
        "set_compression 1",
        "set_rate 1",
        "set_move_speed 1",
    ] {
        assert!(parse(line).is_ok(), "{line}");
    }
}

#[test]
fn search_takes_the_rest_of_the_line_and_explains_refusals() {
    let query = "kind=planet orbits.star_class=m within_ly=50";
    for line in [
        format!("search {query}"),
        format!("search \"{query}\""),
        format!(r#"{{"type":"search","query":"{query}"}}"#),
    ] {
        assert_eq!(
            parse(&line).unwrap(),
            UiCommand::Search {
                query: query.into()
            },
            "{line}"
        );
    }
    assert!(
        parse("search colour=red")
            .unwrap_err()
            .contains("unknown property `colour`")
    );
    assert_eq!(
        parse("set_search work_cap_millions=5").unwrap(),
        UiCommand::SetSearch {
            threads: None,
            work_cap_millions: Some(5.0)
        }
    );
    assert!(parse("set_search").unwrap_err().contains("Give threads"));
    assert!(
        parse("set_search threads=0")
            .unwrap_err()
            .contains("threads must be 1 to 64")
    );
}
