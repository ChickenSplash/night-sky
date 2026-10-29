use super::search::search_request;
use super::state::select_snapshot;
use super::*;
use crate::devtools::command_line::{Source, Typed};
use crate::scene::{CameraPose, CameraRig};
use crate::search::Asker;
use crate::stars::stream::Starfield;
use crate::{Clock, Navigation, Universe};

fn hub() -> Arc<Hub> {
    Arc::new(Hub::new())
}

#[test]
fn search_bodies_are_text_or_json_with_an_origin() {
    let camera = || Some([1.0, 2.0, 3.0]);
    let (query, origin, cap) = search_request("kind=star within_ly=5", camera, 9).unwrap();
    assert_eq!(query.to_text(), "kind=star within_ly=5");
    assert_eq!((origin, cap), ([1.0, 2.0, 3.0], 9));
    let home = space_simulation::galaxy::GalaxyModel::milky_way().home_pc;
    let (query, origin, cap) = search_request(
        r#"{"query":{"kinds":["planet"],"scope":{"within":{"radius_ly":20}}},"origin":"home","work_cap":7}"#,
        || None,
        9,
    )
    .unwrap();
    assert_eq!(query.to_text(), "kind=planet within_ly=20");
    assert_eq!((origin, cap), (home, 7));
    let (_, origin, _) =
        search_request(r#"{"query":"kind=star","origin":[4,5,6]}"#, || None, 9).unwrap();
    assert_eq!(origin, [4.0, 5.0, 6.0]);
    for (body, error) in [
        ("kind=star", "No camera position"),
        ("colour=red", "unknown property"),
        (
            r#"{"query":"kind=star","origin":"mars"}"#,
            "`origin` must be",
        ),
        (
            r#"{"query":"kind=star","origin":[1,2]}"#,
            "`origin` must be",
        ),
        (
            r#"{"query":"kind=star","from":"home"}"#,
            "unknown field `from`",
        ),
        (r#"{"query":3}"#, "must be text or a query object"),
        (
            r#"{"query":"kind=star","origin":"home","work_cap":0}"#,
            "above 0",
        ),
    ] {
        let refused = search_request(body, || None, 9).unwrap_err();
        assert!(refused.contains(error), "{body}: {refused}");
    }
}

#[test]
fn search_endpoints_submit_read_cancel_and_refuse() {
    let server = Server::http((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let hub = hub();
    let api = Arc::new(start(server, Arc::new(Mutex::new(None)), hub.clone(), REPLY_WAIT).unwrap());
    // Stands in for the main thread, which sends searches with no craft.
    let done = Arc::new(AtomicBool::new(false));
    let main_thread = {
        let (api, done) = (api.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                let job = api
                    .receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_millis(10));
                if let Ok(Job::Search(job, reply)) = job {
                    let sent = hub.submit(Asker::Api, job.query, job.origin_pc, job.work_cap, &[]);
                    reply.send(sent).unwrap();
                }
            }
        })
    };
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://127.0.0.1:{port}");
    let post = |path: &str, body: &str| {
        let response = client
            .post(format!("{url}{path}"))
            .header("X-Night-Sky-Client", "cli")
            .body(body.to_owned())
            .send()
            .unwrap();
        (
            response.status().as_u16(),
            response.json::<Value>().unwrap(),
        )
    };
    let get = |path: &str| {
        let response = client.get(format!("{url}{path}")).send().unwrap();
        (
            response.status().as_u16(),
            response.json::<Value>().unwrap(),
        )
    };
    let (status, catalogue) = get("/catalogue");
    assert_eq!(status, 200);
    assert!(catalogue["relations"].as_array().unwrap().len() >= 2);
    assert!(
        catalogue["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|k| k["source"] != "none")
    );
    let (status, body) = post("/search", "kind=star within_ly=5");
    assert_eq!(status, 400, "no snapshot, so no camera");
    assert!(body["error"].as_str().unwrap().contains("give an origin"));
    let (status, body) = post(
        "/search",
        r#"{"query":"kind=star within_ly=10","origin":"home"}"#,
    );
    assert_eq!(status, 202, "{body}");
    let id = body["id"].as_u64().unwrap();
    let entry = loop {
        let (status, entry) = get(&format!("/search/{id}"));
        assert_eq!(status, 200);
        if entry["progress"]["state"] != "running" {
            break entry;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(entry["progress"]["state"], "done");
    assert_eq!(entry["asker"], "api");
    assert!(!entry["results"].as_array().unwrap().is_empty());
    assert_eq!(entry["results"][0]["kind"], "star");
    let (_, list) = get("/search");
    assert_eq!(list[0]["id"], id);
    assert!(list[0].get("results").is_none());
    let (status, body) = post("/search", r#"{"query":"within_ly=50","origin":[0,0,0]}"#);
    assert_eq!(status, 202);
    let centre = body["id"].as_u64().unwrap();
    assert_eq!(post(&format!("/search/{centre}/cancel"), "").0, 202);
    assert_eq!(get("/search/999").0, 404);
    assert_eq!(get("/search/x").0, 404);
    assert_eq!(post("/search/999/cancel", "").0, 404);
    let (status, body) = post("/search", "orbits.planet_count=1..");
    assert_eq!(status, 400);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("applies to system, not star, planet")
    );
    assert_eq!(
        client
            .post(format!("{url}/search"))
            .body("kind=star")
            .send()
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .delete(format!("{url}/catalogue"))
            .send()
            .unwrap()
            .status(),
        405
    );
    done.store(true, Ordering::Relaxed);
    main_thread.join().unwrap();
    drop(api);
}

#[test]
fn discovery_uses_the_shared_command_catalogue_and_snapshot_children() {
    let catalogue = command_catalogue("/commands").unwrap();
    assert_eq!(
        catalogue.as_array().unwrap().len(),
        crate::devtools::command_line::USAGE.len()
    );
    for usage in &crate::devtools::command_line::USAGE {
        let entry = command_catalogue(&format!("/commands/{}", usage.name)).unwrap();
        assert_eq!(entry["fields"], json!(usage.fields));
        assert_eq!(
            entry["synopsis"],
            crate::devtools::command_line::synopsis(usage)
        );
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
        .init_resource::<crate::search::Searches>()
        .init_resource::<crate::craft::Fleet>()
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
    let frame = |sequence| crate::stars::stream::FrameSample {
        sequence,
        elapsed_seconds: 0.0,
        frame_interval_ms: 0.0,
        split: Default::default(),
        sync: Default::default(),
        instrumentation: Default::default(),
        craft: Default::default(),
    };
    let shared = Arc::new(Mutex::new(Some(Published {
        state: Arc::new(json!({
            "schema_version": 1, "magnitude": 6.5,
            "performance": {"latest": {"sequence": 42}, "history": []},
        })),
        history: Arc::from([frame(1), frame(2)]),
    })));
    let api = Arc::new(start(server, shared, hub(), REPLY_WAIT).unwrap());
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
        get("/craft/schema").json::<Value>().unwrap()["command"],
        "steer_craft"
    );
    assert_eq!(
        get("/perf/latest?keys=1").json::<Value>().unwrap(),
        json!(["sequence"])
    );
    let state: Value = get("/state").json().unwrap();
    assert_eq!(state["magnitude"], 6.5);
    assert!(state["performance"].get("history").is_none());
    let perf: Value = get("/perf?history=1").json().unwrap();
    assert_eq!(perf["performance"]["history"][1]["sequence"], 2);
    assert_eq!(get("/missing").status(), 404);
    assert_eq!(get("/state/magnitude").json::<Value>().unwrap(), 6.5);
    assert_eq!(get("/perf/latest/sequence").json::<Value>().unwrap(), 42);
    assert_eq!(get("/perf/history/0/sequence").json::<Value>().unwrap(), 1);
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
    let api = start(
        server,
        Arc::new(Mutex::new(None)),
        hub(),
        Duration::from_millis(1),
    )
    .unwrap();
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
    let api = start(server, Arc::new(Mutex::new(None)), hub(), REPLY_WAIT).unwrap();
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
