//! Background poller: keeps the cached health fresh and ingests the rig's log
//! files into SQLite.
//!
//! The spider is on a light switch, so every failure path here logs and continues:
//! an unreachable rig must never take the web server (or the poller) down.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::config::Settings;
use crate::db::Db;
use crate::spider::{parse_activation_lines, Health, Spider};
use crate::util::now_stamp;

/// What the pages render instead of a live spider call.
#[derive(Clone, Default, Serialize)]
pub struct Status {
    /// Last poll reached the rig (a `503` counts as offline).
    pub online: bool,
    /// Last health we saw; kept while offline so the page can show mode/uptime.
    pub health: Option<Health>,
    /// When `/health` last answered.
    pub last_ok: Option<String>,
    /// Why the rig is offline, when it is.
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Settings>,
    pub db: Db,
    pub spider: Spider,
    pub status: Arc<RwLock<Status>>,
}

/// Spawn the poll loop. The first tick is skipped, so the first poll lands one
/// interval after startup.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(state.cfg.poll_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // the immediate first tick; skip it
        loop {
            ticker.tick().await;
            if let Err(e) = poll_once(&state).await {
                tracing::warn!(error = %e, "poll cycle failed");
            }
        }
    })
}

/// One `/health` poll, plus log ingest when the clock is set. Also driven on
/// demand by the home page's refresh button.
pub(crate) async fn poll_once(state: &AppState) -> anyhow::Result<()> {
    match state.spider.health().await {
        Ok(Some(health)) => {
            let clock_set = health.clock_set;
            let mut status = state.status.write().await;
            status.online = true;
            status.health = Some(health);
            status.last_ok = Some(now_stamp());
            status.last_error = None;
            drop(status);

            if clock_set {
                if let Err(e) = ingest_logs(state).await {
                    tracing::warn!(error = %e, "log ingest failed");
                }
            }
        }
        Ok(None) => {
            let mut status = state.status.write().await;
            status.online = false;
            status.last_error = Some("rig reported 503 (link down)".to_string());
            tracing::warn!("rig answered 503 on /health");
        }
        Err(e) => {
            let mut status = state.status.write().await;
            status.online = false;
            status.last_error = Some(e.to_string());
            tracing::warn!(error = %e, "health poll failed");
        }
    }
    Ok(())
}

/// Fetch the log listing and pull in every file whose size changed since the last
/// fetch. Files are re-read whole and deduped by stamp in SQLite, so a growing file
/// costs one request and adds only its new activations.
async fn ingest_logs(state: &AppState) -> anyhow::Result<()> {
    let listing = match state.spider.log_list().await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %e, "log listing failed");
            return Ok(());
        }
    };
    if listing.truncated {
        tracing::debug!("rig truncated the log listing to the newest files");
    }

    for entry in &listing.logs {
        if !crate::spider::is_log_name(&entry.name) {
            tracing::warn!(name = %entry.name, "skipping listing entry with bad name");
            continue;
        }
        if state.db.log_file_size(&entry.name).await? == Some(entry.size) {
            continue; // unchanged since the last fetch
        }
        let body = match state.spider.log_file(&entry.name).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(name = %entry.name, error = %e, "log fetch failed");
                continue;
            }
        };
        let now = now_stamp();
        let stamps = parse_activation_lines(&body);
        let added = state
            .db
            .insert_activations(&entry.name, &stamps, &now)
            .await?;
        state
            .db
            .upsert_log_file(&entry.name, entry.size, &now)
            .await?;
        if added > 0 {
            tracing::info!(file = %entry.name, added, total_lines = stamps.len(), "ingested activations");
        }
    }
    Ok(())
}
