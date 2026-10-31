//! Where cargo builds, and how many heavy builds run at once.
//!
//! Every build goes to one shared root outside the worktree: a per-user cache folder, or
//! the absolute folder `GALAXY_TARGET_ROOT` names. There is no opt-out: the root can move
//! but never turn off, and a `CARGO_TARGET_DIR` from the environment is ignored.
//!
//! Each run leases one folder and holds it until it exits, so one run uses a folder at a
//! time: a slot from a small pool (`slot-1` to `slot-N`, `GALAXY_TARGET_SLOTS`, default 4),
//! or the folder `GALAXY_TARGET_KEY` names. It prefers the slot this worktree used last,
//! then this branch's, then the most recently used free one, so a new branch starts with
//! warm dependencies. Heavy commands first take one of `GALAXY_BUILD_PERMITS` (default 1;
//! 0 turns it off) machine-wide permits. Every lock is an OS file lock: it is released when
//! the process exits, crash included, so there are no stale markers to clean up.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const DEFAULT_SLOTS: usize = 4;
/// Set on cargo's environment while this process holds a permit, so an xtask started under
/// it doesn't wait for its parent's permit.
const PERMIT_HELD: &str = "GALAXY_BUILD_PERMIT_HELD";
/// Cargo subcommands that compile, and so take a permit.
const HEAVY: &[&str] = &[
    "build", "b", "check", "c", "test", "t", "clippy", "bench", "doc", "d", "run", "r", "rustc",
    "rustdoc", "fix",
];

/// Variables `cargo run` sets for the program it runs, besides `CARGO_PKG_*`.
const RUN_VARS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
];

static HELD: AtomicBool = AtomicBool::new(false);
static ROOT: OnceLock<PathBuf> = OnceLock::new();
static LEASE: OnceLock<Lease> = OnceLock::new();

struct Lease {
    dir: PathBuf,
    _lock: File,
}

/// Holds a build permit until dropped.
pub struct Permit(Option<File>);

impl Drop for Permit {
    fn drop(&mut self) {
        if self.0.is_some() {
            HELD.store(false, Ordering::SeqCst);
        }
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn number(name: &str, default: usize) -> usize {
    var(name)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// The shared root: `GALAXY_TARGET_ROOT` if it is an absolute folder, else the per-user
/// cache folder (`%LOCALAPPDATA%`, `~/Library/Caches` or `~/.cache`).
fn shared_root() -> &'static Path {
    ROOT.get_or_init(|| {
        let default = directories::BaseDirs::new()
            .map(|d| d.cache_dir().join("irudd-galaxy").join("target"))
            .unwrap_or_else(|| std::env::temp_dir().join("irudd-galaxy-target"));
        match var("GALAXY_TARGET_ROOT").map(PathBuf::from) {
            Some(root) if root.is_absolute() => root,
            Some(root) => {
                eprintln!(
                    "note: GALAXY_TARGET_ROOT={} is not an absolute folder. It can move the \
                     shared build folder but not turn it off; using {}",
                    root.display(),
                    default.display()
                );
                default
            }
            None => default,
        }
    })
}

/// The shared folder cargo builds into for this run, leased on first use.
pub fn target_dir() -> &'static Path {
    &LEASE.get_or_init(lease).dir
}

/// Cargo, building into [`target_dir`], with the permit marker and job cap set.
pub fn cargo() -> std::process::Command {
    let mut cmd =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    // `cargo run` gave xtask this worktree's `CARGO_MANIFEST_DIR` and package variables.
    // Passed on, they reach crates whose fingerprints record them (ring does), so every
    // worktree would rebuild those crates and everything above them in a shared folder.
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with("CARGO_PKG_") || RUN_VARS.contains(&name.as_ref()) {
            cmd.env_remove(name.as_ref());
        }
    }
    cmd.env("CARGO_TARGET_DIR", target_dir());
    if HELD.load(Ordering::SeqCst) {
        cmd.env(PERMIT_HELD, "1");
    }
    let permits = number("GALAXY_BUILD_PERMITS", 1);
    if permits > 1 && var("CARGO_BUILD_JOBS").is_none() {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        cmd.env("CARGO_BUILD_JOBS", (cores / permits).max(1).to_string());
    }
    cmd
}

/// Whether `cargo <args>` compiles.
pub fn is_heavy(args: &[std::ffi::OsString]) -> bool {
    args.iter()
        .filter_map(|a| a.to_str())
        .find(|a| !a.starts_with('-') && !a.starts_with('+'))
        .is_some_and(|sub| HEAVY.contains(&sub))
}

/// Waits for a machine-wide build permit, saying so while it waits.
pub fn permit() -> Permit {
    let permits = number("GALAXY_BUILD_PERMITS", 1);
    if permits == 0 || var(PERMIT_HELD).is_some() {
        return Permit(None);
    }
    let dir = shared_root().join("locks");
    let names: Vec<String> = (1..=permits).map(|i| format!("build-{i}")).collect();
    let what = "a build permit (one heavy build at a time; GALAXY_BUILD_PERMITS raises it)";
    let (_, file) = acquire(&dir, &names, what);
    HELD.store(true, Ordering::SeqCst);
    Permit(Some(file))
}

