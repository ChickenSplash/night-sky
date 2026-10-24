//! Opt-in, loopback-only native developer transport. Never touches the ECS
//! from its listener thread; commands and captures are queued to PreUpdate.
mod bridge;
mod commands;
mod search;
mod state;

use super::snapshot::{Published, Snapshot};
use crate::search::Hub;
use bevy::prelude::*;
use bridge::{HttpApi, Job, receive};
use commands::{command_catalogue, commands_route};
use search::catalogue;
use serde_json::{Value, json};
use state::snapshot_route;
use std::io::Read;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub(super) type SharedState = Arc<Mutex<Option<Published>>>;
// tiny_http prebuffers non-Expect bodies up to 1024 bytes before handing
// requests to the listener, so accepted commands cannot block on body reads.
const BODY_LIMIT: usize = 1024;
const QUEUE_LIMIT: usize = 32;
const PER_FRAME: usize = 8;
/// How long a command or capture request waits for the main thread's reply.
const REPLY_WAIT: Duration = Duration::from_secs(2);

pub fn install(app: &mut App) {
    if std::env::var("NIGHT_SKY_API").as_deref() != Ok("1") {
        return;
    }
    let port = match std::env::var("NIGHT_SKY_API_PORT") {
        Ok(value) => match value.parse::<u16>() {
            Ok(port) if port != 0 => port,
            _ => {
                warn!("NIGHT_SKY_API_PORT must be 1..65535; developer API disabled");
                return;
            }
        },
        Err(_) => 4322,
    };
    let server = match Server::http((std::net::Ipv4Addr::LOCALHOST, port)) {
        Ok(server) => server,
        Err(error) => {
            warn!("Cannot bind developer API on 127.0.0.1:{port}: {error}");
            return;
        }
    };
    let shared = Arc::new(Mutex::new(None));
    let hub = app
        .world()
        .resource::<crate::search::Searches>()
        .hub
        .clone();
    match start(server, shared.clone(), hub, REPLY_WAIT) {
        Ok(api) => {
            app.world_mut().resource_mut::<Snapshot>().http = Some(shared);
            app.insert_resource(api).add_systems(PreUpdate, receive);
            info!(
                "Developer API: http://127.0.0.1:{port} (local CLI only; POST requires X-Night-Sky-Client: cli)"
            );
        }
        Err(error) => warn!("Cannot start developer API: {error}"),
    }
}

