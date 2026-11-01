//! Room camera, visibility and automatic anchor publishers. No presentation state.
use crate::{View, screens::Boxed};
use galaxy_api::engine::{BEING, LOCATION, MODEL, PLACE, POSITION, ROOM, SPOT, TRAVEL};
use galaxy_render::{
    Camera, Rect,
    glam::{Quat, Vec2, Vec3},
};
use galaxy_sim::World;
use std::collections::BTreeMap;

pub fn visible(world: &World, id: &str, place: Option<&str>) -> bool {
    let Some(place) = place else {
        return true;
    };
    if world.component(id, PLACE).is_some() {
        return id == place;
    }
    world
        .component(id, LOCATION)
        .is_some_and(|c| c.fields["place"] == place)
}

pub fn screen_world<'a>(world: &'a World, place: Option<&str>) -> std::borrow::Cow<'a, World> {
    if let Some((id, current)) = galaxy_sim::travel::player(world)
        && let Some(place) = place
        && place != current
    {
        let mut shown = world.clone();
        let loc = shown
            .entities
            .get_mut(id)
            .unwrap()
            .get_mut(LOCATION)
            .unwrap();
        loc.fields.insert("place".into(), place.into());
        if let Some(trip) = world.component(id, TRAVEL) {
            loc.fields
                .insert("spot".into(), trip.fields["from_spot"].clone());
            if let Some(p) = shown.entities.get_mut(id).unwrap().get_mut(POSITION) {
                for key in ["x", "y", "height", "angle", "layer"] {
                    p.fields
                        .insert(key.into(), trip.fields[&format!("from_{key}")].clone());
                }
            }
        }
        return std::borrow::Cow::Owned(shown);
    }
    std::borrow::Cow::Borrowed(world)
}

