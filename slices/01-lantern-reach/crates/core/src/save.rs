//! Saves: one SQLite file per save.
//!
//! Tables:
//! - `meta`: key/value strings (`format`, `world_format`, `seed`, `created_by`).
//! - `world`: one row with the world state and kernel state as JSON, the tick and
//!   the state hash at write time.
//! - `history`: the append-only fact log, one row per fact with indexed subject
//!   columns and the full fact as JSON.
//! - `causes`: recorded causes.
//! - `journal`: undo records for journaled causes (rollback by cause).
//! - `snapshots`: complete states taken before game-master changes (hard restore):
//!   world and kernel, and each log as a prefix shared with the save's own table (up
//!   to `history_seq`, `cause_seq`, `journal_seq`) plus a `tail` of rows the snapshot
//!   holds itself, with a `digest` of the whole timeline that is checked on load.
//!
//! The three logs are append-only within one timeline. [`Save::write`] compares the
//! stored rows with the state's and replaces everything from the first difference, so
//! writing a restored snapshot (or any state that diverged from the file) never keeps
//! rows from a discarded timeline. Before it drops rows a snapshot still shares, it
//! moves them into that snapshot's tail, so every listed snapshot stays loadable.
//!
//! Fork a save = copy the file ([`fork`]).

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;

use crate::history::History;
use crate::sim::{KernelState, SimState};

/// Bump when the table layout changes incompatibly.
pub const SAVE_FORMAT_VERSION: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("save file already exists: {0}")]
    Exists(PathBuf),
    #[error("no save file at {0}")]
    Missing(PathBuf),
    #[error("save format {found} is not supported (this build reads {expected})")]
    Format { found: String, expected: u32 },
    #[error("save is corrupt: {0}")]
    Corrupt(String),
    #[error("no snapshot {0}")]
    NoSnapshot(i64),
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

const SCHEMA: &str = r#"
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE world (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    tick INTEGER NOT NULL,
    hash TEXT NOT NULL,
    world TEXT NOT NULL,
    kernel TEXT NOT NULL
);
CREATE TABLE history (
    seq INTEGER PRIMARY KEY,
    tick INTEGER NOT NULL,
    kind TEXT NOT NULL,
    cause INTEGER NOT NULL,
    ship TEXT, station TEXT, ware TEXT, rock TEXT,
    npc TEXT, faction TEXT, region TEXT, other TEXT,
    amount INTEGER,
    fact TEXT NOT NULL
);
CREATE INDEX history_kind ON history (kind, tick);
CREATE INDEX history_station ON history (station, kind);
CREATE INDEX history_ship ON history (ship, kind);
CREATE INDEX history_other ON history (other, kind);
CREATE INDEX history_cause ON history (cause);
CREATE TABLE causes (
    id INTEGER PRIMARY KEY,
    tick INTEGER NOT NULL,
    kind TEXT NOT NULL,
    cause TEXT NOT NULL
);
CREATE TABLE journal (
    seq INTEGER PRIMARY KEY,
    tick INTEGER NOT NULL,
    cause INTEGER NOT NULL,
    entry TEXT NOT NULL
);
CREATE INDEX journal_cause ON journal (cause);
CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tick INTEGER NOT NULL,
    label TEXT NOT NULL,
    hash TEXT NOT NULL,
    world TEXT NOT NULL,
    kernel TEXT NOT NULL,
    history_seq INTEGER NOT NULL,
    cause_seq INTEGER NOT NULL,
    journal_seq INTEGER NOT NULL,
    tail TEXT NOT NULL,
    digest TEXT NOT NULL
);
"#;

/// Summary of a save, for the CLI.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SaveInfo {
    pub format: u32,
    pub seed: u64,
    pub tick: u64,
    pub hash: String,
    pub facts: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SnapshotInfo {
    pub id: i64,
    pub tick: u64,
    pub label: String,
    pub hash: String,
}

/// An open save file.
pub struct Save {
    conn: Connection,
    path: PathBuf,
}

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).expect("sequence numbers fit in i64")
}

/// One of the three logs a save mirrors: rows keyed by an increasing number, each
/// holding one record as JSON.
#[derive(Clone, Copy)]
enum Log {
    History,
    Causes,
    Journal,
}

impl Log {
    const ALL: [Log; 3] = [Log::History, Log::Causes, Log::Journal];

