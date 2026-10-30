//! `cargo xtask <task>`: local checks and code generation. There is no CI; run these locally.
//!
//! - `check`: `fetch-luau` (if needed), fmt, clippy with warnings denied, and the
//!   headless tests.
//! - `contract`: regenerate the JSON Schemas in `crates/contract/schemas/` and the hook
//!   API (`galaxy.d.luau`, `HOOKS.md`) in `crates/contract/api/`.
//! - `fetch-luau`: download the pinned `luau-analyze` into `tools/luau/` (git-ignored).

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use sha2::{Digest, Sha256};

/// The pinned Luau release. Keep it in step with the VM mlua builds
/// (`luau0-src` 0.22.0+luau740) and `galaxy_script::analyzer::LUAU_VERSION`.
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

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "check" => {
            // Without the analyzer the typecheck tests skip; that's a warning, not a failure.
            if let Err(e) = fetch_luau() {
                eprintln!("warning: fetch-luau failed ({e}); typecheck tests will skip");
            }
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
        "contract" => contract(),
        "fetch-luau" => match fetch_luau() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("xtask fetch-luau: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask <check|contract|fetch-luau>");
            ExitCode::FAILURE
        }
    }
}

/// Downloads the pinned Luau release for this OS, checks its SHA-256 and unpacks
/// `luau-analyze` into `tools/luau/`. Does nothing if that version is already there.
fn fetch_luau() -> Result<(), String> {
    let (asset, sha) = luau_asset().ok_or("no Luau release for this OS")?;
    let dir = workspace_root().join("tools").join("luau");
    let exe = if cfg!(windows) {
        "luau-analyze.exe"
    } else {
        "luau-analyze"
    };
    let marker = dir.join("VERSION");
    let want = format!(
        "{LUAU_VERSION} {sha}
"
    );
    if dir.join(exe).is_file() && std::fs::read_to_string(&marker).ok().as_deref() == Some(&want) {
        eprintln!("luau-analyze {LUAU_VERSION} is already in tools/luau/");
        return Ok(());
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
        .by_name(exe)
        .map_err(|e| format!("{asset} has no {exe}: {e}"))?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
    let target = dir.join(exe);
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
    Ok(())
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
