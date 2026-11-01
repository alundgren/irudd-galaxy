//! The interior inside the station model: you never leave the station. The room's set
//! stands where its module is, the module's hull cut away round it (the walls sliced
//! off at the top and the front, the cut painted like a section drawing), a corridor
//! runs back from it to the core, and the camera looks in over the cut toward the core,
//! so the rest of the station and the other modules stay in view beyond the back wall.
//!
//! Authoring a room costs nothing beyond its set: the hull and the corridor are built
//! from the set's floor size and where its module is.

use std::collections::HashMap;

use render::art_bible::ArtMaterial;
use render::glam::{Mat4, Vec2, Vec3};
use render::{
    ArtBible, Camera, Color, InkRole, MAX_POINT_LIGHTS, MeshBuilder, MeshHandle, MeshInstance,
    Renderer, Scene, palette,
};

use super::Room;
use crate::places::{Kind, Place, Places};
use crate::sets::WALL_H;
use crate::stage::{Shown, Stage};
use crate::world::World;

/// The vertical field of view, degrees.
const FOV: f32 = 40.0;
/// The hull's skin round the set: how far out from the set's walls, how thick, how
/// high.
const GAP: f32 = 0.3;
const SKIN: f32 = 0.35;
const HULL_H: f32 = WALL_H + 0.25;
/// Where the station's core ends, from its centre (the modules' struts reach this far).
const CORE: f32 = 12.0;

fn hex(h: u32) -> Color {
    Color::hex(h)
}

/// How a room is looked into: the floor from `x0` to `x1` lands left of the room panel,
/// and the camera stands `down` degrees above the floor at depth `y`. It looks down as
/// far as keeps the station's core in view over the back wall, but always far enough
/// to see the cut at the front (`front`).
struct Look {
    x0: f32,
    x1: f32,
    y: f32,
    front: f32,
    down: f32,
}

fn look(set: &str, floor: Vec2) -> Look {
    // The near corners are bigger in perspective: a margin keeps them on screen.
    let (hw, out) = (floor.x * 0.5, GAP + SKIN);
    let front = -floor.y * 0.5 - 0.2;
    let (x0, x1, y, front, down) = match set {
        // The cradle is wide and deep: its left part, Orrin and the Wren, from higher up
        // so the sockets stack up the screen with room for their labels. Its front cut
        // may leave the screen: the core stays in view instead.
        "cradle" => (-hw - out - 1.2, 3.0, 0.0, -2.4, 38.0),
        // Juno's wide hat hides her face from higher up.
        "market" => (-hw - out - 2.0, hw + out + 1.0, 0.0, front, 22.0),
        _ => (-hw - out - 2.0, hw + out + 1.0, 0.0, front, 34.0),
    };
    Look {
        x0,
        x1,
        y,
        front,
        down,
    }
}

/// An interior's place on the station: set space to world, and its hull, uploaded.
struct Cut {
    at: Mat4,
    hull: MeshHandle,
    /// The module's own mesh, hidden while you are inside it.
    module: MeshHandle,
    look: Look,
    /// The station's core, in set space.
    core: Vec3,
}

#[derive(Default)]
pub struct Inside {
    cuts: HashMap<String, Cut>,
}