fn lease() -> Lease {
    if var("CARGO_TARGET_DIR").is_some() {
        eprintln!(
            "note: ignoring CARGO_TARGET_DIR; cargo xtask always builds in the shared folder"
        );
    }
    let root = shared_root();
    let mut names: Vec<String> = match var("GALAXY_TARGET_KEY") {
        Some(key) => vec![folder_name(&key)],
        None => (1..=number("GALAXY_TARGET_SLOTS", DEFAULT_SLOTS).max(1))
            .map(|i| format!("slot-{i}"))
            .collect(),
    };
    let (worktree, branch) = (crate::workspace_root().display().to_string(), branch());
    names.sort_by_cached_key(|name| {
        let owner = std::fs::read_to_string(root.join(format!("{name}.owner"))).unwrap_or_default();
        let used = std::fs::metadata(root.join(format!("{name}.owner")))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        (
            !owner.lines().any(|l| l == format!("worktree {worktree}")),
            !branch
                .as_ref()
                .is_some_and(|b| owner.lines().any(|l| l == format!("branch {b}"))),
            !root.join(name).is_dir(),
            std::cmp::Reverse(used),
        )
    });
    let (name, file) = acquire(root, &names, "a free target folder");
    let dir = root.join(name);
    eprintln!("==> target folder {}", dir.display());
    Lease { dir, _lock: file }
}

/// Locks the first free `<name>.lock` in `dir`, in order, polling until one is free, and
/// records who holds it in `<name>.owner`.
fn acquire(dir: &Path, names: &[String], what: &str) -> (String, File) {
    let task = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("xtask: can't create {}: {e}", dir.display());
        std::process::exit(1);
    }
    let mut waiting = false;
    loop {
        for name in names {
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(dir.join(format!("{name}.lock")));
            let result = lock.and_then(|file| match file.try_lock() {
                Ok(()) => Ok(Some(file)),
                Err(TryLockError::WouldBlock) => Ok(None),
                Err(TryLockError::Error(e)) => Err(e),
            });
            match result {
                Ok(Some(file)) => {
                    let owner = format!(
                        "worktree {}\nbranch {}\ntask {task}\npid {}\n",
                        crate::workspace_root().display(),
                        branch().unwrap_or_default(),
                        std::process::id()
                    );
                    let _ = std::fs::write(dir.join(format!("{name}.owner")), owner);
                    if waiting {
                        eprintln!("==> got {name}");
                    }
                    return (name.clone(), file);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("xtask: can't lock {name} in {}: {e}", dir.display());
                    std::process::exit(1);
                }
            }
        }
        if !waiting {
            waiting = true;
            eprintln!("==> waiting for {what}. In use:");
            for name in names {
                let owner = std::fs::read_to_string(dir.join(format!("{name}.owner")));
                let line = |key: &str| {
                    owner.as_deref().unwrap_or_default().lines().find_map(|l| {
                        l.strip_prefix(key)
                            .map(|v| v.trim().to_string())
                            .filter(|v| !v.is_empty())
                    })
                };
                eprintln!(
                    "    {name}: `cargo xtask {}` on {}",
                    line("task").unwrap_or_default(),
                    line("branch").unwrap_or_else(|| "a detached HEAD".into())
                );
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The checked-out branch. On a detached HEAD (a read-only review of `origin/<branch>`),
/// the one remote branch at this commit, if there is exactly one.
pub fn branch() -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(crate::workspace_root())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if let Some(name) = git(&["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        return Some(name);
    }
    let remotes = git(&[
        "for-each-ref",
        "--points-at",
        "HEAD",
        "--format=%(refname:lstrip=3)",
        "refs/remotes",
    ])?;
    let names: std::collections::BTreeSet<&str> = remotes
        .lines()
        .filter(|n| !n.is_empty() && *n != "HEAD")
        .collect();
    let mut names = names.into_iter();
    match (names.next(), names.next()) {
        (Some(name), None) => Some(name.to_string()),
        _ => None,
    }
}

/// A key as one folder name: `slice/01/x` becomes `slice-01-x`.
fn folder_name(key: &str) -> String {
    let name: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_start_matches('.');
    if name.is_empty() {
        "detached".into()
    } else {
        name.into()
    }
}

#[cfg(test)]
mod tests {
    use super::{folder_name, is_heavy};

    #[test]
    fn keys_become_one_folder() {
        assert_eq!(
            folder_name("slice/01/playtest-fixes"),
            "slice-01-playtest-fixes"
        );
        assert_eq!(folder_name("slot-2"), "slot-2");
        assert_eq!(folder_name(r"a\b:c"), "a-b-c");
        assert_eq!(folder_name(".."), "detached");
        assert_eq!(folder_name("../up"), "-up");
    }

    #[test]
    fn only_compiling_subcommands_are_heavy() {
        let heavy = |args: &[&str]| is_heavy(&args.iter().map(Into::into).collect::<Vec<_>>());
        assert!(heavy(&["test", "-p", "galaxy-core"]));
        assert!(heavy(&["--locked", "clippy"]));
        assert!(!heavy(&["fmt", "--all"]));
        assert!(!heavy(&["tree"]));
        assert!(!heavy(&[]));
    }
}
