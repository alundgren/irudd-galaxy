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
use galaxy_view::builds::{self, Drawer};
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
    "as-recorded",
    "builds",
];

/// What to film of a recording, however many builds draw it.
struct Plan<'a> {
    rec: &'a Recording,
    path: &'a Path,
    /// The note filmed round: its number, and seconds before and after.
    note: Option<(usize, f32, f32)>,
    from: u64,
    to: u64,
    fps: u32,
    width: u32,
    height: u32,
    scale: f32,
    /// `--size` was given (else the note's frame size or the default).
    sized: bool,
    zooms: Vec<(u64, Zoom)>,
    source: Value,
    probes_only: bool,
    /// Each note's tick and the world's hash it kept.
    kept: Vec<(u64, String)>,
}

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
    let moment = chosen.and_then(|c| c.2.as_ref());
    let (width, height, scale) = match (opts.text("size"), moment) {
        (Some(s), _) => {
            let (w, h) = size(s).map_err(Failure::usage)?;
            (w, h, 1.0)
        }
        (None, Some(m)) => (m.width, m.height, m.scale),
        (None, None) => (960, 540, 1.0),
    };
    let mut zooms = recorded_zooms(&rec);
    if !zooms.iter().any(|(_, z)| matches!(z, Zoom::View(_)))
        && let Some(z) = moment.and_then(|m| m.zoom)
    {
        // An older recording kept no wheel moves; the note's zoom is the best guess.
        zooms.push((0, Zoom::View(z)));
    }
    if let Some((tick, _, Some(moment))) = chosen {
        // At the selected note's tick, use the frame it kept even if the pointer moved
        // again before the next simulation tick.
        zooms.push((*tick, Zoom::Presentation(moment.screens.clone())));
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
    let plan = Plan {
        rec: &rec,
        path,
        note: note.map(|n| (n, before, after)),
        from,
        to,
        fps: fps as u32,
        width,
        height,
        scale,
        sized: opts.text("size").is_some(),
        zooms,
        source,
        probes_only: opts.flag("probes-only"),
        kept,
    };
    if let Some(list) = opts.text("builds") {
        if opts.flag("as-recorded") {
            return Err(Failure::usage(
                "--as-recorded or --builds, not both: --builds recorded,this films both",
            ));
        }
        let drawers = list
            .split(',')
            .map(|w| builds::resolve(w, Some(&rec.build)))
            .collect::<Result<Vec<_>, _>>()?;
        if drawers.len() != 2 {
            return Err(Failure::usage(
                "--builds takes two builds, such as recorded,this",
            ));
        }
        let report = builds::film_on_each(&drawers, &out, !plan.probes_only, &mut |d, dir| {
            film_with(&plan, d, dir).map(|(v, _)| v)
        })?;
        return Ok(Output::Json(report));
    }
    let drawer = if opts.flag("as-recorded") {
        builds::resolve("recorded", Some(&rec.build))?
    } else {
        Drawer::This
    };
    let (out, diverged) = film_with(&plan, &drawer, &out)?;
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

/// Films `plan` into `out`, drawn by `drawer`: its report, and whether the world differs
/// from the recording's at a note.
fn film_with(plan: &Plan, drawer: &Drawer, out: &Path) -> Result<(Value, bool), Failure> {
    let bad = |e: String| Failure::bad_input(e).with("path", plan.path.display().to_string());
    let here = |out: &Path, probes_only: bool| {
        let mut replay = Replay::new(plan.rec)
            .map_err(|e| bad(format!("the recording can't run on this engine: {e}")))?;
        let shoot = Shoot {
            fps: plan.fps,
            from: plan.from,
            to: plan.to,
            width: plan.width,
            height: plan.height,
            scale: plan.scale,
            out: out.to_path_buf(),
            zooms: plan.zooms.clone(),
            source: plan.source.clone(),
        };
        run_shoot(&mut replay, &shoot, probes_only)
    };
    let mut added = None;
    let manifest = match drawer {
        Drawer::This => here(out, plan.probes_only)?,
        Drawer::Other(build) => {
            // Only what every build since films began takes.
            let mut args = vec![
                "film".to_owned(),
                plan.path.display().to_string(),
                "--fps".into(),
                plan.fps.to_string(),
                "--out".into(),
                out.display().to_string(),
            ];
            match plan.note {
                // The note's own frame size, window scale included, as that build reads it.
                Some((n, before, after)) if !plan.sized => args.extend([
                    "--note".into(),
                    n.to_string(),
                    "--before".into(),
                    before.to_string(),
                    "--after".into(),
                    after.to_string(),
                ]),
                _ => args.extend([
                    "--from".into(),
                    plan.from.to_string(),
                    "--to".into(),
                    plan.to.to_string(),
                    "--size".into(),
                    format!("{}x{}", plan.width, plan.height),
                ]),
            }
            if plan.probes_only {
                args.push("--probes-only".into());
            }
            builds::run(build, "galaxy-gm", &args)?;
            // What it didn't record, from this build's probes of the same span.
            let scratch = builds::Scratch::new("probes");
            let probes = here(&scratch.0, true)?;
            let command = format!(
                "galaxy-gm film {}{}",
                plan.path.display(),
                plan.note
                    .map_or(String::new(), |(n, ..)| format!(" --note {n}"))
            );
            let stamp = builds::stamp(build);
            let b = galaxy_sim::recording::builds(&plan.rec.build, &stamp, "drew", &command);
            let (list, _) = builds::adopt(out, b, Some(&probes))
                .map_err(|e| Failure::runtime(e).with("path", out.display().to_string()))?;
            added = Some(list);
            galaxy_motion::Film::read(out)
                .map_err(|e| Failure::runtime(e).with("path", out.display().to_string()))?
                .manifest
        }
    };
    let first = &manifest.frames[0];
    let last = manifest.frames.last().expect("a film has frames");
    // Every note inside the filmed span (the ticks asked for, not just those a frame lands
    // on): is the replayed world the one the note kept? Checked on the note's own tick, which
    // a frame need not land on at a low --fps; a film drawn by
    // another build is also held to its own frame on that tick, where it has one.
    let mut inside: Vec<&(u64, String)> = plan
        .kept
        .iter()
        .filter(|(tick, _)| (plan.from..=plan.to).contains(tick))
        .collect();
    inside.sort_by_key(|(tick, _)| *tick);
    let mut checked = Vec::new();
    if !inside.is_empty() {
        let mut replay = Replay::new(plan.rec)
            .map_err(|e| bad(format!("the recording can't run on this engine: {e}")))?;
        for (tick, hash) in inside {
            while replay.session.world.tick < *tick {
                if !replay.step().map_err(Failure::runtime)? {
                    break;
                }
            }
            let mut identical = replay.session.world.hash() == *hash;
            if matches!(drawer, Drawer::Other(_))
                && let Some(f) = manifest.frames.iter().find(|f| f.tick == *tick)
            {
                identical &= f.hash == *hash;
            }
            checked.push(json!({ "tick": tick, "identical": identical }));
        }
    }
    let diverged = checked.iter().any(|c| c["identical"] == false);
    let mut report = json!({
        "ok": !diverged,
        "command": "film",
        "out": out.display().to_string(),
        "frames": manifest.frames.len(),
        "fps": manifest.fps,
        "size": [manifest.width, manifest.height],
        "drawn": manifest.drawn,
        "from": { "t": first.time, "tick": first.tick },
        "to": { "t": last.time, "tick": last.tick },
        "adapter": manifest.adapter,
        "notes_checked": checked,
        "builds": manifest.builds,
        "next": format!("galaxy-gm motion <tool> <folder>: {}", galaxy_motion::TOOLS.join(", ")),
    });
    if let Some(list) = added {
        report["drawn_by"] = json!(drawer.name());
        report["added"] = json!(format!(
            "{} drew it; this build added what it doesn't record: {}",
            drawer.name(),
            if list.is_empty() {
                "nothing".to_owned()
            } else {
                list.join(", ")
            }
        ));
    }
    Ok((report, diverged))
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
    let (runs, lines, passed) = evaluate(&spec, &out);
    let failed = lines.iter().filter(|l| l["pass"] != json!(true)).count();
    let report = json!({
        "ok": passed,
        "command": "motion check",
        "spec": spec_path.display().to_string(),
        "film": out.display().to_string(),
        "runs": runs,
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

/// The options `galaxy-gm film show` takes.
pub const SHOW_OPTIONS: &[&str] = &[
    "thing", "from", "to", "every", "fields", "gap", "lines", "all",
];

/// Rows `film show` prints when not told `--all`.
const SHOW_ROWS: usize = 30;

/// `galaxy-gm film show <film>`: the film's probes, a frame a line.
pub fn show(dir: &Path, opts: &Opts) -> Result<Output, Failure> {
    opts.only("film show", SHOW_OPTIONS)
        .map_err(Failure::usage)?;
    let bad = |e: String| Failure::bad_input(e).with("path", dir.display().to_string());
    let film = galaxy_motion::Film::read(dir).map_err(bad)?;
    let frames = film.frames();
    let things: Vec<String> = match opts.text("thing") {
        Some(list) => list
            .split(',')
            .map(|t| film.thing_id(t.trim()))
            .collect::<Result<_, _>>()
            .map_err(bad)?,
        None => frames[0]
            .things
            .iter()
            .filter(|(_, t)| t.followed)
            .map(|(id, _)| id.clone())
            .collect(),
    };
    const FIELDS: &[&str] = &["x", "y", "angle", "height", "screen", "on_screen"];
    let fields: Vec<&str> = match opts.text("fields") {
        Some(f) => f.split(',').map(str::trim).collect(),
        None => vec!["x", "y", "angle", "screen"],
    };
    if let Some(f) = fields.iter().find(|f| !FIELDS.contains(f)) {
        return Err(Failure::usage(format!(
            "--fields takes {}, not `{f}`",
            FIELDS.join(", ")
        )));
    }
    // `--gap <y>`: each shown thing's gap to y's surface, outline to outline.
    let gap = match opts.text("gap") {
        None => None,
        Some(y) => {
            let y = film.thing_id(y.trim()).map_err(bad)?;
            let Some(oy) = film.manifest.outlines.get(&y) else {
                return Err(bad(format!(
                    "this film keeps no outline for {y} (filmed by an older build, or it has no \
                     model): film it again for gaps between surfaces"
                )));
            };
            Some((y, oy))
        }
    };
    let num = |name: &str| opts.number(name).map_err(Failure::usage);
    let (from, to) = (num("from")?, num("to")?);
    let every = opts
        .count("every")
        .map_err(Failure::usage)?
        .unwrap_or(1)
        .max(1);
    let mut chosen: Vec<&galaxy_motion::film::Frame> = frames
        .iter()
        .filter(|f| {
            from.is_none_or(|a| f.time >= a - 1e-4) && to.is_none_or(|b| f.time <= b + 1e-4)
        })
        .step_by(every)
        .collect();
    // A time between two frames (or a span narrower than one) shows the nearest frame.
    let mut snapped = None;
    if chosen.is_empty()
        && let Some(t) = match (from, to) {
            (Some(a), Some(b)) => Some((a + b) / 2.0),
            (a, b) => a.or(b),
        }
    {
        let near = &frames[film.frame_at(t)];
        snapped = Some(format!(
            "no frame at t {t}: showing the nearest, frame {} at t {:.3} (frames are {:.4} s apart)",
            near.index,
            near.time,
            film.dt()
        ));
        chosen.push(near);
    }
    let mut columns = vec!["frame t tick alpha".to_owned()];
    for id in &things {
        for f in &fields {
            columns.push(match *f {
                "screen" => format!("{id}.screen_x {id}.screen_y"),
                f => format!("{id}.{f}"),
            });
        }
        if let Some((y, _)) = &gap
            && y != id
        {
            columns.push(format!("{id}.gap_to_{y}"));
        }
    }
    let mut rows: Vec<String> = Vec::new();
    for f in &chosen {
        if opts.flag("lines") {
            for (k, l) in f.lines.iter().enumerate() {
                rows.push(format!(
                    "frame {} t {:.3} tick {} line {k}: {}{} points {} dots {} alpha {:.2} width {:.2}",
                    f.index,
                    f.time,
                    f.tick,
                    l.kind,
                    if l.closed { " closed" } else { "" },
                    l.points.len(),
                    l.dots.len(),
                    l.alpha,
                    l.width
                ));
            }
            continue;
        }
        let mut row = format!("{} {:.3} {} {:.2}", f.index, f.time, f.tick, f.alpha);
        for id in &things {
            let t = f.things.get(id);
            for field in &fields {
                let v = match (t, *field) {
                    (None, "screen") => "- -".to_owned(),
                    (None, _) => "-".to_owned(),
                    (Some(t), "x") => format!("{:.3}", t.x),
                    (Some(t), "y") => format!("{:.3}", t.y),
                    (Some(t), "angle") => format!("{:.4}", t.angle),
                    (Some(t), "height") => format!("{:.3}", t.height),
                    (Some(t), "on_screen") => t.on_screen.to_string(),
                    (Some(t), _) => t
                        .screen
                        .map_or("- -".to_owned(), |s| format!("{:.1} {:.1}", s[0], s[1])),
                };
                row.push(' ');
                row.push_str(&v);
            }
            if let Some((y, oy)) = &gap
                && y != id
            {
                let g = t.zip(film.manifest.outlines.get(id)).and_then(|(t, o)| {
                    galaxy_motion::film::surface_gap((o, t), (oy, f.things.get(y)?))
                });
                row.push(' ');
                row.push_str(&g.map_or("-".to_owned(), |g| {
                    galaxy_motion::track::fine(g).to_string()
                }));
            }
        }
        rows.push(row);
    }
    let total = rows.len();
    if !opts.flag("all") {
        rows.truncate(SHOW_ROWS);
    }
    let m = &film.manifest;
    let mut out = json!({
        "ok": true,
        "command": "film show",
        "film": dir.display().to_string(),
        "fps": m.fps,
        "size": [m.width, m.height],
        "drawn": m.drawn,
        "builds": film.builds(),
        "frames": frames.len(),
        "span": [galaxy_motion::film::when(&frames[0]), galaxy_motion::film::when(frames.last().expect("frames"))],
        "things": frames[0].things.keys().collect::<Vec<_>>(),
        "changes": m.changes.iter().take(20).map(|c| format!("tick {} t {:.3} {} {}", c.tick, c.t, c.kind, c.what)).collect::<Vec<_>>(),
        "units": "t seconds; x, y, height world units; angle radians, counter-clockwise from +x; screen pixels from the top left; gap_to world units between surfaces, as collision measures them (below 0 overlaps, up to 0.05 is contact range)",
        "columns": if opts.flag("lines") { "frame t tick line: kind points dots alpha width".to_owned() } else { columns.join(" ") },
        "rows": rows,
    });
    if let Some(s) = snapped {
        out["snapped"] = json!(s);
    }
    if total > SHOW_ROWS && !opts.flag("all") {
        out["more"] = json!(format!(
            "{} more row(s): use --from/--to (seconds), --every <n> or --all",
            total - SHOW_ROWS
        ));
    }
    if m.changes.len() > 20 {
        out["more_changes"] = json!(m.changes.len() - 20);
    }
    Ok(Output::Json(out))
}
