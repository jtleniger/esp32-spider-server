//! Client for the spider's HTTP API (`API.md` is the contract).
//!
//! The firmware answers with `Connection: close` on every response and keeps no
//! state between requests, so the pool is told to hold no idle sockets. POSTs carry
//! a query string only: a `text/plain` body would arrive as a bogus `plain` argument
//! and make `POST /config` fail.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `GET /health`. Unknown keys are ignored, so the firmware can add fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Health {
    pub mode: u16,
    pub uptime_s: u32,
    pub utc: String,
    pub clock_set: bool,
    pub log_files: u32,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogEntry {
    pub name: String,
    pub size: i64,
}

/// `GET /logs`.
#[derive(Debug, Clone, Deserialize)]
pub struct LogListing {
    #[serde(default)]
    pub logs: Vec<LogEntry>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug)]
pub enum SpiderError {
    /// Connection refused, DNS failure or timeout: the rig is powered off or off the LAN.
    Offline(String),
    /// The firmware rejected the request (`400`) and the body names the field.
    BadRequest(String),
    /// An answer we cannot make sense of.
    Protocol(String),
}

impl fmt::Display for SpiderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpiderError::Offline(msg) => write!(f, "spider unreachable: {msg}"),
            SpiderError::BadRequest(body) => write!(f, "{body}"),
            SpiderError::Protocol(msg) => write!(f, "unexpected response: {msg}"),
        }
    }
}

impl std::error::Error for SpiderError {}

#[derive(Clone)]
pub struct Spider {
    base: String,
    client: reqwest::Client,
}

impl Spider {
    pub fn new(base: &str, timeout: Duration) -> anyhow::Result<Spider> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(0)
            .build()?;
        Ok(Spider {
            base: base.trim_end_matches('/').to_string(),
            client,
        })
    }

    /// `GET /health`: `Some` when the rig answers, `None` on the `503` it sends while
    /// its STA link is down.
    pub async fn health(&self) -> Result<Option<Health>, SpiderError> {
        let resp = self
            .client
            .get(format!("{}/health", self.base))
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(SpiderError::Protocol(format!("GET /health -> {status}")));
        }
        resp.json::<Health>()
            .await
            .map(Some)
            .map_err(|e| SpiderError::Protocol(format!("bad /health body: {e}")))
    }

    /// `GET /config`: every knob with its live value.
    pub async fn config(&self) -> Result<HashMap<String, u32>, SpiderError> {
        let resp = self
            .client
            .get(format!("{}/config", self.base))
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(classify(status, resp.text().await.unwrap_or_default()));
        }
        resp.json::<HashMap<String, u32>>()
            .await
            .map_err(|e| SpiderError::Protocol(format!("bad /config body: {e}")))
    }

    /// `POST /config` with one query argument per edit. The caller keeps the batch at
    /// or under the firmware's `kMaxEdits` (16).
    pub async fn config_set(&self, edits: &[(String, u32)]) -> Result<(), SpiderError> {
        let query: Vec<(&str, String)> = edits
            .iter()
            .map(|(name, value)| (name.as_str(), value.to_string()))
            .collect();
        let resp = self
            .client
            .post(format!("{}/config", self.base))
            .query(&query)
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(classify(status, resp.text().await.unwrap_or_default()))
    }

    /// `POST /config/reset`: drop overrides, back to the built-in defaults.
    pub async fn config_reset(&self) -> Result<(), SpiderError> {
        self.post_empty("/config/reset").await
    }

    /// `POST /mode?m=`, plus `&c=` for mode 1.
    pub async fn set_mode(&self, mode: u16, channel: Option<u16>) -> Result<(), SpiderError> {
        let mut query: Vec<(&str, String)> = vec![("m", mode.to_string())];
        if let Some(c) = channel {
            query.push(("c", c.to_string()));
        }
        let resp = self
            .client
            .post(format!("{}/mode", self.base))
            .query(&query)
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(classify(status, resp.text().await.unwrap_or_default()))
    }

    /// `GET /logs`: the newest (up to 32) log files.
    pub async fn log_list(&self) -> Result<LogListing, SpiderError> {
        let resp = self
            .client
            .get(format!("{}/logs", self.base))
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(classify(status, resp.text().await.unwrap_or_default()));
        }
        resp.json::<LogListing>()
            .await
            .map_err(|e| SpiderError::Protocol(format!("bad /logs body: {e}")))
    }

    /// `GET /logs/<name>`: one file, as written (`text/plain`).
    pub async fn log_file(&self, name: &str) -> Result<String, SpiderError> {
        if !is_log_name(name) {
            return Err(SpiderError::Protocol(format!("refusing bad log name {name}")));
        }
        let resp = self
            .client
            .get(format!("{}/logs/{}", self.base, name))
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return resp
                .text()
                .await
                .map_err(|e| SpiderError::Protocol(e.to_string()));
        }
        let body = resp.text().await.unwrap_or_default();
        match status {
            reqwest::StatusCode::NOT_FOUND => {
                Err(SpiderError::Protocol(format!("no such log: {name}")))
            }
            _ => Err(classify(status, body)),
        }
    }

    /// `POST /logs/clear`: delete every log file on the rig.
    pub async fn log_clear(&self) -> Result<(), SpiderError> {
        self.post_empty("/logs/clear").await
    }

    async fn post_empty(&self, path: &str) -> Result<(), SpiderError> {
        let resp = self
            .client
            .post(format!("{}{path}", self.base))
            .send()
            .await
            .map_err(|e| SpiderError::Offline(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        Err(classify(status, resp.text().await.unwrap_or_default()))
    }
}

/// `503` means "link down"; everything else that is not a success is a `400` whose
/// body names the offending field.
fn classify(status: reqwest::StatusCode, body: String) -> SpiderError {
    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return SpiderError::Offline("rig reported 503 (link down)".to_string());
    }
    SpiderError::BadRequest(body.trim().to_string())
}

