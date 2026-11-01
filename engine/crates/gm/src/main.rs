//! `galaxy-gm`: the GM, which builds and checks modules. So far: `doctor`, `check` (the
//! module loads: manifest, start state, scripts, models and styles), `audit` (a played
//! session's notes drawn again), `film` and `motion` (how motion looks, measured) and
//! `types` (the Luau type file for scripts), `fields` (the keys files take), `model` (a
//! model's parts, outline and morph) and `guide`.
//! Each command's help is in `galaxy_module::help`, which dispatch goes through. Exit
//! codes and error kinds: `engine/docs/exit-codes.md`.

mod audit;
mod fields;
mod film;
mod model;
mod sessions;

use std::path::Path;
use std::process::ExitCode;

use galaxy_api::Registry;
use galaxy_api::engine::MODEL;
use galaxy_module::cli::{self, Output};
use galaxy_module::discover::{self, Tool};
use galaxy_module::failure::code;
use galaxy_module::help;
use galaxy_module::module::Module;
use galaxy_module::{Failure, gate, manifest, version};
use galaxy_script::ScriptSet;
use galaxy_sim::Session;
use galaxy_view::Assets;
use serde_json::{Value, json};

fn main() -> ExitCode {
    cli::main(Tool::Gm, run)
}

fn run(command: &'static help::Command, args: &[String]) -> Result<Output, Failure> {
    let usage = |message: String| cli::command_usage(command, message);
    let cwd = cli::current_dir()?;
    let one = |what: &str| -> Result<&String, Failure> {
        match args {
            [one] if !one.starts_with("--") => Ok(one),
            _ => Err(usage(format!(
                "{} takes {what}: {}",
                command.path,
                command.usage()
            ))),
        }
    };
    match command.path {
        "guide" => match args {
            [] => Ok(Output::Text(help::guide())),
            [topic] => help::guide_topic(topic).map(Output::Text).ok_or_else(|| {
                let names: Vec<&str> = help::TOPICS.iter().map(|t| t.name).collect();
                usage(format!(
                    "no guide topic `{topic}`; the topics are {}",
                    names.join(", ")
                ))
            }),
            _ => Err(usage("guide takes at most one topic".into())),
        },
        "check" => {
            let path = one("one module folder")?;
            let dir = manifest::module_dir(&discover::resolve(&cwd, Path::new(path)));
            gate::open(std::slice::from_ref(&dir))?;
            check(&dir)
        }
        "types" => {
            if !args.is_empty() {
                return Err(usage("types takes no arguments".into()));
            }
            Ok(Output::Text(
                galaxy_script::api::luau_types(&version::engine().to_string())
                    .trim_end()
                    .to_owned(),
            ))
        }
        "fields" => fields::run(args).map_err(|e| match e {
            fields::Error::Usage(m) => usage(m),
            fields::Error::Failure(f) => f,
        }),
        "model" => {
            let (positional, opts) = split_args(args).map_err(usage)?;
            opts.only("model", model::OPTIONS).map_err(usage)?;
            match positional.as_slice() {
                [dir] => model::run(&discover::resolve(&cwd, Path::new(dir)), None),
                [dir, name] => model::run(&discover::resolve(&cwd, Path::new(dir)), Some(name)),
                _ => Err(usage(format!(
                    "model takes a module folder and maybe a model: {}",
                    command.usage()
                ))),
            }
        }
        "audit" => {
            let path = one("one recording")?;
            audit::run(&discover::recording_file(&discover::resolve(
                &cwd,
                Path::new(path),
            )))
        }
        "film" => {
            let (path, rest) = match args {
                [path, rest @ ..] if !path.starts_with("--") => (path, rest),
                _ => {
                    return Err(usage(
                        "film takes a recording first: galaxy-gm film <recording.json> --note <n>"
                            .into(),
                    ));
                }
            };
            let opts = galaxy_motion::Opts::parse(rest).map_err(usage)?;
            let path = discover::recording_file(&discover::resolve(&cwd, Path::new(path)));
            film::film(&path, &opts)
        }
        "film show" => {
            let (path, rest) = match args {
                [path, rest @ ..] if !path.starts_with("--") => (path, rest),
                _ => {
                    return Err(usage(format!(
                        "film show takes a film folder: {}",
                        command.usage()
                    )));
                }
            };
            let opts = galaxy_motion::Opts::parse(rest).map_err(usage)?;
            film::show(&discover::resolve(&cwd, Path::new(path)), &opts)
        }
        "notes" => {
            let (positional, opts) = split_args(args).map_err(usage)?;
            opts.only("notes", sessions::NOTES_OPTIONS).map_err(usage)?;
            let target = match positional.as_slice() {
                [] => None,
                [one] => Some(discover::resolve(&cwd, Path::new(one))),
                _ => return Err(usage("notes takes at most one folder or recording".into())),
            };
            sessions::notes(target, opts.flag("all"))
        }
        "recording" => {
            let (positional, opts) = split_args(args).map_err(usage)?;
            opts.only("recording", sessions::RECORDING_OPTIONS)
                .map_err(usage)?;
            let (path, view) = match positional.as_slice() {
                [path] => (path, "summary"),
                [path, view] => (path, view.as_str()),
                _ => {
                    return Err(usage(format!(
                        "recording takes a recording and a view: {}",
                        command.usage()
                    )));
                }
            };
            let tick = |name: &str| opts.count(name).map(|v| v.map(|v| v as u64)).map_err(usage);
            let path = discover::recording_file(&discover::resolve(&cwd, Path::new(path)));
            sessions::recording(
                &path,
                view,
                tick("from")?,
                tick("to")?,
                opts.text("name"),
                opts.flag("all"),
            )
        }
        "motion check" => film::check(command, args),
        tool => match tool.strip_prefix("motion ") {
            Some(tool) => film::motion(command, tool, args),
            None => unreachable!("galaxy-gm has help for `{tool}` but does not run it"),
        },
    }
}

