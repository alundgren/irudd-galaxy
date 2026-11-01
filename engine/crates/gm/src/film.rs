//! `galaxy-gm film <recording>`: a played session filmed again from its recording, round
//! one of its notes or between two ticks, with the zoom the player had; and
//! `galaxy-gm motion`: the motion tools on a film, and `motion check`, which films a spec
//! and holds the tools to its expectations. How it works: `engine/docs/motion-tools.md`.

use std::path::Path;

use galaxy_module::cli::Output;
use galaxy_module::failure::code;
use galaxy_module::help::Command;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, gate};
use galaxy_motion::Opts;
use galaxy_motion::check::{Spec, evaluate};
use galaxy_render::Renderer;
use galaxy_sim::recording::RecEvent;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::{Recording, Replay, Session};
use galaxy_view::film::{self, Scripted, Shoot, Stepper, Zoom, recorded_zooms};
use galaxy_view::{Moment, View};
use serde_json::{Value, json};

const TICKS_PER_SECOND: f32 = 30.0;

/// `galaxy-gm film <recording> [options]`.
/// The options `galaxy-gm film` takes.
pub const FILM_OPTIONS: &[&str] = &[
    "note",
    "before",
    "after",
    "from",
    "to",
    "fps",
    "size",
    "out",
    "probes-only",
];

pub fn film(path: &Path, opts: &Opts) -> Result<Output, Failure> {
    opts.only("film", FILM_OPTIONS).map_err(Failure::usage)?;
    let rec = Recording::read(path)
        .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?;
    let notes: Vec<(u64, String, Option<Moment>)> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Note {
                tick, text, camera, ..
            } => Some((
                *tick,
                text.clone(),
                camera
                    .as_ref()
                    .and_then(|c| serde_json::from_value(c.clone()).ok()),
            )),
            _ => None,
        })
        .collect();
    // The world's hash at each note, to check the film shows the world the player had.
    let kept: Vec<(u64, String)> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Note {
                tick,
                hash: Some(h),
                ..
            } => Some((*tick, h.clone())),
            _ => None,
        })
        .collect();
    let bad = |e: String| Failure::bad_input(e).with("path", path.display().to_string());
    let note = opts.count("note").map_err(bad)?;
    let chosen = match note {
        Some(n) => Some(notes.get(n.wrapping_sub(1)).ok_or_else(|| {
            bad(format!(
                "the recording has {} note(s); --note counts from 1",
                notes.len()
            ))
        })?),
        None => None,
    };
    let before = opts.number("before").map_err(bad)?.unwrap_or(3.0).max(0.0);
    let after = opts.number("after").map_err(bad)?.unwrap_or(1.0).max(0.0);
    let (from, to) = match chosen {
        Some((tick, ..)) => (
            tick.saturating_sub((before * TICKS_PER_SECOND) as u64),
            tick + (after * TICKS_PER_SECOND) as u64,
        ),
        None => (
            opts.count("from").map_err(bad)?.map_or(0, |v| v as u64),
            opts.count("to")
                .map_err(bad)?
                .map(|v| v as u64)
                .ok_or_else(|| Failure::usage("film a recording round a note (--note <n>) or between ticks (--from and --to)"))?,
        ),
    };
    let to = to.min(rec.end.tick);
    let moment = chosen.and_then(|c| c.2);
    let (width, height, scale) = match (opts.text("size"), moment) {
        (Some(s), _) => {
            let (w, h) = size(s).map_err(Failure::usage)?;
            (w, h, 1.0)
        }
        (None, Some(m)) => (m.width, m.height, m.scale),
        (None, None) => (960, 540, 1.0),
    };
    let mut zooms = recorded_zooms(&rec);
    if zooms.is_empty()
        && let Some(z) = moment.and_then(|m| m.zoom)
    {
        // An older recording kept no wheel moves; the note's zoom is the best guess.
        zooms.push((0, Zoom::View(z)));
    }
    let out = match opts.text("out") {
        Some(o) => discover::resolve(&galaxy_module::cli::current_dir()?, Path::new(o)),
        None => {
            let name = match note {
                Some(n) => format!("note-{n:04}"),
                None => format!("ticks-{from}-{to}"),
            };
            path.parent()
                .unwrap_or(Path::new("."))
                .join("films")
                .join(name)
        }
    };
    let mut replay = Replay::new(&rec)
        .map_err(|e| bad(format!("the recording can't run on this engine: {e}")))?;
    let source = json!({
        "recording": path.display().to_string(),
        "note": chosen.map(|c| json!({ "number": note, "tick": c.0, "text": c.1 })),
        "recorded_build": rec.build,
    });
    let fps = opts
        .count("fps")
        .map_err(Failure::usage)?
        .unwrap_or(film::DEFAULT_FPS as usize);
    if !(1..=240).contains(&fps) {
        return Err(Failure::usage("--fps takes 1 to 240 frames a second"));
    }
    let shoot = Shoot {
        fps: fps as u32,
        from,
        to,
        width,
        height,
        scale,
        out,
        zooms,
        source,
    };
    let manifest = run_shoot(&mut replay, &shoot, opts.flag("probes-only"))?;
    let first = &manifest.frames[0];
    let last = manifest.frames.last().expect("a film has frames");
    // Every note inside the film: is the replayed world the one the note kept?
    let checked: Vec<Value> = kept
        .iter()
        .filter_map(|(tick, hash)| {
            let f = manifest.frames.iter().find(|f| f.tick == *tick)?;
            Some(json!({ "tick": tick, "identical": f.hash == *hash }))
        })
        .collect();
    let diverged = checked.iter().any(|c| c["identical"] == false);
    let out = json!({
        "ok": !diverged,
        "command": "film",
        "out": shoot.out.display().to_string(),
        "frames": manifest.frames.len(),
        "fps": manifest.fps,
        "size": [width, height],
        "drawn": manifest.drawn,
        "from": { "t": first.time, "tick": first.tick },
        "to": { "t": last.time, "tick": last.tick },
        "adapter": manifest.adapter,
        "notes_checked": checked,
        "next": "galaxy-gm motion <tool> <folder>: track, dots, strip, sheet, flicker, flow, compare",
    });
    if diverged {
        let mut failure = Failure::new(
            code::REJECTED,
            "replay_diverged",
            "the replayed world differs from the recording's at a note, so the film does not show what the player saw; was it made by another build?",
        );
        if let Value::Object(fields) = out {
            failure.fields = fields;
        }
        return Err(failure);
    }
    Ok(Output::Json(out))
}

