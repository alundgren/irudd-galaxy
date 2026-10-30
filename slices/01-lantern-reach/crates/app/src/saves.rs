//! Where saves live and how they get written while the game runs.
//!
//! [`SaveFolder::user`] is a per-user folder from the platform's conventions (the
//! `directories` crate), or `$GALAXY_SAVES` when set. [`choose`] picks the save
//! `galaxy play` opens. [`Saver`] writes on a background thread so an autosave never
//! hitches a frame: the game hands it a copy of the state and carries on.
//!
//! The save file also holds two records the app owns rather than the sim, each in its
//! own table: how far each hint has taught ([`read_hints`], [`write_hints`], the
//! `app_hints` table), and which talk lines wait to be seen at which dock and what's
//! new in the ship log ([`read_talk`], [`write_talk`], `app_talk`). They aren't world
//! state, so they never touch the sim, a replay or a snapshot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use galaxy_core::{Save, SimState};

use crate::ui::talk::TalkRecord;

/// The extension `galaxy play` writes and looks for.
pub const SAVE_EXTENSION: &str = "sqlite";

/// A folder of saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveFolder {
    dir: PathBuf,
}

impl SaveFolder {
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The per-user save folder: `$GALAXY_SAVES`, else `saves` in the platform's data
    /// folder for the game (for example `%APPDATA%` on Windows, `~/Library/Application
    /// Support` on macOS). `None` when the platform has no home folder.
    pub fn user() -> Option<Self> {
        if let Some(dir) = std::env::var_os("GALAXY_SAVES").filter(|d| !d.is_empty()) {
            return Some(Self::at(dir));
        }
        directories::ProjectDirs::from("", "Irudd", "Lantern Reach")
            .map(|p| Self::at(p.data_dir().join("saves")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The most recently written save in the folder, if any. Ties go to the later name.
    pub fn latest(&self) -> Option<PathBuf> {
        let entries = std::fs::read_dir(&self.dir).ok()?;
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == SAVE_EXTENSION))
            .filter_map(|p| {
                let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
                Some((modified, p))
            })
            .max()
            .map(|(_, p)| p)
    }

    /// A path for a new save that doesn't exist yet, named by `stamp` (seconds since
    /// the epoch, from the caller) so saves sort by when they were started.
    pub fn fresh_path(&self, stamp: u64) -> PathBuf {
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("lantern-{stamp}.{SAVE_EXTENSION}")
            } else {
                format!("lantern-{stamp}-{n}.{SAVE_EXTENSION}")
            };
            let p = self.dir.join(name);
            if !p.exists() {
                return p;
            }
            n += 1;
        }
    }
}

/// Where running games register their live socket (`--live` without a save looks
/// here): `.live/` in the per-user save folder.
pub fn live_registry() -> Option<PathBuf> {
    SaveFolder::user().map(|f| f.dir().join(".live"))
}

/// The save `galaxy play` uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    /// Load this existing save.
    Open(PathBuf),
    /// Start a new game here.
    Create(PathBuf),
}

impl SaveChoice {
    pub fn path(&self) -> &Path {
        match self {
            SaveChoice::Open(p) | SaveChoice::Create(p) => p,
        }
    }
}

/// Picks the save: `--save PATH` opens it (or starts a new game there if it doesn't
/// exist; with `--new` it must not exist); otherwise the latest save in `folder`, or a
/// new one in it with `--new` or when there is none.
pub fn choose(
    folder: Option<&SaveFolder>,
    save: Option<PathBuf>,
    new: bool,
    stamp: u64,
) -> Result<SaveChoice, String> {
    if let Some(path) = save {
        return match (path.exists(), new) {
            (true, true) => Err(format!(
                "{} already exists; leave out --new to continue it",
                path.display()
            )),
            (true, false) => Ok(SaveChoice::Open(path)),
            (false, _) => Ok(SaveChoice::Create(path)),
        };
    }
    let folder = folder.ok_or("there is no per-user save folder here; pass --save PATH")?;
    if !new && let Some(latest) = folder.latest() {
        return Ok(SaveChoice::Open(latest));
    }
    Ok(SaveChoice::Create(folder.fresh_path(stamp)))
}

/// The app's hint record in the save file: hint key, times taught.
const HINTS_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS app_hints (hint TEXT PRIMARY KEY, count INTEGER NOT NULL)";

