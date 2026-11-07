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
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

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
        // Made, not checked then made: sessions starting together (bots run side by
        // side) each get a folder of their own.
        std::fs::create_dir_all(root)?;
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                std::fs::create_dir(dir.join("notes"))?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
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
    pub editor: NoteEditor,
}

#[derive(Default)]
pub struct NoteEditor {
    offset: f32,
    pub follow: bool,
    measure: galaxy_render::text::TextMeasure,
    rail: Option<(Rect, Rect, f32)>,
    grab: Option<f32>,
}

impl NoteBox {
    pub fn pointer_down(&mut self, point: Vec2) {
        let Some((track, thumb, _)) = self.editor.rail else {
            return;
        };
        if point.x < track.x
            || point.x >= track.x + track.w
            || point.y < track.y
            || point.y >= track.y + track.h
        {
            return;
        }
        self.editor.grab = Some(if point.y >= thumb.y && point.y < thumb.y + thumb.h {
            point.y - thumb.y
        } else {
            thumb.h * 0.5
        });
        self.pointer_move(point);
    }

    pub fn pointer_move(&mut self, point: Vec2) {
        if let (Some(grab), Some((track, thumb, max))) = (self.editor.grab, self.editor.rail) {
            self.editor.follow = false;
            self.editor.offset =
                ((point.y - track.y - grab) / (track.h - thumb.h).max(1.0) * max).clamp(0.0, max);
        }
    }

    pub fn pointer_up(&mut self) {
        self.editor.grab = None;
    }

    pub fn scroll(&mut self, pixels: f32) {
        self.editor.offset = (self.editor.offset + pixels).max(0.0);
        self.editor.follow = false;
    }

