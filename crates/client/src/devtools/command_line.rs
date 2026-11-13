//! Typed commands: `set_presentation_mode table`, `set_display option=orbits on=false`, or
//! a `UiCommand` as JSON. Arguments fill a command's fields in order, or by
//! name; quote values with spaces, or IDs that look like numbers. Vectors
//! are JSON lists without spaces: `set_thrust [0,0,1]`.
//!
//! Lines come from the debug window's terminal tab or from the
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
    /// Receives whether the line was run, or why not.
    pub reply: Option<std::sync::mpsc::SyncSender<Result<(), String>>>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    /// The debug window's terminal tab.
    Window,
    /// Standard input.
    Stdin,
    /// The native developer API (`ns cmd`).
    Api,
}

pub struct CommandLinePlugin;

impl Plugin for CommandLinePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Typed>()
            .add_systems(Update, run.before(actions::apply));
        app.insert_resource(Stdin(std::sync::Mutex::new(read_stdin())))
            .add_systems(Update, forward_stdin.before(run));
    }
}

/// Runs typed lines as commands. The echo, help and errors are logged, so they
/// reach the terminal tab, the log file, bug reports and the launching terminal.
/// Commands that would do nothing are refused with the reason, not dropped.
fn run(
    mut typed: MessageReader<Typed>,
    mut commands: MessageWriter<UiCommand>,
    universe: Res<crate::Universe>,
    nav: Res<crate::Navigation>,
    field: Res<crate::stars::stream::Starfield>,
    fleet: Res<crate::craft::Fleet>,
) {
    for Typed {
        line,
        source,
        reply,
    } in typed.read()
    {
        let line = line.trim();
        if *source == Source::Api {
            info!(target: TARGET, "cli> {line}");
        } else {
            info!(target: TARGET, "> {line}");
        }
        let result = match line {
            "help" => {
                info!(target: TARGET, "help · clear · or a UiCommand as JSON");
                for usage in &USAGE {
                    info!(target: TARGET, "{:<27} {}", synopsis(usage), usage.about);
                }
                Ok(())
            }
            // The window clears its own terminal tab.
            "clear" if *source == Source::Stdin => {
                info!(target: TARGET, "clear empties the debug window's terminal tab; Ctrl+L clears this terminal");
                Ok(())
            }
            line => parse(line).and_then(|command| {
                command.check(&universe.0, &nav, &field, &fleet)?;
                commands.write(command);
                Ok(())
            }),
        };
        if let Err(err) = &result {
            warn!(target: TARGET, "{err}");
        }
        if let Some(reply) = reply {
            let _ = reply.try_send(result);
        }
    }
}

#[derive(Resource)]
struct Stdin(std::sync::Mutex<std::sync::mpsc::Receiver<String>>);

