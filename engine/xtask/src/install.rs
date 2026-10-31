//! `cargo xtask install [--dev]`: build the engine once and install it as a versioned
//! build, side by side with earlier ones.
//!
//! 1. Refuses if `engine/` has uncommitted changes, unless `--dev`. A `--dev` build is
//!    named `<version>-<commit>-dev` and runs only when named (`galaxy --build <name>`).
//! 2. Builds `galaxy`, `galaxy-gm` and the launcher with the release profile (thin LTO) in
//!    the leased slot, holding the build permit.
//! 3. Assembles the build folder next to the others, then moves it into place:
//!    `galaxy`, `galaxy-gm`, `tools/luau/luau-analyze`, `galaxy.d.luau` (the Luau types for
//!    module scripts) and `build.toml`. The built `galaxy-gm` writes the type file
//!    (`types`), so xtask needs no engine crate beyond the install layout.
//! 4. Copies the launcher into the launcher folder as `galaxy` and `galaxy-gm`, unless the
//!    launchers there come from a newer engine version, or this is a `--dev` build and
//!    launchers are already installed.
//! 5. Says where the launcher folder is and how to put it on PATH. It never edits PATH.
//!
//! The layout is `galaxy_module::install`'s; `engine/docs/modules-and-install.md`
//! explains it.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use galaxy_module::Version;
use galaxy_module::install::{self, BuildInfo, LauncherInfo};

use crate::{build, build_binaries, fetch_luau, git};

pub fn run(args: &[OsString]) -> ExitCode {
    let dev = match args {
        [] => false,
        [flag] if flag == "--dev" => true,
        _ => {
            eprintln!("usage: cargo xtask install [--dev]");
            return ExitCode::FAILURE;
        }
    };
    match install(dev) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask install: {e}");
            ExitCode::FAILURE
        }
    }
}

fn install(dev: bool) -> Result<(), String> {
    let dirty = git(&["status", "--porcelain", "--untracked-files=all", "--", "."])?;
    if !dirty.is_empty() && !dev {
        return Err(format!(
            "engine/ has uncommitted changes:\n{dirty}\nCommit them first, or install a \
             branch build with `cargo xtask install --dev`, which runs only when named \
             (`galaxy --build <name> ...`)."
        ));
    }
    let commit = git(&["rev-parse", "--short=12", "HEAD"])?;
    let version = galaxy_module::version::engine();
    let name = install::build_name(version, &commit, dev);
    let root = install::root();
    eprintln!("==> installing engine build {name} into {}", root.display());

    let permit = build::permit();
    let executables = build_binaries(true)?;
    drop(permit);
    let luau = fetch_luau()?;
    let gm = &executables["galaxy-gm"];
    let types = gm_output(gm, &["types"])?;

    let builds = install::builds_dir(&root);
    fs::create_dir_all(&builds).map_err(|e| format!("can't create {}: {e}", builds.display()))?;
    remove_leftovers(&builds);
    let pid = std::process::id();
    let staging = builds.join(format!(".staging-{name}-{pid}"));
    assemble(&staging, &executables, &luau, &types, version, &commit, dev)
        .map_err(|e| format!("can't assemble {}: {e}", staging.display()))?;
    let dir = builds.join(&name);
    if dir.exists() {
        let old = builds.join(format!(".old-{name}-{pid}"));
        fs::rename(&dir, &old).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!(
                "can't replace {}: {e}. Is a program from it still running? Close it and \
                 install again.",
                dir.display()
            )
        })?;
        let _ = fs::remove_dir_all(&old);
    }
    fs::rename(&staging, &dir).map_err(|e| format!("can't move the build into place: {e}"))?;
    eprintln!("==> installed {}", dir.display());

    for tool in ["galaxy", "galaxy-gm"] {
        let out = Command::new(dir.join(install::exe(tool)))
            .arg("--version")
            .output()
            .map_err(|e| format!("the installed {tool} does not start: {e}"))?;
        eprintln!("    {}", String::from_utf8_lossy(&out.stdout).trim());
    }

    let launchers = update_launchers(&root, &executables["galaxy-launcher"], version, &name, dev)
        .map_err(|e| format!("can't install the launchers: {e}"))?;
    eprintln!("==> {launchers}");
    print_path_help(&root);
    if dev {
        eprintln!("A --dev build runs only when named: galaxy-gm --build {name} doctor");
    } else {
        eprintln!("Try it: galaxy-gm doctor");
    }
    Ok(())
}

