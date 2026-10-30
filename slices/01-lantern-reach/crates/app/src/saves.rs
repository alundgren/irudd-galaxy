//! Where saves live and how they get written while the game runs.
//!
//! [`SaveFolder::user`] is a per-user folder from the platform's conventions (the
//! `directories` crate), or `$GALAXY_SAVES` when set. [`choose`] picks the save
//! `galaxy play` opens. [`Saver`] writes on a background thread so an autosave never
//! hitches a frame: the game hands it a copy of the state and carries on.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use galaxy_core::{Save, SimState};

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

enum Job {
    Write(Box<SimState>),
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
    while let Ok(job) = rx.recv() {
        let mut latest = match job {
            Job::Write(s) => s,
            Job::Stop => return,
        };
        let mut stop = false;
        // Coalesce: whatever queued up meanwhile, only the newest state matters.
        while let Ok(next) = rx.try_recv() {
            match next {
                Job::Write(s) => latest = s,
                Job::Stop => {
                    stop = true;
                    break;
                }
            }
        }
        let result = save.write(&latest).map_err(|e| e.to_string());
        if let Ok(mut slot) = errors.lock() {
            *slot = result.err();
        }
        if stop {
            return;
        }
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
    fn user_folder_honours_the_override() {
        // Only checks the shape; the real folder depends on the machine.
        if let Some(f) = SaveFolder::user() {
            assert!(!f.dir().as_os_str().is_empty());
        }
    }
}
