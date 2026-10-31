//! The player's sound settings: a small TOML file next to the saves.
//!
//! Volumes are 0.0 (off) to 1.0 (full) and multiply down the mixer: master, then the
//! UI, world and radio tracks under it. A missing file means the defaults (and is written so
//! the player can find it); a broken one means the defaults too, and is left alone for
//! the whole session. Changes in play are written by a [`Writer`] on its own thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

/// The settings file's name, in the save folder.
pub const FILE: &str = "sound.toml";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub master: f32,
    pub ui: f32,
    pub world: f32,
    /// The radio's music and static, under the cues.
    pub radio: f32,
    /// N in the game toggles this.
    pub muted: bool,
    /// The radio's Off button (or R past the last channel) turns this off.
    pub radio_on: bool,
}

impl Default for Settings {
    /// On, at a modest level, with the UI under the world.
    fn default() -> Self {
        Self {
            master: 0.5,
            ui: 0.6,
            world: 0.8,
            radio: 0.5,
            muted: false,
            radio_on: true,
        }
    }
}

/// What reading the settings file found.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// Something about the file worth telling the player.
    pub note: Option<String>,
    /// False when the file is there but couldn't be read or parsed. It's the player's,
    /// and may hold settings they can fix, so nothing writes over it.
    pub writable: bool,
}

impl Settings {
    /// Reads the file. Missing: the defaults, written out. Unreadable: the defaults, a
    /// note saying why, and not [`Loaded::writable`].
    pub fn load_or_create(path: &Path) -> Loaded {
        let broken = |e: String| Loaded {
            settings: Self::default(),
            note: Some(format!(
                "{}: {e}; using the defaults and leaving the file alone",
                path.display()
            )),
            writable: false,
        };
        match std::fs::read_to_string(path) {
            Ok(text) => match Self::parse(&text) {
                Ok(settings) => Loaded {
                    settings,
                    note: None,
                    writable: true,
                },
                Err(e) => broken(e),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let settings = Self::default();
                let note = settings
                    .save(path)
                    .err()
                    .map(|e| format!("{}: {e}", path.display()));
                Loaded {
                    settings,
                    note,
                    writable: true,
                }
            }
            Err(e) => broken(e.to_string()),
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str::<Self>(text)
            .map(Self::clamped)
            .map_err(|e| e.message().to_owned())
    }

    /// The file's text, with a comment saying what the numbers mean.
    pub fn to_text(&self) -> String {
        format!(
            "# The Lantern Reach: sound. Volumes go from 0.0 (off) to 1.0 (full).\n\
             # N in the game mutes and unmutes. The radio's Off button (or R) sets radio_on.\n\
             master = {:.2}\nui = {:.2}\nworld = {:.2}\nradio = {:.2}\nmuted = {}\n\
             radio_on = {}\n",
            self.master, self.ui, self.world, self.radio, self.muted, self.radio_on
        )
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }

    /// Volumes forced into 0..=1 (NaN counts as 0).
    pub fn clamped(self) -> Self {
        let c = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        Self {
            master: c(self.master),
            ui: c(self.ui),
            world: c(self.world),
            radio: c(self.radio),
            muted: self.muted,
            radio_on: self.radio_on,
        }
    }
}

/// Writes the settings file on a background thread, so a change in play never waits on
/// the disk. Settings queued while one is being written are coalesced: only the newest
/// is written. Dropping it waits for the queued write.
pub struct Writer {
    tx: Option<Sender<Settings>>,
    handle: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<String>>>,
}

impl Writer {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<Settings>();
        let error = Arc::new(Mutex::new(None));
        let errors = error.clone();
        let shown = path.display().to_string();
        let spawned = std::thread::Builder::new()
            .name("sound-settings".into())
            .spawn(move || {
                while let Ok(mut s) = rx.recv() {
                    while let Ok(newer) = rx.try_recv() {
                        s = newer;
                    }
                    let result = s.save(&path).err().map(|e| format!("{shown}: {e}"));
                    if let Ok(mut slot) = errors.lock() {
                        *slot = result;
                    }
                }
            });
        let handle = match spawned {
            Ok(h) => Some(h),
            Err(e) => {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(format!("no thread to write the sound settings: {e}"));
                }
                None
            }
        };
        Self {
            tx: handle.is_some().then_some(tx),
            handle,
            error,
        }
    }

    /// Queues `s` to be written. Returns at once.
    pub fn write(&self, s: Settings) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(s);
        }
    }

    /// The last write's error, if it failed.
    pub fn last_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    /// Waits for every queued write and stops the thread.
    pub fn finish(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.finish();
    }
}

/// A 0..=1 volume as decibels for kira (-60 dB is kira's silence).
pub fn volume_db(v: f32) -> f32 {
    if v <= 0.001 {
        -60.0
    } else {
        (20.0 * v.log10()).max(-60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saves").join(FILE);
        let loaded = Settings::load_or_create(&path);
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.note.is_none(), "{:?}", loaded.note);
        assert!(loaded.writable);
        assert!(path.exists(), "the defaults are written out");
        let muted = Settings {
            muted: true,
            ..loaded.settings
        };
        let mut writer = Writer::start(path.clone());
        writer.write(Settings::default());
        writer.write(muted);
        writer.finish();
        assert!(writer.last_error().is_none());
        assert_eq!(Settings::load_or_create(&path).settings, muted);
    }

    #[test]
    fn a_broken_file_means_the_defaults_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, "master = \"loud\"\n").unwrap();
        let loaded = Settings::load_or_create(&path);
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.note.is_some());
        assert!(!loaded.writable, "nothing may write over it");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "master = \"loud\"\n"
        );
    }

    #[test]
    fn partial_files_fill_in_and_volumes_clamp() {
        let s = Settings::parse("master = 3.0\nmuted = true\n").unwrap();
        assert_eq!(s.master, 1.0);
        assert!(s.muted);
        assert_eq!(s.world, Settings::default().world);
        // A file from before the radio gets its defaults.
        let old = Settings::parse("master = 0.5\nui = 0.6\nworld = 0.8\nmuted = false\n").unwrap();
        assert_eq!(old.radio, Settings::default().radio);
        assert!(old.radio_on);
        let off = Settings {
            radio_on: false,
            ..Settings::default()
        };
        assert_eq!(Settings::parse(&off.to_text()).unwrap(), off);
    }

    #[test]
    fn the_default_is_on_and_modest() {
        let s = Settings::default();
        assert!(!s.muted);
        assert!(volume_db(s.master) <= -6.0 + 1e-3);
        assert!(s.ui < s.world, "UI sits below the world");
        assert!(s.radio < s.world, "the radio sits below the world");
        assert!(s.radio_on);
        assert_eq!(volume_db(0.0), -60.0);
        assert!((volume_db(1.0)).abs() < 1e-6);
    }
}
