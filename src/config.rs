//! Startup configuration, read from the environment.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Every knob the process needs. Immutable after startup and shared as `Arc<Settings>`.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Address the web UI listens on. The spider's own port 80 lives on the device, not here.
    pub listen: SocketAddr,
    /// Spider base URL, no trailing slash, e.g. `http://spider.local`.
    pub spider_url: String,
    /// SQLite file holding the activation history and the desired-mode memory.
    pub db_path: PathBuf,
    /// `/health` poll interval, in whole seconds.
    pub poll_secs: u64,
    /// Per-request timeout for spider calls, in whole seconds.
    pub request_timeout_secs: u64,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Settings> {
        let listen: SocketAddr = env("SPIDER_LISTEN", "0.0.0.0:8080")
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid SPIDER_LISTEN: {e}"))?;
        let spider_url = env("SPIDER_URL", "http://spider.local")
            .trim_end_matches('/')
            .to_string();
        let db_path = PathBuf::from(env("SPIDER_DB_PATH", "spider-server.db"));
        let poll_secs = number("SPIDER_POLL_SECS", 60)?.max(1);
        let request_timeout_secs = number("SPIDER_TIMEOUT_SECS", 5)?;

        Ok(Settings {
            listen,
            spider_url,
            db_path,
            poll_secs,
            request_timeout_secs,
        })
    }
}

fn env(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

fn number(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => v
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid {key}: {e}")),
        _ => Ok(default),
    }
}
