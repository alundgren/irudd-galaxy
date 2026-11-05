//! `galaxy-gm what-if <recording> --modules <dir>`: the recorded session's inputs run on
//! changed modules, beside a replay of the recording, one tick at a time; the first tick
//! where the two worlds differ, and what differs there (entities, components and fields,
//! the facts recorded that tick).
//!
//! The changed run is the recording with the given modules' files in place of the
//! recorded ones (modules it didn't have are added). A session that started fresh starts
//! again from the changed modules' start states, with the recorded seed, so a changed
//! start value counts; one that started from a save resumes that world with the changed
//! modules. Recorded hot-loads of a changed module are skipped: the changed files are
//! used throughout. Both runs are this build, so a difference is the change's alone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use galaxy_module::cli::Output;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, gate};
use galaxy_sim::recording::RecEvent;
use galaxy_sim::world::Component;
use galaxy_sim::{BuildStamp, Recording, Replay, Session, World};
use serde_json::{Value, json};

use crate::sessions::clock;

/// Options `what-if` takes.
pub const OPTIONS: &[&str] = &["modules", "all"];

/// Differences listed of each kind without `--all`.
const CAP: usize = 20;
/// Difference lines in `report` without `--all`.
const LINES: usize = 8;

pub fn run(path: &Path, modules_dir: &Path, all: bool) -> Result<Output, Failure> {
    let at = |e: String| Failure::bad_input(e).with("path", path.display().to_string());
    let rec = Recording::read(path).map_err(at)?;
    let dirs: Vec<PathBuf> = discover::modules_in(modules_dir);
    if dirs.is_empty() {
        return Err(Failure::bad_input(format!(
            "{} is not a module folder and holds none",
            modules_dir.display()
        ))
        .with("path", modules_dir.display().to_string()));
    }
    gate::open(&dirs)?;
    let given = dirs
        .iter()
        .map(|d| Module::read(d))
        .collect::<Result<Vec<_>, _>>()?;

    let (mut changed, mut added, mut same) = (Vec::new(), Vec::new(), Vec::new());
    let mut new = rec.clone();
    for m in &given {
        match rec.modules.get(m.id()) {
            Some(files) if *files == m.files => same.push(m.id().to_owned()),
            Some(_) => changed.push(m.id().to_owned()),
            None => added.push(m.id().to_owned()),
        }
        new.modules.insert(m.id().to_owned(), m.files.clone());
    }
    let replaced: BTreeSet<&String> = changed.iter().chain(&added).collect();
    let mut skipped = Vec::new();
    new.events.retain(|e| match e {
        RecEvent::HotLoad { tick, module, .. } if replaced.contains(module) => {
            skipped.push(json!({ "tick": tick, "module": module }));
            false
        }
        _ => true,
    });
    let start = if fresh(&rec) {
        let modules = modules_of(&new).map_err(at)?;
        new.start = Session::new(modules, rec.seed)
            .map_err(|e| at(format!("the changed modules don't start: {e}")))?
            .world;
        "rebuilt: the session started fresh, so it starts again from the changed modules' \
         start states with the recorded seed"
    } else {
        "resumed: the session started from a save, so its world goes on with the changed \
         modules (new fields get their defaults)"
    };

    let mut base = Replay::new(&rec)
        .map_err(|e| at(format!("the recording can't run on this engine: {e}")))?;
    let mut run = Replay::new(&new)
        .map_err(|e| at(format!("the changed modules can't run this recording: {e}")))?;

    let notes: BTreeMap<u64, &String> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Note {
                tick,
                hash: Some(h),
                ..
            } => Some((*tick, h)),
            _ => None,
        })
        .collect();
    let mut baseline_diverged = None;
    let mut check_baseline = |w: &World| {
        let recorded = notes
            .get(&w.tick)
            .copied()
            .or((w.tick == rec.end.tick).then_some(&rec.end.hash));
        if baseline_diverged.is_none() && recorded.is_some_and(|h| *h != w.hash()) {
            baseline_diverged = Some(w.tick);
        }
    };

    // The tick the recording starts on: 0 for a fresh session, later for one from a save.
    let first = base.session.world.tick;
    let mut found: Option<(u64, Option<String>)> = None;
    let (mut facts, mut causes) = (0, 0);
    check_baseline(&base.session.world);
    if !same_world(&base.session.world, &run.session.world, 0, 0) {
        found = Some((first, None));
    }
    while found.is_none() {
        facts = base.session.world.facts.len();
        causes = base.session.world.causes.len();
        if !base
            .step()
            .map_err(|e| at(format!("the recording can't run on this engine: {e}")))?
        {
            break;
        }
        check_baseline(&base.session.world);
        let tick = base.session.world.tick;
        if let Err(e) = run.step() {
            found = Some((tick, Some(e)));
            break;
        }
        if !same_world(&base.session.world, &run.session.world, facts, causes) {
            found = Some((tick, None));
        }
    }

    let modules = json!({ "changed": changed, "added": added, "same": same });
    let mut out = json!({
        "recording": path.display().to_string(),
        "modules": modules,
        "start": start,
        "skipped_hot_loads": skipped,
        "build": { "recorded": rec.build.label(), "this": BuildStamp::current().label() },
        "from": first,
    });
    let mut report = Vec::new();
    if changed.is_empty() && added.is_empty() {
        report.push(
            "the modules given have the same files as the recorded ones, so nothing changed"
                .to_owned(),
        );
    }
    match found.clone() {
        None => {
            out["differs"] = false.into();
            out["ticks"] = rec.end.tick.into();
            report.push(format!(
                "no difference: the world is the same on every tick, {first} to {} ({} to {})",
                rec.end.tick,
                clock(first),
                clock(rec.end.tick)
            ));
        }
        Some((tick, error)) => {
            out["differs"] = true.into();
            out["tick"] = tick.into();
            out["time"] = clock(tick).into();
            let inputs: Vec<String> = base
                .inputs_at(tick)
                .iter()
                .map(|i| match &i.via {
                    Some(via) => format!("{} ({} via {via})", i.name, i.source),
                    None => format!("{} ({})", i.name, i.source),
                })
                .collect();
            out["inputs"] = json!(inputs);
            let when = if tick == first {
                format!("at the start (tick {tick})")
            } else {
                format!("after tick {tick} ({})", clock(tick))
            };
            if let Some(e) = error {
                report.push(format!(
                    "the changed modules fail on tick {tick} ({}), so the session can't go on: {e}",
                    clock(tick)
                ));
                out["error"] = e.into();
            } else {
                let d = diff(&base.session.world, &run.session.world, facts, causes);
                report.push(format!(
                    "first difference {when}: {} in entities and components, {} in facts{}",
                    d.entities.len(),
                    d.facts.len(),
                    if inputs.is_empty() {
                        String::new()
                    } else {
                        format!("; inputs that tick: {}", inputs.join(", "))
                    }
                ));
                let lines: Vec<String> = d.lines();
                let shown = if all { lines.len() } else { LINES };
                report.extend(lines.iter().take(shown).cloned());
                if lines.len() > shown {
                    report.push(format!(
                        "and {} more (--all lists every difference)",
                        lines.len() - shown
                    ));
                }
                let cap = |list: Vec<Value>| -> (Vec<Value>, usize) {
                    let more = if all {
                        0
                    } else {
                        list.len().saturating_sub(CAP)
                    };
                    (
                        list.into_iter()
                            .take(if all { usize::MAX } else { CAP })
                            .collect(),
                        more,
                    )
                };
                let (entities, more_entities) = cap(d.entities);
                let (facts, more_facts) = cap(d.facts);
                out["differences"] = json!({
                    "entities": entities,
                    "facts": facts,
                    "other": d.other,
                    "more": more_entities + more_facts,
                });
            }
        }
    }
    let reached = found.map_or(rec.end.tick, |(t, _)| t);
    out["baseline"] = match baseline_diverged {
        Some(t) => json!({
            "matches_recording": false,
            "note": format!(
                "the replay of the recording on this build already differs from it at tick {t}; \
                 differences are measured against that replay, not the recording itself. \
                 Was it recorded by another build? It names {}",
                rec.build.label()
            ),
        }),
        None => json!({
            "matches_recording": true,
            "note": format!(
                "the replay of the recording on this build matched every recorded hash (notes, \
                 end) it reached; it ran from tick {first} to tick {reached}, where the \
                 comparison stopped"
            ),
        }),
    };
    out["report"] = json!(report);
    Ok(Output::Json(out))
}

