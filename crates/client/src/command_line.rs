//! Typed commands: `set_view table`, `set_display option=orbits on=false`, or
//! a `UiCommand` as JSON. Arguments fill a command's fields in order, or by
//! name; quote values with spaces, or IDs that look like numbers.
//!
//! Lines come from the debug window's terminal tab or, natively, from the
//! terminal that started the app, as `Typed` messages. `run` handles both the
//! same way and logs its output under `TARGET`.
use crate::actions::{self, UiCommand};
use bevy::prelude::*;
use serde_json::{Map, Value};

/// The log target of typed commands and their output.
pub const TARGET: &str = "console";

#[derive(Message)]
pub struct Typed {
    pub line: String,
    pub source: Source,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// The debug window's terminal tab.
    Window,
    /// Standard input, natively.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Stdin,
}

pub struct CommandLinePlugin;

impl Plugin for CommandLinePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Typed>()
            .add_systems(Update, run.before(actions::apply));
        #[cfg(not(target_arch = "wasm32"))]
        app.insert_resource(Stdin(std::sync::Mutex::new(read_stdin())))
            .add_systems(Update, forward_stdin.before(run));
    }
}

/// Runs typed lines as commands. The echo, help and errors are logged, so they
/// reach the terminal tab, the log file, bug reports and the launching terminal.
fn run(mut typed: MessageReader<Typed>, mut commands: MessageWriter<UiCommand>) {
    for Typed { line, source } in typed.read() {
        let line = line.trim();
        info!(target: TARGET, "> {line}");
        match line {
            "help" => {
                info!(target: TARGET, "help · clear · or a UiCommand as JSON");
                for usage in &USAGE {
                    info!(target: TARGET, "{:<27} {}", synopsis(usage), usage.about);
                }
            }
            // The window clears its own terminal tab.
            "clear" if *source == Source::Stdin => {
                info!(target: TARGET, "clear empties the debug window's terminal tab; Ctrl+L clears this terminal");
            }
            line => match parse(line) {
                Ok(command) => {
                    commands.write(command);
                }
                Err(err) => warn!(target: TARGET, "{err}"),
            },
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct Stdin(std::sync::Mutex<std::sync::mpsc::Receiver<String>>);

/// Reads standard input on its own thread until it closes. A detached launch
/// has none, and the thread ends at once.
#[cfg(not(target_arch = "wasm32"))]
fn read_stdin() -> std::sync::mpsc::Receiver<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("stdin commands".into())
        .spawn(move || {
            for line in std::io::stdin().lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
    if let Err(err) = spawned {
        warn!("Cannot read commands from the terminal: {err}");
    }
    receiver
}

#[cfg(not(target_arch = "wasm32"))]
fn forward_stdin(stdin: Res<Stdin>, mut typed: MessageWriter<Typed>) {
    let Ok(receiver) = stdin.0.lock() else {
        return;
    };
    typed.write_batch(
        receiver
            .try_iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Typed {
                line,
                source: Source::Stdin,
            }),
    );
}

/// A command's name, its fields in positional order, and what it does.
pub struct Usage {
    pub name: &'static str,
    pub fields: &'static [&'static str],
    pub about: &'static str,
}

/// Every `UiCommand`, by its serde name. A test fails, naming the command,
/// when a variant has no entry here or an entry's fields do not match.
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
