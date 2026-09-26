//! Native log file and crash reports under `data/logs/`, so problems leave a trace
//! even when the client is launched detached from a terminal.
use bevy::log::{BoxedLayer, tracing_subscriber};
use bevy::prelude::*;
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub fn logs_dir() -> PathBuf {
    std::env::var_os("SPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .join("data/logs")
}

/// Writes panics, with a backtrace, to their own `crash-*.log` before the default
/// hook runs. Call before building the app.
pub fn install_crash_reports() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = format!(
            "ClickUp Space client {} crashed at {} UTC\nThread: {}\n{info}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            utc_now(),
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture(),
        );
        let dir = logs_dir();
        let path = dir.join(format!("crash-{}.log", utc_now().replace([' ', ':'], "-")));
        if fs::create_dir_all(&dir).is_ok() && fs::write(&path, report).is_ok() {
            eprintln!("Crash report written to {}", path.display());
        }
        previous(info);
    }));
}

/// Adds `data/logs/client.log` to Bevy's logger. The previous run is kept as
/// `client.previous.log`. Smoke tests log separately, so they never rotate away
/// the log of a real session.
pub fn file_layer(_: &mut App) -> Option<BoxedLayer> {
    let dir = logs_dir();
    let name = if std::env::args().any(|a| a == "--smoke-test") {
        "smoke-client"
    } else {
        "client"
    };
    let path = dir.join(format!("{name}.log"));
    let opened = fs::create_dir_all(&dir)
        .and_then(
            |()| match fs::rename(&path, dir.join(format!("{name}.previous.log"))) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
                _ => Ok(()),
            },
        )
        .and_then(|()| fs::File::create(&path));
    match opened {
        Ok(mut file) => {
            let _ = writeln!(file, "ClickUp Space client {}", env!("CARGO_PKG_VERSION"));
            Some(Box::new(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(Mutex::new(file)),
            ))
        }
        Err(err) => {
            eprintln!(
                "Cannot open {}: {err}. Logging to stderr only.",
                path.display()
            );
            None
        }
    }
}

/// A bug can look wrong without crashing. Keep the current state and recent
/// timeline together in one local file the user can hand back for investigation.
pub fn save_bug_report(state: &str) -> io::Result<PathBuf> {
    static REPORT: AtomicU64 = AtomicU64::new(0);
    let dir = logs_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "bug-{}-{}-{}.log",
        utc_now().replace([' ', ':'], "-"),
        std::process::id(),
        REPORT.fetch_add(1, Ordering::Relaxed),
    ));
    let recent = (|| -> io::Result<String> {
        let mut file = fs::File::open(dir.join("client.log"))?;
        let start = file.metadata()?.len().saturating_sub(128 * 1024);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let log = String::from_utf8_lossy(&bytes);
        Ok({
            let log = if start > 0 {
                log.split_once('\n').map_or("", |(_, rest)| rest)
            } else {
                &log
            };
            let lines: Vec<_> = log.lines().rev().take(300).collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        })
    })()
    .unwrap_or_else(|err| format!("Client log unavailable: {err}"));
    fs::write(
        &path,
        format!(
            "ClickUp Space {} · bug report · {} UTC\nOS: {}\nPID: {}\n\n{state}\n\nRecent client events:\n{recent}\n",
            env!("CARGO_PKG_VERSION"),
            utc_now(),
            std::env::consts::OS,
            std::process::id(),
        ),
    )?;
    Ok(path)
}

fn utc_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    utc(seconds)
}

/// `YYYY-MM-DD HH:MM:SS` from Unix seconds (Hinnant's civil-from-days).
fn utc(seconds: u64) -> String {
    let (days, time) = ((seconds / 86_400) as i64, seconds % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_utc_dates() {
        assert_eq!(super::utc(0), "1970-01-01 00:00:00");
        assert_eq!(super::utc(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(super::utc(1_790_376_245), "2026-09-25 22:44:05");
    }

    #[test]
    fn bug_report_captures_context_and_can_be_handed_back() {
        let path = super::save_bug_report("View: Orbit\nPage: 140").unwrap();
        let report = std::fs::read_to_string(&path).unwrap();
        assert!(report.contains("View: Orbit\nPage: 140"));
        assert!(report.contains("Recent client events:"));
        std::fs::remove_file(path).unwrap();
    }
}
