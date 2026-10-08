use bevy::log::{info, warn};
use space_shared::Galaxy;
use std::{fmt::Display, sync::mpsc};

const OFFLINE: &str = "API unavailable · showing last data (demo on first launch)";

pub enum Update {
    Galaxy(Galaxy),
    /// The server still has the galaxy last received, so there is nothing to parse.
    Unchanged,
    Offline(String),
}

/// Polling state shared by both targets. Connection changes are logged once, not
/// on every poll.
#[derive(Default)]
struct Link {
    etag: Option<String>,
    online: Option<bool>,
}

impl Link {
    fn offline(&mut self, cause: impl Display, notice: &str) -> Update {
        if self.online != Some(false) {
            warn!("Galaxy API unavailable: {cause}");
        }
        self.online = Some(false);
        Update::Offline(notice.to_owned())
    }

    fn online(&mut self, update: Update) -> Update {
        if self.online != Some(true) {
            info!("Galaxy API connected");
        }
        self.online = Some(true);
        if let Update::Galaxy(galaxy) = &update {
            info!(
                "Galaxy received: {} systems, {} tasks ({})",
                galaxy.systems.len(),
                galaxy
                    .systems
                    .iter()
                    .map(|s| s.tasks().count())
                    .sum::<usize>(),
                galaxy.source
            );
        }
        update
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start() -> mpsc::Receiver<Update> {
    use reqwest::{StatusCode, header};
    let (tx, rx) = mpsc::channel();
    if std::env::args().any(|arg| arg == "--smoke-test") {
        let _ = tx.send(Update::Galaxy(space_simulation::demo_galaxy()));
        return rx;
    }
    let poll = move || {
        let mut link = Link::default();
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                let _ = tx.send(link.offline(format!("cannot create HTTP client: {err}"), OFFLINE));
                return;
            }
        };
        loop {
            let mut request = client.get("http://127.0.0.1:4321/galaxy");
            if let Some(etag) = &link.etag {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            let update = match request.send().and_then(|r| r.error_for_status()) {
                Ok(response) if response.status() == StatusCode::NOT_MODIFIED => {
                    link.online(Update::Unchanged)
                }
                Ok(response) => {
                    let etag = response
                        .headers()
                        .get(header::ETAG)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    match response.json::<Galaxy>() {
                        Ok(galaxy) => {
                            link.etag = etag;
                            link.online(Update::Galaxy(galaxy))
                        }
                        Err(err) => link.offline(err, "Invalid API response"),
                    }
                }
                Err(err) => link.offline(err, OFFLINE),
            };
            if tx.send(update).is_err() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    };
    if let Err(err) = std::thread::Builder::new()
        .name("galaxy-poll".into())
        .spawn(poll)
    {
        bevy::log::error!("Cannot start the galaxy poller: {err}");
    }
    rx
}

#[cfg(target_arch = "wasm32")]
pub fn start() -> mpsc::Receiver<Update> {
    let (tx, rx) = mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let mut link = Link::default();
        loop {
            let mut request = gloo_net::http::Request::get("/galaxy");
            if let Some(etag) = &link.etag {
                // Set by hand, so the browser passes 304 through instead of
                // replaying its cached body for us to parse again.
                request = request.header("If-None-Match", etag);
            }
            let update = match request.send().await {
                Err(err) => link.offline(err, OFFLINE),
                Ok(response) if response.status() == 304 => link.online(Update::Unchanged),
                Ok(response) if !response.ok() => {
                    link.offline(format!("HTTP {}", response.status()), OFFLINE)
                }
                Ok(response) => {
                    let etag = response.headers().get("etag");
                    match response.json::<Galaxy>().await {
                        Ok(galaxy) => {
                            link.etag = etag;
                            link.online(Update::Galaxy(galaxy))
                        }
                        Err(err) => link.offline(err, "Invalid API response"),
                    }
                }
            };
            if tx.send(update).is_err() {
                break;
            }
            gloo_timers::future::TimeoutFuture::new(2000).await;
        }
    });
    rx
}

#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> i64 {
    js_sys::Date::now() as i64
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
