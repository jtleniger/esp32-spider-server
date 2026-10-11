//! View structs for the Askama templates, plus the render helper.
//!
//! Askama renders to a `String`; there is no axum integration in 0.16, so handlers
//! wrap the result in `axum::response::Html` themselves.

use askama::Template;
use axum::response::Html;

use crate::config::Settings;
use crate::error::AppError;
use crate::modes;
use crate::poller::Status;
use crate::util;

pub fn render<T: Template>(template: &T) -> Result<Html<String>, AppError> {
    Ok(Html(template.render()?))
}

/// A one-line result message shown inside the fragment it belongs to.
pub struct Banner {
    pub ok: bool,
    pub text: String,
}

impl Banner {
    pub fn ok(text: impl Into<String>) -> Banner {
        Banner {
            ok: true,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Banner {
        Banner {
            ok: false,
            text: text.into(),
        }
    }
}

/// The health fragment. Every value comes from the cached `Status`, so it renders
/// instantly even while the rig is powered off.
#[derive(Template)]
#[template(path = "partials/health.html")]
pub struct HealthView {
    pub banner: Option<Banner>,
    pub online: bool,
    pub mode_label: String,
    pub uptime_text: Option<String>,
    pub clock_text: String,
    pub log_files: Option<u32>,
    pub free_text: Option<String>,
    pub last_ok_text: String,
    pub offline_reason: Option<String>,
    /// Offline and never seen since startup: say what to check.
    pub never_seen: bool,
}

impl HealthView {
    pub fn new(status: &Status, banner: Option<Banner>) -> HealthView {
        let health = status.health.as_ref();
        HealthView {
            banner,
            online: status.online,
            mode_label: match health {
                Some(h) => format!("{} ({})", modes::name(h.mode), h.mode),
                None => "unknown".to_string(),
            },
            uptime_text: health.map(|h| util::human_uptime(h.uptime_s)),
            clock_text: match health {
                Some(h) if h.clock_set => h.utc.clone(),
                Some(_) => "waiting for time sync".to_string(),
                None => "unknown".to_string(),
            },
            log_files: health.map(|h| h.log_files),
            free_text: health.map(|h| util::human_bytes(h.free_bytes)),
            last_ok_text: match &status.last_ok {
                Some(stamp) => format!("updated {}", util::human_age(stamp)),
                None => "never".to_string(),
            },
            offline_reason: if status.online {
                None
            } else {
                status.last_error.clone()
            },
            never_seen: !status.online && status.health.is_none(),
        }
    }
}

pub struct ModeButtonView {
    pub n: u16,
    pub name: &'static str,
    pub desc: &'static str,
    pub active: bool,
}

/// Mode buttons, with the running mode marked. Mode 1 is left out: it needs the
/// channel row.
pub fn mode_buttons(active: Option<u16>) -> Vec<ModeButtonView> {
    modes::MODES
        .iter()
        .filter(|m| m.n != 1)
        .map(|m| ModeButtonView {
            n: m.n,
            name: m.name,
            desc: m.desc,
            active: active == Some(m.n),
        })
        .collect()
}

/// The home page: the health fragment as already-rendered HTML (so the partial is
/// not duplicated in two template contexts) plus the mode controls.
#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeView {
    pub health_html: String,
    pub mode_buttons: Vec<ModeButtonView>,
    pub channels: Vec<u16>,
    pub base_url: String,
    pub poll_secs: u64,
}

/// Shared context of every full page: what the footer shows.
pub fn page_context(cfg: &Settings) -> (String, u64) {
    (cfg.spider_url.clone(), cfg.poll_secs)
}

pub struct KnobView {
    pub name: &'static str,
    pub value: Option<u32>,
    pub min: u32,
    pub max: u32,
    pub default: u32,
    pub unit: &'static str,
}

pub struct GroupView {
    pub name: String,
    pub knobs: Vec<KnobView>,
}

/// One row of the logs page's file table.
pub struct FileView {
    pub name: String,
    pub size_text: String,
}

/// The logs page: a day, its activation stamps and the log files seen for it.
#[derive(Template)]
#[template(path = "logs.html")]
pub struct LogsView {
    pub day: String,
    pub prev_day: String,
    pub next_day: String,
    pub today: String,
    pub is_today: bool,
    pub files: Vec<FileView>,
    pub has_files: bool,
    pub stamps: Vec<String>,
    pub total: i64,
    pub no_activations: bool,
    pub has_more: bool,
    pub more: i64,
    pub base_url: String,
    pub poll_secs: u64,
}

/// The tuning form. Its root element is the HTMX swap target, so it carries the
/// attributes on the element that is replaced.
#[derive(Template)]
#[template(path = "partials/config_form.html")]
pub struct ConfigFormView {
    pub groups: Vec<GroupView>,
    pub banner: Option<Banner>,
    /// The values shown did not come from a live fetch.
    pub offline: bool,
    /// How old the cached values are, when they came from the cache.
    pub cache_age: Option<String>,
}

/// The tuning page: the form fragment as already-rendered HTML.
#[derive(Template)]
#[template(path = "config.html")]
pub struct ConfigPageView {
    pub form_html: String,
    pub base_url: String,
    pub poll_secs: u64,
}

/// Build the form view from whatever values we have (`None` per knob renders an
/// empty input with the firmware default as its placeholder).
pub fn config_form(
    values: &std::collections::HashMap<String, u32>,
    banner: Option<Banner>,
    offline: bool,
    cache_age: Option<String>,
) -> ConfigFormView {
    let groups = crate::tuning::groups()
        .into_iter()
        .map(|(name, knobs)| GroupView {
            name: name.to_string(),
            knobs: knobs
                .into_iter()
                .map(|k| KnobView {
                    name: k.name,
                    value: values.get(k.name).copied(),
                    min: k.min,
                    max: k.max,
                    default: k.default,
                    unit: crate::tuning::unit_of(k.name),
                })
                .collect(),
        })
        .collect();
    ConfigFormView {
        groups,
        banner,
        offline,
        cache_age,
    }
}
