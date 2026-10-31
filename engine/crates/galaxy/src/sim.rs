//! `galaxy sim run`, `sim load` and `sim replay`: modules run headless.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use galaxy_api::engine::{HOT_LOADED, SCRIPT_FAILED};
use galaxy_module::cli::{self, Output};
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, gate};
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::step::FixedStep;
use galaxy_sim::watch::ModuleWatcher;
use galaxy_sim::{BuildStamp, Recording, Session, replay, save};
use serde_json::{Value, json};

use crate::{USAGE, module_dirs};

/// Ticks `sim run` runs when not told.
const DEFAULT_TICKS: u64 = 300;

#[derive(Default)]
struct Options {
    modules: Option<String>,
    seed: Option<u64>,
    ticks: Option<u64>,
    inputs: Option<PathBuf>,
    save: Option<PathBuf>,
    record: Option<PathBuf>,
    realtime: bool,
    watch: bool,
    state: bool,
}

fn usage(message: impl Into<String>) -> Failure {
    cli::usage_failure(message, USAGE)
}

fn parse(args: &[String], allowed: &[&str]) -> Result<(Vec<String>, Options), Failure> {
    let cwd = cli::current_dir()?;
    let mut options = Options::default();
    let mut positional = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_owned())),
            _ => (arg.as_str(), None),
        };
        if !flag.starts_with("--") {
            positional.push(arg.clone());
            continue;
        }
        if !allowed.contains(&flag) {
            return Err(usage(format!("unknown option `{flag}` here")));
        }
        let switch = matches!(flag, "--realtime" | "--watch" | "--state");
        if switch {
            if inline.is_some() {
                return Err(usage(format!("{flag} takes no value")));
            }
            match flag {
                "--realtime" => options.realtime = true,
                "--watch" => options.watch = true,
                _ => options.state = true,
            }
            continue;
        }
        let value = inline
            .or_else(|| iter.next().cloned())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| usage(format!("{flag} needs a value")))?;
        let number = |v: &str| {
            v.parse::<u64>()
                .map_err(|_| usage(format!("{flag} takes a whole number, not `{v}`")))
        };
        let path = |v: &str| discover::resolve(&cwd, Path::new(v));
        match flag {
            "--modules" => options.modules = Some(value),
            "--seed" => options.seed = Some(number(&value)?),
            "--ticks" => options.ticks = Some(number(&value)?),
            "--inputs" => options.inputs = Some(path(&value)),
            "--save" => options.save = Some(path(&value)),
            "--record" => options.record = Some(path(&value)),
            _ => unreachable!("checked against `allowed`"),
        }
    }
    Ok((positional, options))
}

const RUN_OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--ticks",
    "--inputs",
    "--save",
    "--record",
    "--realtime",
    "--watch",
    "--state",
];

