//! Reading played sessions without scripts: `galaxy-gm notes` (the player's F8 notes, from
//! a recording, a session folder or the whole sessions folder) and `galaxy-gm recording`
//! (a recording's summary, its timeline, inputs, orders and zooms). Output is capped by
//! default, with how many more there are.

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
pub const RECORDING_OPTIONS: &[&str] = &["from", "to", "all", "name"];

/// The views `recording` has.
pub const VIEWS: &[&str] = &["summary", "timeline", "inputs", "orders", "zooms"];

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

/// `m:ss.s` for a tick: `1:05.3`.
fn clock(tick: u64) -> String {
    let tenths = (tick as f64 / f64::from(TICKS_PER_SECOND) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// `galaxy-gm recording <rec> [summary | timeline | inputs | orders | zooms]
/// [--from <tick>] [--to <tick>] [--name <text>] [--all]`.
pub fn recording(
    path: &Path,
    view: &str,
    from: Option<u64>,
    to: Option<u64>,
    name: Option<&str>,
    all: bool,
) -> Result<Output, Failure> {
    if name.is_some() && view != "inputs" {
        return Err(Failure::usage(
            "--name goes with the inputs view: galaxy-gm recording <rec> inputs --name <text>",
        ));
    }
    let names: Vec<&str> = name.map_or(Vec::new(), |n| n.split(',').map(str::trim).collect());
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
                RecEvent::Input(i)
                    if within(i.tick)
                        && (names.is_empty() || names.iter().any(|n| i.name.contains(n))) =>
                {
                    Some(format!(
                        "tick {} t {:.2} {} {}",
                        i.tick,
                        seconds(i.tick),
                        i.source,
                        changes::input(i)
                    ))
                }
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
        "timeline" => {
            let (rows, identical) = timeline(&rec, from, to)?;
            out["replay_identical"] = json!(identical);
            out["legend"] = json!(
                "m:ss.s (tick) then: order (given by the player: an input came on that tick, named after `from`), auto (orders changed with no input: waypoint steps, scripts), arrived, note, zoom, inputs (other keys, a run of them a line)"
            );
            if !identical {
                out["warning"] = json!(format!(
                    "this build's replay ends in a different world than the recording; it was made by {}, so these orders may not be the player's",
                    rec.build.label()
                ));
            }
            rows
        }
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
                "no recording view `{other}`; the views are {}",
                VIEWS.join(", ")
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
            RecEvent::Presentation { .. } => "presentation",
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
        "galaxy-gm recording <rec> timeline: what the player did, in order; inputs | orders | zooms list each kind"
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

/// One line of a timeline, with what may still be added to it.
struct Line {
    tick: u64,
    text: String,
    /// Added once known: `superseded at ...` or `arrived ...`.
    then: Option<String>,
}

/// Replays the recording and tells what happened, in order: the orders the player gave
/// (an order changed on a tick an input came), the ones that changed with no input
/// (waypoint steps, scripts; a run of them for one thing is one line), arrivals, notes,
/// zoom bursts, and other inputs (a run of them is one line).
fn timeline(rec: &Recording, from: u64, to: u64) -> Result<(Vec<String>, bool), Failure> {
    let mut replay = Replay::new(rec)
        .map_err(|e| Failure::bad_input(format!("the recording can't run on this engine: {e}")))?;
    let mut lines: Vec<Line> = Vec::new();
    let mut notes: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    let mut zooms: Vec<(u64, f32)> = Vec::new();
    let mut n = 0;
    for e in &rec.events {
        match e {
            RecEvent::Note { tick, text, .. } => {
                n += 1;
                notes
                    .entry(*tick)
                    .or_default()
                    .push(format!("note {n}: {text:?}"));
            }
            RecEvent::Zoom { tick, view } => zooms.push((*tick, *view)),
            _ => {}
        }
    }
    // Wheel clicks within a second of each other are one zoom.
    let mut bursts: BTreeMap<u64, String> = BTreeMap::new();
    let mut view_before = rec
        .start
        .entities
        .values()
        .find_map(|c| c.get(galaxy_api::engine::CAMERA))
        .and_then(|c| c.fields.get("view").and_then(Value::as_f64))
        .map(|v| v as f32);
    let mut k = 0;
    while k < zooms.len() {
        let mut j = k;
        while j + 1 < zooms.len() && zooms[j + 1].0 <= zooms[j].0 + u64::from(TICKS_PER_SECOND) {
            j += 1;
        }
        let (start, end) = (zooms[k], zooms[j]);
        let word = match view_before {
            Some(b) if end.1 > b => "zoom out",
            Some(_) => "zoom in",
            None => "zoom",
        };
        let from_view = view_before.map_or(String::new(), |b| format!("{b:.1} -> "));
        bursts.insert(
            start.0,
            format!(
                "{word}: view {from_view}{:.1} world units top to bottom ({} wheel step(s) over {:.1} s)",
                end.1,
                j - k + 1,
                (end.0 - start.0) as f64 / f64::from(TICKS_PER_SECOND)
            ),
        );
        view_before = Some(end.1);
        k = j + 1;
    }
    // The player's last order for each thing that has not arrived yet: its line.
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    // A run of automatic order changes for one thing: its line, count and last order.
    let mut auto: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    // A run of inputs that gave no order: its line, and the inputs by name.
    let mut run: Option<(usize, u64, BTreeMap<String, usize>)> = None;
    let flush_run = |lines: &mut Vec<Line>,
                     run: &mut Option<(usize, u64, BTreeMap<String, usize>)>| {
        if let Some((at, last, names)) = run.take() {
            let total: usize = names.values().sum();
            let list: Vec<String> = names
                .iter()
                .map(|(n, c)| {
                    if *c > 1 {
                        format!("{n} x{c}")
                    } else {
                        n.clone()
                    }
                })
                .collect();
            let span = if last > lines[at].tick {
                format!(" until {}", clock(last))
            } else {
                String::new()
            };
            lines[at].text = format!("inputs: {total}{span}: {}", list.join(", "));
        }
    };
    loop {
        let tick = replay.session.world.tick + 1;
        let inputs: Vec<galaxy_sim::Input> = replay.inputs_at(tick).to_vec();
        let before = changes::orders(&replay.session.world);
        let facts = replay.session.world.facts.len();
        if !replay.step().map_err(Failure::runtime)? {
            break;
        }
        if !(from..=to).contains(&tick) {
            continue;
        }
        let changed = changes::since(&before, &replay.session.world, facts);
        let given = changed.iter().any(|(k, _)| *k == "order") && !inputs.is_empty();
        // Inputs that gave no order: part of a run.
        if !inputs.is_empty() && !given {
            let downs: Vec<&galaxy_sim::Input> = inputs
                .iter()
                .filter(|i| i.payload.get("down").and_then(Value::as_bool) != Some(false))
                .collect();
            if !downs.is_empty() {
                let near = run
                    .as_ref()
                    .is_some_and(|(_, last, _)| tick <= last + 2 * u64::from(TICKS_PER_SECOND));
                if !near {
                    flush_run(&mut lines, &mut run);
                    lines.push(Line {
                        tick,
                        text: String::new(),
                        then: None,
                    });
                    run = Some((lines.len() - 1, tick, BTreeMap::new()));
                }
                if let Some((_, last, names)) = &mut run {
                    *last = tick;
                    for i in downs {
                        *names.entry(i.name.clone()).or_default() += 1;
                    }
                }
            }
        }
        for (kind, what) in changed {
            let entity = what.split(':').next().unwrap_or("").to_owned();
            match kind {
                "order" if given => {
                    flush_run(&mut lines, &mut run);
                    auto.remove(&entity);
                    if let Some(at) = pending.remove(&entity) {
                        lines[at].then =
                            Some(format!("superseded at {} before arriving", clock(tick)));
                    }
                    let keys: Vec<String> = inputs.iter().map(changes::input).collect();
                    lines.push(Line {
                        tick,
                        text: format!("order {what} (from {})", keys.join(", ")),
                        then: None,
                    });
                    if !what.ends_with(": no order") {
                        pending.insert(entity, lines.len() - 1);
                    }
                }
                "order" => match auto.get_mut(&entity) {
                    Some((at, count)) => {
                        *count += 1;
                        let at = *at;
                        let count = *count;
                        lines[at].text = format!(
                            "auto: {count} order changes for {entity}, the last at {}: {what}",
                            clock(tick)
                        );
                    }
                    None => {
                        lines.push(Line {
                            tick,
                            text: format!("auto: order {what}"),
                            then: None,
                        });
                        auto.insert(entity, (lines.len() - 1, 1));
                    }
                },
                _ => {
                    auto.remove(&entity);
                    let after = pending.remove(&entity).map(|at| {
                        let secs = (tick - lines[at].tick) as f64 / f64::from(TICKS_PER_SECOND);
                        lines[at].then = Some(format!("arrived at {}", clock(tick)));
                        format!(" ({secs:.1} s after the order)")
                    });
                    lines.push(Line {
                        tick,
                        text: format!("arrived {what}{}", after.unwrap_or_default()),
                        then: None,
                    });
                }
            }
        }
        for text in notes.remove(&tick).into_iter().flatten() {
            lines.push(Line {
                tick,
                text,
                then: None,
            });
        }
        if let Some(text) = bursts.remove(&tick) {
            lines.push(Line {
                tick,
                text,
                then: None,
            });
        }
    }
    flush_run(&mut lines, &mut run);
    let rows = lines
        .into_iter()
        .map(|l| {
            let then = l.then.map_or(String::new(), |t| format!("; {t}"));
            format!("{} (tick {}) {}{then}", clock(l.tick), l.tick, l.text)
        })
        .collect();
    Ok((rows, replay.session.world.hash() == rec.end.hash))
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn clock_reads_minutes_seconds_and_tenths() {
        assert_eq!(clock(0), "0:00.0");
        assert_eq!(clock(31), "0:01.0");
        assert_eq!(clock(30 * 65 + 9), "1:05.3");
    }
}
