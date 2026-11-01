//! Choice 4: life. Key `4` cycles the variants.
//!
//! A life variant decides how people live in their places: what a story moment's move
//! looks like (the Luau calls `world.move(person, place, state, spot)`), what happens
//! as time passes and when the player arrives somewhere, how each figure stands, and
//! anything it wants on the screen (what changed since you were last here).
//!
//! To add a variant: a new file here with a type that implements [`Life`], and one line
//! in [`all`]. Nothing outside this folder needs to change.

mod day;
mod returning;
mod states;
mod still;

use render::{MeshHandle, Renderer};
use render::glam::Mat4;
use serde_json::Value as Json;

use crate::figures::Pose;
use crate::places::{Person, Places};
use crate::widgets::W;
use crate::world::World;

pub trait Life {
    /// The variant's name, by technique: "still", "states", ...
    fn name(&self) -> &'static str;
    /// Once at start and after F5, for every variant (not only the picked one): upload
    /// any meshes of its own.
    fn setup(&mut self, _r: &mut Renderer, _world: &World, _model: &Places) {}
    /// A story moment moves someone. The baseline moves them at once.
    fn moved(
        &mut self,
        model: &mut Places,
        person: &str,
        place: Option<&str>,
        state: Option<&str>,
        spot: Option<&str>,
    ) {
        model.set_person(person, place, state, spot);
    }
    /// The player arrived in a place (the move's view has landed).
    fn arrived(&mut self, _model: &mut Places, _place: &str) {}
    /// Key 4 is switching away from this variant: finish what is under way (walks land,
    /// story moves held back happen), so the next variant starts from a settled model.
    fn leave(&mut self, _model: &mut Places) {}
    /// Time passes.
    fn step(&mut self, _model: &mut Places, _dt: f32) {}
    /// How a figure stands now. `t` is seconds, offset per person.
    fn pose(&self, who: &Person, t: f32, talking: bool) -> Pose {
        idle(who, t, talking)
    }
    /// Widgets to add to the screen, when it runs.
    fn widgets(&self, _model: &Places) -> Vec<W> {
        Vec::new()
    }
    /// What the Luau sees of the variant, as `world.life` (null: nothing).
    fn world(&self, _model: &Places) -> Json {
        Json::Null
    }
    /// The player did something: an event the screen sent (`sell`, `fit`, ...), after
    /// the Luau handled it.
    fn event(&mut self, _model: &mut Places, _on: &str, _args: &Json) {}
    /// Things the variant adds to a room's set: meshes and where they stand, set space.
    fn props(&self, _model: &Places, _place: &str) -> Vec<(MeshHandle, Mat4)> {
        Vec::new()
    }
    /// Someone is on the move where the player can see: the scripted run waits for it
    /// to finish before a shot.
    fn settling(&self, _model: &Places) -> bool {
        false
    }
}

/// A little idle motion: breathing, a slow look round, a nod while talking; sitting
/// lower while eating.
pub fn idle(who: &Person, t: f32, talking: bool) -> Pose {
    let mut p = Pose::still();
    p.breathe = 1.0 + (t * 1.7).sin() * 0.012;
    p.sway = (t * 0.6).sin() * 0.025;
    p.head_turn = (t * 0.37).sin() * 0.35 + (t * 0.91).sin() * 0.1;
    if talking {
        p.head_nod = (t * 7.0).sin() * 0.07;
        p.head_turn *= 0.3;
        p.lift = ((t * 3.5).sin() * 0.5 + 0.5) * 0.03;
    }
    if who.state == "eat" {
        p.lift -= 0.35;
    }
    p
}

/// Every variant, in the order the key cycles them. The first is the baseline.
pub fn all() -> Vec<Box<dyn Life>> {
    let clock = day::Shared::default();
    vec![
        Box::new(still::Still),
        Box::new(states::States::new(clock.clone())),
        Box::new(returning::Return::new(clock)),
    ]
}
