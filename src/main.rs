//! LAN web UI in front of the ESP32 spider prop's HTTP API.
//!
//! The rig sends no CORS headers and serves no static files, so the browser only
//! ever talks to this origin; every spider call is made server-side by `reqwest`.

mod assets;
mod config;
mod db;
mod error;
mod handlers;
mod logs;
mod modes;
mod poller;
mod spider;
mod templates;
mod tuning;
mod util;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, serve};
use tokio::sync::RwLock;

use crate::poller::{AppState, Status};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Arc::new(config::Settings::from_env()?);
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        ))
        .init();

    let db = db::Db::open(&cfg.db_path)?;
    let spider = spider::Spider::new(
        &cfg.spider_url,
        Duration::from_secs(cfg.request_timeout_secs),
    )?;
    let state = AppState {
        cfg: cfg.clone(),
        db,
        spider,
        status: Arc::new(RwLock::new(Status::default())),
        last_ingest: Arc::new(tokio::sync::Mutex::new(None)),
    };
    poller::spawn(state.clone());

    let app = Router::new()
        .route("/", get(handlers::home))
        .route("/health/partial", get(handlers::health_partial))
        .route("/health/refresh", post(handlers::health_refresh))
        .route("/mode", post(handlers::mode_post))
        .route("/logs/clear", post(handlers::logs_clear))
        .route("/config", get(handlers::config_page).post(handlers::config_post))
        .route("/config/reset", post(handlers::config_reset))
        .route("/logs", get(handlers::logs_page))
        .route("/logs/data", get(handlers::logs_data))
        .route("/static/{*path}", get(static_asset))
        .fallback(not_found)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cfg.listen).await?;
    tracing::info!(
        listen = %cfg.listen,
        spider = %cfg.spider_url,
        db = %cfg.db_path.display(),
        poll_secs = cfg.poll_secs,
        "esp32 spider control server up"
    );
    serve(listener, app).await?;
    Ok(())
}

async fn static_asset(Path(path): Path<String>) -> Response {
    match assets::get(&path) {
        Some(asset) => (
            [
                (header::CONTENT_TYPE, asset.mime),
                // Embedded assets, served from memory: revalidate every load so a new
                // binary's CSS/JS is never shadowed by an hour-old cached copy.
                (header::CACHE_CONTROL, "no-cache"),
            ],
            asset.body,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}
