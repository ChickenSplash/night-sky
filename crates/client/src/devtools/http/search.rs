//! Search discovery and checked submission to the main-thread bridge.
use super::bridge::{Job, SearchJob};
use super::{SharedState, read_body, respond};
use crate::search::Hub;
use serde_json::{Value, json};
use std::{sync::mpsc, time::Duration};
use tiny_http::Request;

pub(super) fn list(request: Request, hub: &Hub) {
    hub.poll(true);
    respond(request, 200, json!(hub.summaries(true).unwrap_or_default()));
}

pub(super) fn read(request: Request, hub: &Hub, id: &str) {
    match id.parse().ok().and_then(|id| hub.get(id)) {
        Some(entry) => respond(request, 200, json!(entry)),
        None => respond(
            request,
            404,
            json!({"error": "Unknown or expired search; GET /search lists those kept"}),
        ),
    }
}

pub(super) fn cancel(request: Request, hub: &Hub, id: &str) {
    match id.parse().ok().and_then(|id| hub.get(id)) {
        Some(entry) => {
            hub.cancel(entry.id);
            respond(
                request,
                202,
                json!({"id": entry.id, "status": "cancelling"}),
            );
        }
        None => respond(request, 404, json!({"error": "Unknown or expired search"})),
    }
}

/// The registry, for discovering what a query can say.
pub(super) fn catalogue() -> Value {
    use space_simulation::search::query::{
        DEFAULT_LIMIT, DEFAULT_RADIUS_LY, MAX_LIMIT, MAX_RADIUS_LY,
    };
    let mut body = json!(space_simulation::search::registry::catalogue());
    body["syntax"] = json!([
        "kind=<kind>[,<kind>] or family=<family>",
        "<property>=<min>..<max> (open ends allowed), =<a>,<b> for choices, =yes|no for flags",
        "<relation>.<property>=... and <relation>.kind=<kind>: all of one relation's filters hold for the same related object",
        "within_ly=<radius> (from the camera) or system=<system id>",
        "limit=<n>; sort=<property> or -<property> within one system only",
        "words without = are an exact designation or ID: TS 1abc-2-34 (AB) b",
    ]);
    body["limits"] = json!({"default_limit": DEFAULT_LIMIT, "max_limit": MAX_LIMIT,
        "default_within_ly": DEFAULT_RADIUS_LY, "max_within_ly": MAX_RADIUS_LY});
    body
}

/// A search body: query text, or JSON `{"query": text or query object,
/// "origin": "camera" | "home" | [x, y, z] parsecs, "work_cap": units}`.
pub(super) fn search_request(
    body: &str,
    camera: impl FnOnce() -> Option<[f64; 3]>,
    default_cap: u64,
) -> Result<(space_simulation::search::Query, [f64; 3], u64), String> {
    use space_simulation::search::Query;
    let (query, origin, cap) = if body.starts_with('{') {
        let value: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
        if let Some(key) = value.as_object().and_then(|o| {
            o.keys()
                .find(|k| !["query", "origin", "work_cap"].contains(&k.as_str()))
        }) {
            return Err(format!(
                "unknown field `{key}`; give query, origin and work_cap"
            ));
        }
        let query = match &value["query"] {
            Value::String(text) => Query::parse(text)?,
            object @ Value::Object(_) => {
                serde_json::from_value(object.clone()).map_err(|e| e.to_string())?
            }
            _ => return Err("`query` must be text or a query object".into()),
        };
        let cap = match &value["work_cap"] {
            Value::Null => default_cap,
            cap => cap
                .as_u64()
                .filter(|&cap| cap > 0)
                .ok_or("`work_cap` must be a whole number above 0")?,
        };
        (query, value["origin"].clone(), cap)
    } else {
        (Query::parse(body)?, Value::Null, default_cap)
    };
    // The query's own problems first: they matter wherever it is sent from.
    query.check()?;
    let origin = match origin {
        Value::Null => camera()
            .ok_or("No camera position yet (waiting for the first app snapshot); give an origin")?,
        Value::String(name) if name == "camera" => camera()
            .ok_or("No camera position yet (waiting for the first app snapshot); give an origin")?,
        Value::String(name) if name == "home" => {
            space_simulation::galaxy::GalaxyModel::milky_way().home_pc
        }
        origin => serde_json::from_value::<[f64; 3]>(origin)
            .ok()
            .filter(|p| p.iter().all(|x| x.is_finite()))
            .ok_or("`origin` must be \"camera\", \"home\" or [x, y, z] parsecs")?,
    };
    Ok((query, origin, cap))
}

pub(super) fn submit(
    mut request: Request,
    shared: &SharedState,
    hub: &Hub,
    queue: &mpsc::SyncSender<Job>,
    wait: Duration,
) {
    let body = match read_body(&mut request) {
        Ok(body) => body,
        Err((status, error)) => {
            respond(request, status, json!({"error": error}));
            return;
        }
    };
    let camera = || {
        let published = shared.lock().ok().and_then(|cached| cached.clone())?;
        serde_json::from_value(published.state["camera"]["position_pc"].clone()).ok()
    };
    let (query, origin_pc, work_cap) = match search_request(body.trim(), camera, hub.work_cap()) {
        Ok(checked) => checked,
        Err(error) => {
            respond(request, 400, json!({"error": error}));
            return;
        }
    };
    let text = query.to_text();
    let (reply, sent) = mpsc::sync_channel(1);
    let job = SearchJob {
        query,
        origin_pc,
        work_cap,
    };
    if queue.try_send(Job::Search(job, reply)).is_err() {
        respond(
            request,
            503,
            json!({"error": "Command queue full or app unavailable"}),
        );
        return;
    }
    match sent.recv_timeout(wait) {
        Ok(Ok(id)) => respond(
            request,
            202,
            json!({"id": id, "status": "running", "query": text,
                "origin_pc": origin_pc, "work_cap": work_cap}),
        ),
        Ok(Err(error)) => respond(request, 400, json!({"error": error})),
        Err(_) => respond(
            request,
            202,
            json!({"status": "queued", "query": text, "note": "No main-thread reply within 2 seconds. The search may still be sent; GET /search lists those kept."}),
        ),
    }
}