impl View {
    pub fn room_camera(&self, world: &World, place: &str) -> Option<Camera> {
        let room = &world.component(place, ROOM)?.fields;
        let model = world.component(place, MODEL)?.fields["model"].as_str()?;
        let nodes = self.assets.nodes.get(model)?;
        let name = room["camera"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("camera");
        let node = nodes.get(name)?;
        Some(crate::CameraSpec::posed(
            node,
            room["fov"].as_f64().unwrap_or(40.0) as f32,
        ))
    }

    pub fn place_anchors(
        &self,
        world: &World,
        alpha: f32,
        size: Vec2,
        camera: Camera,
        anchors: &mut BTreeMap<String, Vec2>,
    ) {
        let place = crate::travel::place(world, alpha);
        let mut publish = |name: String, point: Vec3| {
            if let Some(point) = camera.world_to_screen(point, size) {
                anchors.insert(name, point);
            }
        };
        for (id, c) in &world.entities {
            if let Some(spot) = c.get(SPOT) {
                let f = &spot.fields;
                if place.is_some_and(|p| f["place"] == p) {
                    let n = |k: &str| f[k].as_f64().unwrap_or(0.0) as f32;
                    publish(
                        format!("spot.{}", f["name"].as_str().unwrap_or("")),
                        Vec3::new(
                            n("x"),
                            n("y"),
                            galaxy_sim::services::floor_height(
                                world,
                                f["place"].as_str().unwrap_or(""),
                                f["layer"].as_i64().unwrap_or(0),
                                n("x") as f64,
                                n("y") as f64,
                            ) as f32,
                        ),
                    );
                }
            }
            if !visible(world, id, place) {
                continue;
            }
            let Some(pose) = self.pose(world, id, alpha) else {
                continue;
            };
            if c.contains_key(PLACE) {
                publish(format!("place.{id}"), pose.point());
            }
            if !c.contains_key(BEING) {
                continue;
            }
            let model = c.get(MODEL);
            let name = model.and_then(|m| m.fields["model"].as_str()).unwrap_or("");
            let scale = model
                .and_then(|m| m.fields["scale"].as_f64())
                .unwrap_or(1.0) as f32;
            let top = self
                .assets
                .models
                .get(name)
                .and_then(|m| m.bounds())
                .map_or(1.8, |(_, hi)| hi.z * scale);
            publish(format!("being.{id}"), pose.point() + Vec3::Z * top);
            if let Some(nodes) = self.assets.nodes.get(name) {
                for (part, node) in nodes {
                    publish(
                        format!("being.{id}.{part}"),
                        pose.point() + Quat::from_rotation_z(pose.angle) * node.position * scale,
                    );
                }
            }
        }
    }

    /// Headless check enters every room in a cloned world, exercising its conditional
    /// screens and the same projection as play. The player's actual world is untouched.
    pub fn check_rooms(&mut self, world: &World, size: Vec2) -> Vec<String> {
        let mut findings = Vec::new();
        let player = galaxy_sim::travel::player(world).map(|(id, _)| id.to_owned());
        for (room, c) in &world.entities {
            if !c.contains_key(ROOM) {
                continue;
            }
            let mut staged = world.clone();
            if let Some(id) = &player {
                if let Some(trip) = staged.entities.get_mut(id).unwrap().get_mut(TRAVEL) {
                    trip.fields.insert("active".into(), false.into());
                }
                let loc = staged
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .get_mut(LOCATION)
                    .unwrap();
                loc.fields.insert("place".into(), room.clone().into());
                let arrival = c
                    .get(PLACE)
                    .and_then(|p| p.fields["arrival"].as_str())
                    .unwrap_or("");
                loc.fields.insert(
                    "spot".into(),
                    if arrival.is_empty() {
                        serde_json::Value::Null
                    } else {
                        arrival.into()
                    },
                );
                let spot = world.component(arrival, SPOT);
                let n = |k: &str| spot.and_then(|s| s.fields[k].as_f64()).unwrap_or(0.0);
                let layer = spot.and_then(|s| s.fields["layer"].as_i64()).unwrap_or(0);
                let height = galaxy_sim::services::floor_height(world, room, layer, n("x"), n("y"));
                if let Some(p) = staged.entities.get_mut(id).unwrap().get_mut(POSITION) {
                    p.fields.extend(serde_json::json!({"x": n("x"), "y": n("y"), "layer": layer, "height": height}).as_object().unwrap().clone());
                }
            } else {
                findings.push(format!(
                    "room `{room}` needs a player being to check its screens"
                ));
                continue;
            }
            let scene = self.scene(&staged, 1.0, size, 0.0);
            findings.extend(
                self.screens
                    .problems
                    .iter()
                    .map(|p| format!("room `{room}`: {p}")),
            );
            let mut panels = Vec::new();
            fn panels_in<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
                if b.widget.kind == "panel" {
                    out.push(b);
                }
                for c in &b.children {
                    panels_in(c, out);
                }
            }
            for b in &self.screens.boxes {
                panels_in(b, &mut panels);
            }
            // Empty rooms still need usable framing: check every authored floor spot,
            // including arrivals, even when no visible being occupies it yet.
            for (id, c) in &staged.entities {
                let Some(spot) = c.get(SPOT).filter(|s| s.fields["place"] == *room) else {
                    continue;
                };
                let f = &spot.fields;
                let x = f["x"].as_f64().unwrap_or(0.0);
                let y = f["y"].as_f64().unwrap_or(0.0);
                let z = galaxy_sim::services::floor_height(
                    &staged,
                    room,
                    f["layer"].as_i64().unwrap_or(0),
                    x,
                    y,
                );
                match scene
                    .camera
                    .world_to_screen(Vec3::new(x as f32, y as f32, z as f32), size)
                {
                    Some(p)
                        if p.x >= 8.0
                            && p.y >= 8.0
                            && p.x <= size.x - 8.0
                            && p.y <= size.y - 8.0 =>
                    {
                        for panel in &panels {
                            let r = panel.rect;
                            if p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h {
                                findings.push(format!(
                                    "room `{room}`: spot `{id}` is covered by {}",
                                    panel.widget.path
                                ));
                            }
                        }
                    }
                    _ => findings.push(format!(
                        "room `{room}`: spot `{id}` is outside the camera frame"
                    )),
                }
            }
            for (id, c) in &staged.entities {
                if !c.contains_key(BEING) || !visible(&staged, id, Some(room)) {
                    continue;
                }
                let Some(model) = c.get(MODEL) else {
                    continue;
                };
                let Some(mesh) = model.fields["model"]
                    .as_str()
                    .and_then(|m| self.assets.models.get(m))
                else {
                    continue;
                };
                let Some(pose) = self.pose(&staged, id, 1.0) else {
                    continue;
                };
                let scale = model.fields["scale"].as_f64().unwrap_or(1.0) as f32;
                let mut lo = Vec2::splat(f32::MAX);
                let mut hi = Vec2::splat(f32::MIN);
                let mut outside = false;
                for v in &mesh.vertices {
                    let point = pose.point()
                        + Quat::from_rotation_z(pose.angle) * Vec3::from(v.position) * scale;
                    if let Some(p) = scene.camera.world_to_screen(point, size) {
                        outside |=
                            p.x < 8.0 || p.y < 8.0 || p.x > size.x - 8.0 || p.y > size.y - 8.0;
                        lo = lo.min(p);
                        hi = hi.max(p);
                    } else {
                        outside = true;
                    }
                }
                if outside {
                    findings.push(format!(
                        "room `{room}`: being `{id}` is outside the camera frame"
                    ));
                }
                let bounds = Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y);
                for panel in &panels {
                    let r = panel.rect;
                    if bounds.x < r.x + r.w
                        && bounds.x + bounds.w > r.x
                        && bounds.y < r.y + r.h
                        && bounds.y + bounds.h > r.y
                    {
                        findings.push(format!(
                            "room `{room}`: being `{id}` is covered by {}",
                            panel.widget.path
                        ));
                    }
                }
            }
        }
        findings
    }
}