    /// Adds typed text, dropping control characters.
    pub fn type_text(&mut self, typed: &str) {
        self.editor.follow = true;
        self.editor.measure.clear();
        for c in typed.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < MAX_NOTE {
                self.text.push(c);
            }
        }
    }

    /// Adds pasted text: line breaks and tabs become spaces, so pasted lines don't run
    /// together.
    pub fn paste(&mut self, pasted: &str) {
        let flat: String = pasted
            .replace("\r\n", " ")
            .chars()
            .map(|c| {
                if matches!(c, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        self.type_text(&flat);
    }

    pub fn backspace(&mut self) {
        self.text.pop();
        self.editor.follow = true;
        self.editor.measure.clear();
    }

    /// Draws the box at the bottom of the frame, over everything.
    pub fn draw(&mut self, scene: &mut Scene, logical: Vec2) {
        let width = (logical.x - 48.0).clamp(240.0, 760.0);
        let height = (logical.y * 0.42).clamp(160.0, 300.0);
        let x = (logical.x - width) * 0.5;
        let y = logical.y - height - 24.0;
        let layer = &mut scene.overlay.upper[1];
        layer.panels.push(Panel {
            fill: defaults::PAPER,
            ..Panel::new(Rect::new(x, y, width, height))
        });
        layer.texts.push(
            Text::new(
                "Note (paused). Enter keeps it, Esc drops it. Wheel / PgUp / PgDn scroll.",
                Vec2::new(x + 18.0, y + 12.0),
                15.0,
            )
            .color(Color::BLACK)
            .weight(600),
        );
        let clip = Rect::new(x + 18.0, y + 42.0, width - 84.0, height - 60.0);
        let mut text = Text::new(format!("{}|", self.text), Vec2::ZERO, 18.0)
            .color(Color::BLACK)
            .wrap(clip.w);
        let text_height = self.editor.measure.measure(&text, 1.0).y;
        let max = (text_height - clip.h).max(0.0);
        if self.editor.follow {
            self.editor.offset = max;
        }
        self.editor.offset = self.editor.offset.clamp(0.0, max);
        text.pos = Vec2::new(clip.x, clip.y - self.editor.offset);
        self.editor.rail = None;
        if max > 0.0 {
            let h = (clip.h * clip.h / text_height).max(44.0).min(clip.h);
            let track = Rect::new(x + width - 50.0, clip.y, 32.0, clip.h);
            let thumb = Rect::new(
                track.x,
                clip.y + (clip.h - h) * self.editor.offset / max,
                track.w,
                h,
            );
            self.editor.rail = Some((track, thumb, max));
            let mut rail = Panel::new(track);
            rail.fill = Color::hex(0xe4dccb);
            rail.border_px = 0.0;
            rail.shadow = false;
            let mut handle = Panel::new(thumb);
            handle.fill = Color::hex(0xb69b78);
            handle.border_px = 1.0;
            handle.shadow = false;
            layer.panels.extend([rail, handle]);
        }
        let text_layer = &mut scene.overlay.upper[2];
        text_layer.clip = Some(clip);
        text_layer.texts.push(text);
    }
}

/// Whether a key press is the paste chord: Cmd+V on macOS, Ctrl+V elsewhere, or
/// Shift+Insert. V is matched by its place on the keyboard or by the letter it types, so
/// every layout pastes. Ctrl+Alt is AltGr on some layouts, which types, not pastes.
pub fn is_paste(key: &Key, physical: PhysicalKey, modifiers: ModifiersState) -> bool {
    let chord = if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key() && !modifiers.alt_key()
    };
    let v = physical == PhysicalKey::Code(KeyCode::KeyV)
        || matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("v"));
    (chord && v) || (modifiers.shift_key() && matches!(key, Key::Named(NamedKey::Insert)))
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

    fn note() -> NoteBox {
        NoteBox {
            text: String::new(),
            frame: RgbaImage {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            },
            editor: Default::default(),
            moment: Moment {
                screen_style: None,
                screens: Default::default(),
                alpha: 0.0,
                time: 0.0,
                width: 1,
                height: 1,
                scale: 1.0,
                zoom: None,
                course_age: None,
                still: false,
                eye: [0.0; 3],
                target: [0.0; 3],
            },
        }
    }

    #[test]
    fn the_paste_chord_pastes_and_lines_become_spaces() {
        let chord = if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        };
        let v = Key::Character("v".into());
        let code = PhysicalKey::Code(KeyCode::KeyV);
        assert!(is_paste(&v, code, chord));
        // A layout where that key types another letter still pastes, and so does a
        // layout that puts V elsewhere.
        assert!(is_paste(&Key::Character("м".into()), code, chord));
        assert!(is_paste(&v, PhysicalKey::Code(KeyCode::KeyB), chord));
        assert!(
            !is_paste(&v, code, ModifiersState::empty()),
            "plain v types"
        );
        assert!(is_paste(
            &Key::Named(NamedKey::Insert),
            PhysicalKey::Code(KeyCode::Insert),
            ModifiersState::SHIFT
        ));
        let mut n = note();
        n.type_text("seen: ");
        n.paste("the dock\r\nfeels\tfar\n");
        assert_eq!(n.text, "seen: the dock feels far ");
    }

    #[test]
    fn typing_keeps_printable_text() {
        let mut n = note();
        n.type_text("the dock\tfeels\u{8} far");
        assert_eq!(n.text, "the dockfeels far");
        n.backspace();
        assert_eq!(n.text, "the dockfeels fa");
        n.type_text(&" A long note that wraps onto many lines.".repeat(60));
        let mut scene = Scene::new(galaxy_render::Camera::top_down(Vec2::ZERO, 60.0));
        n.draw(&mut scene, Vec2::new(640.0, 360.0));
        assert!(n.editor.offset > 0.0);
        let bottom = n.editor.offset;
        let layer = &scene.overlay.upper[2];
        let clip = layer.clip.unwrap();
        let text = &layer.texts[0];
        let h = n.editor.measure.measure(text, 1.0).y;
        assert!(
            (text.pos.y + h - clip.y - clip.h).abs() < 0.01,
            "caret is visible"
        );
        n.scroll(-10000.0);
        n.draw(
            &mut Scene::new(galaxy_render::Camera::top_down(Vec2::ZERO, 60.0)),
            Vec2::new(640.0, 360.0),
        );
        assert_eq!(n.editor.offset, 0.0, "start of the note can be read");
        n.type_text("!");
        n.draw(
            &mut Scene::new(galaxy_render::Camera::top_down(Vec2::ZERO, 60.0)),
            Vec2::new(640.0, 360.0),
        );
        assert!(n.editor.offset >= bottom, "typing brings the caret back");
        let (track, thumb, _) = n.editor.rail.unwrap();
        n.pointer_down(thumb.center());
        n.pointer_move(Vec2::new(track.center().x, track.y - 100.0));
        assert_eq!(
            n.editor.offset, 0.0,
            "thumb capture reaches the top outside the track"
        );
        n.pointer_up();
        n.pointer_move(Vec2::new(track.center().x, track.y + track.h));
        assert_eq!(n.editor.offset, 0.0, "release ends capture");
    }
    #[test]
    #[ignore = "needs a GPU: inspect the long F8 editor at both ends"]
    fn draw_long_note_editor() {
        let mut n = note();
        n.type_text(&"A long note must remain readable all the way to the end. ".repeat(30));
        let out = std::env::temp_dir().join("galaxy-note-editor");
        std::fs::create_dir_all(&out).unwrap();
        for (width, height) in [(640, 360), (1280, 720)] {
            let mut renderer = galaxy_render::Renderer::new_headless(width, height).unwrap();
            for end in ["bottom", "top"] {
                if end == "top" {
                    n.scroll(-10000.0);
                } else {
                    n.editor.follow = true;
                }
                let mut scene = Scene::new(galaxy_render::Camera::top_down(Vec2::ZERO, 60.0));
                n.draw(&mut scene, Vec2::new(width as f32, height as f32));
                renderer
                    .render_to_rgba(&scene)
                    .unwrap()
                    .save_png(out.join(format!("{width}-{end}.png")))
                    .unwrap();
            }
        }
        eprintln!("note editor pictures: {}", out.display());
    }
}
