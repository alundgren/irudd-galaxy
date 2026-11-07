//! `galaxy sim run`, `sim load` and `sim replay`: modules run headless.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use galaxy_api::engine::{HOT_LOADED, SCRIPT_FAILED};
use galaxy_module::cli::{self, Output};
use galaxy_module::failure::code;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, gate};
use galaxy_sim::scenario::{DueEdits, InputsFile};
use galaxy_sim::step::FixedStep;
use galaxy_sim::watch::ModuleWatcher;
use galaxy_sim::{BuildStamp, Recording, Session, save};
use serde_json::{Value, json};

use crate::module_dirs;
pub(crate) use crate::usage;

/// Ticks `sim run` runs when not told.
const DEFAULT_TICKS: u64 = 300;

#[derive(Default)]
pub(crate) struct Options {
    pub modules: Option<String>,
    pub seed: Option<u64>,
    pub ticks: Option<u64>,
    pub inputs: Option<PathBuf>,
    pub save: Option<PathBuf>,
    pub load: Option<PathBuf>,
    /// `--continue`: load the newest save of these modules from the sessions folder.
    pub continue_save: bool,
    pub physical_size: bool,
    /// `--reduced-motion` (or `GALAXY_REDUCED_MOTION=1`, read by `play::reduced_motion`):
    /// no camera feel (speed zoom, lead or shake), and screens don't slide, fade, lift,
    /// count or flash.
    pub reduced_motion: bool,
    pub record: Option<PathBuf>,
    pub realtime: bool,
    pub watch: bool,
    /// `--no-live`: play without serving the live socket.
    pub no_live: bool,
    /// `--headless`: play with no window, audio device or live socket, a tick a frame as
    /// fast as the machine goes.
    pub headless: bool,
    /// `--scale <factor>`: the UI scale (logical = physical / scale) instead of the
    /// display's.
    pub scale: Option<f32>,
    /// `--fast-to <tick>`: a window plays up to this tick as fast as it goes, undrawn, then
    /// at real speed, its frame times counted from there.
    pub fast_to: Option<u64>,
    /// `--hold`: the `--inputs` script stops at `--fast-to`, so the window holds that moment.
    pub hold: bool,
    pub state: bool,
    /// `--out`: where a picture goes.
    pub out: Option<PathBuf>,
    /// `--size WxH`: the window or picture size in pixels.
    pub size: Option<(u32, u32)>,
    /// `--frames <n>`: stop after this many frames.
    pub frames: Option<u64>,
    /// `--from <tick>` and `--to <tick>`: what a film covers.
    pub from: Option<u64>,
    pub to: Option<u64>,
    /// `--fps <n>`: a film's frames a second.
    pub fps: Option<u64>,
    /// `--probes-only`: a film without pictures.
    pub probes_only: bool,
    /// `--entity <id>` and `--component <name>`: what `--state` prints.
    pub entity: Option<String>,
    pub component: Option<String>,
    /// `--all`: with `--state`, the whole world rather than a line an entity.
    pub all: bool,
    /// `--crop <thing>`: a shot framed round one thing.
    pub crop: Option<String>,
    /// `--builds a,b`: film with two installed builds and compare.
    pub builds: Option<String>,
}

pub(crate) fn parse(args: &[String], allowed: &[&str]) -> Result<(Vec<String>, Options), Failure> {
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
        let switch = matches!(
            flag,
            "--realtime"
                | "--watch"
                | "--state"
                | "--probes-only"
                | "--all"
                | "--no-live"
                | "--headless"
                | "--hold"
                | "--physical-size"
                | "--reduced-motion"
                | "--continue"
        );
        if switch {
            if inline.is_some() {
                return Err(usage(format!("{flag} takes no value")));
            }
            match flag {
                "--realtime" => options.realtime = true,
                "--watch" => options.watch = true,
                "--probes-only" => options.probes_only = true,
                "--all" => options.all = true,
                "--no-live" => options.no_live = true,
                "--headless" => options.headless = true,
                "--hold" => options.hold = true,
                "--physical-size" => options.physical_size = true,
                "--reduced-motion" => options.reduced_motion = true,
                "--continue" => options.continue_save = true,
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
            "--fast-to" => options.fast_to = Some(number(&value)?),
            "--inputs" => options.inputs = Some(path(&value)),
            "--save" => options.save = Some(path(&value)),
            "--load" => options.load = Some(path(&value)),
            "--record" => options.record = Some(path(&value)),
            "--out" => options.out = Some(path(&value)),
            "--frames" => options.frames = Some(number(&value)?),
            "--from" => options.from = Some(number(&value)?),
            "--to" => options.to = Some(number(&value)?),
            "--fps" => options.fps = Some(number(&value)?),
            "--entity" => options.entity = Some(value),
            "--component" => options.component = Some(value),
            "--crop" => options.crop = Some(value),
            "--builds" => options.builds = Some(value),
            "--scale" => {
                let scale = value
                    .parse::<f32>()
                    .ok()
                    .filter(|s| (0.5..=4.0).contains(s));
                options.scale = Some(scale.ok_or_else(|| {
                    usage(format!(
                        "--scale takes a UI scale from 0.5 to 4, such as 1.25, not `{value}`"
                    ))
                })?);
            }
            "--size" => {
                let size = value
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
                    .filter(|&(w, h)| (64..=8192).contains(&w) && (64..=8192).contains(&h));
                options.size = Some(size.ok_or_else(|| {
                    usage(format!(
                        "--size takes width x height in pixels, such as 1280x720, not `{value}`"
                    ))
                })?);
            }
            _ => unreachable!("checked against `allowed`"),
        }
    }
    Ok((positional, options))
}

