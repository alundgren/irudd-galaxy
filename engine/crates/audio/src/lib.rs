//! Presentation only: decoded module clips, event/place cues, our offline mixer,
//! and CPAL device plumbing. No simulation state or audio device is needed to mix.
mod bank;
mod decode;
mod device;
mod mixer;
mod scene;

pub use bank::{Bank, FILE_KEYS};
pub use decode::{Clip, Pcm};
pub use device::Output;
pub use mixer::{Action, Bus, MAX_CHANNELS, MAX_VOICES, MixFrame, Mixer};
pub use scene::{Heard, Scene};
