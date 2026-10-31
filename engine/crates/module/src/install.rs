//! Where installed engines live, and which one a launcher runs.
//!
//! ```text
//! <root>/                      per-user; GALAXY_INSTALL_ROOT moves it
//!   bin/                       put this on PATH
//!     galaxy, galaxy-gm        launchers (copied executables)
//!     launcher.toml            the build the launchers came from
//!   builds/
//!     0.1.0-<commit>/          one folder per build, kept side by side
//!       galaxy, galaxy-gm
//!       tools/luau/luau-analyze
//!       galaxy.d.luau          Luau types for module scripts
//!       build.toml             version, commit, dev, install time
//!     0.1.0-<commit>-dev/      a --dev build: only run when named
//! ```
//!
//! `cargo xtask install` writes this layout; `engine/docs/modules-and-install.md`
//! explains it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::failure::{Failure, code};
use crate::version::Version;

/// Moves the install root to this absolute folder.
pub const ROOT_VAR: &str = "GALAXY_INSTALL_ROOT";
/// A build's description, in its folder.
pub const BUILD_FILE: &str = "build.toml";
/// The launchers' description, in the launcher folder.
pub const LAUNCHER_FILE: &str = "launcher.toml";
/// The Luau type file in each build.
pub const LUAU_TYPES: &str = "galaxy.d.luau";

/// The default install root: `irudd-galaxy/engine` in the per-user data folder
/// (`%LOCALAPPDATA%`, `~/Library/Application Support` or `~/.local/share`).
pub fn default_root() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("irudd-galaxy")
        .join("engine")
}

/// The install root: `GALAXY_INSTALL_ROOT` if it is an absolute folder, else the default.
pub fn root() -> PathBuf {
    std::env::var_os(ROOT_VAR)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(default_root)
}

pub fn builds_dir(root: &Path) -> PathBuf {
    root.join("builds")
}

pub fn launcher_dir(root: &Path) -> PathBuf {
    root.join("bin")
}

/// An executable's file name on this OS (`galaxy` or `galaxy.exe`).
pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// `luau-analyze`'s path inside a build folder.
pub fn luau_analyze(build_dir: &Path) -> PathBuf {
    build_dir
        .join("tools")
        .join("luau")
        .join(exe("luau-analyze"))
}

/// A build folder's name: version plus commit, and `-dev` for a `--dev` build.
pub fn build_name(version: Version, commit: &str, dev: bool) -> String {
    let dev = if dev { "-dev" } else { "" };
    format!("{version}-{commit}{dev}")
}

/// `build.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub version: String,
    pub commit: String,
    pub dev: bool,
    /// Seconds since 1970 when it was installed; breaks ties between builds of one version.
    pub installed_unix: u64,
}

/// `launcher.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherInfo {
    pub version: String,
    pub build: String,
    pub dev: bool,
}

/// An installed build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub name: String,
    pub dir: PathBuf,
    pub version: Version,
    pub info: BuildInfo,
}

impl Build {
    /// Reads `<dir>/build.toml`.
    pub fn read(dir: &Path) -> Option<Build> {
        let text = std::fs::read_to_string(dir.join(BUILD_FILE)).ok()?;
        let info: BuildInfo = toml::from_str(&text).ok()?;
        Some(Build {
            name: dir.file_name()?.to_str()?.to_owned(),
            dir: dir.to_path_buf(),
            version: info.version.parse().ok()?,
            info,
        })
    }

    /// Ordering for "newest": version, then install time, then name.
    fn age_key(&self) -> (Version, u64, &str) {
        (self.version, self.info.installed_unix, &self.name)
    }
}

/// Every installed build under `root`, newest first. Folders without a readable
/// `build.toml`, and half-written ones (names starting with `.`), are skipped.
pub fn list(root: &Path) -> Vec<Build> {
    let Ok(entries) = std::fs::read_dir(builds_dir(root)) else {
        return Vec::new();
    };
    let mut builds: Vec<Build> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .filter_map(|p| Build::read(&p))
        .collect();
    builds.sort_by(|a, b| b.age_key().cmp(&a.age_key()));
    builds
}

/// The build this executable belongs to, and the install root above it.
pub fn current() -> Option<(PathBuf, Build)> {
    let exe = std::env::current_exe().ok()?;
    let build = Build::read(exe.parent()?)?;
    let root = build.dir.parent()?.parent()?.to_path_buf();
    Some((root, build))
}

/// Which build to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    /// Exactly this build folder (a `--build` choice; later, the build a recording names).
    Named(String),
    /// The newest build that is not `--dev` and is at least this version. If none is new
    /// enough, the newest one: its version gate then refuses with the right message.
    Newest(Option<Version>),
}

