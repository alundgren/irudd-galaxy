//! The parts of the command line `galaxy` and `galaxy-gm` share: argument handling,
//! `--help`, `--version`, `doctor`, and the output format (JSON on stdout).

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use crate::discover::{self, Tool};
use crate::failure::{Failure, code};
use crate::manifest::Manifest;
use crate::{gate, help, install, version};

/// What a successful command prints.
pub enum Output {
    /// Pretty JSON on stdout.
    Json(Value),
    /// Plain text on stdout (`--help`, `--version`).
    Text(String),
}

/// Runs a binary's command line: handles `--help` (anywhere), `help <command>`,
/// `--version` and `doctor`, resolves the command through the help table and passes it
/// and the arguments after its name to `command`; prints the result and returns the exit
/// code.
pub fn main(
    tool: Tool,
    command: impl FnOnce(&'static help::Command, &[String]) -> Result<Output, Failure>,
) -> ExitCode {
    let args: Result<Vec<String>, OsString> = std::env::args_os()
        .skip(1)
        .map(OsString::into_string)
        .collect();
    let result = match args {
        Err(arg) => Err(usage_failure(
            format!("argument {} is not valid text", arg.to_string_lossy()),
            &format!("{} --help", tool.name()),
        )),
        Ok(args) => dispatch(tool, &args, command),
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

fn dispatch(
    tool: Tool,
    args: &[String],
    command: impl FnOnce(&'static help::Command, &[String]) -> Result<Output, Failure>,
) -> Result<Output, Failure> {
    if help::asked(args) {
        return Ok(Output::Text(help::for_args(tool, args)));
    }
    let name = tool.name();
    match args.first().map(String::as_str) {
        None => {
            return Err(usage_failure(
                format!("no command given; `{name} --help` lists them"),
                &help::overview(tool),
            ));
        }
        Some("--version" | "-V") => return Ok(Output::Text(version_line(tool))),
        Some(_) => {}
    }
    match help::resolve(tool, args) {
        Some((c, n)) if c.path == "doctor" => {
            if n < args.len() {
                return Err(command_usage(c, "doctor takes no arguments"));
            }
            current_dir().and_then(|cwd| doctor(tool, &cwd))
        }
        Some((c, n)) => command(c, &args[n..]),
        None => {
            let first = &args[0];
            match help::group(tool, first) {
                Some(list) => {
                    let message = match args.get(1).filter(|a| !a.starts_with("--")) {
                        Some(second) => format!(
                            "no command `{first} {second}`; `{name} {first} --help` lists them"
                        ),
                        None => format!(
                            "`{first}` needs a command after it; `{name} {first} --help` lists them"
                        ),
                    };
                    Err(usage_failure(message, &list))
                }
                None => Err(usage_failure(
                    format!("unknown command `{first}`; `{name} --help` lists them"),
                    &help::overview(tool),
                )),
            }
        }
    }
}

/// Exit 2, kind `usage`, with the usage text.
pub fn usage_failure(message: impl Into<String>, usage: &str) -> Failure {
    Failure::usage(message).with("usage", usage)
}

/// Exit 2, kind `usage`, for a mistake in `command`'s arguments: its synopsis and where
/// its help is.
pub fn command_usage(command: &help::Command, message: impl Into<String>) -> Failure {
    Failure::usage(message).with("usage", command.usage()).with(
        "help",
        format!("{} {} --help", command.tool.name(), command.path),
    )
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
