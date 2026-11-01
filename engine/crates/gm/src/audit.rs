//! `galaxy-gm audit <recording>`: a played session's notes, checked and drawn again.
//!
//! The launcher runs the build the recording names, so the audit runs on the engine that
//! made the session. It replays the recording from its start with its own module files,
//! and at each note:
//!
//! - compares the world's hash with the one the note kept: the world is identical, or the
//!   replay has diverged;
//! - draws the moment again from the replay, between the same two ticks, at the same
//!   size, zoom and presentation time, into `audit/0001.png` beside the recording, and
//!   compares the camera and the picture with the frame the player saw
//!   (`notes/0001.png`). Pixel-identical is expected only on the machine that played; on
//!   another GPU the picture should still visibly match.
//!
//! It reports one line per note, and exits 5 (`replay_diverged`) if any world differs.

use std::path::{Path, PathBuf};

use galaxy_api::engine::HOT_LOADED;
use galaxy_module::Failure;
use galaxy_module::cli::Output;
use galaxy_module::failure::code;
use galaxy_render::glam::Vec2;
use galaxy_render::{Renderer, RgbaImage};
use galaxy_sim::recording::RecEvent;
use galaxy_sim::{BuildStamp, Recording, Replay};
use galaxy_view::{Moment, View};
use serde_json::{Value, json};

/// One note to check, in tick order.
struct Note {
    number: usize,
    tick: u64,
    text: String,
    hash: Option<String>,
    moment: Option<Moment>,
    frame: Option<PathBuf>,
}

fn notes(rec: &Recording, dir: &Path) -> Vec<Note> {
    let mut out: Vec<Note> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Note {
                tick,
                text,
                hash,
                camera,
                frame,
            } => Some((tick, text, hash, camera, frame)),
            _ => None,
        })
        .enumerate()
        .map(|(i, (tick, text, hash, camera, frame))| Note {
            number: i + 1,
            tick: *tick,
            text: text.clone(),
            hash: hash.clone(),
            moment: camera
                .as_ref()
                .and_then(|c| serde_json::from_value(c.clone()).ok()),
            frame: frame
                .as_ref()
                .and_then(|f| f["file"].as_str())
                .map(|f| dir.join(f)),
        })
        .collect();
    out.sort_by_key(|n| (n.tick, n.number));
    out
}

/// Draws `moment` from the world the replay stands on.
fn draw(
    r: &mut Renderer,
    v: &mut View,
    replay: &Replay,
    moment: &Moment,
) -> Result<RgbaImage, String> {
    r.resize(moment.width, moment.height);
    r.set_scale_factor(moment.scale);
    v.eye.view = moment.zoom;
    // Each note is drawn from its own moment only: no course carried over from drawing
    // an earlier note, and the one it shows as old as it was.
    v.courses = galaxy_view::course::Courses::starting_at(moment.course_age);
    let logical = Vec2::new(moment.width as f32, moment.height as f32) / moment.scale;
    let scene = v.scene(&replay.session.world, moment.alpha, logical, moment.time);
    r.render_to_rgba(&scene)
        .map_err(|e| format!("drawing failed: {e}"))
}

pub fn run(path: &Path) -> Result<Output, Failure> {
    let rec = Recording::read(path)
        .map_err(|e| Failure::bad_input(e).with("path", path.display().to_string()))?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let audit_dir = dir.join("audit");
    let notes = notes(&rec, &dir);
    let mut replay = Replay::new(&rec).map_err(|e| {
        Failure::bad_input(format!("the recording can't run on this engine: {e}"))
            .with("path", path.display().to_string())
    })?;
    // A renderer and view from the start, so the view knows the tick before each note
    // and draws between the two as the player saw it.
    let first = notes.iter().find_map(|n| n.moment);
    let mut renderer = match first {
        Some(m) => Some(
            Renderer::new_headless(m.width, m.height)
                .map_err(|e| Failure::runtime(format!("no GPU to draw the notes again: {e}")))?,
        ),
        None => None,
    };
    let mut view = renderer
        .as_mut()
        .map(|r| View::new(r, replay.session.modules(), replay.session.world.seed));
    let mut report = Vec::new();
    let mut diverged = false;
    for note in &notes {
        while replay.session.world.tick < note.tick {
            if let Some(v) = &mut view {
                v.before_step(&replay.session.world);
            }
            let facts = replay.session.world.facts.len();
            replay.step().map_err(Failure::runtime)?;
            let hot_loaded = replay.session.world.facts[facts..]
                .iter()
                .any(|f| f.kind == HOT_LOADED);
            if let (true, Some(v), Some(r)) = (hot_loaded, &mut view, &mut renderer) {
                v.reload(Some(r), replay.session.modules());
            }
        }
        let hash = replay.session.world.hash();
        let identical = note.hash.as_ref().is_none_or(|h| *h == hash);
        diverged |= !identical;
        let mut line = json!({
            "note": note.number,
            "tick": note.tick,
            "text": note.text,
            "identical": identical,
        });
        if let (Some(moment), Some(r), Some(v)) = (&note.moment, &mut renderer, &mut view) {
            let image = draw(r, v, &replay, moment).map_err(Failure::runtime)?;
            std::fs::create_dir_all(&audit_dir).map_err(|e| {
                Failure::runtime(format!("can't make {}: {e}", audit_dir.display()))
            })?;
            let out = audit_dir.join(format!("{:04}.png", note.number));
            image
                .save_png(&out)
                .map_err(|e| Failure::runtime(e.to_string()))?;
            line["drawn"] = json!(out.display().to_string());
            let again = v.moment(
                moment.alpha,
                moment.time,
                (moment.width, moment.height),
                moment.scale,
            );
            line["camera_matches"] = json!(again.is_some_and(|a| a.same_camera(moment)));
            if let Some(frame) = &note.frame {
                line["seen"] = json!(frame.display().to_string());
                match RgbaImage::load_png(frame) {
                    Ok(seen) => {
                        if let Some((mean, share)) = image.difference(&seen) {
                            line["mean_difference"] = json!((mean * 1000.0).round() / 1000.0);
                            line["pixels_differing"] = json!((share * 10000.0).round() / 10000.0);
                        }
                    }
                    Err(e) => line["seen_error"] = json!(e.to_string()),
                }
            }
        }
        report.push(line);
    }
    let out = json!({
        "ok": !diverged,
        "command": "audit",
        "recording": path.display().to_string(),
        "builds": galaxy_sim::recording::builds(
            &rec.build,
            &BuildStamp::current(),
            "drew",
            &format!("galaxy-gm audit {}", path.display()),
        ),
        "notes": report,
    });
    if diverged {
        let mut failure = Failure::new(
            code::REJECTED,
            "replay_diverged",
            "the replay's world differs from the recording's at a note; was it made by \
             another build?",
        );
        if let Value::Object(fields) = out {
            failure.fields = fields;
        }
        return Err(failure);
    }
    Ok(Output::Json(out))
}
