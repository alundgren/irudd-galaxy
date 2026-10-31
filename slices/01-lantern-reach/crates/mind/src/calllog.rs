//! The LLM call log: one SQLite file beside the save, `<stem>.llm-log.sqlite` next to
//! `<stem>.sqlite`, never inside it. Every OpenRouter request the game makes is one
//! row: when, what for (the purpose), the model asked for and the one that answered,
//! the router's task type, tokens, cost as OpenRouter reported it, latency, the prompt
//! and the output, and whether the game accepted or rejected the result (and why).
//!
//! It sits outside the sim: wall-clock time is fine here, nothing in it reaches the
//! world, and the player never sees it. Agents read it with `galaxy llm-log`.
//!
//! - [`CallLog`] is the store: open, prune, query. Tests drive it directly with an
//!   injected clock.
//! - [`CallSink`] is the handle the client and the host write through. It never
//!   blocks: messages go over a channel to one writer thread that owns the connection.
//!   A sink that isn't connected drops what it is given, so the game plays the same
//!   with or without a log.
//! - On startup the writer prunes first ([`CallLog::prune`]): rows older than
//!   `log.keep_days` roll into permanent daily rollups (day, model, purpose, calls,
//!   cost, rejections), and a file over `log.max_mb` rolls its oldest rows up until it
//!   fits.
//!
//! The key is never written: the client redacts response bodies as they arrive, and
//! a sink a client writes through redacts its key from every text in a row and every
//! verdict reason before the writer sees them ([`CallSink::redact_with`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::openrouter::ApiKey;

/// The log file for a save: `<stem>.llm-log.sqlite` in the save's folder.
pub fn path_for(save: &Path) -> PathBuf {
    let stem = save
        .file_stem()
        .map_or_else(|| "galaxy".into(), |s| s.to_string_lossy().into_owned());
    save.with_file_name(format!("{stem}.llm-log.sqlite"))
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Longest prompt or output kept in a row; longer text is cut with a marker.
pub const MAX_TEXT: usize = 64 * 1024;

const DAY_MS: i64 = 86_400_000;

/// How long rows are kept and how big the file may grow (`[log]` in `mind.toml`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogRules {
    /// Days of full rows; older ones roll into daily rollups.
    pub keep_days: u32,
    /// Hard cap on the file, in megabytes; the oldest rows roll up first.
    pub max_mb: u32,
}

impl Default for LogRules {
    fn default() -> Self {
        Self {
            keep_days: 14,
            max_mb: 50,
        }
    }
}

/// What became of a call's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// An answer came; the game hasn't judged it yet (or nothing did).
    Pending,
    Accepted,
    /// The answer came and the game dropped it.
    Rejected,
    /// No usable answer came: an HTTP error, a timeout, an upstream error.
    Failed,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Pending => "pending",
            Verdict::Accepted => "accepted",
            Verdict::Rejected => "rejected",
            Verdict::Failed => "failed",
        }
    }
}

/// A call's ID in the log, unique across runs: the client makes it, the host uses it
/// to record the verdict later.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CallId(pub String);

impl CallId {
    /// A fresh ID: this process's start stamp, its ID and a counter.
    pub fn fresh() -> Self {
        static START: OnceLock<i64> = OnceLock::new();
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let start = *START.get_or_init(now_ms);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(format!("c{start:x}.{:x}.{n}", std::process::id()))
    }
}

/// One request, as logged.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CallRow {
    pub uid: CallId,
    /// Wall clock, ms since the epoch.
    pub ts_ms: i64,
    /// `banter`, `traffic`, `brag`, `discount`, `radio`, `music`, `decision`, `line`.
    pub purpose: String,
    pub requested_model: String,
    pub actual_model: Option<String>,
    /// The auto router's task type (`openrouter_metadata.pipeline[].data.task_type`).
    pub task_type: Option<String>,
    pub session_id: Option<String>,
    pub generation_id: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// US dollars as OpenRouter reported it, summed over attempts.
    pub cost_usd: Option<f64>,
    /// `usage` (the response) or `generation` (looked up by generation ID).
    pub cost_source: Option<String>,
    pub latency_ms: u64,
    pub attempts: u8,
    pub http_status: Option<u16>,
    /// The error tag when the call failed (`timeout`, `rate_limited`, `http` ...).
    pub error_kind: Option<String>,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub prompt: String,
    pub output: String,
}

