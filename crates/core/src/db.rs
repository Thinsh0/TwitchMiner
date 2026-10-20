//! SQLite persistence. One file holds everything: accounts (with credentials),
//! settings, module key-value data, history and daily statistics.
//!
//! The connection lives on a dedicated thread; async code sends closures to it.
//! Writes are serialized, which keeps ordering identical to the event stream.

use std::path::Path;
use std::sync::mpsc as std_mpsc;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::event::{Envelope, Event};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone)]
pub struct Db {
    tx: std_mpsc::Sender<Job>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database thread stopped")]
    Closed,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE accounts (
        id TEXT PRIMARY KEY,
        login TEXT NOT NULL,
        display_name TEXT NOT NULL,
        avatar_url TEXT,
        enabled INTEGER NOT NULL DEFAULT 1,
        credentials TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE settings (scope TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE kv (
        namespace TEXT NOT NULL,
        key TEXT NOT NULL,
        value TEXT NOT NULL,
        PRIMARY KEY (namespace, key)
    );
    CREATE TABLE history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        seq INTEGER NOT NULL,
        ts TEXT NOT NULL,
        account TEXT,
        kind TEXT NOT NULL,
        channel_id TEXT,
        data TEXT NOT NULL
    );
    CREATE INDEX history_account_ts ON history(account, ts);
    CREATE INDEX history_kind ON history(kind);
    CREATE TABLE daily_stats (
        day TEXT NOT NULL,
        account TEXT NOT NULL,
        channel_id TEXT NOT NULL,
        points INTEGER NOT NULL DEFAULT 0,
        watch_minutes INTEGER NOT NULL DEFAULT 0,
        drops INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (day, account, channel_id)
    );
    "#,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub enabled: bool,
    pub credentials: tm_twitch::auth::Credentials,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub account: Option<String>,
    pub channel_id: Option<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Only rows with `id < before_id` (pagination, newest first).
    pub before_id: Option<i64>,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: i64,
    pub seq: u64,
    pub ts: String,
    pub account: Option<String>,
    pub kind: String,
    pub channel_id: Option<String>,
    pub event: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsQuery {
    pub account: Option<String>,
    pub channel_id: Option<String>,
    /// Inclusive `YYYY-MM-DD`.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Group by `day` (default) or `channel`.
    #[serde(default)]
    pub group_by: StatsGroup,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatsGroup {
    #[default]
    Day,
    Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsRow {
    pub key: String,
    pub points: i64,
    pub watch_minutes: i64,
    pub drops: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Keep the WAL small on disk after checkpoints (mobile storage).
        conn.pragma_update(None, "journal_size_limit", 4 * 1024 * 1024)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Self::spawn(conn))
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn)?;
        Ok(Self::spawn(conn))
    }

    fn spawn(mut conn: Connection) -> Self {
        let (tx, rx) = std_mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("tm-db".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })
            .expect("spawn db thread");
        Self { tx }
    }

