//! Typed commands for the debug console: `set_view table`,
//! `set_display option=orbits on=false`, or a `UiCommand` as JSON. Arguments
//! fill a command's fields in order, or by name; quote values with spaces, or
//! IDs that look like numbers.
use crate::actions::UiCommand;
use serde_json::{Map, Value};

/// A command's name, its fields in positional order, and what it does.
pub struct Usage {
    pub name: &'static str,
    pub fields: &'static [&'static str],
    pub about: &'static str,
}

/// Every `UiCommand`, by its serde name. The test below fails to compile
/// when a variant is added without an entry here.
pub const USAGE: [Usage; 14] = [
    Usage {
        name: "pick_centre",
        fields: &[],
        about: "Select the star nearest the view centre",
    },
    Usage {
        name: "pick_star",
        fields: &["id"],
        about: "Select a star by ID, or visit it if already selected",
    },
    Usage {
        name: "jump",
        fields: &["list"],
        about: "Open a list's constellation, by list ID",
    },
    Usage {
        name: "find_task",
        fields: &["list", "task"],
        about: "Fly to a task's page and select it",
    },
    Usage {
        name: "inspect",
        fields: &["planet"],
        about: "Select a planet in the visited system, by planet ID",
    },
    Usage {
        name: "show_page",
        fields: &["page"],
        about: "Show a page of the visited list",
    },
    Usage {
        name: "set_view",
        fields: &["view"],
        about: "orbit, table or constellation",
    },
    Usage {
        name: "frame_system",
        fields: &[],
        about: "Visit the selected star, or reframe the visited system",
    },
    Usage {
        name: "follow",
        fields: &[],
        about: "Follow the selected planet",
    },
    Usage {
        name: "set_paused",
        fields: &["paused"],
        about: "true or false",
    },
    Usage {
        name: "set_rate",
        fields: &["rate"],
        about: "Simulated seconds per real second",
    },
    Usage {
        name: "set_display",
        fields: &["option", "on"],
        about: "exaggerate, orbits, labels, markers, velocity_marker, realistic, galaxy_glow, physical or wireframe; true or false",
    },
    Usage {
        name: "set_fov",
        fields: &["degrees"],
        about: "Vertical field of view, 30 to 130",
    },
    Usage {
        name: "galaxy_reference",
        fields: &["view"],
        about: "inside, face_on or edge_on",
    },
];

/// `name <field> <field>`, as help shows it.
pub fn synopsis(usage: &Usage) -> String {
    usage
        .fields
        .iter()
        .fold(usage.name.to_string(), |line, field| {
            line + &format!(" <{field}>")
        })
}

pub fn parse(line: &str) -> Result<UiCommand, String> {
    let line = line.trim();
    if line.starts_with('{') {
        return serde_json::from_str(line).map_err(|err| err.to_string());
    }
    let words = split(line)?;
    let Some((name, args)) = words.split_first() else {
        return Err("Empty command".into());
    };
    let usage = USAGE
        .iter()
        .find(|u| u.name == name.value && name.field.is_none())
        .ok_or_else(|| format!("Unknown command `{}`. Type help for a list.", name.value))?;
    let mut object = Map::new();
    object.insert("type".into(), usage.name.into());
    let mut next = usage.fields.iter();
    for arg in args {
        let field = match &arg.field {
            Some(field) => usage
                .fields
                .iter()
                .find(|f| *f == field)
                .ok_or_else(|| format!("`{}` has no field `{field}`", usage.name))?,
            None => next
                .next()
                .ok_or_else(|| format!("Too many arguments. Usage: {}", synopsis(usage)))?,
        };
        // Numbers and booleans as such; anything else, or quoted, is text.
        let value = match serde_json::from_str(&arg.value) {
            Ok(value @ (Value::Number(_) | Value::Bool(_))) if !arg.quoted => value,
            _ => arg.value.as_str().into(),
        };
        object.insert((*field).into(), value);
    }
    serde_json::from_value(Value::Object(object))
        .map_err(|err| format!("{err}. Usage: {}", synopsis(usage)))
}

/// One argument: `value`, `"quoted value"` or `field=value`.
struct Word {
    field: Option<String>,
    value: String,
    quoted: bool,
}

/// Splits on whitespace, keeping double-quoted text together.
fn split(line: &str) -> Result<Vec<Word>, String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            return Ok(words);
        }
        let mut word = Word {
            field: None,
            value: String::new(),
            quoted: false,
        };
        let mut open = false;
        while let Some(c) = chars.next_if(|c| open || !c.is_whitespace()) {
            match c {
                '"' => {
                    open = !open;
                    word.quoted = true;
                }
                '=' if !word.quoted && word.field.is_none() => {
                    word.field = Some(std::mem::take(&mut word.value));
                }
                c => word.value.push(c),
            }
        }
        if open {
            return Err("Unclosed quote".into());
        }
        words.push(word);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{Display, GalaxyReference};
    use crate::views::View;

    #[test]
    fn every_command_has_usage() {
        // Exhaustive, so a new variant must be added here and to `USAGE`.
        let name = |command: &UiCommand| match command {
            UiCommand::PickCentre => "pick_centre",
            UiCommand::PickStar { .. } => "pick_star",
            UiCommand::Jump { .. } => "jump",
            UiCommand::FindTask { .. } => "find_task",
            UiCommand::Inspect { .. } => "inspect",
            UiCommand::ShowPage { .. } => "show_page",
            UiCommand::SetView { .. } => "set_view",
            UiCommand::FrameSystem => "frame_system",
            UiCommand::Follow => "follow",
            UiCommand::SetPaused { .. } => "set_paused",
            UiCommand::SetRate { .. } => "set_rate",
            UiCommand::SetDisplay { .. } => "set_display",
            UiCommand::SetFov { .. } => "set_fov",
            UiCommand::GalaxyReference { .. } => "galaxy_reference",
        };
        for usage in &USAGE {
            let args: Vec<_> = usage
                .fields
                .iter()
                .map(|field| match *field {
                    "page" => "1",
                    "rate" | "degrees" => "60",
                    "paused" | "on" => "true",
                    "option" => "orbits",
                    "view" if usage.name == "set_view" => "table",
                    "view" => "inside",
                    _ => "id",
                })
                .collect();
            let line = format!("{} {}", usage.name, args.join(" "));
            let command = parse(&line).unwrap_or_else(|err| panic!("{line}: {err}"));
            assert_eq!(name(&command), usage.name);
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
            parse("set_view constellation").unwrap(),
            UiCommand::SetView {
                view: View::Constellation
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
            parse("set_view colour=red")
                .unwrap_err()
                .contains("no field")
        );
        assert!(
            parse("set_view sideways")
                .unwrap_err()
                .contains("Usage: set_view <view>")
        );
        assert!(parse("jump \"open").unwrap_err().contains("Unclosed"));
    }
}
