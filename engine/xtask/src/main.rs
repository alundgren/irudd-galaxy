//! `cargo xtask <task>` for the engine workspace: local checks, builds and installs. There
//! is no CI; run these locally.
//!
//! - `check`: fmt, clippy with warnings denied, and the tests.
//! - `cargo <args>`: run cargo in the target folder `check` uses.
//! - `target-dir`: print that folder.
//! - `build [--release]`: build `galaxy`, `galaxy-gm` and the launcher.
//! - `run [--release] [--gm] -- <args>`: build the same way, then run `galaxy` (or
//!   `galaxy-gm`).
//! - `install [--dev]`: build with the release profile and install a versioned build
//!   (see [`install`]).
//! - `linux-check [--distro <name>]`: run `check` in WSL while holding this machine's build
//!   permit (see [`linux`]).
//! - `fetch-luau`: download the pinned `luau-analyze` into `tools/luau/` (git-ignored).
//! - `loc [--base <ref>]`: code lines per crate (see [`loc`]).
//!
//! Every command that runs cargo leases a shared target folder, and compiling commands
//! wait for a machine-wide build permit first; [`build`] explains both. The slots and the
//! permit are the same ones the slice's xtask uses.

mod build;
mod install;
mod linux;
mod loc;

use std::ffi::OsString;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use build::cargo;
use sha2::{Digest, Sha256};

/// The pinned Luau release. Keep it in step with the Luau VM the engine embeds, once it
/// does.
const LUAU_VERSION: &str = "0.740";

/// Release asset and its SHA-256 for this OS (from the GitHub release's digests).
fn luau_asset() -> Option<(&'static str, &'static str)> {
    if cfg!(windows) {
        Some((
            "luau-windows.zip",
            "be676d9a1092b3d5ee36be5f5f11e2af1976911c32f5644a052548dc7c66bb9c",
        ))
    } else if cfg!(target_os = "macos") {
        Some((
            "luau-macos.zip",
            "8414763743116cbc2533ee7c932d41ba66bd25ea7c2353bc7eed2934b60b6715",
        ))
    } else if cfg!(target_os = "linux") {
        Some((
            "luau-ubuntu.zip",
            "7d4035db01816f977f6e4c42e3d5e8ef4235d95e9afe63a3c38905b42dde60f2",
        ))
    } else {
        None
    }
}

/// The binaries a build installs, by package (each package's binary has its name).
const BINARIES: [&str; 3] = ["galaxy", "galaxy-gm", "galaxy-launcher"];