/// Mirror of `lib/ledfx/logs.h::isLogName`: `YYYYMMDDTHHMMSSZ.log`, 8 digits, `T`,
/// 6 digits, `Z`, `.log` — exactly 20 bytes. This is what keeps a request inside the
/// rig's log directory, so it is checked before the name is ever put in a URL.
pub fn is_log_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() != 20 {
        return false;
    }
    if !b[..8].iter().all(u8::is_ascii_digit) {
        return false;
    }
    if b[8] != b'T' {
        return false;
    }
    if !b[9..15].iter().all(u8::is_ascii_digit) {
        return false;
    }
    b[15] == b'Z' && &b[16..] == b".log"
}

/// One line per activation, `YYYY-MM-DDTHH:MM:SSZ`. Keeps only lines of exactly that
/// shape; anything else is dropped with a warning rather than stored.
pub fn parse_activation_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if is_utc_stamp(line) {
            out.push(line.to_string());
        } else {
            tracing::warn!(line = %line, "ignoring malformed activation line");
        }
    }
    out
}

/// `YYYY-MM-DDTHH:MM:SSZ`, 20 bytes, digits and separators in the right slots.
pub fn is_utc_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    for i in 0..4 {
        if !digit(i) {
            return false;
        }
    }
    if b[4] != b'-' || !digit(5) || !digit(6) || b[7] != b'-' || !digit(8) || !digit(9) {
        return false;
    }
    if b[10] != b'T' || !digit(11) || !digit(12) || b[13] != b':' || !digit(14) || !digit(15) {
        return false;
    }
    b[16] == b':' && digit(17) && digit(18) && b[19] == b'Z'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_names_follow_the_firmware_rule() {
        assert!(is_log_name("20261011T170405Z.log"));
        assert!(is_log_name("19700101T000000Z.log")); // shape-only check, like the firmware
        assert!(!is_log_name("../x.log"));
        assert!(!is_log_name("/etc/passwd"));
        assert!(!is_log_name("20261011T170405Z"));
        assert!(!is_log_name("20261011T170405Z.log.bak"));
        assert!(!is_log_name("20261311T170405Z.log.bak"));
        assert!(!is_log_name("20261011T170405.log"));
        assert!(!is_log_name("20261011T170405Z.LOG"));
        assert!(!is_log_name(""));
    }

    #[test]
    fn activation_lines_keep_only_real_stamps() {
        let body = "2026-10-11T17:04:05Z\r\n\
                    2026-10-11T17:05:00Z\n\
                    \n\
                    1970\n\
                    not a stamp\n\
                    2026-10-11T17:05:00Z extra\n\
                    2026-10-11 17:05:00Z\n\
                    2026-10-11T17:06:12Z";
        assert_eq!(
            parse_activation_lines(body),
            vec![
                "2026-10-11T17:04:05Z".to_string(),
                "2026-10-11T17:05:00Z".to_string(),
                "2026-10-11T17:06:12Z".to_string(),
            ]
        );
        assert!(parse_activation_lines("").is_empty());
    }
}