/// What the built `galaxy-gm` prints for `args`.
fn gm_output(gm: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(gm)
        .args(args)
        .output()
        .map_err(|e| format!("can't run the built galaxy-gm: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "galaxy-gm {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Writes a complete build folder at `staging`.
fn assemble(
    staging: &Path,
    executables: &std::collections::BTreeMap<String, PathBuf>,
    luau: &Path,
    types: &str,
    version: Version,
    commit: &str,
    dev: bool,
) -> std::io::Result<()> {
    let _ = fs::remove_dir_all(staging);
    let analyzer = install::luau_analyze(staging);
    fs::create_dir_all(analyzer.parent().expect("the analyzer sits in tools/luau"))?;
    for tool in ["galaxy", "galaxy-gm"] {
        fs::copy(&executables[tool], staging.join(install::exe(tool)))?;
    }
    fs::copy(luau, &analyzer)?;
    fs::write(staging.join(install::LUAU_TYPES), types)?;
    let info = BuildInfo {
        version: version.to_string(),
        commit: commit.to_owned(),
        dev,
        installed_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    fs::write(
        staging.join(install::BUILD_FILE),
        toml::to_string(&info).expect("build info serializes"),
    )
}

/// Copies the launcher in as `galaxy` and `galaxy-gm`, unless the installed launchers
/// should stay. Returns what it did.
fn update_launchers(
    root: &Path,
    launcher: &Path,
    version: Version,
    name: &str,
    dev: bool,
) -> std::io::Result<String> {
    let bin = install::launcher_dir(root);
    let complete = ["galaxy", "galaxy-gm"]
        .iter()
        .all(|tool| bin.join(install::exe(tool)).is_file());
    if let Some(current) = install::read_launcher(root).filter(|_| complete) {
        let newer = current
            .version
            .parse::<Version>()
            .is_ok_and(|installed| installed > version);
        if newer {
            return Ok(format!(
                "kept the launchers from build {}: they are newer than this build",
                current.build
            ));
        }
        if dev {
            return Ok(format!(
                "kept the launchers from build {}: a --dev install never replaces them",
                current.build
            ));
        }
    }
    fs::create_dir_all(&bin)?;
    remove_leftovers(&bin);
    for tool in ["galaxy", "galaxy-gm"] {
        replace_file(launcher, &bin.join(install::exe(tool)))?;
    }
    let info = LauncherInfo {
        version: version.to_string(),
        build: name.to_owned(),
        dev,
    };
    fs::write(
        bin.join(install::LAUNCHER_FILE),
        toml::to_string(&info).expect("launcher info serializes"),
    )?;
    Ok(format!(
        "installed the launchers from build {name} in {}",
        bin.display()
    ))
}

/// Puts a copy of `source` at `target`. A running executable can't be overwritten on
/// Windows, but it can be renamed, so the old one is moved aside first and removed when
/// nothing runs it any more (here, or on a later install).
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    let file_name = target
        .file_name()
        .expect("a file name")
        .to_string_lossy()
        .into_owned();
    let pid = std::process::id();
    let fresh = target.with_file_name(format!(".{file_name}.new-{pid}"));
    fs::copy(source, &fresh)?;
    if target.exists() {
        let old = target.with_file_name(format!(".{file_name}.old-{pid}"));
        fs::rename(target, &old)?;
        let _ = fs::remove_file(&old);
    }
    fs::rename(&fresh, target)
}

/// Removes what earlier installs left behind: replaced builds and launchers that were
/// running then. Anything still in use stays until next time.
fn remove_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".old-") {
            let _ = fs::remove_dir_all(entry.path());
        } else if name.starts_with('.') && name.contains(".old-") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn print_path_help(root: &Path) {
    let bin = install::launcher_dir(root);
    if install::launcher_on_path(root) {
        eprintln!("The launcher folder is on PATH: {}", bin.display());
        return;
    }
    eprintln!(
        "Put the launcher folder on PATH (this command does not change PATH):\n  {}",
        bin.display()
    );
    if cfg!(windows) {
        eprintln!(
            "For example, in PowerShell, then open a new terminal:\n  \
             [Environment]::SetEnvironmentVariable('Path', \
             [Environment]::GetEnvironmentVariable('Path', 'User') + ';{}', 'User')",
            bin.display()
        );
    } else {
        eprintln!(
            "For example, add this line to your shell profile (~/.bashrc, ~/.zshrc), then \
             open a new terminal:\n  export PATH=\"{}:$PATH\"",
            bin.display()
        );
    }
}