/// Reads standard input on its own thread until it closes. A detached launch
/// has none, and the thread ends at once.
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
                reply: None,
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
pub const USAGE: [Usage; 37] = [
    Usage {
        name: "steer_craft",
        fields: &["order", "id", "tick", "in_s", "for_s"],
        about: "Ship order JSON: throttle, translate, rotate, point, stop (spin), release, cut (sublight drives and brake spin), configure, warp (turning steers the bubble while engaged); use ns ship --help",
    },
    Usage {
        name: "pick_star",
        fields: &["id"],
        about: "Select a star by ID, or visit it if already selected",
    },
    Usage {
        name: "select_star",
        fields: &["id"],
        about: "Select a known star by ID without travelling",
    },
    Usage {
        name: "select_barycentre",
        fields: &["id"],
        about: "Select a pair of the visited system by ID, such as <star>#AB",
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
        about: "Select any planet by stable ID without travelling",
    },
    Usage {
        name: "select_galaxy",
        fields: &[],
        about: "Select the galaxy without travelling",
    },
    Usage {
        name: "select_list",
        fields: &["id"],
        about: "Select a list without opening it or travelling",
    },
    Usage {
        name: "show_page",
        fields: &["page"],
        about: "Show a page of the visited list",
    },
    Usage {
        name: "set_presentation_mode",
        fields: &["presentation_mode"],
        about: "simulation, table or constellation",
    },
    Usage {
        name: "visit_star",
        fields: &["id", "presentation_mode"],
        about: "Fly to a star's system by ID: simulation, table or constellation",
    },
    Usage {
        name: "fly_to_planet",
        fields: &["planet"],
        about: "Fly to a planet by stable ID and follow it",
    },
    Usage {
        name: "follow",
        fields: &[],
        about: "Follow the selected planet or craft",
    },
    Usage {
        name: "spawn_ship",
        fields: &["body", "altitude_km", "speed_kms", "velocity_kms"],
        about: "Place a ship above a body (planet, star or system ID; default the selection), in a circular orbit unless given a prograde speed or a [x,y,z] velocity, km/s relative to the body",
    },
    Usage {
        name: "despawn_craft",
        fields: &["id"],
        about: "Remove a craft by ID, or every craft without one",
    },
    Usage {
        name: "set_thrust",
        fields: &["thrust", "id"],
        about: "Hold a craft's thrust as [x,y,z] m/s² in its system's axes, up to 100; the selected or only craft without an ID",
    },
    Usage {
        name: "select_craft",
        fields: &["id"],
        about: "Select a craft by ID without travelling",
    },
    Usage {
        name: "fly_to_craft",
        fields: &["id"],
        about: "Fly to a craft by ID and follow it",
    },
    Usage {
        name: "up",
        fields: &[],
        about: "Reframe a tracked planet's system, else clear selection or go up (Esc)",
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
        name: "set_move_speed",
        fields: &["speed"],
        about: "Camera travel speed in metres per second (1 m/s to 10,000 ly/s)",
    },
    Usage {
        name: "set_display",
        fields: &["option", "on"],
        about: "overlays, orbits, labels, markers, velocity_marker, predicted_paths, wireframe, realistic or galaxy_glow; true or false",
    },
    Usage {
        name: "set_zoom",
        fields: &["metres"],
        about: "Camera distance from what it looks at or follows, 1 m to 40,000 ly",
    },
    Usage {
        name: "set_fov",
        fields: &["degrees"],
        about: "Vertical field of view, 30 to 130",
    },
    Usage {
        name: "set_magnitude",
        fields: &["magnitude"],
        about: "Faintest star magnitude drawn as a point, 3 to 10.5; reloads the stars",
    },
    Usage {
        name: "set_exposure",
        fields: &["stops"],
        about: "Manual exposure, -64 to +16 stops; +1 doubles light, -1 halves it",
    },
    Usage {
        name: "set_compression",
        fields: &["amount"],
        about: "How far bodies are exposed for themselves, 0 (one physical exposure) to 1 (each correctly exposed)",
    },
    Usage {
        name: "view_galaxy",
        fields: &[],
        about: "Frame the galaxy from above",
    },
    Usage {
        name: "set_debug",
        fields: &["switch", "on"],
        about: "freeze_stream, galactic_points, point_pixels, msaa, bloom, cull_blocks, point_stars, split_multiples, atmospheres, atmosphere_tables, atmosphere_stop_opaque, atmosphere_focus_steps, atmosphere_multiple, atmosphere_ground, oblateness, ground or ground_levels; true or false",
    },
    Usage {
        name: "set_atmosphere",
        fields: &[
            "preset",
            "pressure_bar",
            "scale_height_km",
            "gas",
            "haze",
            "haze_height_km",
            "haze_colour",
            "haze_absorbed",
            "view_steps",
            "light_steps",
            "enabled",
        ],
        about: "Selected-planet debug override: enabled=false restores generated air; a preset or optical edit enables it. Presets: earth, venus, mars, titan, hydrogen_giant; haze_colour as [r,g,b] 0 to 1",
    },
    Usage {
        name: "search",
        fields: &["query"],
        about: "Search from the camera for the object browser, in ns search text: search kind=planet orbits.star_class=m within_ly=50",
    },
    Usage {
        name: "clear_search",
        fields: &[],
        about: "Cancel and clear the object browser's search",
    },
    Usage {
        name: "set_search",
        fields: &["work_cap_millions"],
        about: "Debug: search work cap in millions of units (1 to 10000); CPU capacity comes from the shared budget",
    },
    Usage {
        name: "set_layered_planets",
        fields: &["on"],
        about: "Pause or resume background planet refinement and safe adoption; already adopted results stay active",
    },
    Usage {
        name: "set_fullscreen",
        fields: &["on"],
        about: "Borderless fullscreen on the window's monitor (F11); true or false",
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
    let command = parse_unchecked(line)?;
    command.check_values()?;
    Ok(command)
}

fn parse_unchecked(line: &str) -> Result<UiCommand, String> {
    let line = line.trim();
    if line.starts_with('{') {
        return serde_json::from_str(line).map_err(|err| err.to_string());
    }
    let words = split(line)?;
    let Some((name, args)) = words.split_first() else {
        return Err("Empty command".into());
    };
    // The rest of the line is the query, `=` signs and all.
    if name.value == "search"
        && name.field.is_none()
        && let Some(query) = line.strip_prefix("search")
    {
        let query = query.trim();
        let query = query
            .strip_prefix('"')
            .and_then(|q| q.strip_suffix('"'))
            .unwrap_or(query);
        return Ok(UiCommand::Search {
            query: query.into(),
        });
    }
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
        // Numbers, booleans and [x,y,z] lists as such; anything else, or
        // quoted, is text.
        let value = match serde_json::from_str(&arg.value) {
            Ok(value @ (Value::Number(_) | Value::Bool(_) | Value::Array(_))) if !arg.quoted => {
                value
            }
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
mod tests;
