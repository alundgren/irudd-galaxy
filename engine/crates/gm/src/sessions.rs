//! Reading played sessions without scripts: `galaxy-gm notes` (the player's F8 notes, from
//! a recording, a session folder or the whole sessions folder) and `galaxy-gm recording`
//! (a recording's summary, inputs, orders and zooms). Output is capped by default, with
//! how many more there are.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use galaxy_api::engine::TICKS_PER_SECOND;
use galaxy_module::cli::Output;
use galaxy_module::{Failure, manifest};
use galaxy_sim::recording::RecEvent;
use galaxy_sim::{Recording, Replay, changes};
use serde_json::{Value, json};

/// Notes listed when not told `--all`.
const NOTES_SHOWN: usize = 20;
/// Rows `recording` lists when not told `--all`.
const ROWS_SHOWN: usize = 40;

/// The options each command takes.
pub const NOTES_OPTIONS: &[&str] = &["all"];
pub const RECORDING_OPTIONS: &[&str] = &["from", "to", "all"];

fn seconds(tick: u64) -> f64 {
    (tick as f64 / f64::from(TICKS_PER_SECOND) * 100.0).round() / 100.0
}

/// A recording read, with the folder it is in.
fn read(path: &Path) -> Result<Recording, Failure> {
    Recording::read(path)
        .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))
}

/// One session's notes, as JSON lines.
fn session_notes(rec: &Recording, dir: &Path) -> Vec<Value> {
    rec.events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Note {
                tick, text, frame, ..
            } => Some((tick, text, frame)),
            _ => None,
        })
        .enumerate()
        .map(|(i, (tick, text, frame))| {
            let n = i + 1;
            let mut v = json!({ "note": n, "tick": tick, "t": seconds(*tick), "text": text });
            if let Some(f) = frame.as_ref().and_then(|f| f["file"].as_str()) {
                let path: PathBuf = f.split('/').fold(dir.to_path_buf(), |p, part| p.join(part));
                v["seen"] = json!(path.display().to_string());
            }
            let drawn = dir.join("audit").join(format!("{n:04}.png"));
            if drawn.is_file() {
                v["drawn_again"] = json!(drawn.display().to_string());
            }
            v
        })
        .collect()
}

/// `galaxy-gm notes [<recording | session folder | sessions folder>] [--all]`.
pub fn notes(target: Option<PathBuf>, all: bool) -> Result<Output, Failure> {
    let target = target.unwrap_or_else(galaxy_module::install::sessions_dir);
    // One recording, or one session folder.
    let single = if target.is_file() {
        Some(target.clone())
    } else if target.join("recording.json").is_file() {
        Some(target.join("recording.json"))
    } else {
        None
    };
    let sessions: Vec<PathBuf> = match &single {
        Some(rec) => vec![rec.clone()],
        None => {
            if !target.is_dir() {
                return Err(Failure::bad_input(format!(
                    "{} is not a recording, a session folder or a folder of sessions",
                    target.display()
                ))
                .with("path", target.display().to_string()));
            }
            let mut found: Vec<PathBuf> = std::fs::read_dir(&target)
                .map_err(|e| Failure::runtime(format!("can't read {}: {e}", target.display())))?
                .flatten()
                .map(|e| e.path().join("recording.json"))
                .filter(|p| p.is_file())
                .collect();
            // Session folders start with the time they began: newest first.
            found.sort();
            found.reverse();
            found
        }
    };
    let mut listed = Vec::new();
    let mut shown = 0;
    let (mut hidden_notes, mut hidden_sessions, mut without) = (0, 0, 0);
    let mut unreadable = Vec::new();
    for path in &sessions {
        let dir = path.parent().unwrap_or(Path::new("."));
        let rec = match read(path) {
            Ok(r) => r,
            Err(f) if single.is_none() => {
                unreadable
                    .push(json!({ "recording": path.display().to_string(), "error": f.message }));
                continue;
            }
            Err(f) => return Err(f),
        };
        let notes = session_notes(&rec, dir);
        if notes.is_empty() && single.is_none() {
            without += 1;
            continue;
        }
        if !all && shown >= NOTES_SHOWN {
            hidden_notes += notes.len();
            hidden_sessions += 1;
            continue;
        }
        let room = if all {
            notes.len()
        } else {
            NOTES_SHOWN - shown
        };
        let count = notes.len();
        let kept: Vec<Value> = notes.into_iter().take(room).collect();
        shown += kept.len();
        hidden_notes += count - kept.len();
        listed.push(json!({
            "session": dir.display().to_string(),
            "recording": path.display().to_string(),
            "build": rec.build.name.clone().unwrap_or_else(|| rec.build.version.clone()),
            "modules": rec.modules.keys().collect::<Vec<_>>(),
            "ticks": rec.end.tick,
            "seconds": seconds(rec.end.tick),
            "notes": kept,
        }));
    }
    let mut out = json!({
        "ok": true,
        "command": "notes",
        "from": target.display().to_string(),
        "sessions": listed,
        "next": "galaxy-gm audit <session> checks each note's world and draws it again; galaxy-gm film <session> --note <n> films the seconds round one",
    });
    if hidden_notes > 0 {
        out["more"] = json!(format!(
            "{hidden_notes} more note(s){}: use --all, or name one session",
            if hidden_sessions > 0 {
                format!(" in {hidden_sessions} more session(s)")
            } else {
                String::new()
            }
        ));
    }
    if without > 0 {
        out["sessions_without_notes"] = json!(without);
    }
    if !unreadable.is_empty() {
        out["unreadable"] = Value::Array(unreadable);
    }
    Ok(Output::Json(out))
}

