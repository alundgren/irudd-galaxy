//! A staged 3D set with a fixed camera: the room is a stage open at the front, its
//! people standing in it, seen from where an audience would sit.

use render::glam::{Mat4, Vec3};
use render::{Camera, Scene};

use super::Room;
use crate::places::Place;
use crate::stage::{self, Shown, Stage};

pub struct Staged;

impl Room for Staged {
    fn name(&self) -> &'static str {
        "staged"
    }

    fn view(&self, stage: &Stage, place: &Place) -> Shown {
        let cam = stage
            .set_camera(&place.id)
            .unwrap_or_else(|| Camera::perspective(Vec3::new(0.0, -10.0, 5.0), Vec3::Z, 34.0));
        let mut shown = Shown::new(Scene::new(cam), &place.id);
        if let Some(set) = stage.world.sets.get(&place.id) {
            shown.scene.clear_color = set.clear;
        }
        stage::room_lights(&mut shown.scene);
        stage.set(&place.id, Mat4::IDENTITY, &mut shown);
        shown
    }
}