/// Picks a build from `builds` (newest first, as [`list`] returns them).
pub fn select<'a>(builds: &'a [Build], want: &Want, root: &Path) -> Result<&'a Build, Failure> {
    let names = || builds.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    match want {
        Want::Named(name) => builds.iter().find(|b| &b.name == name).ok_or_else(|| {
            not_installed(format!("build {name} is not installed"), root).with("builds", names())
        }),
        Want::Newest(minimum) => {
            let mut releases = builds.iter().filter(|b| !b.info.dev);
            let first = releases.clone().next().ok_or_else(|| {
                let dev = if builds.is_empty() {
                    String::new()
                } else {
                    " (only --dev builds, which run only when named with --build <name>)".into()
                };
                not_installed(format!("no engine is installed{dev}"), root).with("builds", names())
            })?;
            Ok(releases
                .find(|b| minimum.is_none_or(|m| b.version >= m))
                .unwrap_or(first))
        }
    }
}

fn not_installed(message: String, root: &Path) -> Failure {
    Failure::new(
        code::CANNOT_START,
        "not_installed",
        format!(
            "{message} in {}. {}",
            root.display(),
            crate::gate::HOW_TO_INSTALL
        ),
    )
    .with("install_root", root.display().to_string())
    .with("install", crate::gate::HOW_TO_INSTALL)
}

/// Reads `<launcher dir>/launcher.toml`.
pub fn read_launcher(root: &Path) -> Option<LauncherInfo> {
    let text = std::fs::read_to_string(launcher_dir(root).join(LAUNCHER_FILE)).ok()?;
    toml::from_str(&text).ok()
}

/// Whether the launcher folder is on `PATH`.
pub fn launcher_on_path(root: &Path) -> bool {
    let want = launcher_dir(root);
    let want = want.canonicalize().unwrap_or(want);
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|p| p.canonicalize().unwrap_or(p) == want)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(root: &Path, version: &str, commit: &str, dev: bool, at: u64) {
        let name = build_name(version.parse().unwrap(), commit, dev);
        let dir = builds_dir(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let info = BuildInfo {
            version: version.into(),
            commit: commit.into(),
            dev,
            installed_unix: at,
        };
        std::fs::write(dir.join(BUILD_FILE), toml::to_string(&info).unwrap()).unwrap();
    }

    fn v(text: &str) -> Option<Version> {
        Some(text.parse().unwrap())
    }

    #[test]
    fn picks_the_newest_build_that_satisfies_the_minimum() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        install(root, "0.1.0", "aaaaaaaaaaaa", false, 10);
        install(root, "0.2.0", "bbbbbbbbbbbb", false, 5);
        install(root, "0.2.0", "cccccccccccc", false, 20);
        install(root, "0.3.0", "dddddddddddd", true, 30);
        std::fs::create_dir_all(builds_dir(root).join(".staging-x")).unwrap();
        let builds = list(root);
        assert_eq!(builds.len(), 4);
        assert_eq!(builds[0].name, "0.3.0-dddddddddddd-dev");

        let pick = |want: Want| select(&builds, &want, root).map(|b| b.name.clone());
        assert_eq!(pick(Want::Newest(None)).unwrap(), "0.2.0-cccccccccccc");
        assert_eq!(pick(Want::Newest(v("0.1"))).unwrap(), "0.2.0-cccccccccccc");
        // None new enough: the newest release, whose own gate refuses.
        assert_eq!(
            pick(Want::Newest(v("0.3.0"))).unwrap(),
            "0.2.0-cccccccccccc"
        );
        assert_eq!(
            pick(Want::Named("0.3.0-dddddddddddd-dev".into())).unwrap(),
            "0.3.0-dddddddddddd-dev"
        );
        assert_eq!(
            pick(Want::Named("0.1.0-aaaaaaaaaaaa".into())).unwrap(),
            "0.1.0-aaaaaaaaaaaa"
        );
        let missing = pick(Want::Named("9.9.9-x".into())).unwrap_err();
        assert_eq!((missing.code, missing.kind), (2, "not_installed"));
    }

    #[test]
    fn dev_builds_alone_are_not_installed_engines() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let none = select(&list(root), &Want::Newest(None), root).unwrap_err();
        assert_eq!((none.code, none.kind), (2, "not_installed"));
        install(root, "0.1.0", "aaaaaaaaaaaa", true, 1);
        let dev_only = select(&list(root), &Want::Newest(None), root).unwrap_err();
        assert!(dev_only.message.contains("--build"));
    }
}
