//! Small formatting/time helpers shared by the poller and the templates.

/// Current UTC instant as the rig's stamp shape, `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Seconds elapsed since `stamp`, or `None` if it is not a stamp we wrote.
pub fn age_secs(stamp: &str) -> Option<i64> {
    let t = chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%dT%H:%M:%SZ")
        .ok()?
        .and_utc();
    Some((chrono::Utc::now() - t).num_seconds())
}

/// "updated Ns ago" phrasing for a stamp.
pub fn human_age(stamp: &str) -> String {
    match age_secs(stamp) {
        None => "unknown".to_string(),
        Some(secs) if secs < 1 => "just now".to_string(),
        Some(secs) if secs < 60 => format!("{secs}s ago"),
        Some(secs) if secs < 3600 => format!("{}m ago", secs / 60),
        Some(secs) if secs < 86400 => format!("{}h ago", secs / 3600),
        Some(secs) => format!("{}d ago", secs / 86400),
    }
}

/// `uptime_s` in the rig's own words: "1d 2h 3m 4s", dropping leading zero units.
pub fn human_uptime(secs: u32) -> String {
    let (d, rem) = (secs / 86400, secs % 86400);
    let (h, rem) = (rem / 3600, rem % 3600);
    let (m, s) = (rem / 60, rem % 60);
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d}d"));
    }
    if h > 0 || !parts.is_empty() {
        parts.push(format!("{h}h"));
    }
    if m > 0 || !parts.is_empty() {
        parts.push(format!("{m}m"));
    }
    parts.push(format!("{s}s"));
    parts.join(" ")
}

/// Byte counts as B/KB/MB/GB with one decimal past the first unit.
pub fn human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

/// Same, for the `i64` byte counts SQLite hands back.
pub fn human_size(bytes: i64) -> String {
    human_bytes(bytes.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_humanise() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1_400_000), "1.3 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(human_size(-1), "0 B");
    }

    #[test]
    fn uptime_humanises() {
        assert_eq!(human_uptime(42), "42s");
        assert_eq!(human_uptime(90), "1m 30s");
        assert_eq!(human_uptime(90061), "1d 1h 1m 1s");
    }

    #[test]
    fn age_of_a_known_stamp_is_computable() {
        let now = chrono::Utc::now() - chrono::Duration::seconds(65);
        let stamp = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        assert_eq!(human_age(&stamp), "1m ago");
        assert_eq!(human_age("not a stamp"), "unknown");
    }
}
