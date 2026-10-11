//! Request handlers. Every page render reads the cached `Status`, never a live
//! spider call, so a page never hangs on an unreachable rig.

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Form, Query, State};
use axum::response::Html;
use axum::Json;
use serde::Deserialize;

use crate::error::AppError;
use crate::logs::{bucket, labels, LogsData, SLOTS};
use crate::modes;
use crate::poller::AppState;
use crate::templates::{self, Banner, ConfigPageView, FileView, HealthView, HomeView, LogsView};
use crate::tuning::{self, Change};
use crate::util;

/// `GET /`: health plus the mode controls.
pub async fn home(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let status = state.status.read().await.clone();
    let health_html = templates::render(&HealthView::new(&status, None))?.0;
    let mode_buttons_html = templates::render(&templates::mode_buttons_view(
        status.health.as_ref().map(|h| h.mode),
        false,
    ))?
    .0;
    let (base_url, poll_secs) = templates::page_context(&state.cfg);
    templates::render(&HomeView {
        health_html,
        mode_buttons_html,
        channels: (0..modes::CHANNELS).collect(),
        base_url,
        poll_secs,
    })
}

/// `GET /health/partial`: the fragment HTMX re-fetches every 5 s.
pub async fn health_partial(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    render_health(&state, None).await
}

/// `POST /health/refresh`: the home page's manual refresh. Runs one `/health` poll
/// (and the log ingest it triggers) right now, then reports back in the fragment.
pub async fn health_refresh(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    if let Err(e) = crate::poller::poll_once(&state).await {
        tracing::warn!(error = %e, "manual health refresh failed");
    }
    render_health(&state, None).await
}

#[derive(Deserialize)]
pub struct ModeForm {
    pub m: u16,
    pub c: Option<u16>,
}

/// `POST /mode`: select the effect, remember it, and report back in the fragment.
pub async fn mode_post(
    State(state): State<AppState>,
    Form(form): Form<ModeForm>,
) -> Result<Html<String>, AppError> {
    // Mode the buttons should mark active: the one just accepted, else the cached one.
    let mut selected = None;
    let banner = if !modes::is_valid(form.m) {
        Some(Banner::error(format!("unknown mode {}", form.m)))
    } else if form.m == 1 && form.c.is_none_or(|c| c >= modes::CHANNELS) {
        Some(Banner::error(format!(
            "mode 1 needs a channel 0–{}",
            modes::CHANNELS - 1
        )))
    } else {
        match state.spider.set_mode(form.m, form.c).await {
            Ok(()) => {
                let mut status = state.status.write().await;
                if let Some(health) = status.health.as_mut() {
                    health.mode = form.m;
                }
                drop(status);
                selected = Some(form.m);
                tracing::info!(mode = form.m, channel = ?form.c, "mode set");
                Some(Banner::ok(format!("mode set to {}", modes::name(form.m))))
            }
            Err(e) => Some(Banner::error(e.to_string())),
        }
    };

    // Refresh the health fragment; on an accepted mode also refresh the mode buttons
    // out of band, so the active button flips without a full page reload. A rejected
    // click leaves the buttons untouched. `selected` carries the chosen mode even
    // before the first poll has populated `status.health`.
    let mut body = render_health(&state, banner).await?.0;
    if selected.is_some() {
        let buttons_html = templates::render(&templates::mode_buttons_view(selected, true))?.0;
        body.push_str(&buttons_html);
    }
    Ok(Html(body))
}

/// `POST /logs/clear`: wipe the rig's log files *and* the local history, which is
/// what the button's confirmation text promises.
pub async fn logs_clear(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let banner = match state.spider.log_clear().await {
        Ok(()) => {
            state.db.clear_activations().await?;
            tracing::info!("cleared spider logs and local history");
            Some(Banner::ok("cleared spider logs and local history"))
        }
        Err(e) => Some(Banner::error(e.to_string())),
    };
    render_health(&state, banner).await
}

/// `GET /logs`: one UTC day of activation history, from SQLite — the rig does not
/// have to be reachable, and a past day keeps rendering after a log clear.
pub async fn logs_page(
    State(state): State<AppState>,
    Query(query): Query<LogsQuery>,
) -> Result<Html<String>, AppError> {
    let day = valid_day(query.day.as_deref());
    let date = parse_day(&day);
    let today = today();
    let stamps = state.db.activations_for_day(&day).await?;
    let files = state.db.log_files_for_day(&compact_day(&day)).await?;
    let (base_url, poll_secs) = templates::page_context(&state.cfg);

    templates::render(&LogsView {
        day: day.clone(),
        prev_day: date
            .pred_opt()
            .unwrap_or(date)
            .format("%Y-%m-%d")
            .to_string(),
        next_day: date
            .succ_opt()
            .unwrap_or(date)
            .format("%Y-%m-%d")
            .to_string(),
        today: today.clone(),
        is_today: day == today,
        has_files: !files.is_empty(),
        files: files
            .into_iter()
            .map(|(name, size)| FileView {
                name,
                size_text: util::human_size(size),
            })
            .collect(),
        no_activations: stamps.is_empty(),
        base_url,
        poll_secs,
    })
}

