//! `cargo xtask <task>`: local checks and code generation. There is no CI; run these locally.
//!
//! - `check`: fmt, clippy with warnings denied, and the headless tests.
//! - `contract`: regenerate the JSON Schemas in `crates/contract/schemas/`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "check" => run_all(&[
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
        ]),
        "contract" => contract(),
        _ => {
            eprintln!("usage: cargo xtask <check|contract>");
            ExitCode::FAILURE
        }
    }
}

fn run_all(steps: &[&[&str]]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for args in steps {
        eprintln!("==> cargo {}", args.join(" "));
        let ok = Command::new(&cargo)
            .args(*args)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .to_path_buf()
}

fn contract() -> ExitCode {
    let root = workspace_root();
    let result = write_schemas(&root.join("crates/contract/schemas"))
        .and_then(|()| write_api(&root.join("crates/contract/api")));
    if let Err(e) = result {
        eprintln!("xtask contract: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `galaxy.d.luau` and `HOOKS.md`, generated from `contract::api`.
fn write_api(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let files = galaxy_contract::api::render();
    for (name, text) in &files {
        std::fs::write(dir.join(name), text)?;
    }
    eprintln!("wrote {} files to crates/contract/api/", files.len());
    Ok(())
}

fn write_schemas(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // Remove schemas that are no longer generated.
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "json") {
            std::fs::remove_file(&path)?;
        }
    }
    let files = galaxy_contract::schema::render();
    for (name, text) in &files {
        std::fs::write(dir.join(name), text)?;
    }
    eprintln!("wrote {} schemas to crates/contract/schemas/", files.len());
    Ok(())
}
