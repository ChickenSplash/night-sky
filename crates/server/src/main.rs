mod clickup;
mod storage;

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use space_shared::{Galaxy, demo_galaxy};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tower_http::{compression::CompressionLayer, services::ServeDir};

struct Snapshot {
    galaxy: Galaxy,
    generation: u128,
    revision: u64,
}

type AppState = Arc<RwLock<Snapshot>>;

#[tokio::main]
async fn main() -> Result<()> {
    let root = std::env::var_os("SPACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    dotenvy::from_path(root.join(".env")).ok();
    std::fs::create_dir_all(root.join("data"))?;
    let db_path = root.join("data/space.sqlite3");
    storage::initialise(&db_path)?;
    let token = std::env::var("CLICKUP_TOKEN").unwrap_or_default();
    let mut lists: Vec<String> = std::env::var("CLICKUP_LIST_IDS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    anyhow::ensure!(
        lists
            .iter()
            .all(|id| id.bytes().all(|b| b.is_ascii_digit())),
        "CLICKUP_LIST_IDS must contain numeric list IDs"
    );
    anyhow::ensure!(
        token.is_empty() == lists.is_empty(),
        "Set both CLICKUP_TOKEN and CLICKUP_LIST_IDS, or leave both empty for demo mode"
    );
    lists.sort();
    lists.dedup();
    let cache_key = lists.join(",");
    let initial = if lists.is_empty() {
        demo_galaxy()
    } else {
        storage::load(&db_path, &cache_key)?
            .map(|mut g| {
                // Older caches used whole-list orbital radii; derive them from slots again.
                for task in g.systems.iter_mut().flat_map(|s| s.tasks.iter_mut()) {
                    let slot = task.stable_slot();
                    task.slot = Some(slot);
                    task.orbit = space_simulation::orbit_for_slot(&task.id, slot);
                }
                g.source = "ClickUp · cached, awaiting sync".into();
                g
            })
            .unwrap_or_else(|| Galaxy {
                source: "ClickUp · connecting".into(),
                systems: vec![],
            })
    };
    // A client may survive a backend restart: its old ETag must not match the
    // first snapshot of the new process even when both have revision zero.
    let generation = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let state: AppState = Arc::new(RwLock::new(Snapshot {
        galaxy: initial,
        generation,
        revision: 0,
    }));
    let bind = std::env::var("SPACE_BIND").unwrap_or_else(|_| "127.0.0.1:4321".into());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .context("Cannot bind port 4321. Is another ClickUp Space server running?")?;
    if !lists.is_empty() {
        let client = clickup::ClickUp::new(&token)?;
        let state = state.clone();
        tokio::spawn(async move {
            loop {
                let mut failures = 0;
                for (index, id) in lists.iter().enumerate() {
                    {
                        let mut snapshot = state.write().await;
                        snapshot.galaxy.source =
                            format!("ClickUp · loading list {}/{}", index + 1, lists.len());
                        snapshot.revision += 1;
                    }
                    println!("Loading list {}/{} ({id})", index + 1, lists.len());
                    match client.system(id, &db_path).await {
                        Ok(system) => {
                            println!("Loaded {}: {} tasks", system.name, system.tasks.len());
                            let snapshot = {
                                let mut snapshot = state.write().await;
                                let galaxy = &mut snapshot.galaxy;
                                if let Some(existing) =
                                    galaxy.systems.iter_mut().find(|s| s.id == system.id)
                                {
                                    *existing = system;
                                } else {
                                    galaxy.systems.push(system);
                                }
                                snapshot.revision += 1;
                                snapshot.galaxy.clone()
                            };
                            if let Err(err) = storage::save(&db_path, &cache_key, &snapshot) {
                                eprintln!("Could not save cache: {err}");
                            }
                        }
                        Err(err) => {
                            failures += 1;
                            eprintln!("List {id} sync failed: {err:#}");
                        }
                    }
                }
                let mut snapshot = state.write().await;
                snapshot.galaxy.source = if failures == 0 {
                    format!("ClickUp · {} lists synced (5 minute refresh)", lists.len())
                } else {
                    format!(
                        "ClickUp · {failures}/{} lists failed; see server log",
                        lists.len()
                    )
                };
                snapshot.revision += 1;
                drop(snapshot);
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            }
        });
    }
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"service": "clickup-space", "status": "ok"})) }),
        )
        .route("/galaxy", get(galaxy))
        .fallback_service(ServeDir::new(root.join("web/dist")).precompressed_gzip())
        .layer(CompressionLayer::new())
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        ))
        .with_state(state);
    println!("ClickUp Space: http://{bind} (Ctrl+C to stop)");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
        })
        .await?;
    Ok(())
}

async fn galaxy(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let snapshot = state.read().await;
    let tag = format!("\"{}-{}\"", snapshot.generation, snapshot.revision);
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| value == tag.as_str())
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
    }
    ([(header::ETAG, tag)], Json(&snapshot.galaxy)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn galaxy_only_sends_changed_snapshots() {
        let state = Arc::new(RwLock::new(Snapshot {
            galaxy: demo_galaxy(),
            generation: 42,
            revision: 3,
        }));
        let response = galaxy(State(state.clone()), HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let tag = response.headers()[header::ETAG].clone();
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, tag.clone());
        let response = galaxy(State(state.clone()), headers.clone()).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()[header::ETAG], tag);
        state.write().await.revision += 1;
        assert_eq!(galaxy(State(state), headers).await.status(), StatusCode::OK);
    }
}
