//! Bucketing of activation stamps for the `/logs` chart: 288 five-minute slots per
//! UTC day.

use serde::Serialize;

/// Five-minute slots in a day: 24 * 12.
pub const SLOTS: usize = 288;

/// Index of the slot a `YYYY-MM-DDTHH:MM:SSZ` stamp falls into, or `None` when the
/// stamp is not one of ours.
pub fn bucket(ts: &str) -> Option<usize> {
    let b = ts.as_bytes();
    if b.len() != 20 {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let mut value = 0u32;
        for &c in &b[range] {
            if !c.is_ascii_digit() {
                return None;
            }
            value = value * 10 + u32::from(c - b'0');
        }
        Some(value)
    };
    let hour = digits(11..13)?;
    let minute = digits(14..16)?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some((hour * 12 + minute / 5) as usize)
}

/// Slot labels, "HH:MM" of each slot's start.
pub fn labels() -> Vec<String> {
    (0..SLOTS)
        .map(|i| {
            let minutes = i * 5;
            format!("{:02}:{:02}", minutes / 60, minutes % 60)
        })
        .collect()
}

/// Body of `GET /logs/data`.
#[derive(Serialize)]
pub struct LogsData {
    pub day: String,
    pub labels: Vec<String>,
    pub counts: Vec<u32>,
    pub total: usize,
    pub files: Vec<(String, i64)>,
    pub first: Option<String>,
    pub last: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_map_the_day_edges() {
        assert_eq!(bucket("2026-10-11T00:00:00Z"), Some(0));
        assert_eq!(bucket("2026-10-11T00:04:59Z"), Some(0));
        assert_eq!(bucket("2026-10-11T00:05:00Z"), Some(1));
        assert_eq!(bucket("2026-10-11T12:05:00Z"), Some(145));
        assert_eq!(bucket("2026-10-11T23:59:00Z"), Some(287));
        assert_eq!(bucket("2026-10-11T23:59:59Z"), Some(287));
    }

    #[test]
    fn malformed_stamps_have_no_bucket() {
        assert_eq!(bucket(""), None);
        assert_eq!(bucket("2026-10-11"), None);
        assert_eq!(bucket("2026-10-11TXX:00:00Z"), None);
        assert_eq!(bucket("2026-10-11T24:00:00Z"), None);
        assert_eq!(bucket("2026-10-11T00:60:00Z"), None);
    }

    #[test]
    fn labels_cover_the_whole_day() {
        let labels = labels();
        assert_eq!(labels.len(), SLOTS);
        assert_eq!(labels[0], "00:00");
        assert_eq!(labels[145], "12:05");
        assert_eq!(labels[SLOTS - 1], "23:55");
    }
}
