//! `galaxy-gm`: the GM, which builds and checks modules. So far: `doctor`, `check` (the
//! module loads: manifest, start state and scripts; and none of its IDs is a word in engine
//! or GM source), `types` (the Luau type file for scripts) and `index-source` (the index of
//! engine source words that `cargo xtask install` ships). Exit codes and error kinds:
//! `engine/docs/exit-codes.md`.

use std::path::Path;
use std::process::ExitCode;

use galaxy_api::Registry;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::failure::code;
use galaxy_module::idscan::{self, INDEX_FILE, SCANNED_MODULE, SourceIndex};
use galaxy_module::module::Module;
use galaxy_module::{Failure, gate, install, manifest, version};
use galaxy_script::ScriptSet;
use galaxy_sim::Session;
use serde_json::{Value, json};

const USAGE: &str = "usage: galaxy-gm <command>

  doctor                the engine, its installed builds and launchers, and the modules
                        near the working folder
  check <module>        check a module folder: it loads (manifest, start state, scripts)
                        and none of its IDs is a word in engine or GM source
  types                 print the Luau type file for module scripts
  index-source <dir>    print the index of words in the engine source under <dir>/crates
                        (cargo xtask install ships it with each build)
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
        [command, dir] if command == "index-source" => {
            let crates = discover::resolve(&cli::current_dir()?, Path::new(dir)).join("crates");
            if !crates.is_dir() {
                return Err(Failure::bad_input(format!(
                    "{} is not a folder: give the engine folder",
                    crates.display()
                )));
            }
            let index = SourceIndex::build(&idscan::program_source(&crates));
            Ok(Output::Json(json!(index)))
        }
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

    let declared = module.declared_ids();
    let mut warnings = Vec::new();
    let index = install::current()
        .map(|(_, build)| build.dir.join(INDEX_FILE))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<SourceIndex>(&text).ok());
    let id_index = match &index {
        Some(index) => {
            // Only the base game's IDs fail the engine check, so only they are rejected;
            // another module hears about a clash, harmless to it, as a warning.
            let scanned = module.id() == SCANNED_MODULE;
            for id in index.clashes(module.id(), &declared) {
                let message = format!(
                    "the {} `{}` is named `{}`, a word engine or GM source uses; the engine \
                     knows no game nouns, so rename it (or, if the engine is wrong to use the \
                     word, raise it as an engine change)",
                    id.what, id.full, id.local
                );
                if scanned {
                    findings.push(finding("id_in_engine", message));
                } else {
                    warnings.push(finding("id_in_engine", message));
                }
            }
            json!({ "checked": declared.len(), "engine": index.engine })
        }
        None => json!({
            "checked": 0,
            "skipped": "this is not an installed build, so it has no index of engine source; \
                        the engine's own check scans for the core's IDs",
        }),
    };
    let ok = findings.is_empty();
    let count = findings.len();
    let out = json!({
        "ok": ok,
        "module": module.id(),
        "version": module.manifest.version.to_string(),
        "declared_ids": declared.len(),
        "id_index": id_index,
        "context_modules": context.iter().map(Module::id).collect::<Vec<_>>(),
        "notes": notes,
        "findings": findings,
        "warnings": warnings,
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
