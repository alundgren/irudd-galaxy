//! Films and the tools that read them.
//!
//! A film ([`film`]) is a session drawn frame by frame on a fixed clock, with probes of
//! where everything was on screen. Each tool answers one question about how motion looks
//! with a few numbers, the timestamps of suspicious moments and at most one small
//! picture, so an agent can check its work without reading dozens of frames. Everything
//! here runs on the CPU. How it works, and which tool answers which question:
//! `engine/docs/motion-tools.md`.

pub mod check;
pub mod compare;
pub mod dots;
pub mod film;
pub mod flicker;
pub mod flip;
pub mod flow;
pub mod image;
pub mod opts;
mod plot;
pub mod sheet;
pub mod strip;
pub mod tone;
pub mod track;
pub mod vdp;
pub mod warp;

use std::path::Path;

use serde_json::Value;

pub use film::Film;
pub use opts::Opts;

/// The tools, in the order an agent usually reaches for them.
pub const TOOLS: &[&str] = &[
    "track", "dots", "strip", "sheet", "flicker", "tone", "flow", "compare",
];

/// The options `tool` takes, besides `--image`; none for a tool that doesn't exist.
pub fn options(tool: &str) -> Option<&'static [&'static str]> {
    Some(match tool {
        "track" => track::OPTIONS,
        "dots" => dots::OPTIONS,
        "strip" => strip::OPTIONS,
        "sheet" => sheet::OPTIONS,
        "flicker" => flicker::OPTIONS,
        "tone" => tone::OPTIONS,
        "flow" => flow::OPTIONS,
        "compare" => compare::OPTIONS,
        _ => return None,
    })
}

/// Runs `tool` on the film in `dir`: its compact report (the reading, the few numbers
/// that decide it, with times, and the picture), or with `--detail` all of it. Either way
/// it says which build drew the film (`builds`).
pub fn run(tool: &str, dir: &Path, opts: &Opts) -> Result<Value, String> {
    let film = Film::read(dir)?;
    let detail = opts.flag("detail");
    let opts = &opts.without("detail");
    let mut report = match tool {
        "track" => track::run(&film, opts),
        "dots" => dots::run(&film, opts),
        "strip" => strip::run(&film, opts),
        "sheet" => sheet::run(&film, opts),
        "flicker" => flicker::run(&film, opts),
        "tone" => tone::run(&film, opts),
        "flow" => flow::run(&film, opts),
        "compare" => compare::run(&film, opts),
        other => Err(format!(
            "no motion tool `{other}`; the tools are {}",
            TOOLS.join(", ")
        )),
    }?;
    report["builds"] = film.builds();
    if detail {
        Ok(report)
    } else {
        Ok(compact(tool, &report))
    }
}

/// What each tool's compact report keeps: dotted paths into the full one, and for lists
/// how many entries (`flags:3`). The reading, builds, warnings and picture always stay.
fn kept(tool: &str) -> &'static [&'static str] {
    match tool {
        "track" => &[
            "thing",
            "world.displacement",
            "world.distance",
            "world.speed_max",
            "world.speed_max_at",
            "screen.judder",
            "camera.judder",
            "screen.offset_px.max",
            "screen.offset_px.max_at",
            "zooms:4",
            "flagged",
            "flags:3",
            "gap.closest",
            "gap.closest_at",
            "gap.overshoot",
            "gap.rebound",
            "gap.rebound_after",
            "gap.settled_at",
            "surface.missing",
            "surface.closest",
            "surface.closest_at",
            "surface.end",
            "surface.overlap.intervals",
            "surface.overlap.seconds",
            "surface.overlap.deepest",
            "surface.overlap.deepest_at",
            "surface.overlap.frames_on_ticks",
            "surface.overlap.frames_between_ticks",
            "surface.overlap.with_order_target",
            "surface.overlap.runs:4",
            "surface.order_target",
            "surface.first_contact",
            "surface.contact.within",
            "surface.contact.intervals",
            "surface.contact.runs:3",
            "surface.frames_round_it",
            "approach",
        ],
        "dots" => &[
            "drift_px_per_frame",
            "moving_steps",
            "steps",
            "lines:6",
            "popped_unexplained",
            "popped:3",
        ],
        "strip" => &[
            "along",
            "drift_px_per_frame",
            "moving_steps",
            "steps",
            "confidence",
            "drifting:3",
            "pops:3",
        ],
        "sheet" => &[
            "frames:12",
            "poses:12",
            "gaps_between_poses",
            "crop",
            "onion",
        ],
        "flicker" => &[
            "pops",
            "blips",
            "flicker_runs",
            "near_split",
            "chosen",
            "events:5",
            "left_out",
        ],
        "tone" => &[
            "thing",
            "hard_steps",
            "steps:5",
            "prominence.max",
            "prominence.max_at",
            "change.max",
        ],
        "flow" => &["moving:5", "along"],
        "compare" => &[
            "with",
            "pixels",
            "same_world_frames",
            "flip.visibly_different_frames",
            "flip.worst_frames:3",
            "behaviour",
            "colorvideovdp",
        ],
        _ => &[],
    }
}

/// The compact form of `tool`'s report: [`kept`]'s fields, moments as one line each
/// (`t 2.633 (frame 158, tick 79)`), and `detail` naming what `--detail` adds.
fn compact(tool: &str, full: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("tool".into(), full["tool"].clone());
    let mut cut = Vec::new();
    for key in ["reading", "builds", "warnings"] {
        if let Some(v) = full.get(key).filter(|v| !is_empty(v)) {
            out.insert(key.into(), v.clone());
        }
    }
    for spec in kept(tool) {
        let (path, limit) = match spec.split_once(':') {
            Some((p, n)) => (p, n.parse::<usize>().ok()),
            None => (*spec, None),
        };
        let Some(mut v) = path
            .split('.')
            .try_fold(full, |v, k| v.get(k))
            .cloned()
            .filter(|v| !is_empty(v))
        else {
            continue;
        };
        if let (Some(n), Value::Array(items)) = (limit, &mut v)
            && items.len() > n
        {
            cut.push(format!("{} more {path}", items.len() - n));
            items.truncate(n);
        }
        insert(&mut out, path, moments(v));
    }
    if !full["image"].is_null() {
        out.insert("image".into(), full["image"].clone());
    }
    let mut detail = String::from("--detail for every field");
    if !cut.is_empty() {
        detail = format!("{detail}, and the {}", cut.join(", "));
    }
    out.insert("detail".into(), Value::String(detail));
    Value::Object(out)
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// `v` at a dotted `path` in `out`, making the objects on the way.
fn insert(out: &mut serde_json::Map<String, Value>, path: &str, v: Value) {
    match path.split_once('.') {
        None => {
            out.insert(path.into(), v);
        }
        Some((head, rest)) => {
            let entry = out
                .entry(head.to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            if let Value::Object(m) = entry {
                insert(m, rest, v);
            }
        }
    }
}

/// Every moment (`{frame, t, tick}`, maybe with more) as one short line.
fn moments(v: Value) -> Value {
    match v {
        Value::Object(m)
            if m.len() == 3 && ["frame", "t", "tick"].iter().all(|k| m.contains_key(*k)) =>
        {
            Value::String(format!(
                "t {} (frame {}, tick {})",
                m["t"], m["frame"], m["tick"]
            ))
        }
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, moments(v))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().map(moments).collect()),
        other => other,
    }
}