/// `galaxy-gm recording <rec> [summary | inputs | orders | zooms] [--from <tick>]
/// [--to <tick>] [--all]`.
pub fn recording(
    path: &Path,
    view: &str,
    from: Option<u64>,
    to: Option<u64>,
    all: bool,
) -> Result<Output, Failure> {
    let rec = read(path)?;
    let from = from.unwrap_or(0);
    let to = to.unwrap_or(u64::MAX);
    let within = |tick: u64| (from..=to).contains(&tick);
    let mut out = json!({
        "ok": true,
        "command": "recording",
        "recording": path.display().to_string(),
        "view": view,
    });
    let rows: Vec<String> = match view {
        "summary" => {
            summary(&rec, &mut out);
            return Ok(Output::Json(out));
        }
        "inputs" => rec
            .events
            .iter()
            .filter_map(|e| match e {
                RecEvent::Input(i) if within(i.tick) => Some(format!(
                    "tick {} t {:.2} {} {}",
                    i.tick,
                    seconds(i.tick),
                    i.source,
                    changes::input(i)
                )),
                _ => None,
            })
            .collect(),
        "zooms" => rec
            .events
            .iter()
            .filter_map(|e| match e {
                RecEvent::Zoom { tick, view } if within(*tick) => Some(format!(
                    "tick {tick} t {:.2} view {view:.1} world units top to bottom",
                    seconds(*tick)
                )),
                _ => None,
            })
            .collect(),
        "orders" => {
            let (rows, identical) = orders(&rec, from, to)?;
            out["replay_identical"] = json!(identical);
            if !identical {
                out["warning"] = json!(format!(
                    "this build's replay ends in a different world than the recording; it was made by {}, so these orders may not be the player's",
                    rec.build.name.as_deref().unwrap_or(&rec.build.version)
                ));
            }
            rows
        }
        other => {
            return Err(Failure::usage(format!(
                "no recording view `{other}`; the views are summary, inputs, orders and zooms"
            )));
        }
    };
    let total = rows.len();
    let shown: Vec<String> = if all {
        rows
    } else {
        rows.into_iter().take(ROWS_SHOWN).collect()
    };
    if shown.len() < total {
        out["more"] = json!(format!(
            "{} more: use --all, or --from/--to ticks (the last shown is {})",
            total - shown.len(),
            shown.last().map_or("", String::as_str)
        ));
    }
    out["count"] = json!(total);
    out["rows"] = json!(shown);
    Ok(Output::Json(out))
}

fn summary(rec: &Recording, out: &mut Value) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &rec.events {
        let kind = match e {
            RecEvent::Input(i) => {
                *names.entry(i.name.as_str()).or_default() += 1;
                "inputs"
            }
            RecEvent::Note { .. } => "notes",
            RecEvent::Zoom { .. } => "zooms",
            RecEvent::HotLoad { .. } => "hot_loads",
        };
        *counts.entry(kind).or_default() += 1;
    }
    let modules: BTreeMap<&String, String> = rec
        .modules
        .iter()
        .map(|(id, files)| {
            let version = files
                .get(manifest::FILE)
                .and_then(|t| manifest::parse(Path::new(id), t).ok())
                .map_or_else(|| "?".to_owned(), |m| m.version.to_string());
            (id, version)
        })
        .collect();
    out["build"] = json!(rec.build);
    out["seed"] = json!(rec.seed);
    out["modules"] = json!(modules);
    out["ticks"] = json!({ "start": rec.start.tick, "end": rec.end.tick });
    out["seconds"] = json!(seconds(rec.end.tick.saturating_sub(rec.start.tick)));
    out["end_hash"] = json!(rec.end.hash);
    out["counts"] = json!(counts);
    out["inputs_by_name"] = json!(names);
    out["next"] = json!(
        "galaxy-gm recording <rec> inputs | orders | zooms lists them; galaxy-gm notes <rec> the notes"
    );
}

/// Replays the recording and lists each order and arrival between `from` and `to`.
fn orders(rec: &Recording, from: u64, to: u64) -> Result<(Vec<String>, bool), Failure> {
    let mut replay = Replay::new(rec)
        .map_err(|e| Failure::bad_input(format!("the recording can't run on this engine: {e}")))?;
    let mut rows = Vec::new();
    loop {
        let tick = replay.session.world.tick + 1;
        let before = changes::orders(&replay.session.world);
        let facts = replay.session.world.facts.len();
        if !replay.step().map_err(Failure::runtime)? {
            break;
        }
        if (from..=to).contains(&tick) {
            for (kind, what) in changes::since(&before, &replay.session.world, facts) {
                rows.push(format!("tick {tick} t {:.2} {kind} {what}", seconds(tick)));
            }
        }
    }
    Ok((rows, replay.session.world.hash() == rec.end.hash))
}