pub fn run(args: &[String]) -> Result<Output, Failure> {
    match args.first().map(String::as_str) {
        Some("run") => {
            let (positional, options) = parse(&args[1..], RUN_OPTIONS)?;
            if !positional.is_empty() {
                return Err(usage(
                    "sim run takes options only: galaxy sim run --modules <dir>",
                ));
            }
            let modules = load_modules(&options)?;
            let session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
            drive(session, &options, DEFAULT_TICKS, "run")
        }
        Some("load") => {
            let allowed: Vec<&str> = RUN_OPTIONS
                .iter()
                .copied()
                .filter(|o| *o != "--seed")
                .collect();
            let (positional, options) = parse(&args[1..], &allowed)?;
            let [save_path] = positional.as_slice() else {
                return Err(usage(
                    "sim load takes one save: galaxy sim load <save> --modules <dir>",
                ));
            };
            let save_path = discover::resolve(&cli::current_dir()?, Path::new(save_path));
            let modules = load_modules(&options)?;
            let (info, world) = save::read(&save_path)
                .map_err(|e| Failure::bad_input(e).with("path", save_path.display().to_string()))?;
            let saved: Vec<&str> = info.modules.iter().map(|m| m.id.as_str()).collect();
            let mut loaded: Vec<&str> = modules.iter().map(Module::id).collect();
            loaded.sort_unstable();
            if saved != loaded {
                return Err(Failure::bad_input(format!(
                    "the save was made with the modules {saved:?}; load it with the same \
                     modules (these are {loaded:?})"
                ))
                .with("saved_modules", json!(info.modules)));
            }
            let session = Session::resume(modules, world).map_err(bad_modules)?;
            drive(session, &options, 0, "load")
        }
        Some("replay") => {
            let (positional, options) = parse(&args[1..], &["--state"])?;
            let [path] = positional.as_slice() else {
                return Err(usage(
                    "sim replay takes one recording: galaxy sim replay <recording>",
                ));
            };
            let path = discover::resolve(&cli::current_dir()?, Path::new(path));
            let rec = Recording::read(&path)
                .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?;
            let (session, report) = replay(&rec).map_err(|e| {
                Failure::bad_input(format!("the recording can't run on this engine: {e}"))
                    .with("path", path.display().to_string())
            })?;
            let this = BuildStamp::current();
            let mut out = json!({
                "ok": report.identical,
                "command": "replay",
                "identical": report.identical,
                "tick": report.tick,
                "hash": report.hash,
                "recorded_hash": report.recorded_hash,
                "recorded_build": rec.build,
                "this_build": this,
                "notes": report.notes,
            });
            if options.state {
                out["state"] = state(&session);
            }
            if !report.identical {
                let mut failure = Failure::new(
                    code::REJECTED,
                    "replay_diverged",
                    format!(
                        "the replay ended with a different world than the recording (tick {}). \
                         Was it made by another build? It names {}",
                        report.tick,
                        rec.build
                            .name
                            .as_deref()
                            .unwrap_or("a build that was not installed")
                    ),
                );
                if let Value::Object(fields) = out {
                    failure.fields = fields;
                }
                return Err(failure);
            }
            Ok(Output::Json(out))
        }
        Some(other) => Err(usage(format!("unknown sim command `{other}`"))),
        None => Err(usage("sim needs a command: run, load or replay")),
    }
}

fn bad_modules(message: String) -> Failure {
    Failure::bad_input(message)
}

/// The modules `--modules` names, through the version gate, read into memory.
fn load_modules(options: &Options) -> Result<Vec<Module>, Failure> {
    let dir = options
        .modules
        .as_deref()
        .ok_or_else(|| usage("this needs the modules: --modules <dir>"))?;
    let dirs = module_dirs(dir)?;
    gate::open(&dirs)?;
    dirs.iter().map(|d| Module::read(d)).collect()
}