    fn table(self) -> &'static str {
        match self {
            Log::History => "history",
            Log::Causes => "causes",
            Log::Journal => "journal",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Log::History | Log::Journal => "seq",
            Log::Causes => "id",
        }
    }

    fn json(self) -> &'static str {
        match self {
            Log::History => "fact",
            Log::Causes => "cause",
            Log::Journal => "entry",
        }
    }

    /// The snapshot column: up to which key a snapshot shares this log's table.
    fn mark(self) -> &'static str {
        match self {
            Log::History => "history_seq",
            Log::Causes => "cause_seq",
            Log::Journal => "journal_seq",
        }
    }
}

/// A log row as the save stores it.
struct Row {
    key: i64,
    json: String,
}

fn row<T: Serialize>(key: u64, value: &T) -> Result<Row, SaveError> {
    Ok(Row {
        key: to_i64(key),
        json: serde_json::to_string(value)?,
    })
}

/// The state's logs as rows in key order, indexed by `Log as usize`.
fn log_rows(state: &SimState) -> Result<[Vec<Row>; 3], SaveError> {
    let facts = state.history.facts().iter().map(|f| row(f.id.0, f));
    let causes = state.history.causes().values().map(|c| row(c.id.0, c));
    let journal = (1..).zip(&state.journal).map(|(i, e)| row(i, e));
    Ok([
        facts.collect::<Result<_, _>>()?,
        causes.collect::<Result<_, _>>()?,
        journal.collect::<Result<_, _>>()?,
    ])
}

/// Per log, the row JSON a snapshot holds itself (its rows past its marks).
type Tail = [Vec<String>; 3];

/// How many of `rows` the stored log already holds as its first rows, unchanged.
fn shared_prefix(conn: &Connection, log: Log, rows: &[Row]) -> Result<usize, SaveError> {
    let (key, json, table) = (log.key(), log.json(), log.table());
    let mut stmt = conn.prepare(&format!("SELECT {key}, {json} FROM {table} ORDER BY {key}"))?;
    let mut stored = stmt.query([])?;
    let mut n = 0;
    while let Some(want) = rows.get(n) {
        let Some(r) = stored.next()? else { break };
        let same = r.get::<_, i64>(0)? == want.key
            && matches!(r.get_ref(1)?, ValueRef::Text(t) if t == want.json.as_bytes());
        if !same {
            break;
        }
        n += 1;
    }
    Ok(n)
}

/// The key of the last of the first `n` rows, 0 for none.
fn mark_at(rows: &[Row], n: usize) -> i64 {
    n.checked_sub(1).map_or(0, |i| rows[i].key)
}