/// Whether the recording started from a fresh world of its modules (not a save).
fn fresh(rec: &Recording) -> bool {
    rec.start.tick == 0
        && modules_of(rec)
            .and_then(|m| Session::new(m, rec.seed))
            .is_ok_and(|s| s.world.hash() == rec.start.hash())
}

fn modules_of(rec: &Recording) -> Result<Vec<Module>, String> {
    rec.modules
        .iter()
        .map(|(id, files)| {
            Module::from_files(&Path::new("recording").join(id), files.clone())
                .map_err(|f| format!("module `{id}`: {}", f.message))
        })
        .collect()
}

/// Whether two worlds that were the same before this tick still are. Facts and causes
/// only grow, so those before `facts` and `causes` are already known to be the same.
fn same_world(a: &World, b: &World, facts: usize, causes: usize) -> bool {
    a.tick == b.tick
        && a.facts.len() == b.facts.len()
        && a.causes.len() == b.causes.len()
        && a.entities == b.entities
        && a.facts[facts..] == b.facts[facts..]
        && a.causes[causes..] == b.causes[causes..]
        && a.rng == b.rng
        && a.next_event == b.next_event
        && a.services == b.services
        && a.seed == b.seed
}

/// What differs between the recorded world (`a`) and the changed one (`b`).
struct Diff {
    entities: Vec<Value>,
    facts: Vec<Value>,
    other: Vec<String>,
}