/// `text` cut to [`MAX_TEXT`] bytes on a character boundary, with a marker.
pub fn cap(text: &str) -> String {
    if text.len() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[cut at {MAX_TEXT} bytes]", &text[..end])
}

#[derive(Debug, thiserror::Error)]
pub enum CallLogError {
    #[error("call log: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("call log: {0}")]
    Io(#[from] std::io::Error),
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS calls (
    id                INTEGER PRIMARY KEY,
    uid               TEXT NOT NULL UNIQUE,
    ts_ms             INTEGER NOT NULL,
    purpose           TEXT NOT NULL,
    requested_model   TEXT NOT NULL,
    actual_model      TEXT,
    task_type         TEXT,
    session_id        TEXT,
    generation_id     TEXT,
    prompt_tokens     INTEGER,
    completion_tokens INTEGER,
    cost_usd          REAL,
    cost_source       TEXT,
    latency_ms        INTEGER NOT NULL,
    attempts          INTEGER NOT NULL,
    http_status       INTEGER,
    error_kind        TEXT,
    verdict           TEXT NOT NULL,
    reason            TEXT,
    prompt            TEXT NOT NULL,
    output            TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS calls_ts ON calls (ts_ms);
CREATE INDEX IF NOT EXISTS calls_verdict ON calls (verdict, ts_ms);
CREATE TABLE IF NOT EXISTS rollups (
    day               TEXT NOT NULL,
    model             TEXT NOT NULL,
    purpose           TEXT NOT NULL,
    calls             INTEGER NOT NULL,
    cost_usd          REAL NOT NULL,
    rejected          INTEGER NOT NULL,
    failed            INTEGER NOT NULL,
    prompt_tokens     INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    PRIMARY KEY (day, model, purpose)
);
"#;

/// What one prune did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PruneReport {
    /// Rows older than `keep_days`, rolled up and removed.
    pub expired: u64,
    /// Rows rolled up and removed to bring the file under `max_mb`.
    pub over_cap: u64,
    /// Bytes in use before and after.
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// An inclusive date range (`YYYY-MM-DD`, UTC). `None` on either side is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Range {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl Range {
    /// Checks a `YYYY-MM-DD` date.
    pub fn date(s: &str) -> Result<String, String> {
        let b = s.as_bytes();
        let ok = b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        if ok {
            Ok(s.to_owned())
        } else {
            Err(format!("`{s}` is not a date (YYYY-MM-DD)"))
        }
    }

    fn since(&self) -> &str {
        self.since.as_deref().unwrap_or("0000-00-00")
    }

    fn until(&self) -> &str {
        self.until.as_deref().unwrap_or("9999-99-99")
    }
}

/// The SQL day of a row's timestamp.
const ROW_DAY: &str = "date(ts_ms / 1000, 'unixepoch')";
/// A row's model for rollups and reports: the one that answered, else the one asked.
const ROW_MODEL: &str = "COALESCE(actual_model, requested_model)";

/// The store.
pub struct CallLog {
    conn: Connection,
}

impl CallLog {
    pub fn open(path: &Path) -> Result<Self, CallLogError> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Opens an existing log read-only, for queries. A missing file is an error.
    pub fn open_read(path: &Path) -> Result<Self, CallLogError> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self, CallLogError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn insert(&self, row: &CallRow) -> Result<(), CallLogError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO calls (uid, ts_ms, purpose, requested_model, actual_model,
                task_type, session_id, generation_id, prompt_tokens, completion_tokens,
                cost_usd, cost_source, latency_ms, attempts, http_status, error_kind,
                verdict, reason, prompt, output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20)",
            params![
                row.uid.0,
                row.ts_ms,
                row.purpose,
                row.requested_model,
                row.actual_model,
                row.task_type,
                row.session_id,
                row.generation_id,
                row.prompt_tokens.map(|t| t as i64),
                row.completion_tokens.map(|t| t as i64),
                row.cost_usd,
                row.cost_source,
                row.latency_ms as i64,
                i64::from(row.attempts),
                row.http_status,
                row.error_kind,
                row.verdict.as_str(),
                row.reason,
                cap(&row.prompt),
                cap(&row.output),
            ],
        )?;
        Ok(())
    }