fn size(s: &str) -> Result<(u32, u32), String> {
    s.split_once('x')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .filter(|&(w, h): &(u32, u32)| (64..=8192).contains(&w) && (64..=8192).contains(&h))
        .ok_or_else(|| format!("--size takes width x height in pixels, such as 960x540, not `{s}`"))
}

fn run_shoot(
    stepper: &mut dyn Stepper,
    shoot: &Shoot,
    probes_only: bool,
) -> Result<galaxy_motion::film::Manifest, Failure> {
    let mut renderer = if probes_only {
        None
    } else {
        Some(
            Renderer::new_headless(shoot.width, shoot.height)
                .map_err(|e| Failure::runtime(format!("no GPU for the film: {e}")))?,
        )
    };
    let seed = stepper.world().seed;
    let mut view = match renderer.as_mut() {
        Some(r) => View::new(r, stepper.modules(), seed),
        None => View::detached(stepper.modules(), seed),
    };
    film::shoot(stepper, &mut view, renderer.as_mut(), shoot)
        .map_err(|e| Failure::runtime(e).with("path", shoot.out.display().to_string()))
}

/// `galaxy-gm motion <tool> <film> [options]`.
pub fn motion(command: &Command, tool: &str, args: &[String]) -> Result<Output, Failure> {
    let cwd = galaxy_module::cli::current_dir()?;
    let usage = |m: String| galaxy_module::cli::command_usage(command, m);
    let (target, rest) = match args {
        [target, rest @ ..] if !target.starts_with("--") => (target, rest),
        _ => {
            return Err(usage(format!(
                "{} takes a film folder first: {}",
                command.path,
                command.usage()
            )));
        }
    };
    let target = discover::resolve(&cwd, Path::new(target));
    // `compare <film> <other film>`: the second film comes bare.
    let mut rest = rest.to_vec();
    if tool == "compare" && rest.first().is_some_and(|a| !a.starts_with("--")) {
        let other = discover::resolve(&cwd, Path::new(&rest.remove(0)));
        rest.insert(0, other.display().to_string());
        rest.insert(0, "--with".into());
    }
    let opts = Opts::parse(&rest).map_err(usage)?;
    galaxy_motion::run(tool, &target, &opts)
        .map(Output::Json)
        .map_err(|e| Failure::bad_input(e).with("path", target.display().to_string()))
}

