//! A played session's folder, and F8 notes.
//!
//! Every `galaxy play` keeps its session in a folder of its own under the sessions folder
//! (`galaxy_module::install::sessions_dir`):
//!
//! ```text
//! <sessions>/2026-11-01-142233-drift/
//!   recording.json      the recording, written at every note and when the window closes
//!   save.db             the world when the window closed
//!   notes/0001.png      the frame that was shown when each note was taken
//! ```
//!
//! F8 pauses the game and opens a note box over the frame that was shown; Enter keeps the
//! note (its text, tick, the world's hash, the camera and that frame), Esc drops it. The
//! game carries on from the same tick either way. `galaxy-gm audit` replays the recording
//! to each note and draws the moment again.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use galaxy_render::glam::Vec2;
use galaxy_render::{Color, Panel, Rect, RgbaImage, Scene, Text, defaults};
use galaxy_view::Moment;

/// The longest note, in characters.
pub const MAX_NOTE: usize = 2000;

/// Days since 1970-01-01 as a civil (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `2026-11-01-142233` for now, in UTC.
fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}",
        t / 3600,
        t / 60 % 60,
        t % 60
    )
}

/// A new, empty folder for a session of `modules` under `root`.
pub fn session_dir(root: &Path, modules: &[&str]) -> std::io::Result<PathBuf> {
    let base = format!("{}-{}", stamp(), modules.join("-"));
    for n in 0..100 {
        let name = if n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let dir = root.join(name);
        if !dir.exists() {
            std::fs::create_dir_all(dir.join("notes"))?;
            return Ok(dir);
        }
    }
    Err(std::io::Error::other(
        "too many sessions started this second",
    ))
}

/// A note being written: the game is paused on `tick`.
pub struct NoteBox {
    pub text: String,
    pub frame: RgbaImage,
    pub moment: Moment,
}

impl NoteBox {
    /// Adds typed text, dropping control characters.
    pub fn type_text(&mut self, typed: &str) {
        for c in typed.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < MAX_NOTE {
                self.text.push(c);
            }
        }
    }

    pub fn backspace(&mut self) {
        self.text.pop();
    }

    /// Draws the box at the bottom of the frame, over everything.
    pub fn draw(&self, scene: &mut Scene, logical: Vec2) {
        let width = (logical.x - 48.0).clamp(240.0, 760.0);
        let height = 112.0;
        let x = (logical.x - width) * 0.5;
        let y = logical.y - height - 24.0;
        let layer = &mut scene.overlay.upper[2];
        layer.panels.push(Panel {
            fill: defaults::PAPER,
            ..Panel::new(Rect::new(x, y, width, height))
        });
        layer.texts.push(
            Text::new(
                "Note (paused). Enter keeps it, Esc drops it.",
                Vec2::new(x + 18.0, y + 12.0),
                15.0,
            )
            .color(Color::BLACK)
            .weight(600),
        );
        layer.texts.push(
            Text::new(
                format!("{}|", self.text),
                Vec2::new(x + 18.0, y + 40.0),
                18.0,
            )
            .color(Color::BLACK)
            .wrap(width - 36.0),
        );
    }
}

/// The file a note's frame goes in, inside the session folder: `notes/0001.png`.
pub fn frame_file(number: usize) -> String {
    format!("notes/{number:04}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_758), (2026, 11, 1));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(-1), (1969, 12, 31));
    }

    #[test]
    fn a_session_folder_is_new_each_time() {
        let tmp = tempfile::tempdir().unwrap();
        let a = session_dir(tmp.path(), &["aa", "bb"]).unwrap();
        let b = session_dir(tmp.path(), &["aa", "bb"]).unwrap();
        assert_ne!(a, b);
        assert!(a.join("notes").is_dir());
        assert!(a.file_name().unwrap().to_str().unwrap().ends_with("aa-bb"));
    }

    #[test]
    fn typing_keeps_printable_text() {
        let mut n = NoteBox {
            text: String::new(),
            frame: RgbaImage {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            },
            moment: Moment {
                alpha: 0.0,
                time: 0.0,
                width: 1,
                height: 1,
                scale: 1.0,
                zoom: None,
                course_age: None,
                eye: [0.0; 3],
                target: [0.0; 3],
            },
        };
        n.type_text("the dock\tfeels\u{8} far");
        assert_eq!(n.text, "the dockfeels far");
        n.backspace();
        assert_eq!(n.text, "the dockfeels fa");
    }
}
