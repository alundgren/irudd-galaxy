//! Choice 1: getting between places. Key `1` cycles the variants.
//!
//! When the player goes somewhere, the place model changes at once; a travel variant
//! only decides what the player sees on the way. While it is `busy` the screen is
//! hidden and clicks land the move.
//!
//! To add a variant: a new file here with a type that implements [`Travel`], and one
//! line in [`all`]. Nothing outside this folder needs to change.

mod cut;

use render::{Renderer, UiLayer};

use crate::places::Places;
use crate::stage::{Shown, Stage};
use crate::world::World;

pub trait Travel {
    /// The variant's name, by technique: "cut", "glide", ...
    fn name(&self) -> &'static str;
    /// Once at start and after F5, for every variant (not only the picked one): upload
    /// any meshes of its own.
    fn setup(&mut self, _r: &mut Renderer, _world: &World, _model: &Places) {}
    /// Starts a move from one place to another (both are place ids the player has
    /// already left and arrived in, in the model).
    fn start(&mut self, from: &str, to: &str);
    /// Moves on by `dt` seconds.
    fn step(&mut self, dt: f32);
    /// Still on the way: the screen is hidden, and a click lands the move.
    fn busy(&self) -> bool;
    /// Lands at once.
    fn finish(&mut self);
    /// The view this frame. At rest, the resting view of where the player is.
    fn compose(&self, stage: &Stage) -> Shown {
        stage.view(&stage.model.here)
    }
    /// Drawn over the screen and the scene (a title card, a fade).
    fn overlay(&self, _stage: &Stage, _layer: &mut UiLayer) {}
}

/// Every variant, in the order the key cycles them. The first is the baseline.
pub fn all() -> Vec<Box<dyn Travel>> {
    vec![Box::new(cut::Cut::default())]
}
