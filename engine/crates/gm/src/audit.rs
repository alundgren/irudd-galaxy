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
//! It reports one line per note, and how many pictures it compared, in words when that
//! is not all of them: a recording copied out of its session folder has no `notes/`
//! beside it, so only the world hashes can be checked. It exits 5 (`replay_diverged`) if
//! any world differs.

use std::path::{Path, PathBuf};

use galaxy_module::Failure;
use galaxy_module::cli::Output;
use galaxy_module::failure::code;
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
    let scene = v.note_scene(&replay.session.world, moment);
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
    let first = notes.iter().find_map(|n| n.moment.as_ref());
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
    let mut pictures = Pictures::default();
    for note in &notes {
        while replay.session.world.tick < note.tick {
            if let Some(v) = &mut view {
                v.before_step(&replay.session.world);
            }
            replay.step().map_err(Failure::runtime)?;
            let hot_loaded = replay.modules_changed;
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
        if note.moment.is_none() || note.frame.is_none() {
            pictures.no_frame += 1;
        }
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
                            pictures.compared += 1;
                        } else {
                            pictures.other_size += 1;
                        }
                    }
                    Err(e) => {
                        line["seen_error"] = json!(e.to_string());
                        pictures.missing.get_or_insert_with(|| frame.clone());
                    }
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
        "pictures_compared": pictures.compared,
        "pictures": pictures.summary(notes.len()),
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

/// How the pictures went: compared, or why not.
#[derive(Default)]
struct Pictures {
    compared: usize,
    /// The first frame a note named that couldn't be read.
    missing: Option<PathBuf>,
    /// Frames of another size than the moment drawn.
    other_size: usize,
    /// Notes that kept no frame or moment to draw.
    no_frame: usize,
}

impl Pictures {
    /// One plain sentence: how many of `notes` pictures were compared, and why not all.
    fn summary(&self, notes: usize) -> String {
        if notes == 0 {
            return "the recording has no notes, so there were no pictures to compare".into();
        }
        if self.compared == notes {
            return format!("compared all {notes} note pictures with the frames the player saw");
        }
        let mut out = if self.compared == 0 {
            format!("compared no pictures: of {notes} notes, none could be compared")
        } else {
            format!(
                "compared {} of {notes} note pictures; the rest could not be",
                self.compared
            )
        };
        if let Some(path) = &self.missing {
            out.push_str(&format!(
                ". The frames the player saw were not found (first: {}); a recording moved \
                 out of its session folder leaves notes/ behind, so audit it inside the \
                 session folder",
                path.display()
            ));
        }
        if self.other_size > 0 {
            out.push_str(&format!(
                ". {} frames are a different size from the moment drawn",
                self.other_size
            ));
        }
        if self.no_frame > 0 {
            out.push_str(&format!(
                ". {} notes kept no frame (from galaxy sim run, or an older recording), so \
                 only their world hash was checked",
                self.no_frame
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Pictures;
    use std::path::PathBuf;

    #[test]
    fn the_summary_says_plainly_when_no_pictures_were_compared() {
        let none = Pictures {
            missing: Some(PathBuf::from("copy/notes/0001.png")),
            ..Pictures::default()
        };
        assert!(!none.summary(3).contains("kept no frame"));
        let sim_notes = Pictures {
            no_frame: 2,
            ..Pictures::default()
        };
        assert!(sim_notes.summary(2).contains("2 notes kept no frame"));
        let text = none.summary(3);
        assert!(text.starts_with("compared no pictures"), "{text}");
        assert!(text.contains("copy/notes/0001.png"), "{text}");
        assert!(text.contains("session folder"), "{text}");
        let all = Pictures {
            compared: 2,
            ..Pictures::default()
        };
        assert_eq!(
            all.summary(2),
            "compared all 2 note pictures with the frames the player saw"
        );
        assert!(Pictures::default().summary(0).contains("no notes"));
        let some = Pictures {
            compared: 1,
            ..Pictures::default()
        };
        assert!(some.summary(2).starts_with("compared 1 of 2"));
    }
}