fn side(in_a: bool) -> &'static str {
    if in_a { "recorded" } else { "changed" }
}

fn diff(a: &World, b: &World, facts: usize, causes: usize) -> Diff {
    let mut entities = Vec::new();
    let mut quiet = Vec::new();
    let ids: BTreeSet<&String> = a.entities.keys().chain(b.entities.keys()).collect();
    let empty = BTreeMap::new();
    for id in ids {
        let (ca, cb) = (a.entities.get(id), b.entities.get(id));
        if ca.is_none() || cb.is_none() {
            entities.push(json!({ "entity": id, "only_in": side(ca.is_some()) }));
            continue;
        }
        let (ca, cb) = (ca.unwrap_or(&empty), cb.unwrap_or(&empty));
        let names: BTreeSet<&String> = ca.keys().chain(cb.keys()).collect();
        for name in names {
            match (ca.get(name), cb.get(name)) {
                (Some(x), Some(y)) if x != y => {
                    let fields = field_diffs(x, y);
                    if fields.is_empty() {
                        quiet.push(format!(
                            "{id} {name}: the same fields, last written by another cause \
                             or on another tick"
                        ));
                    }
                    for (field, ra, rb) in fields {
                        entities.push(json!({
                            "entity": id, "component": name, "field": field,
                            "recorded": ra, "changed": rb,
                        }));
                    }
                }
                (Some(_), None) | (None, Some(_)) => entities.push(json!({
                    "entity": id, "component": name, "only_in": side(ca.contains_key(name)),
                })),
                _ => {}
            }
        }
    }
    let mut facts_diff = Vec::new();
    let key = |f: &galaxy_sim::world::Fact| (f.kind.clone(), Value::Object(f.payload.clone()));
    let mut left: Vec<_> = a
        .facts
        .get(facts..)
        .unwrap_or(&[])
        .iter()
        .map(key)
        .collect();
    let mut right: Vec<_> = b
        .facts
        .get(facts..)
        .unwrap_or(&[])
        .iter()
        .map(key)
        .collect();
    left.retain(|f| match right.iter().position(|g| g == f) {
        Some(i) => {
            right.remove(i);
            false
        }
        None => true,
    });
    let same_set = left.is_empty() && right.is_empty();
    for (list, in_a) in [(left, true), (right, false)] {
        for (kind, payload) in list {
            facts_diff.push(json!({ "kind": kind, "payload": payload, "only_in": side(in_a) }));
        }
    }
    // Both runs recorded the same facts this tick, but not as the same sequence: another
    // order (which decides the order they're handled in next tick) or other causes.
    let (fa, fb) = (
        a.facts.get(facts..).unwrap_or(&[]),
        b.facts.get(facts..).unwrap_or(&[]),
    );
    if same_set && let Some(i) = (0..fa.len().min(fb.len())).find(|&i| fa[i] != fb[i]) {
        let order = fa[i].kind != fb[i].kind || fa[i].payload != fb[i].payload;
        let fact = |f: &galaxy_sim::world::Fact| json!({ "kind": f.kind, "payload": f.payload, "cause": f.cause });
        facts_diff.push(json!({
            "differs": if order { "order" } else { "cause" },
            "position": i,
            "recorded": fact(&fa[i]),
            "changed": fact(&fb[i]),
        }));
    }
    let mut other = quiet;
    if a.rng != b.rng {
        other.push("the random streams: scripts drew a different number of values".into());
    }
    if a.causes.get(causes..) != b.causes.get(causes..) || a.causes.len() != b.causes.len() {
        other.push("the causes recorded this tick (which handlers wrote)".into());
    }
    if a.next_event != b.next_event || a.services != b.services {
        other.push("event bookkeeping (facts not yet handled, engine services)".into());
    }
    Diff {
        entities,
        facts: facts_diff,
        other,
    }
}

