//! `galaxy-audio`: the game's sound, behind a small wrapper around kira.
//!
//! The slice ships silent (the kickoff decision on PR #11): [`Audio::open`] starts a
//! kira manager on the default output device and plays nothing. When sounds arrive
//! they get methods here, so the app never talks to kira directly.
//!
//! Sound is never required. If no device opens (no sound card, a headless machine, a
//! busy driver) `open` returns a silent [`Audio`] and the game carries on.

use kira::{AudioManager, AudioManagerSettings, DefaultBackend};

/// The game's audio. Silent when no device could be opened.
pub struct Audio {
    manager: Option<AudioManager<DefaultBackend>>,
    /// Why there is no device, when there isn't one.
    problem: Option<String>,
}

impl Audio {
    /// Opens the default output device. Never fails: without a device the game is
    /// silent and [`Audio::problem`] says why.
    pub fn open() -> Self {
        match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => Self {
                manager: Some(manager),
                problem: None,
            },
            Err(e) => Self {
                manager: None,
                problem: Some(e.to_string()),
            },
        }
    }

    /// No device at all; every call is a no-op. For tests and `galaxy shot`.
    pub fn silent() -> Self {
        Self {
            manager: None,
            problem: None,
        }
    }

    /// True when an output device is open.
    pub fn has_device(&self) -> bool {
        self.manager.is_some()
    }

    /// Why the device didn't open, if it didn't.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_audio_has_no_device() {
        let a = Audio::silent();
        assert!(!a.has_device());
        assert!(a.problem().is_none());
    }
}
