//! Choice 3: how people talk. Key `3` cycles the variants.
//!
//! A screen says who speaks and what (`ui.talk{ who = "ilse", text = ..., more = ...,
//! on = "talk_next" }`); the talk variant decides how that shows. When the screen runs,
//! [`Talk::widgets`] turns the line into ordinary widgets (a bubble on the person, a
//! portrait panel, a column); every frame [`Talk::frame`] may move the view (a camera
//! that frames the speaker).
//!
//! Whatever a variant does, the speaker's face must stay on screen and uncovered while
//! their line shows: the lab checks it every frame and `--shots` fails if it breaks.
//!
//! To add a variant: a new file here with a type that implements [`Talk`], and one line
//! in [`all`]. Nothing outside this folder needs to change.

mod bubble;

use render::Renderer;
use render::glam::Vec2;
use serde_json::Value as Json;

use crate::places::Places;
use crate::stage::Shown;
use crate::widgets::W;
use crate::world::World;

/// A line being said, with the speaker as the place model knows them.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The speaker's person id: their anchors are `person.<who>` (over the head) and
    /// `person.<who>.face`.
    pub who: String,
    pub name: String,
    pub role: String,
    pub text: String,
    /// More lines follow.
    pub more: bool,
    /// The event a click on the line sends, and its args.
    pub on: Option<String>,
    pub args: Json,
    /// Lines said before this one in the same talk, oldest first.
    #[allow(dead_code)] // for variants (a text column)
    pub before: Vec<String>,
}

pub trait Talk {
    /// The variant's name, by technique: "bubble", "portrait", ...
    fn name(&self) -> &'static str;
    /// Once at start and after F5, for every variant (not only the picked one): upload
    /// any meshes of its own.
    fn setup(&mut self, _r: &mut Renderer, _world: &World, _model: &Places) {}
    /// The widgets that show this line, when the screen runs (not per frame).
    fn widgets(&self, line: &Line) -> Vec<W>;
    /// Each frame, while in a place: may change the view, for instance its camera, with
    /// the line being said if there is one. `viewport` is the screen in logical pixels.
    fn frame(&mut self, _shown: &mut Shown, _line: Option<&Line>, _viewport: Vec2, _dt: f32) {}
}

/// Every variant, in the order the key cycles them. The first is the baseline.
pub fn all() -> Vec<Box<dyn Talk>> {
    vec![Box::new(bubble::Bubble)]
}
