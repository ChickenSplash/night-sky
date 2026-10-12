//! Opt-in, loopback-only native developer transport. Never touches the ECS
//! from its listener thread; commands and captures are queued to PreUpdate.
use super::snapshot::Snapshot;
use crate::command_line::{Source, Typed};
use crate::scene::{CameraPose, CameraRig};
use crate::stars::stream::Starfield;
use crate::{Clock, Navigation, Universe};
use bevy::prelude::*;
use serde_json::{Value, json};
use std::io::Read;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub(super) type SharedState = Arc<Mutex<Option<Arc<Value>>>>;
// tiny_http prebuffers non-Expect bodies up to 1024 bytes before handing
// requests to the listener, so accepted commands cannot block on body reads.
const BODY_LIMIT: usize = 1024;
const QUEUE_LIMIT: usize = 32;
const PER_FRAME: usize = 8;
/// How long a command or capture request waits for the main thread's reply.
const REPLY_WAIT: Duration = Duration::from_secs(2);

type Verdict = mpsc::SyncSender<Result<(), String>>;

enum Job {
    /// A parsed line, run as `Typed` so it is echoed and checked like the
    /// terminal's, and refused with the reason if it would do nothing.
    Command(String, Verdict),
    Capture(mpsc::SyncSender<Value>),
}

#[derive(Resource)]
struct HttpApi {
    receiver: Mutex<mpsc::Receiver<Job>>,
    stop: Arc<AtomicBool>,
}