/// `GET /logs/data`: the 288 five-minute buckets the chart draws.
pub async fn logs_data(
    State(state): State<AppState>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<LogsData>, AppError> {
    let day = valid_day(query.day.as_deref());
    let stamps = state.db.activations_for_day(&day).await?;
    let mut counts = vec![0u32; SLOTS];
    for stamp in &stamps {
        if let Some(slot) = bucket(stamp) {
            counts[slot] += 1;
        }
    }
    Ok(Json(LogsData {
        day: day.clone(),
        labels: labels(),
        counts,
        total: stamps.len(),
        files: state.db.log_files_for_day(&compact_day(&day)).await?,
        first: stamps.first().cloned(),
        last: stamps.last().cloned(),
    }))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub day: Option<String>,
}

/// A day that is not a `YYYY-MM-DD` date falls back to today rather than erroring:
/// a bad `?day=` must never 400 a navigation link.
fn valid_day(day: Option<&str>) -> String {
    day.and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(today)
}

fn parse_day(day: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").expect("validated day")
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// `2026-10-11` -> `20261011`, the prefix of that day's log-file names.
fn compact_day(day: &str) -> String {
    day.replace('-', "")
}

async fn render_health(
    state: &AppState,
    banner: Option<Banner>,
) -> Result<Html<String>, AppError> {
    let status = state.status.read().await.clone();
    templates::render(&HealthView::new(&status, banner))
}

/// `GET /config`: the live knob table, or the cached one when the rig is off.
pub async fn config_page(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    let (values, offline, cache_age) = live_or_cached_config(&state).await?;
    let form_html = templates::render(&templates::config_form(&values, None, offline, cache_age))?.0;
    let (base_url, poll_secs) = templates::page_context(&state.cfg);
    templates::render(&ConfigPageView {
        form_html,
        base_url,
        poll_secs,
    })
}

/// `POST /config`: diff the form against the live values, validate locally, then send
/// only the changed knobs in batches of 16.
pub async fn config_post(
    State(state): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    // 1. Local validation: a batch we can reject ourselves never touches the rig.
    let mut submitted: Vec<Change> = Vec::new();
    for (name, raw) in form {
        if raw.is_empty() {
            continue;
        }
        if tuning::knob(&name).is_none() {
            return local_config_error(&state, &submitted, format!("unknown field {name}")).await;
        }
        let Ok(value) = raw.parse::<u32>() else {
            return local_config_error(&state, &submitted, format!("value out of range: {name}")).await;
        };
        submitted.push(Change { name, value });
    }
    submitted.sort_by(|a, b| a.name.cmp(&b.name));
    if let Err(field) = tuning::check_ranges(&submitted) {
        return local_config_error(&state, &submitted, format!("value out of range: {field}")).await;
    }

    // 2. The live values are what the diff and the constraint check are against.
    let current = match state.spider.config().await {
        Ok(values) => {
            cache_config(&state, &values).await?;
            values
        }
        Err(e) => {
            tracing::warn!(error = %e, "config fetch failed; nothing applied");
            let (values, _, age) = live_or_cached_config(&state).await?;
            return config_fragment(
                &overlay(values, &submitted),
                Some(Banner::error(format!("spider unreachable; nothing applied ({e})"))),
                true,
                age,
            )
            .await;
        }
    };

    // 3. Only what actually differs from the rig.
    let changes: Vec<Change> = submitted
        .into_iter()
        .filter(|c| current.get(&c.name).copied() != Some(c.value))
        .collect();
    if changes.is_empty() {
        return config_fragment(&current, Some(Banner::ok("no changes")), false, None).await;
    }

    // 4. Reject a batch the firmware would reject, naming the same field it would.
    let mut before: BTreeMap<String, u32> = current.iter().map(|(k, v)| (k.clone(), *v)).collect();
    let target = overlay_map(&before, &changes);
    if let Err(field) = tuning::check_constraints(&target) {
        return config_fragment(
            &overlay(current, &changes),
            Some(Banner::error(format!("inconsistent with another field: {field}"))),
            false,
            None,
        )
        .await;
    }

    // 5. Sequence the changes so every request the firmware sees is consistent on its
    //    own, then send at most 16 edits per request.
    let ordered = match tuning::order_changes(&before, &changes) {
        Ok(ordered) => ordered,
        Err(e) => {
            return config_fragment(
                &overlay(current, &changes),
                Some(Banner::error(format!("could not sequence the changes: {e}"))),
                false,
                None,
            )
            .await
        }
    };

    let mut applied = 0usize;
    let mut failure: Option<String> = None;
    for chunk in ordered.chunks(MAX_EDITS) {
        let edits: Vec<(String, u32)> = chunk
            .iter()
            .map(|c| (c.name.clone(), c.value))
            .collect();
        match state.spider.config_set(&edits).await {
            Ok(()) => {
                applied += chunk.len();
                for change in chunk {
                    before.insert(change.name.clone(), change.value);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "config batch rejected");
                failure = Some(e.to_string());
                break;
            }
        }
    }

    // 6. Show what the rig really holds now.
    let (values, offline, cache_age) = match state.spider.config().await {
        Ok(values) => {
            cache_config(&state, &values).await?;
            (values, false, None)
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not re-read config");
            (
                before.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                true,
                None,
            )
        }
    };
    let banner = match failure {
        Some(message) => Banner::error(message),
        None => Banner::ok(format!("saved {applied} knob(s)")),
    };
    config_fragment(&values, Some(banner), offline, cache_age).await
}

/// `POST /config/reset`: back to the firmware defaults.
pub async fn config_reset(State(state): State<AppState>) -> Result<Html<String>, AppError> {
    if let Err(e) = state.spider.config_reset().await {
        return config_fragment(
            &HashMap::new(),
            Some(Banner::error(e.to_string())),
            true,
            None,
        )
        .await;
    }

    let (values, offline, cache_age) = match state.spider.config().await {
        Ok(values) => {
            cache_config(&state, &values).await?;
            (values, false, None)
        }
        Err(e) => {
            tracing::warn!(error = %e, "reset applied but the re-read failed");
            (tuning::defaults().into_iter().collect(), true, None)
        }
    };
    config_fragment(
        &values,
        Some(Banner::ok("reset to firmware defaults")),
        offline,
        cache_age,
    )
    .await
}

/// The firmware's `kMaxEdits`: more arguments than this in one request is a `400`.
const MAX_EDITS: usize = 16;

const CACHE_KEY: &str = "config_cache";
const CACHE_AT_KEY: &str = "config_cache_at";

/// Live values when the rig answers, otherwise the cached ones and how old they are.
async fn live_or_cached_config(
    state: &AppState,
) -> Result<(HashMap<String, u32>, bool, Option<String>), AppError> {
    match state.spider.config().await {
        Ok(values) => {
            cache_config(state, &values).await?;
            Ok((values, false, None))
        }
        Err(e) => {
            tracing::warn!(error = %e, "config fetch failed; falling back to the cache");
            let cached: Option<HashMap<String, u32>> = state.db.settings_json(CACHE_KEY).await?;
            let at = state.db.get_setting(CACHE_AT_KEY).await?;
            Ok((
                cached.unwrap_or_default(),
                true,
                at.as_deref().map(util::human_age),
            ))
        }
    }
}

async fn cache_config(state: &AppState, values: &HashMap<String, u32>) -> anyhow::Result<()> {
    state.db.set_settings_json(CACHE_KEY, values).await?;
    state.db.set_setting(CACHE_AT_KEY, &util::now_stamp()).await
}

/// A batch rejected without asking the rig: show the cache with the submitted values
/// overlaid, so the user's edits survive to be corrected.
async fn local_config_error(
    state: &AppState,
    submitted: &[Change],
    message: String,
) -> Result<Html<String>, AppError> {
    let cached: Option<HashMap<String, u32>> = state.db.settings_json(CACHE_KEY).await?;
    config_fragment(
        &overlay(cached.unwrap_or_default(), submitted),
        Some(Banner::error(message)),
        false,
        None,
    )
    .await
}

fn overlay(mut values: HashMap<String, u32>, changes: &[Change]) -> HashMap<String, u32> {
    for change in changes {
        values.insert(change.name.clone(), change.value);
    }
    values
}

fn overlay_map(base: &BTreeMap<String, u32>, changes: &[Change]) -> BTreeMap<String, u32> {
    let mut values = base.clone();
    for change in changes {
        values.insert(change.name.clone(), change.value);
    }
    values
}

async fn config_fragment(
    values: &HashMap<String, u32>,
    banner: Option<Banner>,
    offline: bool,
    cache_age: Option<String>,
) -> Result<Html<String>, AppError> {
    templates::render(&templates::config_form(values, banner, offline, cache_age))
}