pub(crate) const RUN_OPTIONS: &[&str] = &[
    "--modules",
    "--seed",
    "--ticks",
    "--inputs",
    "--save",
    "--record",
    "--realtime",
    "--watch",
    "--state",
    "--entity",
    "--component",
    "--all",
];

pub(crate) const REPLAY_OPTIONS: &[&str] = &["--state", "--entity", "--component", "--all"];

/// `galaxy sim run`.
pub fn run(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, RUN_OPTIONS)?;
    if !positional.is_empty() {
        return Err(usage(
            "sim run takes options only: galaxy sim run --modules <dir>",
        ));
    }
    let modules = load_modules(&options)?;
    let session = Session::new(modules, options.seed.unwrap_or(1)).map_err(bad_modules)?;
    drive(session, &options, DEFAULT_TICKS, "run")
}

/// The options `sim load` takes: `sim run`'s without `--seed`, and `--continue`.
pub(crate) fn load_options() -> Vec<&'static str> {
    RUN_OPTIONS
        .iter()
        .copied()
        .filter(|o| *o != "--seed")
        .chain(["--continue"])
        .collect()
}

/// `galaxy sim load`.
pub fn load(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, &load_options())?;
    let modules = load_modules(&options)?;
    let save_path = match (positional.as_slice(), options.continue_save) {
        ([save], false) => discover::resolve(&cli::current_dir()?, Path::new(save)),
        ([], true) => continuing_save(&modules)?,
        _ => {
            return Err(usage(
                "sim load takes one save, or --continue for the newest save of the modules: \
                 galaxy sim load <save> --modules <dir>",
            ));
        }
    };
    let (session, resumed) = resume_save(&save_path, modules, false)?;
    let mut out = drive(session, &options, 0, "load")?;
    if let Output::Json(v) = &mut out {
        v["resumed"] = resumed;
    }
    Ok(out)
}

/// The save `--continue` loads: the newest `save.db` in the sessions folder made with
/// exactly these modules (by ID). Bad input when there is none.
pub(crate) fn continuing_save(modules: &[Module]) -> Result<PathBuf, Failure> {
    let root = galaxy_module::install::sessions_dir();
    newest_save(&root, modules).ok_or_else(|| {
        let ids: Vec<&str> = modules.iter().map(Module::id).collect();
        Failure::bad_input(format!(
            "no saved game of the modules {ids:?} in {}: play them once with galaxy play \
             --modules <dir> and quit, which saves the session there",
            root.display()
        ))
        .with("path", root.display().to_string())
    })
}

/// The newest `<session>/save.db` under `root` whose modules have these IDs.
pub(crate) fn newest_save(root: &Path, modules: &[Module]) -> Option<PathBuf> {
    let mut ids: Vec<&str> = modules.iter().map(Module::id).collect();
    ids.sort_unstable();
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path().join("save.db");
        let Ok(when) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
            continue;
        };
        let Ok(info) = save::info(&path) else {
            continue;
        };
        if !info
            .modules
            .iter()
            .map(|m| m.id.as_str())
            .eq(ids.iter().copied())
        {
            continue;
        }
        if best.as_ref().is_none_or(|(t, _)| when > *t) {
            best = Some((when, path));
        }
    }
    best.map(|(_, path)| path)
}

