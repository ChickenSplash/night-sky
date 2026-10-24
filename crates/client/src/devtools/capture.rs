//! Copies every tracing event into an in-memory buffer for the log and
//! terminal tabs.
use super::DevTools;
use super::command_line;
use bevy::log::tracing::{
    Event, Subscriber,
    field::{Field, Visit as Fields},
};
use bevy::log::tracing_subscriber::{Layer, layer::Context};
use bevy::log::{BoxedLayer, Level};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use std::collections::VecDeque;
use std::fmt::{self, Write};
use std::sync::{Mutex, OnceLock};

/// Log lines kept in memory for the console.
pub(super) const LOG_LINES: usize = 2000;

pub const LEVELS: [&str; 4] = ["error", "warn", "info", "debug"];

#[derive(Clone)]
pub struct Line {
    pub seconds: f64,
    pub level: usize,
    pub target: String,
    pub text: String,
    /// Lower-case target and text, for the console filter.
    pub search: String,
}

struct Log {
    lines: VecDeque<Line>,
    /// Lines logged so far, so the console can tell which are new.
    total: u64,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    lines: VecDeque::new(),
    total: 0,
});
static START: OnceLock<Instant> = OnceLock::new();

/// Bevy's extra log layers: the console's buffer, plus the log file.
pub fn log_layers(app: &mut App) -> Option<BoxedLayer> {
    START.get_or_init(Instant::now);
    let console: BoxedLayer = Box::new(Console);
    if let Some(file) = crate::devtools::diagnostics::file_layer(app) {
        return Some(Box::new(vec![file, console]));
    }
    let _ = app;
    Some(console)
}

/// Copies each event into `LOG`. Never log while holding its lock.
struct Console;

impl<S: Subscriber> Layer<S> for Console {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Message::default();
        event.record(&mut fields);
        let meta = event.metadata();
        let target = fields.target.unwrap_or_else(|| meta.target().to_string());
        // One row per line keeps the console's rows a fixed height.
        let text = (fields.message + &fields.extra).replace('\n', " ⏎ ");
        let line = Line {
            seconds: START.get().map_or(0.0, |s| s.elapsed().as_secs_f64()),
            level: match *meta.level() {
                Level::ERROR => 0,
                Level::WARN => 1,
                Level::INFO => 2,
                _ => 3,
            },
            search: format!("{target} {text}").to_lowercase(),
            target,
            text,
        };
        if let Ok(mut log) = LOG.lock() {
            if log.lines.len() == LOG_LINES {
                log.lines.pop_front();
            }
            log.lines.push_back(line);
            log.total += 1;
        }
    }
}

#[derive(Default)]
struct Message {
    message: String,
    extra: String,
    /// The original target of a `log` crate record.
    pub target: Option<String>,
}

impl Message {
    fn add(&mut self, field: &Field, value: fmt::Arguments) {
        match field.name() {
            "message" => {
                let _ = self.message.write_fmt(value);
            }
            "log.target" => self.target = Some(value.to_string()),
            name if name.starts_with("log.") => {}
            name => {
                let _ = write!(self.extra, " {name}={value}");
            }
        }
    }
}

impl Fields for Message {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, format_args!("{value}"));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.add(field, format_args!("{value:?}"));
    }
}

impl DevTools {
    /// Takes the lines logged since the last call.
    pub fn sync(&mut self) {
        let Ok(log) = LOG.lock() else {
            return;
        };
        let new = (log.total - self.seen).min(log.lines.len() as u64) as usize;
        for line in log.lines.iter().skip(log.lines.len() - new) {
            if let Some(unseen) = self.unseen.get_mut(line.level) {
                *unseen += 1;
            }
            if line.target == command_line::TARGET {
                self.terminal.push_back(line.clone());
            }
            self.lines.push_back(line.clone());
        }
        self.seen = log.total;
        drop(log);
        for lines in [&mut self.lines, &mut self.terminal] {
            while lines.len() > LOG_LINES {
                lines.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};

    #[test]
    fn console_keeps_messages_and_fields_on_one_line() {
        let subscriber = Registry::default().with(Console);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            warn!(cells = 175, "Console test\nsecond line");
        });
        let log = LOG.lock().unwrap();
        let line = log
            .lines
            .iter()
            .find(|l| l.text.starts_with("Console test"))
            .unwrap();
        assert_eq!(line.text, "Console test ⏎ second line cells=175");
        assert_eq!(line.level, 1);
        assert!(line.search.contains("console test"));
    }
}
