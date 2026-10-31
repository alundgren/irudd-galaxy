//! Saves: one SQLite file per save.
//!
//! Tables:
//! - `meta`: key and value text: `format`, `engine`, `build`, `seed`, `tick`, `hash`,
//!   `next_event`.
//! - `modules`: every module the world ran with: `id`, `version`, `min_engine_version`
//!   and `files` (the hash of its files).
//! - `components`: one row per component: `entity`, `component`, `fields` (JSON), and the
//!   `cause` and `tick` of its last change.
//! - `facts`: the history, one row per fact: `seq`, `tick`, `kind`, `cause`, `payload`.
//! - `causes`: every cause: `id`, `tick`, `cause` (JSON).
//! - `state`: the rest of the sim state as JSON: the random streams and the engine
//!   services' causes.
//!
//! Tables rather than one blob, so tools (and later the GM's live reads) can query a save
//! with plain SQL. Loading rebuilds the world and checks its hash against `meta.hash`, so a
//! save that doesn't load back exactly is refused. Writing goes to a temporary file that
//! replaces the target only when complete.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;

use crate::recording::BuildStamp;
use crate::world::{CauseRecord, Component, Fact, World};

/// The save format this engine writes and reads. Saves in other formats are refused with
/// a message; the game is a prototype, so there are no migrations.
pub const FORMAT: u32 = 1;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE modules (
    id TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    min_engine_version TEXT NOT NULL,
    files TEXT NOT NULL
);
CREATE TABLE components (
    entity TEXT NOT NULL,
    component TEXT NOT NULL,
    fields TEXT NOT NULL,
    cause INTEGER NOT NULL,
    tick INTEGER NOT NULL,
    PRIMARY KEY (entity, component)
);
CREATE TABLE facts (
    seq INTEGER PRIMARY KEY,
    tick INTEGER NOT NULL,
    kind TEXT NOT NULL,
    cause INTEGER NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX facts_kind ON facts (kind, tick);
CREATE TABLE causes (id INTEGER PRIMARY KEY, tick INTEGER NOT NULL, cause TEXT NOT NULL);
CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

/// A module a save lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SavedModule {
    pub id: String,
    pub version: String,
    pub min_engine_version: String,
    pub files: String,
}

/// What a save says about itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SaveInfo {
    pub format: u32,
    pub engine: String,
    pub build: Option<String>,
    pub seed: u64,
    pub tick: u64,
    pub hash: String,
    pub modules: Vec<SavedModule>,
}

fn i(n: u64) -> i64 {
    i64::try_from(n).expect("counts fit in i64")
}

fn sql(e: rusqlite::Error) -> String {
    format!("SQLite: {e}")
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("save values serialize")
}

/// Writes `bytes` to `path` through a temporary file beside it.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = temp_beside(path);
    std::fs::write(&tmp, bytes).map_err(|e| format!("can't write {}: {e}", tmp.display()))?;
    replace(&tmp, path)
}

fn temp_beside(path: &Path) -> std::path::PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

fn replace(tmp: &Path, path: &Path) -> Result<(), String> {
    std::fs::rename(tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(tmp);
        format!("can't write {}: {e}", path.display())
    })
}

