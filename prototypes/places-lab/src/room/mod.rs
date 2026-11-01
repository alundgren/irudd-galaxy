//! Choice 2: what a room is. Key `2` cycles the variants.
//!
//! A room variant decides how an interior looks while you are in it: the camera, what
//! is drawn, and so where its people and spots land on screen. It draws with the
//! [`Stage`]: `stage.set(place, transform, shown)` puts the set and its people anywhere,
//! `stage.exterior(..)` gives the station from outside, `stage.set_camera(place)` the
//! set's own fixed camera.
//!
//! To add a variant: a new file here with a type that implements [`Room`], and one line
//! in [`all`]. Nothing outside this folder needs to change.

mod inside;
mod layers;
mod staged;

use render::Renderer;

use crate::places::{Place, Places};
use crate::stage::{Shown, Stage};
use crate::widgets::W;
use crate::world::World;

pub trait Room {
    /// The variant's name, by technique: "staged", "layers", ...
    fn name(&self) -> &'static str;
    /// Once at start and after F5, for every variant (not only the picked one): upload
    /// any meshes of its own.
    fn setup(&mut self, _r: &mut Renderer, _world: &World, _model: &Places) {}
    /// Time passes (while this variant is picked).
    fn step(&mut self, _dt: f32, _model: &Places) {}
    /// The view of an interior while you are in it.
    fn view(&self, stage: &Stage, place: &Place) -> Shown;
    /// Widgets of the room's own (scene text, captions), added under the screen's when
    /// it runs, while the player is in `place`.
    fn widgets(&self, _place: &Place, _model: &Places) -> Vec<W> {
        Vec::new()
    }
}

/// Every variant, in the order the key cycles them. The first is the baseline.
pub fn all() -> Vec<Box<dyn Room>> {
    vec![
        Box::new(staged::Staged),
        Box::new(layers::Layers::default()),
        Box::new(inside::Inside::default()),
    ]
}