/// Words before the first option, and the options after them.
fn split_args(args: &[String]) -> Result<(Vec<String>, galaxy_motion::Opts), String> {
    let at = args
        .iter()
        .position(|a| a.starts_with("--"))
        .unwrap_or(args.len());
    Ok((
        args[..at].to_vec(),
        galaxy_motion::Opts::parse(&args[at..])?,
    ))
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

/// Which things wear each look and each model in the modules' start states:
/// `{looks: {look: [entity]}, models: {model: [entity]}}`, with every look and model a
/// module has, worn or not (an empty list).
pub(crate) fn worn(all: &[Module]) -> Value {
    use std::collections::BTreeMap;
    let assets = Assets::load(all);
    let mut looks: BTreeMap<String, Vec<String>> = assets
        .looks
        .keys()
        .map(|k| (k.clone(), Vec::new()))
        .collect();
    let mut models: BTreeMap<String, Vec<String>> = assets
        .models
        .keys()
        .map(|k| (k.clone(), Vec::new()))
        .collect();
    for module in all {
        for (entity, components) in &module.start {
            let Some(fields) = components.get(MODEL) else {
                continue;
            };
            let text = |k: &str| fields.get(k).and_then(Value::as_str).unwrap_or("");
            if !text("model").is_empty() {
                models
                    .entry(text("model").to_owned())
                    .or_default()
                    .push(entity.clone());
            }
            if !text("look").is_empty() {
                looks
                    .entry(text("look").to_owned())
                    .or_default()
                    .push(entity.clone());
            }
        }
    }
    json!({ "looks": looks, "models": models })
}

/// Of [`worn`], the looks and models module `id` has.
fn own(mut worn: Value, id: &str) -> Value {
    let prefix = format!("{id}.");
    for kind in ["looks", "models"] {
        if let Some(m) = worn[kind].as_object_mut() {
            m.retain(|k, _| k.starts_with(&prefix));
        }
    }
    worn
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
            Ok(_) => match Session::new(all.clone(), 1) {
                Err(e) => findings.push(finding("start", e)),
                Ok(mut session) => {
                    if let Err(e) = session.step(&[]) {
                        findings.push(finding("start", e));
                    }
                    let mut view = galaxy_view::View::detached(session.modules(), 1);
                    view.scene(
                        &session.world,
                        1.0,
                        galaxy_render::glam::Vec2::new(1280.0, 720.0),
                        0.0,
                    );
                    findings.extend(
                        view.screens
                            .problems
                            .iter()
                            .filter(|p| p.starts_with(&format!("{}:", module.id())))
                            .map(|p| finding("screen", p.clone())),
                    );
                    findings.extend(
                        view.check_rooms(
                            &session.world,
                            galaxy_render::glam::Vec2::new(1280.0, 720.0),
                        )
                        .into_iter()
                        .map(|p| finding("room", p)),
                    );
                }
            },
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
        "worn": own(worn(&all), module.id()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_unstable();
        v
    }

    /// Every option a command takes is in its help, and the other way round, and every
    /// motion tool has help.
    #[test]
    fn help_lists_the_options_each_command_takes() {
        // Every summary that lists the tools takes them from the table, in this order.
        assert_eq!(help::motion_tools(), galaxy_motion::TOOLS);
        let overview = help::overview(Tool::Gm);
        let words: Vec<&str> = overview.split_whitespace().collect();
        assert!(words.join(" ").contains(&galaxy_motion::TOOLS.join(", ")));
        for tool in galaxy_motion::TOOLS {
            let c = help::find(Tool::Gm, &format!("motion {tool}"))
                .unwrap_or_else(|| panic!("no help for motion {tool}"));
            let mut takes: Vec<&str> = galaxy_motion::options(tool).unwrap().to_vec();
            takes.push("image");
            takes.push("detail");
            // compare's second film comes bare on the command line.
            takes.retain(|o| *o != "with");
            assert_eq!(sorted(c.options()), sorted(takes), "motion {tool}");
        }
        let film = help::find(Tool::Gm, "film").unwrap();
        assert_eq!(sorted(film.options()), sorted(film::FILM_OPTIONS.to_vec()));
        let check = help::find(Tool::Gm, "motion check").unwrap();
        assert_eq!(check.options(), film::CHECK_OPTIONS);
        for (path, takes) in [
            ("fields", fields::OPTIONS),
            ("model", model::OPTIONS),
            ("film show", film::SHOW_OPTIONS),
            ("notes", sessions::NOTES_OPTIONS),
            ("recording", sessions::RECORDING_OPTIONS),
        ] {
            let c = help::find(Tool::Gm, path).unwrap();
            assert_eq!(sorted(c.options()), sorted(takes.to_vec()), "{path}");
        }
        for c in help::of(Tool::Gm) {
            if let Some(tool) = c.path.strip_prefix("motion ") {
                assert!(
                    tool == "check" || galaxy_motion::TOOLS.contains(&tool),
                    "help for motion {tool}, which doesn't exist"
                );
            }
        }
    }

    /// Every command in the table is one `run` knows: a bad option is a usage error, not
    /// a panic.
    #[test]
    fn every_command_in_the_help_runs() {
        for c in help::of(Tool::Gm).filter(|c| c.path != "doctor") {
            let args = vec!["--no-such-option".to_owned()];
            match run(c, &args) {
                Err(f) => assert!(
                    f.kind == "usage" || f.kind == "bad_input",
                    "{}: {} {}",
                    c.path,
                    f.kind,
                    f.message
                ),
                Ok(_) if c.path == "guide" || c.path == "types" => {}
                Ok(_) => panic!("{} took --no-such-option", c.path),
            }
        }
    }
}