/// The stored JSON of a log's rows with keys in `(after, upto]`, in key order.
fn stored_json(
    conn: &Connection,
    log: Log,
    after: i64,
    upto: i64,
) -> Result<Vec<String>, SaveError> {
    let (key, json, table) = (log.key(), log.json(), log.table());
    let mut stmt = conn.prepare(&format!(
        "SELECT {json} FROM {table} WHERE {key} > ?1 AND {key} <= ?2 ORDER BY {key}"
    ))?;
    let rows = stmt.query_map([after, upto], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// BLAKE3 over every row of a timeline's three logs.
fn timeline_digest<'a>(logs: impl IntoIterator<Item = Vec<&'a str>>) -> String {
    let mut h = blake3::Hasher::new();
    for rows in logs {
        h.update(&(rows.len() as u64).to_le_bytes());
        for json in rows {
            h.update(&(json.len() as u64).to_le_bytes());
            h.update(json.as_bytes());
        }
    }
    h.finalize().to_hex().to_string()
}

/// JSON can't hold NaN or infinity (`serde_json` writes `null` and then can't read it
/// back), so a state holding one is refused instead of written.
fn check_saveable(state: &SimState) -> Result<(), SaveError> {
    crate::finite::check(state).map_err(|e| {
        SaveError::Json(serde::ser::Error::custom(format!(
            "can't save a non-finite number: {e}"
        )))
    })
}

/// Before `log` rows with keys above `keep` are dropped, moves the ones a snapshot
/// still shares into that snapshot's tail.
fn detach_snapshots(conn: &Connection, log: Log, keep: i64) -> Result<(), SaveError> {
    let mark = log.mark();
    let affected: Vec<(i64, i64, String)> = {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, {mark}, tail FROM snapshots WHERE {mark} > ?1"
        ))?;
        let rows = stmt.query_map([keep], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (id, shared, tail) in affected {
        let mut tail: Tail = serde_json::from_str(&tail)?;
        let mut moved = stored_json(conn, log, keep, shared)?;
        moved.append(&mut tail[log as usize]);
        tail[log as usize] = moved;
        conn.execute(
            &format!("UPDATE snapshots SET {mark} = ?1, tail = ?2 WHERE id = ?3"),
            params![keep, serde_json::to_string(&tail)?, id],
        )?;
    }
    Ok(())
}

impl Save {
    /// Creates a new save file holding `state`. Fails if the file exists.
    pub fn create(path: impl AsRef<Path>, state: &SimState) -> Result<Save, SaveError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(SaveError::Exists(path));
        }
        check_saveable(state)?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn = Connection::open(&path)?;
        {
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA)?;
            let meta = [
                ("format", SAVE_FORMAT_VERSION.to_string()),
                ("world_format", state.world.meta.format.to_string()),
                ("seed", state.world.meta.seed.to_string()),
                (
                    "created_by",
                    format!("galaxy {}", env!("CARGO_PKG_VERSION")),
                ),
            ];
            for (k, v) in meta {
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    params![k, v],
                )?;
            }
            tx.commit()?;
        }
        let mut save = Save { conn, path };
        save.write(state)?;
        Ok(save)
    }

    /// Opens an existing save and checks its format.
    pub fn open(path: impl AsRef<Path>) -> Result<Save, SaveError> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(SaveError::Missing(path));
        }
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let save = Save { conn, path };
        let found = save.meta("format")?.unwrap_or_default();
        if found != SAVE_FORMAT_VERSION.to_string() {
            return Err(SaveError::Format {
                found,
                expected: SAVE_FORMAT_VERSION,
            });
        }
        Ok(save)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn meta(&self, key: &str) -> Result<Option<String>, SaveError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// Writes `state`: replaces the world row and syncs the logs. Stored log rows from
    /// the first one that differs from `state` onward (another timeline, after a hard
    /// restore) are replaced; snapshots that shared them keep their own copies.
    /// Refuses a state with a non-finite number in it, leaving the file as it was.
    pub fn write(&mut self, state: &SimState) -> Result<(), SaveError> {
        check_saveable(state)?;
        let hash = crate::hash::state_hash(state);
        let world = serde_json::to_string(&state.world)?;
        let kernel = serde_json::to_string(&state.kernel)?;
        let rows = log_rows(state)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO world (id, tick, hash, world, kernel) VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET tick = ?1, hash = ?2, world = ?3, kernel = ?4",
            params![to_i64(state.world.meta.tick.0), hash, world, kernel],
        )?;
        for log in Log::ALL {
            let rows = &rows[log as usize];
            let n = shared_prefix(&tx, log, rows)?;
            let keep = mark_at(rows, n);
            detach_snapshots(&tx, log, keep)?;
            tx.execute(
                &format!("DELETE FROM {} WHERE {} > ?1", log.table(), log.key()),
                [keep],
            )?;
            match log {
                Log::History => insert_facts(&tx, &state.history.facts()[n..], &rows[n..])?,
                Log::Causes => {
                    let causes = state.history.causes().values().skip(n);
                    insert_causes(&tx, causes, &rows[n..])?;
                }
                Log::Journal => insert_journal(&tx, &state.journal[n..], &rows[n..])?,
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Reads the full state back.
    pub fn load(&self) -> Result<SimState, SaveError> {
        let (world, kernel): (String, String) = self
            .conn
            .query_row("SELECT world, kernel FROM world WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or_else(|| SaveError::Corrupt("no world row".into()))?;
        let mut logs: [Vec<String>; 3] = Default::default();
        for log in Log::ALL {
            logs[log as usize] = stored_json(&self.conn, log, i64::MIN, i64::MAX)?;
        }
        assemble(&world, &kernel, logs)
    }

    /// The stored world state as JSON, without loading the rest.
    pub fn world_json(&self) -> Result<serde_json::Value, SaveError> {
        let text: String =
            self.conn
                .query_row("SELECT world FROM world WHERE id = 1", [], |r| r.get(0))?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn info(&self) -> Result<SaveInfo, SaveError> {
        let (tick, hash): (i64, String) =
            self.conn
                .query_row("SELECT tick, hash FROM world WHERE id = 1", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        let facts: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))?;
        let parse = |key: &str| -> Result<u64, SaveError> {
            self.meta(key)?
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| SaveError::Corrupt(format!("meta `{key}` missing")))
        };
        Ok(SaveInfo {
            format: parse("format")? as u32,
            seed: parse("seed")?,
            tick: tick as u64,
            hash,
            facts: facts as u64,
        })
    }

    /// Queries the history table directly (indexed), with the same semantics as
    /// [`History::find`].
    pub fn query_history(&self, q: &HistoryQuery) -> Result<Vec<Fact>, SaveError> {
        let mut sql = String::from("SELECT fact FROM history WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        let mut text = |col: &str, v: Option<&str>, sql: &mut String| {
            if let Some(v) = v {
                args.push(v.to_owned().into());
                sql.push_str(&format!(" AND {col} = ?{}", args.len()));
            }
        };
        text("kind", q.kind.as_ref().map(|x| x.as_str()), &mut sql);
        text("ship", q.ship.as_ref().map(|x| x.as_str()), &mut sql);
        text("station", q.station.as_ref().map(|x| x.as_str()), &mut sql);
        text("ware", q.ware.as_ref().map(|x| x.as_str()), &mut sql);
        text("rock", q.rock.as_ref().map(|x| x.as_str()), &mut sql);
        text("npc", q.npc.as_ref().map(|x| x.as_str()), &mut sql);
        text("faction", q.faction.as_ref().map(|x| x.as_str()), &mut sql);
        text("region", q.region.as_ref().map(|x| x.as_str()), &mut sql);
        text("other", q.other.as_ref().map(|x| x.as_str()), &mut sql);
        let mut num = |col: &str, op: &str, v: Option<u64>, sql: &mut String| {
            if let Some(v) = v {
                args.push(to_i64(v).into());
                sql.push_str(&format!(" AND {col} {op} ?{}", args.len()));
            }
        };
        num("cause", "=", q.cause.map(|c| c.0), &mut sql);
        num("tick", ">=", q.since.map(|t| t.0), &mut sql);
        num("tick", "<=", q.until.map(|t| t.0), &mut sql);
        num("seq", ">", q.after.map(|f| f.0), &mut sql);
        sql.push_str(" ORDER BY seq");
        if let Some(limit) = q.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }

    /// Stores a complete snapshot of `state` (before a game-master change, say), whether
    /// or not `state` has been written. Log rows it shares with the save's tables are
    /// referenced; the rest is copied into the snapshot.
    pub fn snapshot(&mut self, state: &SimState, label: &str) -> Result<i64, SaveError> {
        check_saveable(state)?;
        let rows = log_rows(state)?;
        let tx = self.conn.transaction()?;
        let mut marks = [0; 3];
        let mut tail = Tail::default();
        for log in Log::ALL {
            let rows = &rows[log as usize];
            let n = shared_prefix(&tx, log, rows)?;
            marks[log as usize] = mark_at(rows, n);
            tail[log as usize] = rows[n..].iter().map(|r| r.json.clone()).collect();
        }
        let digest = timeline_digest(
            rows.iter()
                .map(|log| log.iter().map(|r| r.json.as_str()).collect()),
        );
        tx.execute(
            "INSERT INTO snapshots (tick, label, hash, world, kernel, history_seq, cause_seq,
             journal_seq, tail, digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                to_i64(state.world.meta.tick.0),
                label,
                crate::hash::state_hash(state),
                serde_json::to_string(&state.world)?,
                serde_json::to_string(&state.kernel)?,
                marks[0],
                marks[1],
                marks[2],
                serde_json::to_string(&tail)?,
                digest,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn snapshots(&self) -> Result<Vec<SnapshotInfo>, SaveError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, tick, label, hash FROM snapshots ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(SnapshotInfo {
                id: r.get(0)?,
                tick: r.get::<_, i64>(1)? as u64,
                label: r.get(2)?,
                hash: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The state as it was at a snapshot. `write` it back for a hard restore (that
    /// replaces later history in the file; other snapshots stay loadable). Fails with
    /// [`SaveError::Corrupt`] rather than return a state with incomplete logs.
    pub fn load_snapshot(&self, id: i64) -> Result<SimState, SaveError> {
        type Row = (String, String, [i64; 3], String, String);
        let row: Option<Row> = self
            .conn
            .query_row(
                "SELECT world, kernel, history_seq, cause_seq, journal_seq, tail, digest
                 FROM snapshots WHERE id = ?1",
                [id],
                |r| {
                    let marks = [r.get(2)?, r.get(3)?, r.get(4)?];
                    Ok((r.get(0)?, r.get(1)?, marks, r.get(5)?, r.get(6)?))
                },
            )
            .optional()?;
        let (world, kernel, marks, tail, digest) = row.ok_or(SaveError::NoSnapshot(id))?;
        let mut tail: Tail = serde_json::from_str(&tail)?;
        let mut logs: [Vec<String>; 3] = Default::default();
        for log in Log::ALL {
            let i = log as usize;
            logs[i] = stored_json(&self.conn, log, i64::MIN, marks[i])?;
            logs[i].append(&mut tail[i]);
        }
        let found = timeline_digest(
            logs.iter()
                .map(|log| log.iter().map(String::as_str).collect()),
        );
        if found != digest {
            return Err(SaveError::Corrupt(format!(
                "snapshot {id} no longer matches its recorded history"
            )));
        }
        assemble(&world, &kernel, logs)
    }
}

/// A state from its stored world, kernel and log rows.
fn assemble(world: &str, kernel: &str, logs: [Vec<String>; 3]) -> Result<SimState, SaveError> {
    fn parse<T: serde::de::DeserializeOwned>(rows: &[String]) -> Result<Vec<T>, SaveError> {
        rows.iter().map(|r| Ok(serde_json::from_str(r)?)).collect()
    }
    let [facts, causes, journal] = logs;
    let legacy_canon = serde_json::from_str::<serde_json::Value>(world)?
        .get("canon")
        .is_none();
    let mut world: WorldState = serde_json::from_str(world)?;
    let facts: Vec<Fact> = parse(&facts)?;
    if legacy_canon {
        for f in facts
            .iter()
            .filter(|f| f.kind.as_str() == "core.canon_opened")
        {
            if let Some(n) = f
                .subjects
                .other
                .as_ref()
                .and_then(|id| world.canon.nodes.get_mut(id.as_str()))
                && n.status == galaxy_contract::canon::CanonStatus::Sealed
            {
                n.status = galaxy_contract::canon::CanonStatus::Open;
            }
        }
    }
    // World format 2 stores money as centicredits; format 1 stored whole credits.
    // Snapshots carry their own world, so this guards them too.
    if world.meta.format != WORLD_FORMAT_VERSION {
        return Err(SaveError::Format {
            found: format!("world {}", world.meta.format),
            expected: WORLD_FORMAT_VERSION,
        });
    }
    Ok(SimState {
        world,
        kernel: serde_json::from_str::<KernelState>(kernel)?,
        history: History::from_parts(facts, parse(&causes)?),
        journal: parse(&journal)?,
    })
}

fn insert_facts(conn: &Connection, facts: &[Fact], rows: &[Row]) -> Result<(), SaveError> {
    let mut insert = conn.prepare(
        "INSERT INTO history (seq, tick, kind, cause, ship, station, ware, rock,
         npc, faction, region, other, amount, fact)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    for (f, row) in facts.iter().zip(rows) {
        let s = &f.subjects;
        insert.execute(params![
            row.key,
            to_i64(f.tick.0),
            f.kind.as_str(),
            to_i64(f.cause.0),
            s.ship.as_ref().map(|x| x.as_str()),
            s.station.as_ref().map(|x| x.as_str()),
            s.ware.as_ref().map(|x| x.as_str()),
            s.rock.as_ref().map(|x| x.as_str()),
            s.npc.as_ref().map(|x| x.as_str()),
            s.faction.as_ref().map(|x| x.as_str()),
            s.region.as_ref().map(|x| x.as_str()),
            s.other.as_ref().map(|x| x.as_str()),
            f.amount,
            row.json,
        ])?;
    }
    Ok(())
}

fn insert_causes<'a>(
    conn: &Connection,
    causes: impl Iterator<Item = &'a RecordedCause>,
    rows: &[Row],
) -> Result<(), SaveError> {
    let mut insert =
        conn.prepare("INSERT INTO causes (id, tick, kind, cause) VALUES (?1, ?2, ?3, ?4)")?;
    for (c, row) in causes.zip(rows) {
        let kind = serde_json::to_value(&c.cause.kind)?["kind"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        insert.execute(params![row.key, to_i64(c.tick.0), kind, row.json])?;
    }
    Ok(())
}

fn insert_journal(
    conn: &Connection,
    entries: &[JournalEntry],
    rows: &[Row],
) -> Result<(), SaveError> {
    let mut insert =
        conn.prepare("INSERT INTO journal (seq, tick, cause, entry) VALUES (?1, ?2, ?3, ?4)")?;
    for (e, row) in entries.iter().zip(rows) {
        insert.execute(params![
            row.key,
            to_i64(e.tick.0),
            to_i64(e.cause.0),
            row.json
        ])?;
    }
    Ok(())
}

/// Forks a save: a plain file copy. Fails if `to` exists.
pub fn fork(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<(), SaveError> {
    let (from, to) = (from.as_ref(), to.as_ref());
    if !from.is_file() {
        return Err(SaveError::Missing(from.to_path_buf()));
    }
    if to.exists() {
        return Err(SaveError::Exists(to.to_path_buf()));
    }
    std::fs::copy(from, to)?;
    Ok(())
}
