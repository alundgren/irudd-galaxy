//! The parts of the command line `galaxy` and `galaxy-gm` share: argument handling,
//! `--help`, `--version`, `doctor`, and the output format (JSON on stdout).

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::discover::{self, Tool};
use crate::failure::{Failure, code};
use crate::manifest::Manifest;
use crate::{gate, install, version};

/// What a successful command prints.
pub enum Output {
    /// Pretty JSON on stdout.
    Json(Value),
    /// Plain text on stdout (`--help`, `--version`).
    Text(String),
}

/// Runs a binary's command line: handles `--help`, `--version` and `doctor`, passes the
/// rest to `command`, prints the result and returns the exit code.
pub fn main(
    tool: Tool,
    usage: &str,
    command: impl FnOnce(&[String]) -> Result<Output, Failure>,
) -> ExitCode {
    let args: Result<Vec<String>, OsString> = std::env::args_os()
        .skip(1)
        .map(OsString::into_string)
        .collect();
    let result = match args {
        Err(arg) => Err(usage_failure(
            format!("argument {} is not valid text", arg.to_string_lossy()),
            usage,
        )),
        Ok(args) => match args.first().map(String::as_str) {
            None => Err(usage_failure("no command given", usage)),
            Some("--help" | "-h" | "help") => Ok(Output::Text(usage.to_owned())),
            Some("--version" | "-V") => Ok(Output::Text(version_line(tool))),
            Some("doctor") if args.len() == 1 => current_dir().and_then(|cwd| doctor(tool, &cwd)),
            Some("doctor") => Err(usage_failure("doctor takes no arguments", usage)),
            Some(_) => command(&args),
        },
    };
    match result {
        Ok(Output::Json(value)) => {
            print_json(&value);
            ExitCode::from(code::OK)
        }
        Ok(Output::Text(text)) => {
            println!("{text}");
            ExitCode::from(code::OK)
        }
        Err(failure) => failure.report(),
    }
}

/// Exit 2, kind `usage`, with the usage text.
pub fn usage_failure(message: impl Into<String>, usage: &str) -> Failure {
    Failure::usage(message).with("usage", usage)
}

pub fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON values serialize")
    );
}

pub fn current_dir() -> Result<std::path::PathBuf, Failure> {
    std::env::current_dir()
        .map_err(|e| Failure::runtime(format!("can't read the working folder: {e}")))
}

/// `galaxy 0.1.0`, plus the build folder's name when this is an installed build.
pub fn version_line(tool: Tool) -> String {
    let engine = version::engine();
    match install::current() {
        Some((_, build)) => format!("{} {engine} (build {})", tool.name(), build.name),
        None => format!("{} {engine} (not an installed build)", tool.name()),
    }
}

/// `doctor`: this engine, the installed builds and launchers, and the modules near the
/// working folder, which must pass the version gate.
pub fn doctor(tool: Tool, cwd: &Path) -> Result<Output, Failure> {
    let manifests = gate::open(&discover::modules_near(cwd))?;
    let (root, build) = match install::current() {
        Some((root, build)) => (root, Some(build)),
        None => (install::root(), None),
    };
    let builds: Vec<Value> = install::list(&root)
        .iter()
        .map(|b| json!({ "name": b.name, "version": b.version.to_string(), "dev": b.info.dev }))
        .collect();
    let launcher = install::read_launcher(&root);
    let luau = build
        .as_ref()
        .map(|b| install::luau_analyze(&b.dir))
        .filter(|p| p.is_file());
    Ok(Output::Json(json!({
        "ok": true,
        "tool": tool.name(),
        "engine": version::engine().to_string(),
        "build": build.as_ref().map(|b| b.name.clone()),
        "install_root": root.display().to_string(),
        "installed_builds": builds,
        "launchers": {
            "folder": install::launcher_dir(&root).display().to_string(),
            "from_build": launcher.map(|l| l.build),
            "on_path": install::launcher_on_path(&root),
        },
        "luau_analyze": luau.map(|p| p.display().to_string()),
        "modules": modules_json(&manifests),
    })))
}

pub fn modules_json(manifests: &[Manifest]) -> Vec<Value> {
    manifests
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "version": m.version.to_string(),
                "min_engine_version": m.min_engine_version.to_string(),
                "dir": m.dir.display().to_string(),
            })
        })
        .collect()
}

/// Exit 1, kind `not_built`: the modules passed the gate but the command is a stub.
pub fn not_built(tool: Tool, command: &str, manifests: &[Manifest]) -> Failure {
    let engine = version::engine();
    Failure::new(
        code::RUNTIME,
        "not_built",
        format!(
            "`{} {command}` is not built yet in engine {engine}. The modules passed the engine \
             version gate.",
            tool.name()
        ),
    )
    .with("command", command)
    .with("engine", engine.to_string())
    .with("modules", modules_json(manifests))
}