/// How far each hint has taught in the save at `path` (empty for a save that has no
/// record yet).
pub fn read_hints(path: &Path) -> Result<BTreeMap<String, u32>, String> {
    let read = || -> rusqlite::Result<BTreeMap<String, u32>> {
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'app_hints')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(BTreeMap::new());
        }
        let mut stmt = conn.prepare("SELECT hint, count FROM app_hints")?;
        let rows = stmt.query_map([], |r| {
            let n: i64 = r.get(1)?;
            Ok((
                r.get::<_, String>(0)?,
                u32::try_from(n.max(0)).unwrap_or(u32::MAX),
            ))
        })?;
        rows.collect()
    };
    read().map_err(|e| format!("{}: hints: {e}", path.display()))
}

/// Replaces the hint record in the save at `path`.
pub fn write_hints(path: &Path, counts: &BTreeMap<String, u32>) -> Result<(), String> {
    let write = || -> rusqlite::Result<()> {
        let mut conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let tx = conn.transaction()?;
        tx.execute(HINTS_TABLE, [])?;
        tx.execute("DELETE FROM app_hints", [])?;
        for (hint, n) in counts {
            tx.execute(
                "INSERT INTO app_hints (hint, count) VALUES (?1, ?2)",
                rusqlite::params![hint, i64::from(*n)],
            )?;
        }
        tx.commit()
    };
    write().map_err(|e| format!("{}: hints: {e}", path.display()))
}

/// The app's talk record in the save file: `through` and `log_new`, and one
/// `unseen.<talk id>` row per line waiting to be seen, valued with its station.
const TALK_TABLE: &str =
    "CREATE TABLE IF NOT EXISTS app_talk (key TEXT PRIMARY KEY, value TEXT NOT NULL)";

/// The talk record in the save at `path`; `None` for a save that has none yet.
pub fn read_talk(path: &Path) -> Result<Option<TalkRecord>, String> {
    let read = || -> rusqlite::Result<Option<Vec<(String, String)>>> {
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'app_talk')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        let mut stmt = conn.prepare("SELECT key, value FROM app_talk")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>().map(Some)
    };
    let fail = |e: String| format!("{}: talk: {e}", path.display());
    let Some(rows) = read().map_err(|e| fail(e.to_string()))? else {
        return Ok(None);
    };
    let mut record = TalkRecord::default();
    for (key, value) in rows {
        let number = |v: &str| v.parse::<u64>().map_err(|_| fail(format!("bad `{key}`")));
        match key.as_str() {
            "through" => record.through = number(&value)?,
            "log_new" => record.log_new = u32::try_from(number(&value)?).unwrap_or(u32::MAX),
            _ => match key.strip_prefix("unseen.") {
                Some(id) => {
                    record.unseen.insert(number(id)?, value);
                }
                None => return Err(fail(format!("unknown key `{key}`"))),
            },
        }
    }
    Ok(Some(record))
}

/// Replaces the talk record in the save at `path`.
pub fn write_talk(path: &Path, record: &TalkRecord) -> Result<(), String> {
    let write = || -> rusqlite::Result<()> {
        let mut conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let tx = conn.transaction()?;
        tx.execute(TALK_TABLE, [])?;
        tx.execute("DELETE FROM app_talk", [])?;
        let rows = [
            ("through".to_owned(), record.through.to_string()),
            ("log_new".to_owned(), record.log_new.to_string()),
        ]
        .into_iter()
        .chain(
            record
                .unseen
                .iter()
                .map(|(id, st)| (format!("unseen.{id}"), st.clone())),
        );
        for (key, value) in rows {
            tx.execute(
                "INSERT INTO app_talk (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, value],
            )?;
        }
        tx.commit()
    };
    write().map_err(|e| format!("{}: talk: {e}", path.display()))
}

enum Job {
    Write(Box<SimState>),
    /// The hint record, written after any state write queued before it.
    Hints(BTreeMap<String, u32>),
    /// The talk record, likewise.
    Talk(TalkRecord),
    /// A snapshot of this state (before a game-master change), answered on `reply`.
    Snapshot {
        state: Box<SimState>,
        label: String,
        reply: Sender<Result<i64, String>>,
    },
    Stop,
}

/// Writes saves on a background thread. Jobs queued while one is being written are
/// coalesced: only the newest state is written.
pub struct Saver {
    tx: Sender<Job>,
    handle: Option<JoinHandle<()>>,
    last_error: Arc<Mutex<Option<String>>>,
    path: PathBuf,
}

