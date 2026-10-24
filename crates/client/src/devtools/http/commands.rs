//! Command discovery and checked submission to the main-thread bridge.
use super::bridge::Job;
use super::{decode_path, read_body, respond};
use bevy::prelude::warn;
use serde_json::{Value, json};
use std::{sync::mpsc, time::Duration};
use tiny_http::Request;

pub(super) fn commands_route(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url);
    path == "/commands" || path.starts_with("/commands/")
}

pub(super) fn command_catalogue(url: &str) -> Result<Value, (u16, &'static str)> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if !query.is_empty() {
        return Err((400, "Command catalogue takes no query parameters"));
    }
    let entry = |usage: &crate::devtools::command_line::Usage| {
        json!({
            "name": usage.name, "fields": usage.fields,
            "synopsis": crate::devtools::command_line::synopsis(usage), "about": usage.about,
        })
    };
    if path == "/commands" {
        return Ok(Value::Array(
            crate::devtools::command_line::USAGE
                .iter()
                .map(entry)
                .collect(),
        ));
    }
    let name = decode_path(path.strip_prefix("/commands/").unwrap_or(""))?;
    crate::devtools::command_line::USAGE
        .iter()
        .find(|usage| usage.name == name)
        .map(entry)
        .ok_or((404, "Unknown command"))
}

pub(super) fn submit(mut request: Request, queue: &mpsc::SyncSender<Job>, wait: Duration) {
    let body = match read_body(&mut request) {
        Ok(body) => body,
        Err((status, error)) => {
            respond(request, status, json!({"error": error}));
            return;
        }
    };
    // Syntax and ranges are checked here, so they fail fast even if
    // the main thread is busy; the current state is checked there.
    if let Err(error) = crate::devtools::command_line::parse(body.trim()) {
        warn!(target: crate::devtools::command_line::TARGET, "cli> {}: {error}", body.trim());
        respond(request, 400, json!({"error": error}));
        return;
    }
    let (reply, verdict) = mpsc::sync_channel(1);
    if queue
        .try_send(Job::Command(body.trim().to_owned(), reply))
        .is_err()
    {
        respond(
            request,
            503,
            json!({"error": "Command queue full or app unavailable"}),
        );
        return;
    }
    match verdict.recv_timeout(wait) {
        Ok(Ok(())) => respond(
            request,
            202,
            json!({"status": "accepted", "note": "Checked against the current state and applied this frame. Flights and star loading continue afterwards; use ns cmd --wait or read /state/camera."}),
        ),
        Ok(Err(error)) => respond(request, 409, json!({"error": error})),
        Err(_) => respond(
            request,
            202,
            json!({"status": "queued", "note": "No main-thread reply within 2 seconds. The command may still run; read /state before retrying."}),
        ),
    }
}
