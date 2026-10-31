//! `galaxy-gm`: the GM, which builds and checks modules. So far: `doctor`, `check` (the
//! module loads: manifest, start state and scripts) and `types` (the Luau type file for
//! scripts). Exit codes and error kinds: `engine/docs/exit-codes.md`.

use std::path::Path;
use std::process::ExitCode;

use galaxy_api::Registry;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, gate, manifest, version};
use galaxy_script::ScriptSet;
use galaxy_sim::Session;
use serde_json::{Value, json};

const USAGE: &str = "usage: galaxy-gm <command>

  doctor                the engine, its installed builds and launchers, and the modules
                        near the working folder
  check <module>        check a module folder: it loads (manifest, start state, scripts)
  types                 print the Luau type file for module scripts
  --version             print the version
  --help                print this";

fn main() -> ExitCode {
    cli::main(Tool::Gm, USAGE, run)
}

fn run(args: &[String]) -> Result<Output, Failure> {
    match args {
        [command, path] if command == "check" => {
            let cwd = cli::current_dir()?;
            let dir = manifest::module_dir(&discover::resolve(&cwd, Path::new(path)));
            gate::open(std::slice::from_ref(&dir))?;
            check(&dir)
        }
        [command, ..] if command == "check" => Err(cli::usage_failure(
            "check takes one module folder: galaxy-gm check <module>",
            USAGE,
        )),
        [command] if command == "types" => Ok(Output::Text(
            galaxy_script::api::luau_types(&version::engine().to_string())
                .trim_end()
                .to_owned(),
        )),
        [command, ..] => Err(cli::usage_failure(
            format!("unknown command `{command}`"),
            USAGE,
        )),
        [] => unreachable!("cli::main handles an empty command line"),
    }
}

/// One thing `check` found wrong.
fn finding(kind: &str, message: impl Into<String>) -> Value {
    json!({ "kind": kind, "message": message.into() })
}

/// Checks the module at `dir`, with the modules beside it loaded too (a module may write
/// or handle another's types and events).
fn check(dir: &Path) -> Result<Output, Failure> {
    let module = Module::read(dir)?;
    let mut context = Vec::new();
    let mut notes = Vec::new();
    if let Some(parent) = dir.parent() {
        for other in discover::modules_in(parent) {
            if other == dir {
                continue;
            }
            match Module::read(&other) {
                Ok(m) if m.id() != module.id() => context.push(m),
                Ok(_) => {}
                Err(f) => notes.push(format!("skipped {}: {}", other.display(), f.message)),
            }
        }
    }
    let mut findings = Vec::new();
    let mut all: Vec<Module> = context.clone();
    all.push(module.clone());
    match Registry::new(&all.iter().collect::<Vec<_>>()) {
        Err(e) => findings.push(finding("schema", e)),
        Ok(registry) => match ScriptSet::load(&module, &registry) {
            Err(problems) => {
                findings.extend(problems.iter().map(|p| finding("script", p.to_string())))
            }
            Ok(_) => {
                if let Err(e) = Session::new(all, 1) {
                    findings.push(finding("start", e));
                }
            }
        },
    }

    let ok = findings.is_empty();
    let count = findings.len();
    let out = json!({
        "ok": ok,
        "module": module.id(),
        "version": module.manifest.version.to_string(),
        "context_modules": context.iter().map(Module::id).collect::<Vec<_>>(),
        "notes": notes,
        "findings": findings,
    });
    if ok {
        return Ok(Output::Json(out));
    }
    let mut failure = Failure::new(
        code::REJECTED,
        "findings",
        format!("{} has {count} finding(s)", module.id()),
    );
    if let Value::Object(fields) = out {
        failure.fields = fields;
    }
    Err(failure)
}