impl Room for Inside {
    fn name(&self) -> &'static str {
        "inside"
    }

    fn setup(&mut self, r: &mut Renderer, world: &World, model: &Places) {
        for (_, c) in self.cuts.drain() {
            r.remove_mesh(c.hull);
        }
        for p in model.places.iter().filter(|p| p.kind == Kind::Interior) {
            let Some(m) = model.module_of(&p.id).and_then(|m| world.module(&m.id)) else {
                continue;
            };
            // The set's back (+y) faces the core, its open front faces out to space.
            let pos = m.pos().truncate();
            let inward = (-pos).normalize_or(Vec2::Y);
            let yaw = inward.to_angle() - std::f32::consts::FRAC_PI_2;
            let at = Mat4::from_translation(pos.extend(0.0)) * Mat4::from_rotation_z(yaw);
            let reach = (pos.length() - CORE - p.floor.y * 0.5).max(0.0);
            let hull = r.upload_mesh(&hull(p.floor, reach));
            self.cuts.insert(
                p.id.clone(),
                Cut {
                    at,
                    hull,
                    module: m.mesh,
                    look: look(p.set.as_deref().unwrap_or(""), p.floor),
                    core: at.inverse().transform_point3(Vec3::Z * 2.0),
                },
            );
        }
    }

    fn view(&self, stage: &Stage, place: &Place) -> Shown {
        let Some(cut) = self.cuts.get(&place.id) else {
            return Shown::new(
                Scene::new(Camera::perspective(Vec3::NEG_Y, Vec3::ZERO, FOV)),
                &place.id,
            );
        };
        let cam = camera(&cut.look, stage.aspect(), cut.at, cut.core);
        // The station round the room, without the module the room replaces.
        let mut shown = stage.exterior(None, Some(cam));
        shown.place = place.id.clone();
        shown.points.clear();
        let scene = &mut shown.scene;
        scene.meshes.retain(|m| m.mesh != cut.module);
        let station_lights = std::mem::take(&mut scene.lights.points);
        // Lit as the room is: a key from the front left and above, turned with the room.
        scene.lights.sun_direction = cut
            .at
            .transform_vector3(Vec3::new(-0.45, -0.7, 0.75))
            .normalize();
        scene.lights.ambient = render::Color::hex(0x4a3a58);
        let bible = ArtBible::bundled();
        scene.meshes.push(
            MeshInstance::new(cut.hull, cut.at).with_material(bible.material(InkRole::Station)),
        );
        // The room's own lamps first, then the station's while there's room.
        stage.set(&place.id, cut.at, &mut shown);
        for l in station_lights {
            if shown.scene.lights.points.len() < MAX_POINT_LIGHTS {
                shown.scene.lights.points.push(l);
            }
        }
        shown
    }
}

/// The camera, in set space and then through `at`: the floor's span from `x0` to `x1`
/// fills the part of the screen left of the room panel. It looks down enough to keep
/// the room on screen, and no further than keeps the core (at `core`, set space) in
/// view near the top.
fn camera(l: &Look, aspect: f32, at: Mat4, core: Vec3) -> Camera {
    let half = FOV * 0.5;
    // The room panel takes up to 45% on the right: the span goes in the left 54%.
    let width = (l.x1 - l.x0) / 0.54;
    let dist = width * 0.5 / (half.to_radians().tan() * aspect);
    let shift = (l.x0 + l.x1) * 0.5 + (0.5 - 0.27) * width;
    let down = l.down.to_radians();
    let eye = Vec3::new(shift, l.y, 0.0) + Vec3::new(0.0, -down.cos(), down.sin()) * dist;
    let below = |y: f32, z: f32| (eye.z - z).atan2(y - eye.y).to_degrees();
    let pitch = (below(core.y, core.z) + half - 5.0)
        .max(below(l.front, -0.6) - half + 2.0)
        .to_radians();
    let target = eye + Vec3::new(0.0, pitch.cos(), -pitch.sin()) * dist;
    Camera::perspective(at.transform_point3(eye), at.transform_point3(target), FOV)
}

fn cube(b: &mut MeshBuilder, at: Vec3, size: Vec3) {
    b.with(Mat4::from_translation(at), |b| b.cuboid(size));
}