/// Each field whose value differs: (field, recorded, changed), absent as null.
fn field_diffs(x: &Component, y: &Component) -> Vec<(String, Value, Value)> {
    let names: BTreeSet<&String> = x.fields.keys().chain(y.fields.keys()).collect();
    names
        .into_iter()
        .filter_map(|n| {
            let (a, b) = (x.fields.get(n), y.fields.get(n));
            (a != b).then(|| {
                (
                    n.clone(),
                    a.cloned().unwrap_or(Value::Null),
                    b.cloned().unwrap_or(Value::Null),
                )
            })
        })
        .collect()
}

impl Diff {
    /// A line a difference, for `report`.
    fn lines(&self) -> Vec<String> {
        let short = |v: &Value| {
            let s = v.to_string();
            if s.chars().count() > 80 {
                format!("{}...", s.chars().take(77).collect::<String>())
            } else {
                s
            }
        };
        let mut out: Vec<String> = self
            .entities
            .iter()
            .map(|e| {
                let id = e["entity"].as_str().unwrap_or("");
                match (e["component"].as_str(), e["field"].as_str()) {
                    (Some(c), Some(f)) => format!(
                        "{id} {c}.{f}: recorded {}, changed {}",
                        short(&e["recorded"]),
                        short(&e["changed"])
                    ),
                    (Some(c), None) => format!(
                        "{id} has {c} only in the {} run",
                        e["only_in"].as_str().unwrap_or("")
                    ),
                    _ => format!(
                        "{id} exists only in the {} run",
                        e["only_in"].as_str().unwrap_or("")
                    ),
                }
            })
            .collect();
        out.extend(self.facts.iter().map(|f| {
            let fact = |f: &Value| {
                let kind = f["kind"].as_str().unwrap_or("");
                format!("{kind} {}", short(&f["payload"]))
            };
            match f["differs"].as_str() {
                Some("order") => format!(
                    "facts: the same facts in another order; fact {} this tick: \
                     recorded {}, changed {}",
                    f["position"].as_u64().unwrap_or(0) + 1,
                    fact(&f["recorded"]),
                    fact(&f["changed"])
                ),
                Some(_) => format!(
                    "facts: fact {} this tick ({}) has cause {} recorded, {} changed",
                    f["position"].as_u64().unwrap_or(0) + 1,
                    fact(&f["recorded"]),
                    f["recorded"]["cause"],
                    f["changed"]["cause"]
                ),
                None => format!(
                    "fact {} only in the {} run",
                    fact(f),
                    f["only_in"].as_str().unwrap_or("")
                ),
            }
        }));
        out.extend(self.other.iter().map(|o| format!("also differs: {o}")));
        out
    }
}
