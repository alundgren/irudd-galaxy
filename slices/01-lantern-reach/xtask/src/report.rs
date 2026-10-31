//! `cargo xtask build-report [--no-cold]`: times the builds an agent waits on and writes
//! them as `build-report.json` in the leased target folder (also printed on stdout).
//!
//! Steps, in order: a cold game build in an empty folder (skipped with `--no-cold`; the
//! folder is deleted afterwards), the game build in the leased folder (what a new branch
//! pays), a no-op rebuild, a rebuild after touching `galaxy-core`, the workspace test
//! build and link, the same after touching `galaxy-core` again, and the folder's size.
//! Each step counts the units of our crates cargo compiled and names the third-party ones.
//! Run it on an idle machine: it holds the build permit throughout.

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Instant, SystemTime};

use serde_json::{Value, json};

pub fn run(args: &[std::ffi::OsString]) -> ExitCode {
    let cold = !args.iter().any(|a| a == "--no-cold");
    let _permit = crate::build::permit();
    let Some(slot) = crate::build::target_dir() else {
        eprintln!(
            "build-report needs a shared target folder (CARGO_TARGET_DIR or GALAXY_TARGET_ROOT=off is set)"
        );
        return ExitCode::FAILURE;
    };
    let core = crate::workspace_root().join("crates/core/src/lib.rs");
    let mut steps = serde_json::Map::new();
    let mut step = |name: &str, mut cmd: Command| -> bool {
        eprintln!("==> build-report: {name}");
        match timed(&mut cmd) {
            Some(result) => {
                steps.insert(name.into(), result);
                true
            }
            None => {
                eprintln!("build-report: step {name} failed");
                false
            }
        }
    };
    let test_build = || {
        let mut cmd = crate::build::cargo();
        cmd.args(["test", "--workspace", "--no-run"]);
        cmd
    };
    let ok = (!cold || {
        let dir = slot.with_file_name("build-report-cold");
        let _ = std::fs::remove_dir_all(&dir);
        let mut cmd = crate::game_build(false);
        cmd.env("CARGO_TARGET_DIR", &dir);
        let ok = step("cold_build", cmd);
        let _ = std::fs::remove_dir_all(&dir);
        ok
    }) && step("first_build", crate::game_build(false))
        && step("noop_build", crate::game_build(false))
        && touch(&core)
        && step("touch_core_build", crate::game_build(false))
        && step("test_build", test_build())
        && touch(&core)
        && step("touch_core_test_build", test_build());
    if !ok {
        return ExitCode::FAILURE;
    }
    let report = json!({
        "commit": git_head(),
        "os": std::env::consts::OS,
        "cores": std::thread::available_parallelism().map_or(1, |n| n.get()),
        "target_folder": slot.file_name().map(|n| n.to_string_lossy()),
        "rustc_wrapper_env": std::env::var("RUSTC_WRAPPER").ok(),
        "unix_time": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "steps": steps,
        "target_size_gb": (size(slot) as f64 / 1e9 * 100.0).round() / 100.0,
    });
    let text = serde_json::to_string_pretty(&report).expect("report serialises");
    if let Err(e) = std::fs::write(slot.join("build-report.json"), &text) {
        eprintln!("build-report: can't write it into the target folder: {e}");
    }
    println!("{text}");
    ExitCode::SUCCESS
}

/// Runs a cargo build and returns its time and what it compiled, or `None` if it failed.
fn timed(cmd: &mut Command) -> Option<Value> {
    let start = Instant::now();
    let output = cmd
        .arg("--message-format=json-render-diagnostics")
        .stderr(Stdio::inherit())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let seconds = (start.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    let (mut ours, mut third_party) = (0, Vec::new());
    for message in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["fresh"] == false)
    {
        // Workspace packages are `path+file://...`; registry and git ones are not.
        if message["package_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("path+"))
        {
            ours += 1;
        } else if let Some(name) = message["target"]["name"].as_str() {
            third_party.push(name.to_string());
        }
    }
    third_party.sort();
    third_party.dedup();
    Some(json!({
        "seconds": seconds,
        "compiled_ours": ours,
        "compiled_third_party": third_party.len(),
        "third_party": third_party,
    }))
}

/// Marks a source file changed without changing it.
fn touch(path: &Path) -> bool {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()))
        .map_err(|e| eprintln!("build-report: can't touch {}: {e}", path.display()))
        .is_ok()
}

fn size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => size(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                _ => 0,
            })
            .sum()
    })
}

fn git_head() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(crate::workspace_root())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