    /// Records the game's verdict on a pending call. A failed call keeps its verdict.
    pub fn set_verdict(
        &self,
        uid: &CallId,
        verdict: Verdict,
        reason: Option<&str>,
    ) -> Result<(), CallLogError> {
        self.conn.execute(
            "UPDATE calls SET verdict = ?2, reason = ?3 WHERE uid = ?1 AND verdict = 'pending'",
            params![uid.0, verdict.as_str(), reason],
        )?;
        Ok(())
    }

    /// Bytes the file's pages hold, free pages left out.
    pub fn used_bytes(&self) -> Result<u64, CallLogError> {
        let pages: i64 = self
            .conn
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free: i64 = self
            .conn
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let size: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        Ok(((pages - free).max(0) * size) as u64)
    }

    /// Rolls the rows matching `filter` into the daily rollups and removes them, in one
    /// transaction. Returns how many.
    fn roll_up(&mut self, filter: &str, arg: i64) -> Result<u64, CallLogError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO rollups (day, model, purpose, calls, cost_usd, rejected, failed,
                    prompt_tokens, completion_tokens)
                 SELECT {ROW_DAY}, {ROW_MODEL}, purpose, COUNT(*), COALESCE(SUM(cost_usd), 0),
                    SUM(verdict = 'rejected'), SUM(verdict = 'failed'),
                    COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0)
                 FROM calls WHERE {filter} GROUP BY 1, 2, 3
                 ON CONFLICT (day, model, purpose) DO UPDATE SET
                    calls = calls + excluded.calls,
                    cost_usd = cost_usd + excluded.cost_usd,
                    rejected = rejected + excluded.rejected,
                    failed = failed + excluded.failed,
                    prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                    completion_tokens = completion_tokens + excluded.completion_tokens"
            ),
            params![arg],
        )?;
        let n = tx.execute(&format!("DELETE FROM calls WHERE {filter}"), params![arg])?;
        tx.commit()?;
        Ok(n as u64)
    }

    /// Prunes at `now_ms`: rows older than `keep_days` roll into the daily rollups;
    /// then, while the file is over `max_mb`, the oldest tenth of the rows rolls up too
    /// and the file is compacted. Rollups are kept for ever.
    pub fn prune(&mut self, now_ms: i64, rules: &LogRules) -> Result<PruneReport, CallLogError> {
        let mut report = PruneReport {
            bytes_before: self.used_bytes()?,
            ..PruneReport::default()
        };
        let cutoff = now_ms - i64::from(rules.keep_days) * DAY_MS;
        report.expired = self.roll_up("ts_ms < ?1", cutoff)?;
        let cap = u64::from(rules.max_mb) * 1024 * 1024;
        while self.used_bytes()? > cap {
            let rows: i64 = self
                .conn
                .query_row("SELECT COUNT(*) FROM calls", [], |r| r.get(0))?;
            if rows == 0 {
                break;
            }
            let n = (rows / 10).max(1);
            report.over_cap += self.roll_up(
                "id IN (SELECT id FROM calls ORDER BY ts_ms, id LIMIT ?1)",
                n,
            )?;
        }
        if report.over_cap > 0 {
            self.conn.execute_batch("VACUUM")?;
        }
        report.bytes_after = self.used_bytes()?;
        Ok(report)
    }

    /// The latest rejected or failed calls, newest first.
    pub fn recent_rejections(&self, limit: u32) -> Result<Vec<Value>, CallLogError> {
        let mut st = self.conn.prepare(
            "SELECT uid, strftime('%Y-%m-%dT%H:%M:%fZ', ts_ms / 1000.0, 'unixepoch'), purpose,
                requested_model, actual_model, task_type, verdict, reason, http_status,
                error_kind, cost_usd, latency_ms, output
             FROM calls WHERE verdict IN ('rejected', 'failed')
             ORDER BY ts_ms DESC, id DESC LIMIT ?1",
        )?;
        let rows = st.query_map(params![i64::from(limit)], |r| {
            let output: String = r.get(12)?;
            Ok(json!({
                "uid": r.get::<_, String>(0)?,
                "at": r.get::<_, String>(1)?,
                "purpose": r.get::<_, String>(2)?,
                "requested_model": r.get::<_, String>(3)?,
                "actual_model": r.get::<_, Option<String>>(4)?,
                "task_type": r.get::<_, Option<String>>(5)?,
                "verdict": r.get::<_, String>(6)?,
                "reason": r.get::<_, Option<String>>(7)?,
                "http_status": r.get::<_, Option<i64>>(8)?,
                "error_kind": r.get::<_, Option<String>>(9)?,
                "cost_usd": r.get::<_, Option<f64>>(10)?,
                "latency_ms": r.get::<_, i64>(11)?,
                "output": preview(&output),
            }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Calls, cost and rejections grouped by `by` (`model` or `purpose`), rows and
    /// rollups together, most expensive first.
    pub fn cost_by(&self, by: Group, range: &Range) -> Result<Vec<Value>, CallLogError> {
        let (row_col, roll_col) = match by {
            Group::Model => (ROW_MODEL, "model"),
            Group::Purpose => ("purpose", "purpose"),
        };
        let sql = format!(
            "SELECT k, SUM(calls), SUM(cost), SUM(rejected), SUM(failed) FROM (
                SELECT {row_col} AS k, COUNT(*) AS calls, COALESCE(SUM(cost_usd), 0) AS cost,
                    SUM(verdict = 'rejected') AS rejected, SUM(verdict = 'failed') AS failed
                FROM calls WHERE {ROW_DAY} BETWEEN ?1 AND ?2 GROUP BY 1
                UNION ALL
                SELECT {roll_col}, SUM(calls), SUM(cost_usd), SUM(rejected), SUM(failed)
                FROM rollups WHERE day BETWEEN ?1 AND ?2 GROUP BY 1
             ) GROUP BY k ORDER BY 3 DESC, 1"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![range.since(), range.until()], |r| {
            let calls: i64 = r.get(1)?;
            let rejected: i64 = r.get(3)?;
            let mut v = json!({
                "calls": calls,
                "cost_usd": r.get::<_, f64>(2)?,
                "rejected": rejected,
                "failed": r.get::<_, i64>(4)?,
                "rejection_rate": rate(rejected, calls),
            });
            v[by.as_str()] = json!(r.get::<_, String>(0)?);
            Ok(v)
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Totals over a range: calls, cost, tokens, rejections, rows and rollups together.
    pub fn totals(&self, range: &Range) -> Result<Value, CallLogError> {
        let sql = format!(
            "SELECT COALESCE(SUM(calls), 0), COALESCE(SUM(cost), 0), COALESCE(SUM(rejected), 0),
                COALESCE(SUM(failed), 0), COALESCE(SUM(pt), 0), COALESCE(SUM(ct), 0),
                COALESCE(SUM(rows), 0) FROM (
                SELECT COUNT(*) AS calls, COALESCE(SUM(cost_usd), 0) AS cost,
                    COALESCE(SUM(verdict = 'rejected'), 0) AS rejected,
                    COALESCE(SUM(verdict = 'failed'), 0) AS failed,
                    COALESCE(SUM(prompt_tokens), 0) AS pt,
                    COALESCE(SUM(completion_tokens), 0) AS ct, COUNT(*) AS rows
                FROM calls WHERE {ROW_DAY} BETWEEN ?1 AND ?2
                UNION ALL
                SELECT SUM(calls), SUM(cost_usd), SUM(rejected), SUM(failed),
                    SUM(prompt_tokens), SUM(completion_tokens), 0
                FROM rollups WHERE day BETWEEN ?1 AND ?2
             )"
        );
        let v = self
            .conn
            .query_row(&sql, params![range.since(), range.until()], |r| {
                let calls: i64 = r.get(0)?;
                let rejected: i64 = r.get(2)?;
                let rows: i64 = r.get(6)?;
                Ok(json!({
                    "since": range.since,
                    "until": range.until,
                    "calls": calls,
                    "cost_usd": r.get::<_, f64>(1)?,
                    "rejected": rejected,
                    "failed": r.get::<_, i64>(3)?,
                    "rejection_rate": rate(rejected, calls),
                    "prompt_tokens": r.get::<_, i64>(4)?,
                    "completion_tokens": r.get::<_, i64>(5)?,
                    "full_rows": rows,
                    "rolled_up": calls - rows,
                }))
            })?;
        Ok(v)
    }

    /// The daily rollups in a range, oldest first.
    pub fn rollups(&self, range: &Range) -> Result<Vec<Value>, CallLogError> {
        let mut st = self.conn.prepare(
            "SELECT day, model, purpose, calls, cost_usd, rejected, failed FROM rollups
             WHERE day BETWEEN ?1 AND ?2 ORDER BY day, model, purpose",
        )?;
        let rows = st.query_map(params![range.since(), range.until()], |r| {
            let calls: i64 = r.get(3)?;
            let rejected: i64 = r.get(5)?;
            Ok(json!({
                "day": r.get::<_, String>(0)?,
                "model": r.get::<_, String>(1)?,
                "purpose": r.get::<_, String>(2)?,
                "calls": calls,
                "cost_usd": r.get::<_, f64>(4)?,
                "rejected": rejected,
                "failed": r.get::<_, i64>(6)?,
                "rejection_rate": rate(rejected, calls),
            }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// One row by its ID, every column (for tests and debugging).
    pub fn row(&self, uid: &CallId) -> Result<Option<Value>, CallLogError> {
        Ok(self
            .conn
            .query_row(
                "SELECT purpose, requested_model, actual_model, task_type, session_id,
                    prompt_tokens, completion_tokens, cost_usd, cost_source, attempts,
                    http_status, error_kind, verdict, reason, prompt, output, generation_id
                 FROM calls WHERE uid = ?1",
                params![uid.0],
                |r| {
                    Ok(json!({
                        "purpose": r.get::<_, String>(0)?,
                        "requested_model": r.get::<_, String>(1)?,
                        "actual_model": r.get::<_, Option<String>>(2)?,
                        "task_type": r.get::<_, Option<String>>(3)?,
                        "session_id": r.get::<_, Option<String>>(4)?,
                        "prompt_tokens": r.get::<_, Option<i64>>(5)?,
                        "completion_tokens": r.get::<_, Option<i64>>(6)?,
                        "cost_usd": r.get::<_, Option<f64>>(7)?,
                        "cost_source": r.get::<_, Option<String>>(8)?,
                        "attempts": r.get::<_, i64>(9)?,
                        "http_status": r.get::<_, Option<i64>>(10)?,
                        "error_kind": r.get::<_, Option<String>>(11)?,
                        "verdict": r.get::<_, String>(12)?,
                        "reason": r.get::<_, Option<String>>(13)?,
                        "prompt": r.get::<_, String>(14)?,
                        "output": r.get::<_, String>(15)?,
                        "generation_id": r.get::<_, Option<String>>(16)?,
                    }))
                },
            )
            .optional()?)
    }

    /// Every row, oldest first (for tests).
    pub fn rows(&self) -> Result<Vec<Value>, CallLogError> {
        let mut st = self
            .conn
            .prepare("SELECT uid FROM calls ORDER BY ts_ms, id")?;
        let uids: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        uids.into_iter()
            .filter_map(|u| self.row(&CallId(u)).transpose())
            .collect()
    }
}

/// How [`CallLog::cost_by`] groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Model,
    Purpose,
}

impl Group {
    fn as_str(self) -> &'static str {
        match self {
            Group::Model => "model",
            Group::Purpose => "purpose",
        }
    }
}

fn rate(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

fn preview(text: &str) -> String {
    const N: usize = 400;
    if text.chars().count() <= N {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(N).collect::<String>())
    }
}

enum Msg {
    Record(Box<CallRow>),
    Verdict(CallId, Verdict, Option<String>),
    Flush(Sender<()>),
}

/// Where calls are written. Cheap to clone; clones share one writer. Never blocks: a
/// sink that isn't connected (no save, a log that couldn't open) drops what it gets.
#[derive(Clone, Default)]
pub struct CallSink {
    tx: Arc<OnceLock<Sender<Msg>>>,
    /// Keys redacted from everything written, shared by every clone.
    keys: Arc<Mutex<Vec<ApiKey>>>,
}

impl std::fmt::Debug for CallSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.is_connected() {
            "CallSink(connected)"
        } else {
            "CallSink(off)"
        })
    }
}

impl CallSink {
    /// A sink that drops everything (until [`CallSink::open`] connects it).
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_connected(&self) -> bool {
        self.tx.get().is_some()
    }

    /// Opens the log at `path` and starts its writer thread, which prunes first (at
    /// the wall clock, by `rules`) and then writes what this sink and its clones send.
    /// Opening happens here, so an unreadable file is reported; a sink connects once.
    pub fn open(&self, path: &Path, rules: &LogRules) -> Result<(), CallLogError> {
        if self.is_connected() {
            return Ok(());
        }
        let log = CallLog::open(path)?;
        self.start(log, rules.clone(), now_ms);
        Ok(())
    }

    /// Starts a writer over `log`, pruning at `clock()` first. For tests: an in-memory
    /// log and a fixed clock.
    pub fn start(&self, mut log: CallLog, rules: LogRules, clock: fn() -> i64) {
        let (tx, rx) = mpsc::channel::<Msg>();
        if self.tx.set(tx).is_err() {
            return;
        }
        std::thread::Builder::new()
            .name("galaxy-call-log".into())
            .spawn(move || {
                // A log that can't prune still logs.
                let _ = log.prune(clock(), &rules);
                // Ends when every sink is gone. Write errors are dropped: the log is a
                // diagnostic and must never stop the game.
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Record(row) => {
                            let _ = log.insert(&row);
                        }
                        Msg::Verdict(uid, v, reason) => {
                            let _ = log.set_verdict(&uid, v, reason.as_deref());
                        }
                        Msg::Flush(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
            })
            .expect("spawning the call log thread");
    }

    /// From now on, `key` is redacted from every text this sink and its clones write:
    /// prompts, outputs, reasons, model names and tags ([`ApiKey::redact`]).
    pub fn redact_with(&self, key: &ApiKey) {
        self.keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(key.clone());
    }

    fn redact(&self, text: &mut String) {
        for key in self.keys.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            *text = key.redact(text);
        }
    }

    pub fn record(&self, mut row: CallRow) {
        if let Some(tx) = self.tx.get() {
            for text in [
                &mut row.purpose,
                &mut row.requested_model,
                &mut row.prompt,
                &mut row.output,
            ]
            .into_iter()
            .chain(
                [
                    &mut row.actual_model,
                    &mut row.task_type,
                    &mut row.session_id,
                    &mut row.generation_id,
                    &mut row.cost_source,
                    &mut row.error_kind,
                    &mut row.reason,
                ]
                .into_iter()
                .flatten(),
            ) {
                self.redact(text);
            }
            let _ = tx.send(Msg::Record(Box::new(row)));
        }
    }

    /// The game's verdict on a call (only a pending call takes it).
    pub fn verdict(&self, uid: &CallId, verdict: Verdict, mut reason: Option<String>) {
        if let Some(tx) = self.tx.get() {
            if let Some(r) = &mut reason {
                self.redact(r);
            }
            let _ = tx.send(Msg::Verdict(uid.clone(), verdict, reason));
        }
    }

    /// Waits up to `wait` until everything sent so far is written. `false` if it
    /// wasn't in time (or there is no writer).
    pub fn flush(&self, wait: Duration) -> bool {
        let Some(tx) = self.tx.get() else {
            return false;
        };
        let (ack, done) = mpsc::channel();
        tx.send(Msg::Flush(ack)).is_ok() && done.recv_timeout(wait).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_404_800_000; // 2026-10-31T00:00:00Z

    fn row(
        uid: &str,
        ts_ms: i64,
        purpose: &str,
        model: &str,
        cost: f64,
        verdict: Verdict,
    ) -> CallRow {
        CallRow {
            uid: CallId(uid.into()),
            ts_ms,
            purpose: purpose.into(),
            requested_model: "openrouter/auto".into(),
            actual_model: Some(model.into()),
            task_type: None,
            session_id: Some("station.lantern_wharf".into()),
            generation_id: None,
            prompt_tokens: Some(100),
            completion_tokens: Some(20),
            cost_usd: Some(cost),
            cost_source: Some("usage".into()),
            latency_ms: 900,
            attempts: 1,
            http_status: Some(200),
            error_kind: None,
            verdict,
            reason: (verdict == Verdict::Rejected).then(|| "reference: unknown ware".into()),
            prompt: "{}".into(),
            output: "{}".into(),
        }
    }

    /// A log with rows over the last 30 days: one a day, alternating models and
    /// purposes, every third rejected.
    fn seeded() -> CallLog {
        let log = CallLog::in_memory().unwrap();
        for d in 0..30 {
            let verdict = if d % 3 == 0 {
                Verdict::Rejected
            } else {
                Verdict::Accepted
            };
            let (purpose, model) = if d % 2 == 0 {
                ("banter", "m/a")
            } else {
                ("discount", "m/b")
            };
            log.insert(&row(
                &format!("u{d}"),
                NOW - d * DAY_MS - 1000,
                purpose,
                model,
                0.001,
                verdict,
            ))
            .unwrap();
        }
        log
    }

    #[test]
    fn path_sits_beside_the_save() {
        let p = path_for(Path::new("saves/run1.sqlite"));
        assert_eq!(p, Path::new("saves/run1.llm-log.sqlite"));
    }

    #[test]
    fn prune_rolls_old_rows_into_daily_rollups_and_keeps_the_totals() {
        let mut log = seeded();
        let before = log.totals(&Range::default()).unwrap();
        let report = log.prune(NOW, &LogRules::default()).unwrap();
        // Days 0..=13 are inside 14 days (the row of day 14 is 14 days and a second old).
        assert_eq!(report.expired, 16);
        assert_eq!(log.rows().unwrap().len(), 14);
        let after = log.totals(&Range::default()).unwrap();
        assert_eq!(after["calls"], before["calls"]);
        assert_eq!(after["rejected"], before["rejected"]);
        assert!((after["cost_usd"].as_f64().unwrap() - 0.03).abs() < 1e-9);
        assert_eq!(after["rolled_up"], 16);
        let rollups = log.rollups(&Range::default()).unwrap();
        assert_eq!(rollups.len(), 16, "one per day, model and purpose");
        // Pruning again changes nothing.
        let again = log.prune(NOW, &LogRules::default()).unwrap();
        assert_eq!((again.expired, again.over_cap), (0, 0));
        // A range reads rollups and rows alike.
        let range = Range {
            since: Some("2026-10-01".into()),
            until: Some("2026-10-02".into()),
        };
        assert_eq!(log.totals(&range).unwrap()["calls"], 2);
    }

    #[test]
    fn the_size_cap_rolls_up_the_oldest_rows_first() {
        let mut log = CallLog::in_memory().unwrap();
        let big = "x".repeat(60_000);
        for i in 0..60 {
            let mut r = row(
                &format!("b{i}"),
                NOW - (60 - i) * 60_000,
                "banter",
                "m/a",
                0.0001,
                Verdict::Accepted,
            );
            r.output = big.clone();
            log.insert(&r).unwrap();
        }
        assert!(log.used_bytes().unwrap() > 3 * 1024 * 1024);
        let report = log
            .prune(
                NOW,
                &LogRules {
                    keep_days: 14,
                    max_mb: 1,
                },
            )
            .unwrap();
        assert!(report.over_cap > 0);
        assert!(log.used_bytes().unwrap() <= 1024 * 1024);
        let rows = log.rows().unwrap();
        let kept: Vec<&str> = rows.iter().map(|r| r["output"].as_str().unwrap()).collect();
        assert!(!kept.is_empty() && kept.len() < 60);
        // The newest survive; nothing is lost from the totals.
        assert_eq!(log.totals(&Range::default()).unwrap()["calls"], 60);
    }

    #[test]
    fn queries_group_by_model_and_purpose_and_list_rejections() {
        let mut log = seeded();
        log.prune(NOW, &LogRules::default()).unwrap();
        let by_model = log.cost_by(Group::Model, &Range::default()).unwrap();
        assert_eq!(by_model.len(), 2);
        assert_eq!(
            by_model
                .iter()
                .map(|v| v["calls"].as_i64().unwrap())
                .sum::<i64>(),
            30
        );
        let by_purpose = log.cost_by(Group::Purpose, &Range::default()).unwrap();
        let banter = by_purpose
            .iter()
            .find(|v| v["purpose"] == "banter")
            .unwrap();
        assert_eq!(banter["calls"], 15);
        assert!(banter["rejection_rate"].as_f64().unwrap() > 0.0);
        let rejections = log.recent_rejections(3).unwrap();
        assert_eq!(rejections.len(), 3);
        assert_eq!(rejections[0]["uid"], "u0", "newest first");
        assert_eq!(rejections[0]["reason"], "reference: unknown ware");
    }

    #[test]
    fn verdicts_only_settle_pending_calls() {
        let log = CallLog::in_memory().unwrap();
        log.insert(&row("p", NOW, "banter", "m/a", 0.0, Verdict::Pending))
            .unwrap();
        log.insert(&row("f", NOW, "banter", "m/a", 0.0, Verdict::Failed))
            .unwrap();
        log.set_verdict(
            &CallId("p".into()),
            Verdict::Rejected,
            Some("text: no first person"),
        )
        .unwrap();
        log.set_verdict(&CallId("f".into()), Verdict::Accepted, None)
            .unwrap();
        assert_eq!(
            log.row(&CallId("p".into())).unwrap().unwrap()["verdict"],
            "rejected"
        );
        assert_eq!(
            log.row(&CallId("f".into())).unwrap().unwrap()["verdict"],
            "failed"
        );
    }

    #[test]
    fn a_sink_writes_off_thread_and_an_unconnected_one_drops() {
        let off = CallSink::new();
        off.record(row("x", NOW, "banter", "m/a", 0.0, Verdict::Pending));
        assert!(!off.flush(Duration::from_millis(10)));

        let dir = std::env::temp_dir().join(format!("galaxy-calllog-{}", std::process::id()));
        let path = dir.join("t.llm-log.sqlite");
        let _ = std::fs::remove_file(&path);
        let sink = CallSink::new();
        sink.open(&path, &LogRules::default()).unwrap();
        let clone = sink.clone();
        clone.record(row("y", NOW, "traffic", "m/a", 0.0002, Verdict::Pending));
        sink.verdict(&CallId("y".into()), Verdict::Accepted, None);
        assert!(sink.flush(Duration::from_secs(5)));
        let read = CallLog::open_read(&path).unwrap();
        assert_eq!(
            read.row(&CallId("y".into())).unwrap().unwrap()["verdict"],
            "accepted"
        );
        drop(read);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_text_is_capped_on_a_char_boundary() {
        let s = "é".repeat(MAX_TEXT);
        let c = cap(&s);
        assert!(c.len() < s.len() && c.ends_with("bytes]"));
    }
}
