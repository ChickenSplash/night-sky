use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use space_shared::Galaxy;
use space_simulation::demo_galaxy;
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
    // A client may survive a backend restart: its old ETag must not match the
    // first snapshot of the new process even when both have revision zero.
    let generation = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let state: AppState = Arc::new(RwLock::new(Snapshot {
        galaxy: demo_galaxy(),
        generation,
        revision: 0,
    }));
    let bind = std::env::var("SPACE_BIND").unwrap_or_else(|_| "127.0.0.1:4321".into());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .context("Cannot bind port 4321. Is another Night Sky server running?")?;
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"service": "night-sky", "status": "ok"})) }),
        )
        .route("/galaxy", get(galaxy))
        .fallback_service(ServeDir::new(root.join("web/dist")).precompressed_gzip())
        .layer(CompressionLayer::new())
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-cache"),
        ))
        .with_state(state);
    println!("Night Sky: http://{bind} (Ctrl+C to stop)");
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