const USAGE: &str = "usage: cargo xtask <check|cargo <args>|target-dir|build [--release]|\
                     run [--release] [--gm] -- <args>|install [--dev]|\
                     linux-check [--distro <name>]|fetch-luau|loc [--base <ref>]>";

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let rest: Vec<OsString> = std::env::args_os().skip(2).collect();
    match task.as_str() {
        "build" | "run" => binary_command(&task, rest),
        "check" => {
            let _permit = build::permit();
            check()
        }
        "cargo" => {
            if let Err(message) = build::validate_args(&rest) {
                eprintln!("xtask: {message}");
                return ExitCode::FAILURE;
            }
            let _permit = build::is_heavy(&rest).then(build::permit);
            exit(cargo().args(rest).status().is_ok_and(|s| s.success()))
        }
        "target-dir" => {
            println!("{}", build::target_dir().display());
            ExitCode::SUCCESS
        }
        "install" => install::run(&rest),
        "linux-check" => linux::run(&rest),
        "loc" => loc::run(&rest),
        "fetch-luau" => match fetch_luau() {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask fetch-luau: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// fmt, clippy and the tests. The caller holds the permit.
fn check() -> ExitCode {
    run_all(&[
        &["fmt", "--all", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace"],
    ])
}

/// `build` and `run`. Cargo reports the executable path, including custom target folders;
/// do not guess it.
fn binary_command(task: &str, args: Vec<OsString>) -> ExitCode {
    let mut release = false;
    let mut gm = false;
    let mut rest = args.into_iter();
    let mut passed = Vec::new();
    for arg in rest.by_ref() {
        match arg.to_str() {
            Some("--release") => release = true,
            Some("--gm") if task == "run" => gm = true,
            Some("--") => break,
            _ => {
                passed.push(arg);
                break;
            }
        }
    }
    passed.extend(rest);
    if task == "build" && !passed.is_empty() {
        eprintln!("usage: cargo xtask build [--release]");
        return ExitCode::FAILURE;
    }
    let permit = build::permit();
    let executables = match build_binaries(release) {
        Ok(executables) => executables,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if task == "build" {
        return ExitCode::SUCCESS;
    }
    // The binary runs without the permit but keeps the folder leased: Windows locks a
    // running executable, so nobody may rebuild it meanwhile.
    drop(permit);
    let name = if gm { "galaxy-gm" } else { "galaxy" };
    let executable = &executables[name];
    match Command::new(executable).args(passed).status() {
        Ok(status) => ExitCode::from(u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)),
        Err(error) => {
            eprintln!("could not start {name}: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Builds [`BINARIES`] and returns each one's executable, by name. The caller holds the
/// permit.
fn build_binaries(release: bool) -> Result<std::collections::BTreeMap<String, PathBuf>, String> {
    let mut build = cargo();
    build.arg("build");
    for package in BINARIES {
        build.args(["-p", package]);
    }
    if release {
        build.arg("--release");
    }
    let output = build
        .arg("--message-format=json-render-diagnostics")
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !output.status.success() {
        return Err("the build failed".into());
    }
    let executables: std::collections::BTreeMap<String, PathBuf> =
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|message| message["reason"] == "compiler-artifact")
            .filter_map(|message| {
                let name = message["target"]["name"].as_str()?.to_owned();
                let path = PathBuf::from(message["executable"].as_str()?);
                BINARIES.contains(&name.as_str()).then_some((name, path))
            })
            .collect();
    for name in BINARIES {
        if !executables.contains_key(name) {
            return Err(format!("cargo did not report the {name} executable"));
        }
    }
    Ok(executables)
}

fn exit(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Downloads the pinned Luau release for this OS, checks its SHA-256 and unpacks
/// `luau-analyze` into `tools/luau/`. Does nothing if that version is already there.
/// Returns the analyzer's path.
fn fetch_luau() -> Result<PathBuf, String> {
    let (asset, sha) = luau_asset().ok_or("no Luau release for this OS")?;
    let dir = workspace_root().join("tools").join("luau");
    let exe = galaxy_module::install::exe("luau-analyze");
    let target = dir.join(&exe);
    let marker = dir.join("VERSION");
    let want = format!("{LUAU_VERSION} {sha}\n");
    if target.is_file() && std::fs::read_to_string(&marker).ok().as_deref() == Some(&want) {
        eprintln!("luau-analyze {LUAU_VERSION} is already in tools/luau/");
        return Ok(target);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let url = format!("https://github.com/luau-lang/luau/releases/download/{LUAU_VERSION}/{asset}");
    let zip_path = dir.join(asset);
    eprintln!("==> downloading {url}");
    let ok = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "2",
            "--output",
        ])
        .arg(&zip_path)
        .arg(&url)
        .status()
        .map_err(|e| format!("can't run curl: {e}"))?
        .success();
    if !ok {
        return Err(format!("download of {url} failed"));
    }
    let bytes = std::fs::read(&zip_path).map_err(|e| e.to_string())?;
    let got: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != sha {
        let _ = std::fs::remove_file(&zip_path);
        return Err(format!(
            "{asset}: SHA-256 is {got}, expected {sha}; refusing to use it"
        ));
    }
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("{asset}: {e}"))?;
    let mut entry = archive
        .by_name(&exe)
        .map_err(|e| format!("{asset} has no {exe}: {e}"))?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
    std::fs::write(&target, data).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&zip_path);
    std::fs::write(&marker, want).map_err(|e| e.to_string())?;
    eprintln!("installed luau-analyze {LUAU_VERSION} in tools/luau/");
    Ok(target)
}

fn run_all(steps: &[&[&str]]) -> ExitCode {
    for args in steps {
        eprintln!("==> cargo {}", args.join(" "));
        let ok = cargo().args(*args).status().is_ok_and(|s| s.success());
        if !ok {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// The engine workspace (`engine/`).
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

/// Runs git in the engine workspace and returns its trimmed output.
pub(crate) fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace_root())
        .output()
        .map_err(|e| format!("can't run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
