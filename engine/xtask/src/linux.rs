//! `cargo xtask linux-check [--distro <name>]`: the engine check on Linux, from Windows.
//!
//! It takes this machine's build permit first, so the Linux build never runs beside
//! another heavy build, then runs `cargo xtask check` inside WSL on this same checkout.
//! In WSL, xtask builds in WSL's own shared folder (its per-user cache folder, so the
//! slots there stay warm between runs) and builds itself in `~/.cache/irudd-galaxy/xtask`,
//! never in this checkout's `target/`. The permit is given back when WSL finishes.
//!
//! The distro is `--distro <name>`, else `GALAXY_WSL_DISTRO`, else WSL's default. It needs
//! rustup and a C compiler (see "Linux" under "Platforms" in the root AGENTS.md).

use std::ffi::OsString;
use std::process::{Command, ExitCode};

use crate::{build, workspace_root};

/// What runs inside WSL, from the engine folder.
const INSIDE: &str = "cargo run --quiet --package xtask \
                      --target-dir \"$HOME/.cache/irudd-galaxy/xtask\" -- check";

pub fn run(args: &[OsString]) -> ExitCode {
    let distro = match args {
        [] => std::env::var("GALAXY_WSL_DISTRO")
            .ok()
            .filter(|d| !d.trim().is_empty()),
        [flag, name] if flag == "--distro" => Some(name.to_string_lossy().into_owned()),
        _ => {
            eprintln!("usage: cargo xtask linux-check [--distro <name>]");
            return ExitCode::FAILURE;
        }
    };
    if !cfg!(windows) {
        eprintln!(
            "xtask linux-check runs the check in WSL from Windows. On Linux, run \
             `cargo xtask check`; on macOS, see \"Platforms\" in the root AGENTS.md for Docker."
        );
        return ExitCode::FAILURE;
    }
    let _permit = build::permit();
    let mut wsl = Command::new("wsl.exe");
    if let Some(distro) = &distro {
        wsl.args(["--distribution", distro]);
    }
    wsl.arg("--cd")
        .arg(workspace_root())
        .args(["--exec", "bash", "-lc", INSIDE]);
    eprintln!(
        "==> cargo xtask check in WSL ({})",
        distro.as_deref().unwrap_or("the default distro")
    );
    match wsl.status() {
        Ok(status) if status.success() => {
            eprintln!("==> linux-check passed");
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!("==> linux-check failed ({status})");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("xtask linux-check: can't run wsl.exe: {e}");
            ExitCode::FAILURE
        }
    }
}