/// Writes the world and the modules it runs with to a save file, replacing any file there.
pub fn write<'a>(
    path: &Path,
    world: &World,
    modules: impl Iterator<Item = &'a galaxy_module::module::Module>,
    build: &BuildStamp,
) -> Result<(), String> {
    let tmp = temp_beside(path);
    let _ = std::fs::remove_file(&tmp);
    {
        let mut conn = Connection::open(&tmp).map_err(sql)?;
        conn.execute_batch(SCHEMA).map_err(sql)?;
        let tx = conn.transaction().map_err(sql)?;
        {
            let mut meta = tx
                .prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")
                .map_err(sql)?;
            for (key, value) in [
                ("format", FORMAT.to_string()),
                ("engine", build.version.clone()),
                ("build", build.name.clone().unwrap_or_default()),
                ("seed", world.seed.to_string()),
                ("tick", world.tick.to_string()),
                ("hash", world.hash()),
                ("next_event", world.next_event.to_string()),
            ] {
                meta.execute(params![key, value]).map_err(sql)?;
            }
            let mut module_row = tx
                .prepare("INSERT INTO modules VALUES (?1, ?2, ?3, ?4)")
                .map_err(sql)?;
            for m in modules {
                module_row
                    .execute(params![
                        m.id(),
                        m.manifest.version.to_string(),
                        m.manifest.min_engine_version.to_string(),
                        m.files.hash()
                    ])
                    .map_err(sql)?;
            }
            let mut component = tx
                .prepare("INSERT INTO components VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(sql)?;
            for (entity, components) in &world.entities {
                for (name, c) in components {
                    component
                        .execute(params![
                            entity,
                            name,
                            json(&c.fields),
                            i(c.cause),
                            i(c.tick)
                        ])
                        .map_err(sql)?;
                }
            }
            let mut fact = tx
                .prepare("INSERT INTO facts VALUES (?1, ?2, ?3, ?4, ?5)")
                .map_err(sql)?;
            for f in &world.facts {
                fact.execute(params![
                    i(f.seq),
                    i(f.tick),
                    f.kind,
                    i(f.cause),
                    json(&f.payload)
                ])
                .map_err(sql)?;
            }
            let mut cause = tx
                .prepare("INSERT INTO causes VALUES (?1, ?2, ?3)")
                .map_err(sql)?;
            for c in &world.causes {
                cause
                    .execute(params![i(c.id), i(c.tick), json(&c.cause)])
                    .map_err(sql)?;
            }
            let mut state = tx
                .prepare("INSERT INTO state VALUES (?1, ?2)")
                .map_err(sql)?;
            state
                .execute(params!["rng", json(&world.rng)])
                .map_err(sql)?;
            state
                .execute(params!["services", json(&world.services)])
                .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        conn.close().map_err(|(_, e)| sql(e))?;
    }
    replace(&tmp, path)
}

fn meta(conn: &Connection, key: &str) -> Result<String, String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .map_err(|_| format!("the save has no `{key}`"))
}

fn number(conn: &Connection, key: &str) -> Result<u64, String> {
    meta(conn, key)?
        .parse()
        .map_err(|_| format!("the save's `{key}` is not a number"))
}

fn open(path: &Path) -> Result<Connection, String> {
    if !path.is_file() {
        return Err(format!("there is no save at {}", path.display()));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{} is not a save: {e}", path.display()))?;
    let format = conn
        .query_row("SELECT value FROM meta WHERE key = 'format'", [], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| format!("{} is not a save of this engine", path.display()))?;
    if format != FORMAT.to_string() {
        return Err(format!(
            "{} is save format {format}, and this engine reads format {FORMAT}. Saves from \
             other builds are not converted while the game is a prototype",
            path.display()
        ));
    }
    Ok(conn)
}

/// What a save says about itself, without loading the world.
pub fn info(path: &Path) -> Result<SaveInfo, String> {
    let conn = open(path)?;
    let mut stmt = conn
        .prepare("SELECT id, version, min_engine_version, files FROM modules ORDER BY id")
        .map_err(sql)?;
    let modules = stmt
        .query_map([], |r| {
            Ok(SavedModule {
                id: r.get(0)?,
                version: r.get(1)?,
                min_engine_version: r.get(2)?,
                files: r.get(3)?,
            })
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    let build = meta(&conn, "build")?;
    Ok(SaveInfo {
        format: FORMAT,
        engine: meta(&conn, "engine")?,
        build: (!build.is_empty()).then_some(build),
        seed: number(&conn, "seed")?,
        tick: number(&conn, "tick")?,
        hash: meta(&conn, "hash")?,
        modules,
    })
}

fn parse<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("the save's {what} is corrupt: {e}"))
}

fn u(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// Reads a save's world, and checks it hashes as it did when it was written.
pub fn read(path: &Path) -> Result<(SaveInfo, World), String> {
    let info = info(path)?;
    let conn = open(path)?;
    let state = |key: &str| -> Result<String, String> {
        conn.query_row("SELECT value FROM state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .map_err(|_| format!("the save has no `{key}` state"))
    };
    let mut world = World::new(info.seed);
    world.tick = info.tick;
    world.next_event = number(&conn, "next_event")?;
    world.rng = parse(&state("rng")?, "rng")?;
    world.services = parse(&state("services")?, "services")?;

    let mut stmt = conn
        .prepare("SELECT entity, component, fields, cause, tick FROM components")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(sql)?;
    for row in rows {
        let (entity, component, fields, cause, tick) = row.map_err(sql)?;
        world.entities.entry(entity).or_default().insert(
            component,
            Component {
                fields: parse(&fields, "components")?,
                cause: u(cause),
                tick: u(tick),
            },
        );
    }
    let mut stmt = conn
        .prepare("SELECT seq, tick, kind, cause, payload FROM facts ORDER BY seq")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(sql)?;
    for row in rows {
        let (seq, tick, kind, cause, payload) = row.map_err(sql)?;
        world.facts.push(Fact {
            seq: u(seq),
            tick: u(tick),
            kind,
            cause: u(cause),
            payload: parse(&payload, "facts")?,
        });
    }
    let mut stmt = conn
        .prepare("SELECT id, tick, cause FROM causes ORDER BY id")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(sql)?;
    for row in rows {
        let (id, tick, cause) = row.map_err(sql)?;
        world.causes.push(CauseRecord {
            id: u(id),
            tick: u(tick),
            cause: parse(&cause, "causes")?,
        });
    }
    if world.hash() != info.hash {
        return Err(format!(
            "{} is damaged: its world doesn't hash as it did when it was saved",
            path.display()
        ));
    }
    Ok((info, world))
}
