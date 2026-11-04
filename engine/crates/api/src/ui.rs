//! UI sound cues: what the player just did to a screen, as the view saw it.
//!
//! Presentation only. The view notices an action (a hover, a press, a drag), the game
//! plays the module's sound for it at once and records the cue beside the session's other
//! presentation events, so an offline mix of the recording hears the same thing. The world
//! never sees a cue, and nothing here enters a hash or a save.
use serde::{Deserialize, Serialize};

/// The UI actions a module can give a sound, as `audio.toml` names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiAction {
    /// The pointer entered something a click acts on.
    Hover,
    /// A click on a ready button, a bubble, a model target or a speaker.
    Press,
    /// A panel, list or bubble appeared.
    Open,
    /// A panel, list or bubble went away.
    Close,
    /// A drag source was picked up.
    Drag,
    /// A held value was let go: onto a target that accepts it, or back home.
    Drop,
    /// A click on a resting button, or a value let go on a target that refuses it.
    Deny,
}

impl UiAction {
    pub const ALL: [UiAction; 7] = [
        UiAction::Hover,
        UiAction::Press,
        UiAction::Open,
        UiAction::Close,
        UiAction::Drag,
        UiAction::Drop,
        UiAction::Deny,
    ];

    pub fn name(self) -> &'static str {
        match self {
            UiAction::Hover => "hover",
            UiAction::Press => "press",
            UiAction::Open => "open",
            UiAction::Close => "close",
            UiAction::Drag => "drag",
            UiAction::Drop => "drop",
            UiAction::Deny => "deny",
        }
    }
}

/// The widget kinds a UI sound can be set for (`[ui.kinds.<kind>]`).
pub const UI_KINDS: &[&str] = &[
    "button",
    "bubble",
    "target",
    "speaker",
    "drag_source",
    "panel",
    "scroll",
];

/// One UI action on one widget. `time` is the screen clock in seconds (tick plus
/// interpolation, as screen motion uses), which spaces hover sounds and seeds variation;
/// `tick` is the world tick it happened after.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiCue {
    pub tick: u64,
    pub time: f32,
    pub action: UiAction,
    /// The widget's kind: one of [`UI_KINDS`].
    pub kind: String,
    /// The widget's look, when it has one.
    #[serde(default)]
    pub look: Option<String>,
    /// The widget's path (a target's widget path, a speaker's talk ID).
    pub widget: String,
}