/// Runs a session for the asked ticks, with inputs, hot-loads, saving and recording.
fn drive(
    mut session: Session,
    options: &Options,
    default_ticks: u64,
    command: &str,
) -> Result<Output, Failure> {
    let script = match &options.inputs {
        Some(path) => InputsFile::read(path)
            .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?,
        None => InputsFile::default(),
    };
    for input in &script.inputs {
        session.check_input(input).map_err(|e| {
            Failure::bad_input(format!(
                "input `{}` on tick {}: {e}",
                input.name, input.tick
            ))
        })?;
    }
    let inputs = script.by_tick();
    let mut loads: BTreeMap<u64, Vec<Module>> = BTreeMap::new();
    for (tick, dir) in &script.hot_loads {
        loads.entry(*tick).or_default().push(Module::read(dir)?);
    }
    let mut notes: BTreeMap<u64, Vec<&str>> = BTreeMap::new();
    for (tick, text) in &script.notes {
        notes.entry(*tick).or_default().push(text);
    }
    if options.record.is_some() {
        session.start_recording(BuildStamp::current());
    }
    let first_fact = session.world.facts.len();
    let realtime = options.realtime || options.watch;
    let watcher = if options.watch {
        let roots = session
            .modules()
            .map(|m| (m.id().to_owned(), m.manifest.dir.clone()))
            .collect();
        Some(
            ModuleWatcher::new(roots)
                .map_err(|e| Failure::runtime(format!("can't watch the module folders: {e}")))?,
        )
    } else {
        None
    };
    let dirs: BTreeMap<String, PathBuf> = session
        .modules()
        .map(|m| (m.id().to_owned(), m.manifest.dir.clone()))
        .collect();
    let mut watch_failures = Vec::new();
    let mut pace = FixedStep::default();
    let mut last = Instant::now();
    let mut due = 0;
    let end = session.world.tick + options.ticks.unwrap_or(default_ticks);
    while session.world.tick < end {
        if realtime {
            let now = Instant::now();
            due += pace.advance((now - last).as_secs_f64());
            last = now;
            if due == 0 {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            due -= 1;
        }
        let tick = session.world.tick + 1;
        if let Some(watcher) = &watcher {
            for id in watcher.changed() {
                let result = Module::read(&dirs[&id])
                    .map_err(|f| f.message)
                    .and_then(|m| session.hot_load(m));
                if let Err(e) = result {
                    watch_failures.push(json!({ "tick": tick, "module": id, "error": e }));
                }
            }
        }
        for module in loads.remove(&tick).unwrap_or_default() {
            let id = module.id().to_owned();
            session.hot_load(module).map_err(|e| {
                Failure::bad_input(format!("the hot-load of `{id}` before tick {tick}: {e}"))
            })?;
        }
        session
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .map_err(Failure::runtime)?;
        for text in notes.get(&session.world.tick).into_iter().flatten() {
            session.note(text);
        }
    }

    let build = BuildStamp::current();
    if let Some(path) = &options.save {
        save::write(path, &session.world, session.modules(), &build)
            .map_err(|e| Failure::runtime(e).with("path", path.display().to_string()))?;
    }
    if let Some(path) = &options.record {
        let rec = session.recording().expect("recording was started");
        rec.write(path)
            .map_err(|e| Failure::runtime(e).with("path", path.display().to_string()))?;
    }
    let new_facts = &session.world.facts[first_fact..];
    let of_kind = |kind: &str| -> Vec<Value> {
        new_facts
            .iter()
            .filter(|f| f.kind == kind)
            .map(|f| {
                let mut v = Value::Object(f.payload.clone());
                v["tick"] = json!(f.tick);
                v
            })
            .collect()
    };
    let mut out = json!({
        "ok": true,
        "command": command,
        "tick": session.world.tick,
        "hash": session.world.hash(),
        "seed": session.world.seed,
        "modules": session.modules().map(|m| json!({
            "id": m.id(),
            "version": m.manifest.version.to_string(),
        })).collect::<Vec<_>>(),
        "entities": session.world.entities.len(),
        "facts": session.world.facts.len(),
        "script_failures": of_kind(SCRIPT_FAILED),
        "hot_loads": of_kind(HOT_LOADED),
        "watch_failures": watch_failures,
        "log": session.log,
        "saved": options.save.as_ref().map(|p| p.display().to_string()),
        "recorded": options.record.as_ref().map(|p| p.display().to_string()),
    });
    if options.state {
        out["state"] = state(&session);
    }
    Ok(Output::Json(out))
}

/// The world, for `--state`: entities with their components' fields, and the facts.
fn state(session: &Session) -> Value {
    let entities: serde_json::Map<String, Value> = session
        .world
        .entities
        .iter()
        .map(|(id, components)| {
            let components: serde_json::Map<String, Value> = components
                .iter()
                .map(|(name, c)| (name.clone(), Value::Object(c.fields.clone())))
                .collect();
            (id.clone(), Value::Object(components))
        })
        .collect();
    json!({
        "entities": entities,
        "facts": session.world.facts,
        "causes": session.world.causes,
    })
}