    /// Runs `f` on the database thread and returns its result.
    pub async fn call<R, F>(&self, f: F) -> Result<R, DbError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, DbError> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Box::new(move |c| {
                let _ = tx.send(f(c));
            }))
            .map_err(|_| DbError::Closed)?;
        rx.await.map_err(|_| DbError::Closed)?
    }

    /// Fire-and-forget write; errors are logged.
    pub fn spawn_write<F>(&self, what: &'static str, f: F)
    where
        F: FnOnce(&mut Connection) -> Result<(), DbError> + Send + 'static,
    {
        let _ = self.tx.send(Box::new(move |c| {
            if let Err(e) = f(c) {
                tracing::error!(error = %e, what, "database write failed");
            }
        }));
    }

    // ------------------------------------------------------------- accounts

    pub async fn accounts(&self) -> Result<Vec<StoredAccount>, DbError> {
        self.call(|c| {
            let mut st = c.prepare(
                "SELECT id, login, display_name, avatar_url, enabled, credentials FROM accounts ORDER BY created_at",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, login, display_name, avatar_url, enabled, creds) = row?;
                out.push(StoredAccount {
                    id,
                    login,
                    display_name,
                    avatar_url,
                    enabled,
                    credentials: serde_json::from_str(&creds)?,
                });
            }
            Ok(out)
        })
        .await
    }

    pub async fn upsert_account(&self, a: StoredAccount) -> Result<(), DbError> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO accounts (id, login, display_name, avatar_url, enabled, credentials, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET login=?2, display_name=?3, avatar_url=?4, enabled=?5, credentials=?6",
                params![
                    a.id,
                    a.login,
                    a.display_name,
                    a.avatar_url,
                    a.enabled,
                    serde_json::to_string(&a.credentials)?,
                    chrono::Utc::now().to_rfc3339()
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_account(&self, id: String) -> Result<(), DbError> {
        self.call(move |c| {
            c.execute("DELETE FROM accounts WHERE id = ?1", [&id])?;
            c.execute(
                "DELETE FROM settings WHERE scope LIKE ?1",
                [format!("account:{id}/%")],
            )?;
            c.execute(
                "DELETE FROM kv WHERE namespace LIKE ?1",
                [format!("{id}/%")],
            )?;
            Ok(())
        })
        .await
    }

    // ------------------------------------------------------------- settings

    pub async fn settings(&self) -> Result<Vec<(String, Value)>, DbError> {
        self.call(|c| {
            let mut st = c.prepare("SELECT scope, value FROM settings")?;
            let rows =
                st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (scope, v) = row?;
                out.push((scope, serde_json::from_str(&v)?));
            }
            Ok(out)
        })
        .await
    }

    pub async fn put_settings(&self, scope: String, value: Value) -> Result<(), DbError> {
        self.call(move |c| {
            if value.is_null() {
                c.execute("DELETE FROM settings WHERE scope = ?1", [&scope])?;
            } else {
                c.execute(
                    "INSERT INTO settings (scope, value) VALUES (?1, ?2)
                     ON CONFLICT(scope) DO UPDATE SET value = ?2",
                    params![scope, value.to_string()],
                )?;
            }
            Ok(())
        })
        .await
    }

    // ------------------------------------------------------------------ kv

    pub async fn kv_get(&self, namespace: String, key: String) -> Result<Option<Value>, DbError> {
        self.call(move |c| {
            let v: Option<String> = c
                .query_row(
                    "SELECT value FROM kv WHERE namespace = ?1 AND key = ?2",
                    [&namespace, &key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(v.map(|v| serde_json::from_str(&v)).transpose()?)
        })
        .await
    }

    pub async fn kv_put(
        &self,
        namespace: String,
        key: String,
        value: Value,
    ) -> Result<(), DbError> {
        self.call(move |c| {
            if value.is_null() {
                c.execute(
                    "DELETE FROM kv WHERE namespace = ?1 AND key = ?2",
                    [&namespace, &key],
                )?;
            } else {
                c.execute(
                    "INSERT INTO kv (namespace, key, value) VALUES (?1, ?2, ?3)
                     ON CONFLICT(namespace, key) DO UPDATE SET value = ?3",
                    params![namespace, key, value.to_string()],
                )?;
            }
            Ok(())
        })
        .await
    }

    // ------------------------------------------------------------- history

    /// Persists the parts of an event worth keeping (history row, daily stats).
    pub fn record(&self, env: &Envelope) {
        let day = env.ts.format("%Y-%m-%d").to_string();
        let account = env.event.account().map(str::to_string);
        let channel = env.event.channel_id().map(str::to_string);
        let stats: Option<(i64, i64, i64)> = match &env.event {
            Event::PointsEarned { amount, .. } => Some((*amount, 0, 0)),
            Event::PredictionResult { points_won, .. } => Some((*points_won, 0, 0)),
            Event::WatchMinute { .. } => Some((0, 1, 0)),
            Event::DropClaimed { .. } => Some((0, 0, 1)),
            _ => None,
        };
        let history = env.event.is_historical().then(|| {
            (
                env.seq,
                env.ts.to_rfc3339(),
                env.event.kind(),
                serde_json::to_string(&env.event).unwrap_or_default(),
            )
        });
        if stats.is_none() && history.is_none() {
            return;
        }
        self.spawn_write("record", move |c| {
            if let Some((seq, ts, kind, data)) = history {
                c.execute(
                    "INSERT INTO history (seq, ts, account, kind, channel_id, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![seq as i64, ts, account, kind, channel, data],
                )?;
            }
            if let (Some((points, minutes, drops)), Some(acc)) = (stats, &account) {
                c.execute(
                    "INSERT INTO daily_stats (day, account, channel_id, points, watch_minutes, drops)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(day, account, channel_id) DO UPDATE SET
                       points = points + ?4, watch_minutes = watch_minutes + ?5, drops = drops + ?6",
                    params![day, acc, channel.clone().unwrap_or_default(), points, minutes, drops],
                )?;
            }
            Ok(())
        });
    }

    pub async fn history(&self, q: HistoryQuery) -> Result<Vec<HistoryRow>, DbError> {
        self.call(move |c| {
            let mut sql = String::from(
                "SELECT id, seq, ts, account, kind, channel_id, data FROM history WHERE 1=1",
            );
            let mut args: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(a) = q.account {
                sql.push_str(" AND account = ?");
                args.push(a.into());
            }
            if let Some(ch) = q.channel_id {
                sql.push_str(" AND channel_id = ?");
                args.push(ch.into());
            }
            if !q.kinds.is_empty() {
                sql.push_str(&format!(
                    " AND kind IN ({})",
                    vec!["?"; q.kinds.len()].join(",")
                ));
                args.extend(q.kinds.into_iter().map(Into::into));
            }
            if let Some(b) = q.before_id {
                sql.push_str(" AND id < ?");
                args.push(b.into());
            }
            if let Some(s) = q.since {
                sql.push_str(" AND ts >= ?");
                args.push(s.to_rfc3339().into());
            }
            sql.push_str(" ORDER BY id DESC LIMIT ?");
            args.push(i64::from(q.limit.unwrap_or(100).min(1000)).into());
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(rusqlite::params_from_iter(args), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, seq, ts, account, kind, channel_id, data) = row?;
                out.push(HistoryRow {
                    id,
                    seq: seq as u64,
                    ts,
                    account,
                    kind,
                    channel_id,
                    event: serde_json::from_str(&data)?,
                });
            }
            Ok(out)
        })
        .await
    }

    pub async fn stats(&self, q: StatsQuery) -> Result<Vec<StatsRow>, DbError> {
        self.call(move |c| {
            let key = match q.group_by {
                StatsGroup::Day => "day",
                StatsGroup::Channel => "channel_id",
            };
            let mut sql = format!(
                "SELECT {key}, SUM(points), SUM(watch_minutes), SUM(drops) FROM daily_stats WHERE 1=1"
            );
            let mut args: Vec<rusqlite::types::Value> = Vec::new();
            for (col, op, v) in [
                ("account", "=", q.account),
                ("channel_id", "=", q.channel_id),
                ("day", ">=", q.from),
                ("day", "<=", q.to),
            ] {
                if let Some(v) = v {
                    sql.push_str(&format!(" AND {col} {op} ?"));
                    args.push(v.into());
                }
            }
            sql.push_str(&format!(" GROUP BY {key} ORDER BY {key}"));
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(rusqlite::params_from_iter(args), |r| {
                Ok(StatsRow { key: r.get(0)?, points: r.get(1)?, watch_minutes: r.get(2)?, drops: r.get(3)? })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// Folds the WAL back into the main file (called at shutdown).
    pub async fn checkpoint(&self) -> Result<(), DbError> {
        self.call(|c| {
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            Ok(())
        })
        .await
    }

    /// Writes a consistent, compacted copy of the whole database to `path`
    /// (safe while the engine runs). Fails if `path` already exists.
    pub async fn backup_to(&self, path: std::path::PathBuf) -> Result<(), DbError> {
        self.call(move |c| {
            c.execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])?;
            Ok(())
        })
        .await
    }

    /// Deletes history older than `days` (statistics are kept).
    pub async fn prune_history(&self, days: u32) -> Result<usize, DbError> {
        self.call(move |c| {
            let cutoff =
                (chrono::Utc::now() - chrono::Duration::days(i64::from(days))).to_rfc3339();
            Ok(c.execute("DELETE FROM history WHERE ts < ?1", [cutoff])?)
        })
        .await
    }
}

fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
        tracing::info!(version = i + 1, "database migrated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn history_and_stats_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let ev = Event::PointsEarned {
            account: "1".into(),
            channel_id: "9".into(),
            amount: 10,
            reason: "WATCH".into(),
            balance: 10,
        };
        db.record(&Envelope {
            seq: 1,
            ts: Utc::now(),
            event: ev.clone(),
        });
        db.record(&Envelope {
            seq: 2,
            ts: Utc::now(),
            event: ev,
        });
        db.record(&Envelope {
            seq: 3,
            ts: Utc::now(),
            event: Event::WatchMinute {
                account: "1".into(),
                channel_id: "9".into(),
            },
        });
        let h = db
            .history(HistoryQuery {
                account: Some("1".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].seq, 2);
        let s = db
            .stats(StatsQuery {
                group_by: StatsGroup::Channel,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(s[0].points, 20);
        assert_eq!(s[0].watch_minutes, 1);
    }

    #[tokio::test]
    async fn backup_is_a_usable_copy() {
        let dir = std::env::temp_dir().join(format!("tm-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("a.db")).unwrap();
        db.kv_put("n".into(), "k".into(), serde_json::json!(42))
            .await
            .unwrap();
        db.backup_to(dir.join("b.db")).await.unwrap();
        let copy = Db::open(&dir.join("b.db")).unwrap();
        assert_eq!(
            copy.kv_get("n".into(), "k".into()).await.unwrap(),
            Some(serde_json::json!(42))
        );
        assert!(
            db.backup_to(dir.join("b.db")).await.is_err(),
            "never overwrites"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn kv_and_settings() {
        let db = Db::open_in_memory().unwrap();
        db.kv_put("a".into(), "k".into(), serde_json::json!([1, 2]))
            .await
            .unwrap();
        assert_eq!(
            db.kv_get("a".into(), "k".into()).await.unwrap(),
            Some(serde_json::json!([1, 2]))
        );
        db.put_settings("engine".into(), serde_json::json!({"x": 1}))
            .await
            .unwrap();
        assert_eq!(db.settings().await.unwrap().len(), 1);
    }
}
