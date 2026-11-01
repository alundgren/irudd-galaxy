//! `galaxy-gm`: the GM, which builds and checks modules. So far: `doctor`, `check` (the
//! module loads: manifest, start state, scripts, models and styles), `audit` (a played
//! session's notes drawn again), `film` and `motion` (how motion looks, measured) and
//! `types` (the Luau type file for scripts). Exit codes and error kinds: `engine/docs/exit-codes.md`.

mod audit;
mod film;

use std::path::Path;
use std::process::ExitCode;

use galaxy_api::Registry;
use galaxy_api::engine::MODEL;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, gate, manifest, version};
use galaxy_script::ScriptSet;
use galaxy_sim::Session;
use galaxy_view::Assets;
use serde_json::{Value, json};

const USAGE: &str = "usage: galaxy-gm <command>

  doctor                the engine, its installed builds and launchers, and the modules
                        near the working folder
  check <module>        check a module folder: it loads (manifest, start state, scripts,
                        models and style), and what its start state wears exists
  audit <recording>     replay a played session to each of its notes: check the world
                        is identical and draw the moment again beside the recording
                        (audit/0001.png), to compare with the frame that was seen
  film <recording>      film a played session again, frame by frame on a fixed clock as
                        the window showed it, with probes of where things are on screen:
                        --note <n> [--before 3] [--after 1] (seconds round a note), or
                        --from <tick> --to <tick>; --fps 60, --size <w>x<h>, --out <folder>,
                        --probes-only (no GPU). Scripted sessions: `galaxy film`.
  motion <tool> <film>  answer one question about how motion looks, with numbers,
                        timestamps and one picture (engine/docs/motion-tools.md):
                          track [--thing x] [--to y]   follow, judder, overshoot, settling
                          dots                         dotted lines stay put (probes only)
                          strip [--along course|x,y;x,y] [--screen x,y;x,y]
                                                       crawl, jitter, popping on a path
                          sheet [--count n] [--at t,t] [--crop x] [--onion x]
                                                       frames side by side; an onion skin
                          flicker [--screen]           pops, blips and flicker
                          flow [--along course]        motion without a named thing
                          compare <other film> [--vdp] where two films differ (FLIP,
                                                       ColorVideoVDP)
  motion check <spec>   film a spec and hold the tools to its expectations
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
        [command, path] if command == "audit" => {
            let cwd = cli::current_dir()?;
            audit::run(&discover::resolve(&cwd, Path::new(path)))
        }
        [command, ..] if command == "audit" => Err(cli::usage_failure(
            "audit takes one recording: galaxy-gm audit <recording.json>",
            USAGE,
        )),
        [command, rest @ ..] if command == "motion" => film::motion(rest, USAGE),
        [command, path, rest @ ..] if command == "film" => {
            let cwd = cli::current_dir()?;
            let opts =
                galaxy_motion::Opts::parse(rest).map_err(|e| cli::usage_failure(e, USAGE))?;
            film::film(&discover::resolve(&cwd, Path::new(path)), &opts)
        }
        [command] if command == "film" => Err(cli::usage_failure(
            "film takes a recording: galaxy-gm film <recording.json> --note <n>",
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

/// What `module` brings for the eye: its models and style read, and every model and look
/// its start state wears is one some module has.
fn looks(module: &Module, all: &[Module]) -> Vec<Value> {
    let assets = Assets::load(all);
    let own = format!("{}: ", module.id());
    let mut out: Vec<Value> = assets
        .problems
        .iter()
        .filter(|p| p.starts_with(&own))
        .map(|p| finding("look", p.clone()))
        .collect();
    out.extend(
        assets
            .conflicts
            .iter()
            .filter(|c| c.modules.iter().any(|m| m == module.id()))
            .map(|c| finding("look", c.message())),
    );
    for (entity, components) in &module.start {
        let Some(fields) = components.get(MODEL) else {
            continue;
        };
        let text = |k: &str| fields.get(k).and_then(Value::as_str).unwrap_or("");
        let model = text("model");
        if !assets.models.contains_key(model) {
            out.push(finding(
                "look",
                format!(
                    "start.toml: {entity} wears the model `{model}`, which no module has \
                     (`<module>.<name>` is models/<name>.gltf in that module)"
                ),
            ));
        }
        let look = text("look");
        if !look.is_empty() && !assets.looks.contains_key(look) {
            out.push(finding(
                "look",
                format!(
                    "start.toml: {entity} wears the look `{look}`, which no module's \
                     style.toml has"
                ),
            ));
        }
    }
    out
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
                if let Err(e) = Session::new(all.clone(), 1) {
                    findings.push(finding("start", e));
                }
            }
        },
    }
    findings.extend(looks(&module, &all));

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