/// The options `motion check` takes.
pub const CHECK_OPTIONS: &[&str] = &["out"];

/// `motion check <spec>`: films what the spec says and holds each tool to it.
pub fn check(command: &Command, args: &[String]) -> Result<Output, Failure> {
    let usage = |m: String| galaxy_module::cli::command_usage(command, m);
    let (spec_path, rest) = match args {
        [spec, rest @ ..] if !spec.starts_with("--") => (spec, rest),
        _ => {
            return Err(usage(format!(
                "motion check takes a spec: {}",
                command.usage()
            )));
        }
    };
    let spec_path = &discover::resolve(&galaxy_module::cli::current_dir()?, Path::new(spec_path));
    let opts = Opts::parse(rest).map_err(usage)?;
    opts.only("check", CHECK_OPTIONS).map_err(usage)?;
    let bad = |e: String| Failure::bad_input(e).with("path", spec_path.display().to_string());
    let spec = Spec::read(spec_path).map_err(bad)?;
    let dirs = discover::modules_in(&spec.modules);
    if dirs.is_empty() {
        return Err(bad(format!("no modules in {}", spec.modules.display())));
    }
    gate::open(&dirs)?;
    let modules = dirs
        .iter()
        .map(|d| Module::read(d))
        .collect::<Result<Vec<_>, _>>()?;
    let session = Session::new(modules, spec.seed).map_err(bad)?;
    let script = match &spec.inputs {
        Some(p) => InputsFile::read(p).map_err(bad)?,
        None => InputsFile::default(),
    };
    let zooms = script
        .zooms
        .iter()
        .map(|&(t, s)| (t, Zoom::Steps(s)))
        .collect();
    let mut scripted = Scripted::new(session, &script).map_err(bad)?;
    let name = spec_path
        .file_stem()
        .map_or("spec".into(), |s| s.to_string_lossy().into_owned());
    let out = match opts.text("out") {
        Some(o) => discover::resolve(&galaxy_module::cli::current_dir()?, Path::new(o)),
        None => std::env::temp_dir().join("galaxy-motion-check").join(name),
    };
    let shoot = Shoot {
        fps: spec.fps.clamp(1, 240),
        from: spec.from,
        to: spec.to,
        width: spec.size.0,
        height: spec.size.1,
        scale: 1.0,
        out: out.clone(),
        zooms,
        source: json!({ "spec": spec_path.display().to_string() }),
    };
    run_shoot(&mut scripted, &shoot, spec.probes_only)?;
    let (lines, passed) = evaluate(&spec, &out);
    let failed = lines.iter().filter(|l| l["pass"] != json!(true)).count();
    let report = json!({
        "ok": passed,
        "command": "motion check",
        "spec": spec_path.display().to_string(),
        "film": out.display().to_string(),
        "expectations": lines,
    });
    if passed {
        return Ok(Output::Json(report));
    }
    let mut failure = Failure::new(
        code::REJECTED,
        "motion_expectations",
        format!(
            "{failed} motion expectation(s) of {} did not hold",
            spec.expects.len()
        ),
    );
    if let Value::Object(fields) = report {
        failure.fields = fields;
    }
    Err(failure)
}
