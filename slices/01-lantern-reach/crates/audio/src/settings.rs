//! The player's sound settings: a small TOML file next to the saves.
//!
//! Volumes are 0.0 (off) to 1.0 (full) and multiply down the mixer: master, then the
//! UI and world tracks under it. A missing file means the defaults (and is written so
//! the player can find it); a broken one means the defaults too, and is left alone.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The settings file's name, in the save folder.
pub const FILE: &str = "sound.toml";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub master: f32,
    pub ui: f32,
    pub world: f32,
    /// N in the game toggles this.
    pub muted: bool,
}

impl Default for Settings {
    /// On, at a modest level, with the UI under the world.
    fn default() -> Self {
        Self {
            master: 0.5,
            ui: 0.6,
            world: 0.8,
            muted: false,
        }
    }
}

impl Settings {
    /// Reads the file. Missing: the defaults, written out. Unreadable: the defaults and
    /// a note saying why.
    pub fn load_or_create(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match Self::parse(&text) {
                Ok(s) => (s, None),
                Err(e) => (
                    Self::default(),
                    Some(format!("{}: {e}; using the defaults", path.display())),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let s = Self::default();
                let note = s
                    .save(path)
                    .err()
                    .map(|e| format!("{}: {e}", path.display()));
                (s, note)
            }
            Err(e) => (
                Self::default(),
                Some(format!("{}: {e}; using the defaults", path.display())),
            ),
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
             # N in the game mutes and unmutes.\n\
             master = {:.2}\nui = {:.2}\nworld = {:.2}\nmuted = {}\n",
            self.master, self.ui, self.world, self.muted
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
            muted: self.muted,
        }
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
        let (s, note) = Settings::load_or_create(&path);
        assert_eq!(s, Settings::default());
        assert!(note.is_none(), "{note:?}");
        assert!(path.exists(), "the defaults are written out");
        let muted = Settings { muted: true, ..s };
        muted.save(&path).unwrap();
        assert_eq!(Settings::load_or_create(&path).0, muted);
    }

    #[test]
    fn a_broken_file_means_the_defaults_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, "master = \"loud\"\n").unwrap();
        let (s, note) = Settings::load_or_create(&path);
        assert_eq!(s, Settings::default());
        assert!(note.is_some());
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
    }

    #[test]
    fn the_default_is_on_and_modest() {
        let s = Settings::default();
        assert!(!s.muted);
        assert!(volume_db(s.master) <= -6.0 + 1e-3);
        assert!(s.ui < s.world, "UI sits below the world");
        assert_eq!(volume_db(0.0), -60.0);
        assert!((volume_db(1.0)).abs() < 1e-6);
    }
}
