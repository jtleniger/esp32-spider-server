//! SQLite storage for activation history, the log-file bookkeeping and the
//! config-cache settings.
//!
//! `rusqlite::Connection` is `Send` but not `Sync`, so the mutex is taken *inside*
//! the blocking task and `&Connection` never crosses an `.await`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    /// Open (creating if needed) the database and run the idempotent migration.
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS activations (
               ts          TEXT PRIMARY KEY,   -- 'YYYY-MM-DDTHH:MM:SSZ'
               source_file TEXT NOT NULL,
               first_seen  TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS log_files (
               name         TEXT PRIMARY KEY,  -- 'YYYYMMDDTHHMMSSZ.log'
               size         INTEGER NOT NULL,
               last_fetched TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS settings (
               key   TEXT PRIMARY KEY,
               value TEXT NOT NULL
             );",
        )?;
        Ok(Db(Arc::new(Mutex::new(conn))))
    }

    /// Run `f` against the connection on the blocking pool.
    pub async fn call<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> anyhow::Result<T> + Send + 'static,
    {
        let conn = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
            f(&guard)
        })
        .await?
    }

    /// Insert activation timestamps, deduped by stamp: `ts` is the primary key, so a
    /// re-fetch of a growing file, a re-fetch after a restart, or a stamp shared by two
    /// boots' files adds nothing new. Returns how many rows were actually added.
    pub async fn insert_activations(
        &self,
        file: &str,
        stamps: &[String],
        now: &str,
    ) -> anyhow::Result<usize> {
        let file = file.to_string();
        let stamps = stamps.to_vec();
        let now = now.to_string();
        self.call(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut added = 0usize;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR IGNORE INTO activations (ts, source_file, first_seen) VALUES (?, ?, ?)",
                )?;
                for ts in &stamps {
                    added += stmt.execute(rusqlite::params![ts, file, now])?;
                }
            }
            tx.commit()?;
            Ok(added)
        })
        .await
    }

    /// Every activation stamp in the UTC day `day` (`YYYY-MM-DD`), ascending.
    pub async fn activations_for_day(&self, day: &str) -> anyhow::Result<Vec<String>> {
        let day = day.to_string();
        self.call(move |conn| {
            let lo = format!("{day}T00:00:00Z");
            let hi = format!("{}T00:00:00Z", next_day(&day)?);
            let mut stmt =
                conn.prepare("SELECT ts FROM activations WHERE ts >= ?1 AND ts < ?2 ORDER BY ts")?;
            let rows = stmt.query_map(rusqlite::params![lo, hi], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .await
    }

    /// `(name, size)` for every seen log file whose name starts with `YYYYMMDD`.
    pub async fn log_files_for_day(&self, compact: &str) -> anyhow::Result<Vec<(String, i64)>> {
        let prefix = compact.to_string();
        self.call(move |conn| {
            let mut stmt = conn
                .prepare("SELECT name, size FROM log_files WHERE name LIKE ?1 || '%' ORDER BY name")?;
            let rows = stmt.query_map([prefix], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .await
    }

    pub async fn log_file_size(&self, name: &str) -> anyhow::Result<Option<i64>> {
        let name = name.to_string();
        self.call(move |conn| {
            let size = conn
                .query_row("SELECT size FROM log_files WHERE name = ?1", [name], |r| {
                    r.get::<_, i64>(0)
                })
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            Ok(size)
        })
        .await
    }

    pub async fn upsert_log_file(&self, name: &str, size: i64, now: &str) -> anyhow::Result<()> {
        let (name, now) = (name.to_string(), now.to_string());
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO log_files (name, size, last_fetched) VALUES (?1, ?2, ?3)
                 ON CONFLICT(name) DO UPDATE SET size = excluded.size, last_fetched = excluded.last_fetched",
                rusqlite::params![name, size, now],
            )?;
            Ok(())
        })
        .await
    }

    /// Forget both the activation history and the log-file bookkeeping. The next poll
    /// re-fetches whatever the spider still holds.
    pub async fn clear_activations(&self) -> anyhow::Result<()> {
        self.call(|conn| {
            conn.execute_batch("DELETE FROM activations; DELETE FROM log_files;")?;
            Ok(())
        })
        .await
    }

    pub async fn get_setting(&self, key: &str) -> anyhow::Result<Option<String>> {
        let key = key.to_string();
        self.call(move |conn| {
            let value = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                    r.get::<_, String>(0)
                })
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            Ok(value)
        })
        .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let (key, value) = (key.to_string(), value.to_string());
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn settings_json<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<Option<T>> {
        match self.get_setting(key).await? {
            Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
            None => Ok(None),
        }
    }

    pub async fn set_settings_json<T: Serialize>(&self, key: &str, value: &T) -> anyhow::Result<()> {
        self.set_setting(key, &serde_json::to_string(value)?).await
    }
}

/// `YYYY-MM-DD` -> the following day, same format.
fn next_day(day: &str) -> anyhow::Result<String> {
    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("bad day {day}: {e}"))?;
    let next = date
        .succ_opt()
        .ok_or_else(|| anyhow::anyhow!("no day after {day}"))?;
    Ok(next.format("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> Db {
        let mut path = std::env::temp_dir();
        path.push(format!("spider-test-{tag}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Db::open(&path).unwrap()
    }

    #[tokio::test]
    async fn dedupes_by_stamp_across_files_and_restarts() {
        let db = temp_db("dedupe");
        let shared = "2026-10-11T17:04:05Z".to_string();
        let a = vec![shared.clone(), "2026-10-11T17:05:00Z".to_string()];
        let b = vec![shared.clone(), "2026-10-10T09:01:12Z".to_string()];

        assert_eq!(
            db.insert_activations("20261011T170405Z.log", &a, "2026-10-11T18:00:00Z")
                .await
                .unwrap(),
            2
        );
        // Re-fetch of the same file, then a second file sharing one stamp.
        assert_eq!(
            db.insert_activations("20261011T170405Z.log", &a, "2026-10-11T18:00:10Z")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            db.insert_activations("20261010T090112Z.log", &b, "2026-10-11T18:00:20Z")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            db.activations_for_day("2026-10-11").await.unwrap().len()
                + db.activations_for_day("2026-10-10").await.unwrap().len(),
            3
        );
        assert_eq!(
            db.activations_for_day("2026-10-11").await.unwrap(),
            vec!["2026-10-11T17:04:05Z".to_string(), "2026-10-11T17:05:00Z".to_string()]
        );
        assert_eq!(db.activations_for_day("2026-10-10").await.unwrap().len(), 1);
        assert!(db.activations_for_day("2026-10-12").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn log_file_sizes_and_settings_round_trip() {
        let db = temp_db("misc");
        assert_eq!(db.log_file_size("20261011T170405Z.log").await.unwrap(), None);
        db.upsert_log_file("20261011T170405Z.log", 120, "2026-10-11T18:00:00Z")
            .await
            .unwrap();
        assert_eq!(db.log_file_size("20261011T170405Z.log").await.unwrap(), Some(120));
        db.upsert_log_file("20261011T170405Z.log", 140, "2026-10-11T18:00:10Z")
            .await
            .unwrap();
        assert_eq!(db.log_file_size("20261011T170405Z.log").await.unwrap(), Some(140));
        assert_eq!(
            db.log_files_for_day("20261011").await.unwrap(),
            vec![("20261011T170405Z.log".to_string(), 140)]
        );

        assert_eq!(db.get_setting("probe_key").await.unwrap(), None);
        db.set_setting("probe_key", "3").await.unwrap();
        db.set_setting("probe_key", "6").await.unwrap();
        assert_eq!(db.get_setting("probe_key").await.unwrap().as_deref(), Some("6"));

        db.set_settings_json("config_cache", &vec![("pulse_levels", 64u32)])
            .await
            .unwrap();
        let cached: Option<Vec<(String, u32)>> = db.settings_json("config_cache").await.unwrap();
        assert_eq!(cached, Some(vec![("pulse_levels".to_string(), 64)]));

        db.clear_activations().await.unwrap();
        assert!(db.activations_for_day("2026-10-11").await.unwrap().is_empty());
        assert!(db.log_files_for_day("20261011").await.unwrap().is_empty());
    }
}