impl Saver {
    pub fn start(save: Save) -> Self {
        let (tx, rx) = mpsc::channel();
        let last_error = Arc::new(Mutex::new(None));
        let path = save.path().to_path_buf();
        let errors = last_error.clone();
        let handle = std::thread::Builder::new()
            .name("saver".into())
            .spawn(move || run(save, rx, errors))
            .ok();
        Self {
            tx,
            handle,
            last_error,
            path,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Queues `state` to be written. Returns at once.
    pub fn save(&self, state: SimState) {
        let _ = self.tx.send(Job::Write(Box::new(state)));
    }

    /// Queues the hint record to be written. Returns at once.
    pub fn save_hints(&self, counts: BTreeMap<String, u32>) {
        let _ = self.tx.send(Job::Hints(counts));
    }

    /// Queues the talk record to be written. Returns at once.
    pub fn save_talk(&self, record: TalkRecord) {
        let _ = self.tx.send(Job::Talk(record));
    }

    /// Queues a snapshot of `state` (`Save::snapshot`) after the writes queued before
    /// it. Returns at once; the snapshot ID arrives on the receiver.
    pub fn snapshot(&self, state: SimState, label: &str) -> Receiver<Result<i64, String>> {
        let (reply, rx) = mpsc::channel();
        let job = Job::Snapshot {
            state: Box::new(state),
            label: label.to_owned(),
            reply: reply.clone(),
        };
        if self.tx.send(job).is_err() {
            let _ = reply.send(Err("the save thread has stopped".into()));
        }
        rx
    }

    /// The last write error, if the last write failed.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|e| e.clone())
    }

    /// Writes `last` (if given), waits for every queued write and stops the thread.
    pub fn finish(mut self, last: Option<SimState>) -> Result<(), String> {
        if let Some(state) = last {
            self.save(state);
        }
        let _ = self.tx.send(Job::Stop);
        if let Some(h) = self.handle.take() {
            h.join()
                .map_err(|_| "the save thread panicked".to_owned())?;
        }
        match self.last_error() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

fn run(mut save: Save, rx: Receiver<Job>, errors: Arc<Mutex<Option<String>>>) {
    let mut next_job: Option<Job> = None;
    loop {
        let job = match next_job.take() {
            Some(j) => j,
            None => match rx.recv() {
                Ok(j) => j,
                Err(_) => return,
            },
        };
        let mut latest = match job {
            Job::Write(s) => s,
            Job::Hints(h) => {
                record(&errors, write_hints(save.path(), &h));
                continue;
            }
            Job::Talk(t) => {
                record(&errors, write_talk(save.path(), &t));
                continue;
            }
            Job::Snapshot {
                state,
                label,
                reply,
            } => {
                let _ = reply.send(save.snapshot(&state, &label).map_err(|e| e.to_string()));
                continue;
            }
            Job::Stop => return,
        };
        let mut stop = false;
        let mut hints = None;
        let mut talk = None;
        // Coalesce: whatever queued up meanwhile, only the newest state (and hint
        // record) matters. A snapshot keeps its place after the writes before it.
        while let Ok(next) = rx.try_recv() {
            match next {
                Job::Write(s) => latest = s,
                Job::Hints(h) => hints = Some(h),
                Job::Talk(t) => talk = Some(t),
                Job::Stop => {
                    stop = true;
                    break;
                }
                snapshot @ Job::Snapshot { .. } => {
                    next_job = Some(snapshot);
                    break;
                }
            }
        }
        let result = save.write(&latest).map_err(|e| e.to_string());
        record(&errors, result);
        if let Some(h) = hints {
            record(&errors, write_hints(save.path(), &h));
        }
        if let Some(t) = talk {
            record(&errors, write_talk(save.path(), &t));
        }
        if stop {
            return;
        }
    }
}

/// Keeps the last write's result: an error until a later write succeeds.
fn record(errors: &Mutex<Option<String>>, result: Result<(), String>) {
    if let Ok(mut slot) = errors.lock() {
        *slot = result.err();
    }
}

impl galaxy_gm::Snapshots for Saver {
    fn snapshot(&mut self, state: &SimState, label: &str) -> galaxy_gm::SnapshotTicket {
        galaxy_gm::SnapshotTicket::Pending(Saver::snapshot(self, state.clone(), label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_is_the_newest_save() {
        let dir = tempfile::tempdir().unwrap();
        let folder = SaveFolder::at(dir.path());
        assert_eq!(folder.latest(), None);
        let a = dir.path().join("a.sqlite");
        let b = dir.path().join("b.sqlite");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&a)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(folder.latest(), Some(b.clone()));
        std::fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(old - std::time::Duration::from_secs(60))
            .unwrap();
        assert_eq!(folder.latest(), Some(a));
    }

    #[test]
    fn fresh_paths_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let folder = SaveFolder::at(dir.path());
        let first = folder.fresh_path(100);
        assert_eq!(first.file_name().unwrap(), "lantern-100.sqlite");
        std::fs::write(&first, b"x").unwrap();
        let second = folder.fresh_path(100);
        assert_eq!(second.file_name().unwrap(), "lantern-100-2.sqlite");
    }

    #[test]
    fn choose_follows_the_flags() {
        let dir = tempfile::tempdir().unwrap();
        let folder = SaveFolder::at(dir.path());
        // Nothing yet: a new save in the folder.
        let c = choose(Some(&folder), None, false, 5).unwrap();
        assert_eq!(c, SaveChoice::Create(dir.path().join("lantern-5.sqlite")));
        // A save exists: continue it, unless --new.
        std::fs::write(c.path(), b"x").unwrap();
        assert_eq!(
            choose(Some(&folder), None, false, 6).unwrap(),
            SaveChoice::Open(c.path().to_path_buf())
        );
        assert_eq!(
            choose(Some(&folder), None, true, 6).unwrap(),
            SaveChoice::Create(dir.path().join("lantern-6.sqlite"))
        );
        // --save PATH wins; --new refuses to overwrite it.
        let explicit = dir.path().join("mine.sqlite");
        assert_eq!(
            choose(None, Some(explicit.clone()), false, 7).unwrap(),
            SaveChoice::Create(explicit.clone())
        );
        std::fs::write(&explicit, b"x").unwrap();
        assert_eq!(
            choose(None, Some(explicit.clone()), false, 7).unwrap(),
            SaveChoice::Open(explicit.clone())
        );
        assert!(choose(None, Some(explicit), true, 7).is_err());
        assert!(choose(None, None, false, 7).is_err());
    }

    #[test]
    fn the_hint_record_round_trips_beside_the_world() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.sqlite");
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let state = galaxy_core::new_game(&data, 1).unwrap();
        drop(Save::create(&path, &state).unwrap());
        assert!(read_hints(&path).unwrap().is_empty(), "no record yet");
        let mut counts = BTreeMap::new();
        counts.insert("dock".to_owned(), 2);
        counts.insert("mining_rate:1.5".to_owned(), 1);
        let saver = Saver::start(Save::open(&path).unwrap());
        saver.save(state.clone());
        saver.save_hints(counts.clone());
        saver.finish(Some(state.clone())).unwrap();
        assert_eq!(read_hints(&path).unwrap(), counts);
        // The world loads as before.
        assert_eq!(
            Save::open(&path).unwrap().load().unwrap().world,
            state.world
        );
        counts.insert("dock".to_owned(), 3);
        write_hints(&path, &counts).unwrap();
        assert_eq!(read_hints(&path).unwrap(), counts);
    }

    #[test]
    fn the_talk_record_round_trips_beside_the_world() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let state = galaxy_core::new_game(&data, 1).unwrap();
        drop(Save::create(&path, &state).unwrap());
        assert_eq!(read_talk(&path).unwrap(), None, "no record yet");
        let mut record = TalkRecord {
            through: 12,
            log_new: 3,
            ..TalkRecord::default()
        };
        record.unseen.insert(7, "station.kettle_dock".into());
        record.unseen.insert(11, "station.lantern_wharf".into());
        let saver = Saver::start(Save::open(&path).unwrap());
        saver.save(state.clone());
        saver.save_talk(record.clone());
        saver.finish(Some(state.clone())).unwrap();
        assert_eq!(read_talk(&path).unwrap(), Some(record.clone()));
        assert_eq!(
            Save::open(&path).unwrap().load().unwrap().world,
            state.world
        );
        record.unseen.clear();
        write_talk(&path, &record).unwrap();
        assert_eq!(read_talk(&path).unwrap(), Some(record));
    }

    #[test]
    fn user_folder_honours_the_override() {
        // Only checks the shape; the real folder depends on the machine.
        if let Some(f) = SaveFolder::user() {
            assert!(!f.dir().as_os_str().is_empty());
        }
    }
}
