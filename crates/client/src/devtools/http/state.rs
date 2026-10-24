//! Cached snapshot paths, discovery and bounded performance history selection.
use super::{SharedState, decode_path, respond};
use serde_json::{Value, json};
use std::sync::Arc;
use tiny_http::Request;

pub(super) fn read(request: Request, shared: &SharedState, url: &str) {
    let published = shared.lock().ok().and_then(|cached| cached.clone());
    let Some(published) = published else {
        respond(
            request,
            503,
            json!({"error": "Waiting for the first app snapshot"}),
        );
        return;
    };
    // Only requests that may need the history pay for encoding it.
    let state = if url.contains("history") {
        Arc::new(published.full())
    } else {
        published.state
    };
    match select_snapshot(&state, url) {
        Ok(body) => respond(request, 200, body),
        Err((status, error)) => respond(request, status, json!({"error": error})),
    }
}

pub(super) fn snapshot_route(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url);
    path == "/state" || path == "/perf" || path.starts_with("/state/") || path.starts_with("/perf/")
}

/// Nested paths are JSON pointers relative to state or its performance object.
/// Select before cloning, so small queries do not copy/encode the whole history.
pub(super) fn select_snapshot(state: &Value, url: &str) -> Result<Value, (u16, &'static str)> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let (root, pointer) = if let Some(pointer) = path.strip_prefix("/state") {
        (state, pointer)
    } else if let Some(pointer) = path.strip_prefix("/perf") {
        (&state["performance"], pointer)
    } else {
        return Err((404, "Unknown endpoint"));
    };
    let pointer = decode_path(pointer)?;
    let history_path = if path.starts_with("/state") {
        "/performance/history"
    } else {
        "/history"
    };
    let mut include_history = false;
    let mut seconds = None;
    let mut keys = false;
    if !query.is_empty() {
        for parameter in query.split('&') {
            match parameter.split_once('=') {
                Some(("keys", "1")) if !keys => keys = true,
                Some(("history", "1")) if pointer.is_empty() && !include_history => {
                    include_history = true
                }
                Some(("seconds", value)) if pointer == history_path && seconds.is_none() => {
                    let value = value
                        .parse::<f64>()
                        .map_err(|_| (400, "seconds must be a number in (0, 10]"))?;
                    if !value.is_finite() || value <= 0.0 || value > 10.0 {
                        return Err((400, "seconds must be a number in (0, 10]"));
                    }
                    seconds = Some(value);
                }
                _ => return Err((400, "Unsupported or duplicate query parameter")),
            }
        }
    }
    if keys && (include_history || seconds.is_some()) {
        return Err((400, "keys cannot be combined with history or seconds"));
    }
    let selected = root
        .pointer(&pointer)
        .ok_or((404, "Unknown snapshot path"))?;
    if keys {
        // Discover the logical subtree, not the legacy /perf root envelope.
        return Ok(match selected {
            Value::Object(object) => json!(object.keys().collect::<Vec<_>>()),
            Value::Array(array) => {
                json!((0..array.len()).map(|i| i.to_string()).collect::<Vec<_>>())
            }
            _ => json!([]),
        });
    }
    if let Some(seconds) = seconds {
        let history = selected.as_array().ok_or((404, "History is unavailable"))?;
        // Use the snapshot's latest recorded frame as the reference, not wall
        // time. Sample time remains available on the root for stale-data checks.
        let latest = state
            .pointer("/performance/latest/elapsed_seconds")
            .and_then(Value::as_f64);
        return Ok(Value::Array(
            history
                .iter()
                .filter(|frame| {
                    latest.zip(frame["elapsed_seconds"].as_f64()).is_some_and(
                        |(latest, elapsed)| elapsed >= latest - seconds && elapsed <= latest,
                    )
                })
                .cloned()
                .collect(),
        ));
    }
    let mut body = if pointer.is_empty() && path == "/perf" {
        // Preserve the original root response envelope for existing clients.
        json!({"schema_version": state["schema_version"], "process_id": state["process_id"],
            "sampled_unix_ms": state["sampled_unix_ms"], "performance": selected})
    } else {
        selected.clone()
    };
    if pointer.is_empty()
        && !include_history
        && let Some(perf) = body["performance"].as_object_mut()
    {
        perf.remove("history");
    }
    Ok(body)
}