/// The module's hull round a set of floor `size`, cut open at the top and the front, and
/// a corridor `reach` long from its back to the core. Set space: the floor centred on
/// the origin, the open front toward -y.
fn hull(size: Vec2, reach: f32) -> render::MeshData {
    let mut b = MeshBuilder::new();
    let (hw, hd) = (size.x * 0.5, size.y * 0.5);
    let out = GAP + SKIN;
    let (cream, trim, cut) = (palette::HULL_CREAM, hex(0x2f7f86), palette::TERRACOTTA);
    let (x_out, y_back) = (hw + out, hd + out);
    // The deck under the set, and the hull's belly below it, narrowing.
    b.paint(cream, 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, out * 0.5, -0.6),
        Vec3::new(x_out * 2.0, size.y + out, 0.6),
    );
    b.paint(cream.scale(0.8), 0.0);
    b.with(
        Mat4::from_translation(Vec3::new(0.0, out * 0.5, -0.9))
            * Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0)),
        |b| {
            let (w, d) = (x_out, hd + out * 0.5);
            b.extrude(
                &[
                    Vec2::new(-w, -d),
                    Vec2::new(w, -d),
                    Vec2::new(w, d),
                    Vec2::new(-w, d),
                ],
                1.4,
                0.55,
            )
        },
    );
    // The skin: the side and back walls outside the set's.
    b.paint(cream, 0.0);
    for side in [-1.0f32, 1.0] {
        let x = side * (hw + GAP + SKIN * 0.5);
        cube(
            &mut b,
            Vec3::new(x, out * 0.5, HULL_H * 0.5 - 0.3),
            Vec3::new(SKIN, size.y + out, HULL_H + 0.6),
        );
    }
    cube(
        &mut b,
        Vec3::new(0.0, hd + GAP + SKIN * 0.5, HULL_H * 0.5 - 0.3),
        Vec3::new(x_out * 2.0, SKIN, HULL_H + 0.6),
    );
    // Hull bands round the outside.
    b.paint(trim, 0.0);
    let mut y = -hd + 0.8;
    while y < hd {
        for side in [-1.0f32, 1.0] {
            cube(
                &mut b,
                Vec3::new(side * (x_out + 0.05), y, HULL_H * 0.5 - 0.3),
                Vec3::new(0.12, 0.3, HULL_H + 0.5),
            );
        }
        y += 2.2;
    }
    cube(
        &mut b,
        Vec3::new(0.0, y_back + 0.05, -0.2),
        Vec3::new(x_out * 2.0 + 0.2, 0.12, 0.3),
    );
    // The cut: painted along the top and front edges of the skin and the deck, like a
    // section drawing.
    b.paint(cut, 0.0);
    for side in [-1.0f32, 1.0] {
        let x = side * (hw + GAP + SKIN * 0.5);
        cube(
            &mut b,
            Vec3::new(x, out * 0.5, HULL_H + 0.02),
            Vec3::new(SKIN + 0.02, size.y + out, 0.04),
        );
        cube(
            &mut b,
            Vec3::new(x, -hd - 0.01, HULL_H * 0.5 - 0.3),
            Vec3::new(SKIN + 0.02, 0.04, HULL_H + 0.6),
        );
    }
    cube(
        &mut b,
        Vec3::new(0.0, hd + GAP + SKIN * 0.5, HULL_H + 0.02),
        Vec3::new(x_out * 2.0 + 0.02, SKIN + 0.02, 0.04),
    );
    cube(
        &mut b,
        Vec3::new(0.0, -hd - 0.01, -0.6),
        Vec3::new(x_out * 2.0 + 0.02, 0.04, 0.62),
    );
    // The corridor back to the core: a tube with bands, and lit windows along it.
    if reach > 0.5 {
        let (w, h) = (2.2, 2.2);
        let y0 = y_back;
        b.paint(cream, 0.0);
        cube(
            &mut b,
            Vec3::new(0.0, y0 + reach * 0.5, h * 0.5 - 0.2),
            Vec3::new(w, reach, h),
        );
        let mut y = y0 + 1.2;
        while y < y0 + reach - 0.5 {
            b.paint(trim, 0.0);
            cube(
                &mut b,
                Vec3::new(0.0, y, h * 0.5 - 0.2),
                Vec3::new(w + 0.2, 0.35, h + 0.2),
            );
            b.paint(palette::WINDOW, 1.0);
            for side in [-1.0f32, 1.0] {
                cube(
                    &mut b,
                    Vec3::new(side * (w * 0.5 + 0.02), y + 1.1, h * 0.6),
                    Vec3::new(0.04, 0.4, 0.3),
                );
            }
            y += 3.0;
        }
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hull_is_well_formed() {
        for (size, reach) in [(Vec2::new(8.0, 5.5), 40.0), (Vec2::new(14.0, 9.0), 0.0)] {
            let m = hull(size, reach);
            assert!(m.is_well_formed() && !m.is_empty());
        }
    }
}
