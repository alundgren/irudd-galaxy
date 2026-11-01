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
    println!("{}", json_text(value));
}

/// Columns a JSON line may take before its value is spread over several lines.
const JSON_WIDTH: usize = 100;

/// `value` as JSON laid out for reading in few lines: indented like pretty JSON, but any
/// array or object that fits on its line (a box `[x, y, w, h]`, a moment
/// `{"frame": 3, "t": 0.05, "tick": 1}`) stays on it.
pub fn json_text(value: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, value, 0, 0);
    out
}

/// One line: `{"a": 1, "b": [1, 2]}`, spaced after colons and commas.
fn json_line(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(json_line).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", Value::String(k.clone()), json_line(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        scalar => scalar.to_string(),
    }
}

/// Writes `value` at `indent` spaces, where `used` columns of its line are already taken
/// (by its key).
fn write_json(out: &mut String, value: &Value, indent: usize, used: usize) {
    let line = json_line(value);
    let empty = match value {
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        _ => true,
    };
    if empty || indent + used + line.len() <= JSON_WIDTH {
        out.push_str(&line);
        return;
    }
    let pad = " ".repeat(indent + 2);
    match value {
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, v) in items.iter().enumerate() {
                out.push_str(&pad);
                write_json(out, v, indent + 2, 0);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        Value::Object(map) => {
            out.push_str("{\n");
            for (i, (k, v)) in map.iter().enumerate() {
                let key = format!("{}: ", Value::String(k.clone()));
                out.push_str(&pad);
                out.push_str(&key);
                write_json(out, v, indent + 2, key.len());
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
        _ => unreachable!("scalars fit"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_text_keeps_what_fits_on_a_line_and_reads_back() {
        let v = json!({
            "box": [1, 2, 3, 4],
            "at": { "frame": 3, "t": 0.05, "tick": 1 },
            "rows": (0..30).map(|i| format!("row {i} with some words in it")).collect::<Vec<_>>(),
            "empty": [],
        });
        let text = json_text(&v);
        assert!(text.contains("\"box\": [1, 2, 3, 4]"), "{text}");
        assert!(
            text.contains("\"at\": {\"frame\": 3, \"t\": 0.05, \"tick\": 1}"),
            "{text}"
        );
        assert!(
            text.contains("\n    \"row 0 with some words in it\",\n"),
            "{text}"
        );
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, v);
    }
}