fn start(
    server: Server,
    shared: SharedState,
    hub: Arc<Hub>,
    wait: Duration,
) -> std::io::Result<HttpApi> {
    let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let port = server.server_addr().to_ip().expect("TCP listener").port();
    std::thread::Builder::new()
        .name("developer HTTP".into())
        .spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match server.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(request)) => handle(request, &shared, &hub, &sender, port, wait),
                    Ok(None) => {}
                    Err(error) => {
                        warn!("Developer API listener stopped: {error}");
                        break;
                    }
                }
            }
        })?;
    Ok(HttpApi {
        receiver: Mutex::new(receiver),
        stop,
    })
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn handle(
    request: Request,
    shared: &SharedState,
    hub: &Hub,
    queue: &mpsc::SyncSender<Job>,
    port: u16,
    wait: Duration,
) {
    // Loopback binding alone is not sufficient protection against websites
    // or DNS rebinding. No browser origins, no CORS, and a non-simple header
    // for mutations. Local CLI processes are trusted; this is not remote auth.
    let host = header(&request, "Host");
    let expected = format!("127.0.0.1:{port}");
    let localhost = format!("localhost:{port}");
    if host != Some(expected.as_str()) && host != Some(localhost.as_str())
        || header(&request, "Origin").is_some()
        || header(&request, "Sec-Fetch-Site").is_some()
    {
        respond(request, 403, json!({"error": "Local CLI requests only"}));
        return;
    }
    if *request.method() == Method::Post && header(&request, "X-Night-Sky-Client") != Some("cli") {
        respond(
            request,
            403,
            json!({"error": "POST requires X-Night-Sky-Client: cli"}),
        );
        return;
    }
    let url = request.url().to_owned();
    match (request.method(), url.as_str()) {
        (&Method::Get, "/health") => respond(
            request,
            200,
            json!({"status": "ok", "process_id": std::process::id(), "schema_version": 1}),
        ),
        (&Method::Get, "/craft/schema") => respond(request, 200, crate::craft::schema::catalogue()),
        (&Method::Get, _) if commands_route(&url) => match command_catalogue(&url) {
            Ok(body) => respond(request, 200, body),
            Err((status, error)) => respond(request, status, json!({"error": error})),
        },
        (&Method::Get, _) if snapshot_route(&url) => {
            state::read(request, shared, &url);
        }
        (&Method::Post, "/command") => commands::submit(request, queue, wait),
        (&Method::Post, "/screenshot") => {
            if request.body_length().unwrap_or(0) != 0
                || header(&request, "Transfer-Encoding").is_some()
            {
                respond(
                    request,
                    400,
                    json!({"error": "Screenshot requests have no body"}),
                );
                return;
            }
            let (reply, result) = mpsc::sync_channel(1);
            if queue.try_send(Job::Capture(reply)).is_err() {
                respond(
                    request,
                    503,
                    json!({"error": "Capture queue full or app unavailable"}),
                );
                return;
            }
            match result.recv_timeout(wait) {
                Ok(body) => respond(
                    request,
                    if body["status"] == "failed" { 500 } else { 202 },
                    body,
                ),
                Err(_) => respond(
                    request,
                    202,
                    json!({"status": "queued", "note": "No main-thread reply within 2 seconds. Capture may still run; inspect data/logs/bug-* before retrying."}),
                ),
            }
        }
        (&Method::Get, "/catalogue") => respond(request, 200, catalogue()),
        (&Method::Get, "/search") => {
            search::list(request, hub);
        }
        (&Method::Post, "/search") => search::submit(request, shared, hub, queue, wait),
        (&Method::Get, path) if path.starts_with("/search/") => {
            search::read(request, hub, &path["/search/".len()..]);
        }
        (&Method::Post, path) if path.starts_with("/search/") && path.ends_with("/cancel") => {
            search::cancel(
                request,
                hub,
                &path["/search/".len()..path.len() - "/cancel".len()],
            );
        }
        _ if snapshot_route(&url) || commands_route(&url) || url.starts_with("/search/") => {
            respond(request, 405, json!({"error": "Method not allowed"}))
        }
        (
            _,
            "/health" | "/command" | "/screenshot" | "/catalogue" | "/search" | "/craft/schema",
        ) => respond(request, 405, json!({"error": "Method not allowed"})),
        _ => respond(request, 404, json!({"error": "Unknown endpoint"})),
    }
}

fn decode_path(path: &str) -> Result<String, (u16, &'static str)> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input.next().and_then(|c| (c as char).to_digit(16));
            let low = input.next().and_then(|c| (c as char).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                return Err((400, "Invalid URL path encoding"));
            };
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).map_err(|_| (400, "Invalid UTF-8 path"))
}

fn read_body(request: &mut Request) -> Result<String, (u16, &'static str)> {
    // Disallow streaming/chunked requests so a command cannot monopolise the
    // listener with an unbounded body. tiny_http buffers bodies this small.
    if header(request, "Transfer-Encoding").is_some() || header(request, "Expect").is_some() {
        return Err((400, "Chunked and Expect command bodies are not supported"));
    }
    let length = request
        .body_length()
        .ok_or((411, "Content-Length required"))?;
    if length > BODY_LIMIT {
        return Err((413, "Command body exceeds 1024 bytes"));
    }
    let mut body = String::new();
    request
        .as_reader()
        .take((BODY_LIMIT + 1) as u64)
        .read_to_string(&mut body)
        .map_err(|_| (400, "Invalid UTF-8 command body"))?;
    Ok(body)
}

fn respond(request: Request, status: u16, body: Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    let _ = request.respond(response);
}

#[cfg(test)]
mod tests;
