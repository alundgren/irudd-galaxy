//! Which modules a command is about.
//!
//! - `galaxy-gm check <path>`: the module at `<path>` (a folder or its `manifest.toml`).
//! - `galaxy play --modules <dir>`, `galaxy sim run --modules <dir>` and
//!   `galaxy sim load <save> --modules <dir>`: `<dir>` if it is a module, else every module
//!   folder in it.
//! - `doctor`: the modules near the working folder ([`modules_near`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::manifest::{self, FILE};

/// `path` against the working folder, with this OS's separators throughout.
pub fn resolve(cwd: &Path, path: &Path) -> PathBuf {
    cwd.join(path).components().collect()
}

/// `dir` itself if it holds a manifest, else its subfolders that do, sorted by name.
pub fn modules_in(dir: &Path) -> Vec<PathBuf> {
    if dir.join(FILE).is_file() {
        return vec![dir.to_path_buf()];
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join(FILE).is_file())
        .collect();
    found.sort();
    found
}

/// The modules `doctor` reports on: walking up from `start`, the first folder that is a
/// module, holds modules, or holds `game/modules`. Empty if there is none.
pub fn modules_near(start: &Path) -> Vec<PathBuf> {
    for dir in start.ancestors() {
        let found = modules_in(dir);
        if !found.is_empty() {
            return found;
        }
        let found = modules_in(&dir.join("game").join("modules"));
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// Which tool a command line is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Game,
    Gm,
}

impl Tool {
    /// The executable's name without its extension.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Game => "galaxy",
            Tool::Gm => "galaxy-gm",
        }
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        match name {
            "galaxy" => Some(Tool::Game),
            "galaxy-gm" => Some(Tool::Gm),
            _ => None,
        }
    }
}

/// The module folders a command line names, best effort: the launcher uses this to pick
/// a build, and the build itself reports anything wrong with the arguments.
pub fn modules_for(tool: Tool, args: &[OsString], cwd: &Path) -> Vec<PathBuf> {
    let at = |path: &OsString| resolve(cwd, Path::new(path));
    let command = args.first().and_then(|a| a.to_str());
    match (tool, command) {
        (_, Some("doctor")) => modules_near(cwd),
        (Tool::Gm, Some("check")) => args
            .get(1)
            .map(|path| vec![manifest::module_dir(&at(path))])
            .unwrap_or_default(),
        (Tool::Game, Some("play")) => modules_dir_arg(&args[1..])
            .map(|dir| modules_in(&at(&dir)))
            .unwrap_or_default(),
        (Tool::Game, Some("sim")) => modules_dir_arg(args.get(2..).unwrap_or_default())
            .map(|dir| modules_in(&at(&dir)))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The build a recording names, for `galaxy sim replay <recording>`: the launcher runs
/// exactly that build. `None` for other commands, or a recording made by a build that was
/// not installed.
pub fn recording_build(tool: Tool, args: &[OsString], cwd: &Path) -> Option<String> {
    let command: Vec<&str> = args.iter().take(2).filter_map(|a| a.to_str()).collect();
    if tool != Tool::Game || command != ["sim", "replay"] {
        return None;
    }
    let path = resolve(cwd, Path::new(args.get(2)?));
    let text = std::fs::read_to_string(path).ok()?;
    let recording: serde_json::Value = serde_json::from_str(&text).ok()?;
    recording["build"]["name"].as_str().map(str::to_owned)
}

/// The value of `--modules <dir>` or `--modules=<dir>`.
pub fn modules_dir_arg(args: &[OsString]) -> Option<OsString> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--modules" {
            return iter.next().cloned();
        }
        if let Some(value) = arg.to_str().and_then(|a| a.strip_prefix("--modules=")) {
            return Some(value.into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            "id = \"m\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n",
        )
        .unwrap();
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(Into::into).collect()
    }

    #[test]
    fn finds_modules_from_arguments_and_the_working_folder() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let modules = root.join("game").join("modules");
        module(&modules.join("b"));
        module(&modules.join("a"));
        std::fs::create_dir_all(root.join("engine").join("docs")).unwrap();

        let both = vec![modules.join("a"), modules.join("b")];
        assert_eq!(modules_in(&modules), both);
        assert_eq!(modules_near(&root.join("engine").join("docs")), both);
        assert_eq!(modules_near(&modules.join("a")), vec![modules.join("a")]);

        assert_eq!(
            modules_for(Tool::Gm, &args(&["check", "game/modules/a"]), root),
            vec![root.join("game/modules/a")]
        );
        assert_eq!(
            modules_for(
                Tool::Game,
                &args(&["play", "--modules", "game/modules"]),
                root
            ),
            both
        );
        assert_eq!(
            modules_for(
                Tool::Game,
                &args(&["play", "--modules=game/modules/b"]),
                root
            ),
            vec![modules.join("b")]
        );
        assert_eq!(modules_for(Tool::Gm, &args(&["doctor"]), root), both);
        assert!(modules_for(Tool::Gm, &args(&["play", "--modules", "x"]), root).is_empty());
        assert!(modules_for(Tool::Game, &args(&["--version"]), root).is_empty());
        assert_eq!(
            modules_for(
                Tool::Game,
                &args(&["sim", "run", "--modules", "game/modules"]),
                root
            ),
            both
        );
        assert_eq!(
            modules_for(
                Tool::Game,
                &args(&["sim", "load", "s.db", "--modules=game/modules/a"]),
                root
            ),
            vec![modules.join("a")]
        );

        std::fs::write(
            root.join("r.json"),
            r#"{"format": 1, "build": {"name": "0.2.0-abc", "version": "0.2.0"}}"#,
        )
        .unwrap();
        assert_eq!(
            recording_build(Tool::Game, &args(&["sim", "replay", "r.json"]), root).as_deref(),
            Some("0.2.0-abc")
        );
        assert_eq!(
            recording_build(Tool::Gm, &args(&["sim", "replay", "r.json"]), root),
            None
        );
    }
}