impl Drop for HttpApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

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
    match start(server, shared.clone(), REPLY_WAIT) {
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

fn start(server: Server, shared: SharedState, wait: Duration) -> std::io::Result<HttpApi> {
    let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let port = server.server_addr().to_ip().expect("TCP listener").port();
    std::thread::Builder::new()
        .name("developer HTTP".into())
        .spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match server.recv_timeout(Duration::from_millis(100)) {
                    Ok(Some(request)) => handle(request, &shared, &sender, port, wait),
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

fn receive(
    api: Res<HttpApi>,
    mut writer: MessageWriter<Typed>,
    mut commands: Commands,
    scene: (Res<Universe>, Res<Navigation>, Res<Clock>),
    camera: (Res<CameraRig>, Res<CameraPose>),
    field: Res<Starfield>,
) {
    let Ok(receiver) = api.receiver.lock() else {
        return;
    };
    for job in receiver.try_iter().take(PER_FRAME) {
        match job {
            Job::Command(line, reply) => {
                writer.write(Typed {
                    line,
                    source: Source::Api,
                    reply: Some(reply),
                });
            }
            Job::Capture(reply) => {
                let (universe, nav, clock) = &scene;
                let (rig, pose) = &camera;
                let result = super::tabs::tools::save_bug_report(
                    &mut commands,
                    universe,
                    nav,
                    clock,
                    (rig, pose),
                    &field,
                );
                let report = result.strip_prefix("Saved ").map(|path| {
                    let path = std::path::Path::new(path);
                    path.canonicalize().unwrap_or_else(|_| path.to_owned())
                });
                let _ = reply.try_send(json!({
                    "status": if report.is_some() { "capture_scheduled" } else { "failed" },
                    "report": report.as_ref().map_or(result.clone(), |path| path.display().to_string()),
                    "png": report.map(|path| path.with_extension("png").display().to_string()),
                    "note": "The report is saved synchronously; the PNG is written by the renderer a frame or two later.",
                }));
            }
        }
    }
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

fn handle(
    mut request: Request,
    shared: &SharedState,
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
        (&Method::Get, _) if commands_route(&url) => match command_catalogue(&url) {
            Ok(body) => respond(request, 200, body),
            Err((status, error)) => respond(request, status, json!({"error": error})),
        },
        (&Method::Get, _) if snapshot_route(&url) => {
            let state = shared.lock().ok().and_then(|cached| cached.clone());
            let Some(state) = state else {
                respond(
                    request,
                    503,
                    json!({"error": "Waiting for the first app snapshot"}),
                );
                return;
            };
            match select_snapshot(&state, &url) {
                Ok(body) => respond(request, 200, body),
                Err((status, error)) => respond(request, status, json!({"error": error})),
            }
        }
        (&Method::Post, "/command") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err((status, error)) => {
                    respond(request, status, json!({"error": error}));
                    return;
                }
            };
            // Syntax and ranges are checked here, so they fail fast even if
            // the main thread is busy; the current state is checked there.
            if let Err(error) = crate::command_line::parse(body.trim()) {
                warn!(target: crate::command_line::TARGET, "cli> {}: {error}", body.trim());
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
        _ if snapshot_route(&url) || commands_route(&url) => {
            respond(request, 405, json!({"error": "Method not allowed"}))
        }
        (_, "/health" | "/command" | "/screenshot") => {
            respond(request, 405, json!({"error": "Method not allowed"}))
        }
        _ => respond(request, 404, json!({"error": "Unknown endpoint"})),
    }
}

fn commands_route(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url);
    path == "/commands" || path.starts_with("/commands/")
}

fn command_catalogue(url: &str) -> Result<Value, (u16, &'static str)> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if !query.is_empty() {
        return Err((400, "Command catalogue takes no query parameters"));
    }
    let entry = |usage: &crate::command_line::Usage| {
        json!({
            "name": usage.name, "fields": usage.fields,
            "synopsis": crate::command_line::synopsis(usage), "about": usage.about,
        })
    };
    if path == "/commands" {
        return Ok(Value::Array(
            crate::command_line::USAGE.iter().map(entry).collect(),
        ));
    }
    let name = decode_path(path.strip_prefix("/commands/").unwrap_or(""))?;
    crate::command_line::USAGE
        .iter()
        .find(|usage| usage.name == name)
        .map(entry)
        .ok_or((404, "Unknown command"))
}

fn snapshot_route(url: &str) -> bool {
    let path = url.split('?').next().unwrap_or(url);
    path == "/state" || path == "/perf" || path.starts_with("/state/") || path.starts_with("/perf/")
}

/// Nested paths are JSON pointers relative to state or its performance object.
/// Select before cloning, so small queries do not copy/encode the whole history.
fn select_snapshot(state: &Value, url: &str) -> Result<Value, (u16, &'static str)> {
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
mod tests {
    use super::*;

    #[test]
    fn discovery_uses_the_shared_command_catalogue_and_snapshot_children() {
        let catalogue = command_catalogue("/commands").unwrap();
        assert_eq!(
            catalogue.as_array().unwrap().len(),
            crate::command_line::USAGE.len()
        );
        for usage in &crate::command_line::USAGE {
            let entry = command_catalogue(&format!("/commands/{}", usage.name)).unwrap();
            assert_eq!(entry["fields"], json!(usage.fields));
            assert_eq!(entry["synopsis"], crate::command_line::synopsis(usage));
            assert_eq!(entry["about"], usage.about);
        }
        assert_eq!(command_catalogue("/commands/missing").unwrap_err().0, 404);
        assert_eq!(command_catalogue("/commands?keys=1").unwrap_err().0, 400);
        let state = json!({"camera": {"position": [1, 2], "flying": false}, "selection": null,
            "performance": {"latest": {"split": {}}, "history": []}});
        assert_eq!(
            select_snapshot(&state, "/state/camera?keys=1").unwrap(),
            json!(["flying", "position"])
        );
        assert_eq!(
            select_snapshot(&state, "/perf?keys=1").unwrap(),
            json!(["history", "latest"])
        );
        assert_eq!(
            select_snapshot(&state, "/state/camera/position?keys=1").unwrap(),
            json!(["0", "1"])
        );
        assert_eq!(
            select_snapshot(&state, "/state/selection?keys=1").unwrap(),
            json!([])
        );
        assert_eq!(
            select_snapshot(&state, "/state/missing?keys=1")
                .unwrap_err()
                .0,
            404
        );
        for url in [
            "/state?keys=1&keys=1",
            "/state?keys=0",
            "/state?keys=1&history=1",
            "/perf/history?keys=1&seconds=2",
        ] {
            assert_eq!(select_snapshot(&state, url).unwrap_err().0, 400);
        }
    }

    #[test]
    fn nested_snapshot_paths_select_objects_scalars_arrays_and_nulls() {
        let state = json!({
            "camera": {"position_pc": [8200.0, 0.0, 0.0]},
            "debug": {"split_multiples": false}, "selection": null,
            "a/b": {"~key": 7}, "space key": 8,
            "performance": {"latest": {"split": {"preparation_ms": 1.25}},
                "history": [], "averages": {"split_search_ms": 0.5}},
        });
        let select = |path| select_snapshot(&state, path).unwrap();
        assert_eq!(select("/state/camera"), state["camera"]);
        assert_eq!(select("/state/camera/position_pc/0"), 8200.0);
        assert_eq!(select("/state/debug/split_multiples"), false);
        assert_eq!(select("/state/selection"), Value::Null);
        assert_eq!(select("/perf/latest/split/preparation_ms"), 1.25);
        assert_eq!(select("/perf/averages"), state["performance"]["averages"]);
        assert_eq!(select("/state/a~1b/~0key"), 7);
        assert_eq!(select("/state/space%20key"), 8);
        assert_eq!(select("/state?history=1"), state);
        assert!(select("/perf")["performance"].get("history").is_none());
        assert_eq!(
            select("/perf?history=1")["performance"],
            state["performance"]
        );
        for path in [
            "/state/missing",
            "/perf/missing",
            "/state/selection/key",
            "/state/camera/position_pc/3",
        ] {
            assert_eq!(select_snapshot(&state, path).unwrap_err().0, 404, "{path}");
        }
        for path in [
            "/state/%",
            "/state/%GG",
            "/state/%FF",
            "/perf/latest?history=1",
            "/state?unknown=1",
        ] {
            assert_eq!(select_snapshot(&state, path).unwrap_err().0, 400, "{path}");
        }
    }

    #[test]
    fn history_queries_limit_seconds_relative_to_the_latest_sample() {
        let state = json!({"performance": {
            "latest": {"elapsed_seconds": 12.0},
            "history": [{"elapsed_seconds": 9.0}, {"elapsed_seconds": 10.0}, {"elapsed_seconds": 12.0}],
        }});
        let expected = json!([{"elapsed_seconds": 10.0}, {"elapsed_seconds": 12.0}]);
        assert_eq!(
            select_snapshot(&state, "/perf/history?seconds=2").unwrap(),
            expected
        );
        assert_eq!(
            select_snapshot(&state, "/state/performance/history?seconds=2").unwrap(),
            expected
        );
        assert_eq!(
            select_snapshot(&state, "/perf/history")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            select_snapshot(&state, "/perf/history?seconds=0.5")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        for query in [
            "seconds=0",
            "seconds=-1",
            "seconds=11",
            "seconds=NaN",
            "seconds=inf",
            "seconds=abc",
            "seconds=",
            "seconds=1&seconds=2",
            "history=1",
        ] {
            assert_eq!(
                select_snapshot(&state, &format!("/perf/history?{query}"))
                    .unwrap_err()
                    .0,
                400,
                "{query}"
            );
        }
        assert_eq!(
            select_snapshot(&state, "/perf?seconds=2").unwrap_err().0,
            400
        );
        let empty = json!({"performance": {"latest": null, "history": []}});
        assert_eq!(
            select_snapshot(&empty, "/perf/history?seconds=2").unwrap(),
            json!([])
        );
    }

    #[test]
    fn commands_are_forwarded_on_the_main_thread_with_a_per_frame_limit() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let mut app = App::new();
        app.add_message::<Typed>()
            .insert_resource(HttpApi {
                receiver: Mutex::new(receiver),
                stop: Arc::new(AtomicBool::new(false)),
            })
            .insert_resource(Universe(space_simulation::demo_galaxy()))
            .insert_resource(Navigation::default())
            .insert_resource(Clock::default())
            .insert_resource(CameraRig::default())
            .insert_resource(CameraPose::default())
            .insert_resource(Starfield::default())
            .add_systems(PreUpdate, receive);
        let (reply, _verdicts) = mpsc::sync_channel(10);
        for _ in 0..10 {
            sender
                .send(Job::Command("set_debug bloom false".into(), reply.clone()))
                .unwrap();
        }
        let mut cursor = bevy::ecs::message::MessageCursor::<Typed>::default();
        app.update();
        let typed = app.world().resource::<Messages<Typed>>();
        let first: Vec<_> = cursor.read(typed).collect();
        assert_eq!(first.len(), PER_FRAME);
        assert!(
            first
                .iter()
                .all(|t| t.source == Source::Api && t.reply.is_some())
        );
        app.update();
        assert_eq!(
            cursor
                .read(app.world().resource::<Messages<Typed>>())
                .count(),
            2
        );
    }

    #[test]
    fn localhost_api_reads_snapshots_validates_commands_and_rejects_browser_requests() {
        let server = Server::http((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let shared = Arc::new(Mutex::new(Some(Arc::new(json!({
            "schema_version": 1, "magnitude": 6.5,
            "performance": {"latest": {"sequence": 42}, "history": [1, 2]},
        })))));
        let api = Arc::new(start(server, shared, REPLY_WAIT).unwrap());
        // Stands in for the main thread: refuses `jump missing`, runs the rest.
        let done = Arc::new(AtomicBool::new(false));
        let main_thread = {
            let (api, done) = (api.clone(), done.clone());
            std::thread::spawn(move || {
                let mut lines = Vec::new();
                while !done.load(Ordering::Relaxed) {
                    let job = api
                        .receiver
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_millis(10));
                    if let Ok(Job::Command(line, reply)) = job {
                        let verdict = if line == "jump missing" {
                            Err("Unknown list `missing`".to_owned())
                        } else {
                            Ok(())
                        };
                        reply.send(verdict).unwrap();
                        lines.push(line);
                    }
                }
                lines
            })
        };
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}");
        let get = |path: &str| client.get(format!("{url}{path}")).send().unwrap();
        assert_eq!(get("/health").status(), 200);
        assert_eq!(
            get("/commands/set_debug").json::<Value>().unwrap()["name"],
            "set_debug"
        );
        assert!(get("/commands").json::<Value>().unwrap().is_array());
        assert_eq!(
            get("/perf/latest?keys=1").json::<Value>().unwrap(),
            json!(["sequence"])
        );
        let state: Value = get("/state").json().unwrap();
        assert_eq!(state["magnitude"], 6.5);
        assert!(state["performance"].get("history").is_none());
        let perf: Value = get("/perf?history=1").json().unwrap();
        assert_eq!(perf["performance"]["history"], json!([1, 2]));
        assert_eq!(get("/missing").status(), 404);
        assert_eq!(get("/state/magnitude").json::<Value>().unwrap(), 6.5);
        assert_eq!(get("/perf/latest/sequence").json::<Value>().unwrap(), 42);
        assert_eq!(get("/perf/history/0").json::<Value>().unwrap(), 1);
        assert_eq!(get("/perf/unknown").status(), 404);
        assert_eq!(get("/perf/history?seconds=20").status(), 400);
        assert_eq!(
            client
                .post(format!("{url}/perf/latest"))
                .header("X-Night-Sky-Client", "cli")
                .send()
                .unwrap()
                .status(),
            405
        );
        let post = |body: &str| {
            client
                .post(format!("{url}/command"))
                .header("X-Night-Sky-Client", "cli")
                .body(body.to_owned())
                .send()
                .unwrap()
        };
        let accepted = post("set_debug split_multiples false");
        assert_eq!(accepted.status(), 202);
        assert_eq!(accepted.json::<Value>().unwrap()["status"], "accepted");
        assert_eq!(
            post(r#"{"type":"set_magnitude","magnitude":5.5}"#).status(),
            202
        );
        let refused = post("jump missing");
        assert_eq!(refused.status(), 409);
        assert_eq!(
            refused.json::<Value>().unwrap()["error"],
            "Unknown list `missing`"
        );
        assert_eq!(post("not_a_command").status(), 400);
        // Out-of-range values never reach the main thread.
        assert_eq!(post("set_fov 500").status(), 400);
        assert_eq!(post(&"x".repeat(BODY_LIMIT + 1)).status(), 413);
        assert_eq!(
            client
                .post(format!("{url}/command"))
                .body("set_debug bloom false")
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{url}/state"))
                .header("Origin", "https://example.com")
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .get(format!("{url}/state"))
                .header("Host", "evil.example")
                .send()
                .unwrap()
                .status(),
            403
        );
        assert_eq!(
            client
                .post(format!("{url}/state"))
                .header("X-Night-Sky-Client", "cli")
                .send()
                .unwrap()
                .status(),
            405
        );
        done.store(true, Ordering::Relaxed);
        assert_eq!(
            main_thread.join().unwrap(),
            [
                "set_debug split_multiples false",
                r#"{"type":"set_magnitude","magnitude":5.5}"#,
                "jump missing"
            ]
        );
        drop(api);
    }

    #[test]
    fn a_stalled_main_thread_fills_the_queue_then_refuses_commands() {
        let server = Server::http((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let api = start(server, Arc::new(Mutex::new(None)), Duration::from_millis(1)).unwrap();
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let post = || {
            let response = client
                .post(format!("http://127.0.0.1:{port}/command"))
                .header("X-Night-Sky-Client", "cli")
                .body("set_debug bloom false")
                .send()
                .unwrap();
            let status = response.status();
            (status, response.json::<Value>().unwrap())
        };
        for _ in 0..QUEUE_LIMIT {
            let (status, body) = post();
            assert_eq!(status, 202);
            assert_eq!(body["status"], "queued");
        }
        assert_eq!(post().0, 503);
        drop(api);
    }

    #[test]
    fn screenshot_endpoint_reports_scheduling_not_finished_pngs() {
        let server = Server::http((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let api = start(server, Arc::new(Mutex::new(None)), REPLY_WAIT).unwrap();
        let request = std::thread::spawn(move || {
            reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(format!("http://127.0.0.1:{port}/screenshot"))
                .header("X-Night-Sky-Client", "cli")
                .send()
                .unwrap()
        });
        let job = api
            .receiver
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let Job::Capture(reply) = job else {
            panic!("expected capture job")
        };
        reply.send(json!({"status": "capture_scheduled"})).unwrap();
        let response = request.join().unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(
            response.json::<Value>().unwrap()["status"],
            "capture_scheduled"
        );
    }
}
