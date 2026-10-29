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
//! - `snapshots`: full copies of world + kernel with the history high-water marks,
//!   taken before game-master changes (hard restore).
//!
//! Fork a save = copy the file ([`fork`]).

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;

use crate::history::History;
use crate::sim::{KernelState, SimState};

/// Bump when the table layout changes incompatibly.
pub const SAVE_FORMAT_VERSION: u32 = 1;

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
    journal_seq INTEGER NOT NULL
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

impl Save {
    /// Creates a new save file holding `state`. Fails if the file exists.
    pub fn create(path: impl AsRef<Path>, state: &SimState) -> Result<Save, SaveError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(SaveError::Exists(path));
        }
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

    /// Writes `state`: replaces the world row and syncs the append-only tables. Rows
    /// past what `state` holds (after a hard restore) are dropped.
    pub fn write(&mut self, state: &SimState) -> Result<(), SaveError> {
        let hash = crate::hash::state_hash(state);
        let world = serde_json::to_string(&state.world)?;
        let kernel = serde_json::to_string(&state.kernel)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO world (id, tick, hash, world, kernel) VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET tick = ?1, hash = ?2, world = ?3, kernel = ?4",
            params![to_i64(state.world.meta.tick.0), hash, world, kernel],
        )?;

        let last_fact = state.history.last_fact_id().map_or(0, |f| f.0);
        tx.execute("DELETE FROM history WHERE seq > ?1", [to_i64(last_fact)])?;
        let stored: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM history", [], |r| {
            r.get(0)
        })?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO history (seq, tick, kind, cause, ship, station, ware, rock,
                 npc, faction, region, other, amount, fact)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;
            for f in state
                .history
                .facts()
                .iter()
                .filter(|f| to_i64(f.id.0) > stored)
            {
                let s = &f.subjects;
                insert.execute(params![
                    to_i64(f.id.0),
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
                    serde_json::to_string(f)?,
                ])?;
            }
        }

        let last_cause = state.history.causes().keys().next_back().map_or(0, |c| c.0);
        tx.execute("DELETE FROM causes WHERE id > ?1", [to_i64(last_cause)])?;
        let stored: i64 =
            tx.query_row("SELECT COALESCE(MAX(id), 0) FROM causes", [], |r| r.get(0))?;
        {
            let mut insert =
                tx.prepare("INSERT INTO causes (id, tick, kind, cause) VALUES (?1, ?2, ?3, ?4)")?;
            for c in state
                .history
                .causes()
                .values()
                .filter(|c| to_i64(c.id.0) > stored)
            {
                let kind = serde_json::to_value(&c.cause.kind)?["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                insert.execute(params![
                    to_i64(c.id.0),
                    to_i64(c.tick.0),
                    kind,
                    serde_json::to_string(c)?
                ])?;
            }
        }

        let journal_len = state.journal.len() as i64;
        tx.execute("DELETE FROM journal WHERE seq > ?1", [journal_len])?;
        let stored: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) FROM journal", [], |r| {
            r.get(0)
        })?;
        {
            let mut insert = tx
                .prepare("INSERT INTO journal (seq, tick, cause, entry) VALUES (?1, ?2, ?3, ?4)")?;
            for (i, e) in state.journal.iter().enumerate().skip(stored as usize) {
                insert.execute(params![
                    i as i64 + 1,
                    to_i64(e.tick.0),
                    to_i64(e.cause.0),
                    serde_json::to_string(e)?
                ])?;
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
        let world: WorldState = serde_json::from_str(&world)?;
        let kernel: KernelState = serde_json::from_str(&kernel)?;
        self.load_rest(world, kernel, i64::MAX, i64::MAX, i64::MAX)
    }

    fn load_rest(
        &self,
        world: WorldState,
        kernel: KernelState,
        history_seq: i64,
        cause_seq: i64,
        journal_seq: i64,
    ) -> Result<SimState, SaveError> {
        let facts = self.json_rows::<Fact>(
            "SELECT fact FROM history WHERE seq <= ?1 ORDER BY seq",
            history_seq,
        )?;
        let causes = self.json_rows::<RecordedCause>(
            "SELECT cause FROM causes WHERE id <= ?1 ORDER BY id",
            cause_seq,
        )?;
        let journal = self.json_rows::<JournalEntry>(
            "SELECT entry FROM journal WHERE seq <= ?1 ORDER BY seq",
            journal_seq,
        )?;
        Ok(SimState {
            world,
            kernel,
            history: History::from_parts(facts, causes),
            journal,
        })
    }

    fn json_rows<T: serde::de::DeserializeOwned>(
        &self,
        sql: &str,
        bound: i64,
    ) -> Result<Vec<T>, SaveError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([bound], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
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

    /// Stores a snapshot of `state` (before a game-master change, say).
    pub fn snapshot(&mut self, state: &SimState, label: &str) -> Result<i64, SaveError> {
        self.conn.execute(
            "INSERT INTO snapshots (tick, label, hash, world, kernel, history_seq, cause_seq, journal_seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                to_i64(state.world.meta.tick.0),
                label,
                crate::hash::state_hash(state),
                serde_json::to_string(&state.world)?,
                serde_json::to_string(&state.kernel)?,
                to_i64(state.history.last_fact_id().map_or(0, |f| f.0)),
                to_i64(state.history.causes().keys().next_back().map_or(0, |c| c.0)),
                state.journal.len() as i64,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
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
    /// drops later history from the file).
    pub fn load_snapshot(&self, id: i64) -> Result<SimState, SaveError> {
        type Row = (String, String, i64, i64, i64);
        let row: Option<Row> = self
            .conn
            .query_row(
                "SELECT world, kernel, history_seq, cause_seq, journal_seq FROM snapshots WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let (world, kernel, h, c, j) = row.ok_or(SaveError::NoSnapshot(id))?;
        self.load_rest(
            serde_json::from_str(&world)?,
            serde_json::from_str(&kernel)?,
            h,
            c,
            j,
        )
    }
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