/// Shared restore path. Windowed play (`same_build`) needs the build that saved it. A
/// module whose files changed since the save is carried over when the saved world still
/// fits it (new fields get defaults; a change that would lose data is refused), as a
/// hot-load is. The session's next tick tells scripts with `engine.resumed`. Returns the
/// session and what was resumed: the save, its tick, and the modules whose files changed.
pub(crate) fn resume_save(
    save_path: &Path,
    modules: Vec<Module>,
    same_build: bool,
) -> Result<(Session, Value), Failure> {
    let (info, world) = save::read(save_path)
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
    if same_build {
        let build = BuildStamp::current();
        if info.engine != build.version || info.build != build.name {
            return Err(Failure::bad_input(
                "the save was made by another engine build; reopen it with the build that saved it",
            )
            .with("saved_engine", info.engine)
            .with("saved_build", json!(info.build)));
        }
    }
    let changed: Vec<String> = info
        .modules
        .iter()
        .filter(|saved| {
            modules
                .iter()
                .any(|m| m.id() == saved.id && m.files.hash() != saved.files)
        })
        .map(|saved| saved.id.clone())
        .collect();
    let mut session = Session::resume(modules, world).map_err(|e| {
        if changed.is_empty() {
            return bad_modules(e);
        }
        Failure::bad_input(format!(
            "the save doesn't fit the changed module files of {changed:?}: {e}. Load it with \
             the module files that saved it, --load an earlier save, or start a new game"
        ))
        .with("changed_modules", json!(changed))
    })?;
    session.announce_resume();
    let resumed = json!({
        "save": save_path.display().to_string(),
        "tick": info.tick,
        "changed_modules": changed,
        "edited": galaxy_sim::edit::edits(&session.world),
    });
    Ok((session, resumed))
}

/// `galaxy sim replay`.
pub fn replay(args: &[String]) -> Result<Output, Failure> {
    let (positional, options) = parse(args, REPLAY_OPTIONS)?;
    filters_need_state(&options)?;
    let [path] = positional.as_slice() else {
        return Err(usage(
            "sim replay takes one recording: galaxy sim replay <recording>",
        ));
    };
    let path = discover::recording_file(&discover::resolve(&cli::current_dir()?, Path::new(path)));
    let rec = Recording::read(&path)
        .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?;
    let (session, report) = galaxy_sim::replay(&rec).map_err(|e| {
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
        "edited": galaxy_sim::edit::edits(&session.world),
        "builds": galaxy_sim::recording::builds(
            &rec.build,
            &this,
            "replayed",
            &format!("galaxy sim replay {}", path.display()),
        ),
        "notes": report.notes,
    });
    if options.state {
        out["state"] = state(&session, &options)?;
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

pub(crate) fn bad_modules(message: String) -> Failure {
    Failure::bad_input(message)
}

/// The modules `--modules` names, through the version gate, read into memory.
pub(crate) fn load_modules(options: &Options) -> Result<Vec<Module>, Failure> {
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
    filters_need_state(options)?;
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
    let mut edits = DueEdits::new(&script.edits, session.world.tick, "inputs", &session)
        .map_err(Failure::bad_input)?;
    let mut loads: BTreeMap<u64, Vec<Module>> = BTreeMap::new();
    if script.needs_window() {
        return Err(Failure::bad_input(
            "clicks, drags and key presses ([[click]], [[pointer]], [[key]]) are resolved against a played screen: run them with galaxy play --headless --ticks <n> (no window, fast), then galaxy sim replay <session>/recording.json",
        ));
    }
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
    // Edits on the tick the run starts on are made before it, and recorded.
    edits.apply(&mut session).map_err(Failure::bad_input)?;
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
        edits.apply(&mut session).map_err(Failure::bad_input)?;
        for text in notes.get(&session.world.tick).into_iter().flatten() {
            session.note(text, None, None);
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
        "edits": of_kind(galaxy_api::engine::EDITED),
        "edited": galaxy_sim::edit::edits(&session.world),
        "watch_failures": watch_failures,
        "log": session.log,
        "saved": options.save.as_ref().map(|p| p.display().to_string()),
        "recorded": options.record.as_ref().map(|p| p.display().to_string()),
    });
    if options.state {
        out["state"] = state(&session, options)?;
    }
    Ok(Output::Json(out))
}

/// `--entity`, `--component` and `--all` choose what `--state` prints.
fn filters_need_state(options: &Options) -> Result<(), Failure> {
    if (options.entity.is_some() || options.component.is_some() || options.all) && !options.state {
        return Err(usage(
            "--entity, --component and --all choose what --state prints; add --state",
        ));
    }
    Ok(())
}

/// The world, for `--state` (`galaxy_live::reads::state`, which `galaxy-gm live state`
/// prints too).
fn state(session: &Session, options: &Options) -> Result<Value, Failure> {
    galaxy_live::reads::state(
        &session.world,
        &galaxy_live::reads::StateQuery {
            entity: options.entity.clone(),
            component: options.component.clone(),
            all: options.all,
        },
    )
    .map_err(usage)
}
