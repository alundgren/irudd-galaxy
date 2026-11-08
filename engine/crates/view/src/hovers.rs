//! One picking path for live hover labels, recorded looks and staged checks.
mod check;
use crate::{View, screens::Boxed};
pub(crate) use check::PartScans;
use galaxy_api::engine::{ANCHOR, BEING, MODEL, SPOT};
use galaxy_render::{
    Overlay,
    glam::{Vec2, Vec3},
};
use galaxy_script::screens::anchored;
use galaxy_sim::{Input, World, talk::text};
use serde_json::{Value, json};

fn binding(world: &World, anchor: &str) -> (Option<String>, Option<String>) {
    if let Some((publisher, _name)) = anchor.split_once('#') {
        let target = world
            .component(publisher, ANCHOR)
            .map(|c| text(&c.fields, "target"))
            .filter(|t| !t.is_empty())
            .unwrap_or(publisher);
        return (Some(target.into()), None);
    }
    if let Some(name) = anchor.strip_prefix("spot.") {
        let place = galaxy_sim::travel::player(world).map(|(_, p)| p);
        return (
            world
                .entities
                .iter()
                .find(|(_, c)| {
                    c.get(SPOT).is_some_and(|s| {
                        text(&s.fields, "name") == name && Some(text(&s.fields, "place")) == place
                    })
                })
                .map(|(id, _)| id.clone()),
            None,
        );
    }
    let name = anchor
        .strip_prefix("being.")
        .or_else(|| anchor.strip_prefix("place."));
    if let Some(name) = name {
        // Entity IDs may contain dots: prefer the longest actual ID.
        if let Some(id) = world
            .entities
            .keys()
            .filter(|id| {
                name == id.as_str()
                    || name
                        .strip_prefix(id.as_str())
                        .is_some_and(|s| s.starts_with('.'))
            })
            .max_by_key(|id| id.len())
        {
            return (
                Some(id.clone()),
                name.strip_prefix(&format!("{id}.")).map(str::to_owned),
            );
        }
    }
    (None, None)
}

pub(crate) struct Picked {
    t: f32,
    id: String,
    part: Option<String>,
}

pub(crate) struct Anchored {
    pub(crate) widget: galaxy_script::screens::Widget,
    pub(crate) anchor: String,
    pub(crate) entity: Option<String>,
    part: Option<String>,
    model: bool,
    at: Vec2,
}

fn inside(b: &Boxed, p: Vec2) -> bool {
    let r = b.rect;
    p.x >= r.x && p.y >= r.y && p.x < r.x + r.w && p.y < r.y + r.h
}

impl View {
    fn hover_uses_mesh(&self, world: &World, id: &str, part: Option<&str>) -> bool {
        let Some(model) = world.component(id, MODEL) else {
            return false;
        };
        part.is_none_or(|name| {
            self.assets
                .parts
                .get(text(&model.fields, "model"))
                .is_some_and(|parts| parts.iter().any(|p| p.name == name))
        })
    }

    fn hover_anchor_world(&self, world: &World, anchor: &str, alpha: f32) -> Option<Vec3> {
        let (id, part) = binding(world, anchor);
        let id = id?;
        if let Some(spot) = world.component(&id, SPOT) {
            let f = &spot.fields;
            let n = |k| f.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            return Some(Vec3::new(
                n("x") as f32,
                n("y") as f32,
                galaxy_sim::services::floor_height(
                    world,
                    text(f, "place"),
                    f["layer"].as_i64().unwrap_or(0),
                    n("x"),
                    n("y"),
                ) as f32,
            ));
        }
        let pose = self.pose(world, &id, alpha)?;
        let mut offset = Vec3::ZERO;
        if let Some((publisher, _)) = anchor.split_once('#') {
            let f = &world.component(publisher, ANCHOR)?.fields;
            let n = |k| f.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            offset = Vec3::new(n("x"), n("y"), n("height"));
        } else if let Some(part) = part {
            let model = &world.component(&id, MODEL)?.fields;
            let node = self.assets.nodes.get(text(model, "model"))?.get(&part)?;
            offset = node.position * model["scale"].as_f64().unwrap_or(1.0) as f32;
        } else if anchor.starts_with("being.") {
            offset.z = 1.8;
        }
        Some(pose.point() + galaxy_render::glam::Quat::from_rotation_z(pose.angle) * offset)
    }

    /// Anchored declarations are bot targets too; resolve against this frame's geometry.
    pub fn bot_pointer(
        &mut self,
        world: &World,
        action: &galaxy_sim::scenario::Pointer,
    ) -> Result<Option<galaxy_sim::Input>, String> {
        if action.action != "move" {
            return self.screens.bot_pointer(action);
        }
        let targets: Vec<_> = self
            .screens
            .all()
            .into_iter()
            .filter(|b| {
                anchored(&b.widget.kind)
                    && (b.widget.id.as_deref() == Some(&action.widget)
                        || b.widget.path == action.widget)
            })
            .map(|b| (b.widget.clone(), b.rect))
            .collect();
        if targets.len() > 1 {
            return Err("ambiguous hover target; use its full widget path".into());
        }
        let Some((w, rect)) = targets.first() else {
            return self.screens.bot_pointer(action);
        };
        if w.kind == "target" {
            let point = self
                .target_point(world, w, *rect)
                .ok_or("target is covered or not visible")?;
            return Ok(self.screens.pointer_move(point, action.tick));
        }
        let anchor = w.anchor.as_deref().unwrap();
        let mut point = *self
            .screens
            .anchors
            .get(anchor)
            .ok_or("hover anchor is missing")?;
        let (entity, part) = binding(world, anchor);
        let (alpha, size) = self.talk_frame;
        if let Some(id) = entity
            && self.hover_uses_mesh(world, &id, part.as_deref())
        {
            let camera = self.camera.ok_or("no displayed camera")?;
            let geometry = self.geometry(world, alpha);
            let g = geometry
                .iter()
                .find(|g| g.id == id)
                .ok_or("hover model is not visible")?;
            let mut found = None;
            for tri in g.mesh.indices.as_chunks::<3>().0 {
                let local = tri
                    .iter()
                    .map(|i| Vec3::from(g.mesh.vertices[*i as usize].position))
                    .sum::<Vec3>()
                    / 3.0;
                let Some(p) = camera.world_to_screen(g.transform.transform_point3(local), size)
                else {
                    continue;
                };
                let hit = self.look(world, alpha, size, p);
                if hit["entity"].as_str() == Some(&id)
                    && hit["widget"].as_str() == Some(&w.path)
                    && (part.is_none()
                        || anchor.contains('#')
                        || hit["part"].as_str() == part.as_deref())
                {
                    found = Some(p);
                    break;
                }
            }
            point = found.ok_or("hover model is covered")?;
        }
        let hit = self.look(world, alpha, size, point);
        if hit["widget"].as_str() != Some(&w.path) {
            return Err("hover anchor is covered".into());
        }
        Ok(self.screens.pointer_move(point, action.tick))
    }

    /// Point and returned rectangles are logical pixels in the displayed frame.
    pub fn look(&mut self, world: &World, alpha: f32, size: Vec2, point: Vec2) -> Value {
        let shown = crate::rooms::screen_world(world, crate::travel::place(world, alpha));
        let geometry = self.geometry(&shown, alpha);
        self.look_in(&shown, alpha, size, point, &geometry)
    }

    fn look_in(
        &mut self,
        world: &World,
        alpha: f32,
        size: Vec2,
        point: Vec2,
        geometry: &[crate::talk::Geometry],
    ) -> Value {
        let mut out = json!({"point":point.to_array(), "place":crate::travel::place(world,alpha), "widget":null, "entity":null, "being":null, "part":null, "anchor":null, "hover":null});
        if !point.is_finite()
            || point.x < 0.0
            || point.y < 0.0
            || point.x >= size.x
            || point.y >= size.y
        {
            return out;
        }
        if crate::travel::Cut::step(world, alpha).is_some_and(|c| c.busy()) {
            out["widget"] = json!("engine.travel");
            return out;
        }
        let widget = self.screens.hover_hit(point).cloned();
        let mut label = None;
        let mut origin = point;
        if let Some(w) = widget {
            out["widget"] = json!(w.path);
            if let Some(a) = &w.anchor {
                let (entity, part) = binding(world, a);
                out["entity"] = json!(entity);
                out["part"] = json!(part);
                out["anchor"] = json!(a);
                origin = self.screens.anchors.get(a).copied().unwrap_or(point);
            }
            label = w.hover.or_else(|| {
                self.screens
                    .all()
                    .into_iter()
                    .filter(|b| {
                        w.path.starts_with(&format!("{}/", b.widget.path))
                            && b.widget.hover.is_some()
                    })
                    .max_by_key(|b| b.widget.path.len())
                    .and_then(|b| b.widget.hover.clone())
            });
            if label.is_none()
                && let Some(a) = &w.anchor
            {
                label = self
                    .screens
                    .all()
                    .into_iter()
                    .rev()
                    .find(|b| {
                        anchored(&b.widget.kind)
                            && b.widget.anchor.as_deref() == Some(a)
                            && !b.widget.text.is_empty()
                    })
                    .map(|b| b.widget.text.clone());
            }
        } else {
            let picked = self.pick_mesh(world, point, size, geometry);
            if let Some(p) = &picked {
                out["entity"] = json!(p.id);
                out["part"] = json!(p.part);
            }
            // Targets without text label nothing; an earlier hover still can.
            let found = self.anchored_at(world, alpha, point, size, picked.as_ref(), |w| {
                w.kind == "hover" || !w.text.is_empty()
            });
            if let Some(found) = found {
                out["widget"] = json!(found.widget.path);
                out["anchor"] = json!(found.anchor);
                out["entity"] = json!(found.entity);
                if !found.model {
                    out["part"] = json!(found.part);
                }
                label = Some(found.widget.text.clone());
                origin = found.at;
            }
        }
        if let Some(entity) = out["entity"].as_str()
            && world.component(entity, BEING).is_some()
        {
            out["being"] = json!(entity);
        }
        if self.screens.presented.drag.is_some() || self.screens.presented.scroll_grab.is_some() {
            label = None;
        }
        if let Some(label) = label {
            out["hover"] = self.screens.hover_label(&label, origin, size).0;
        }
        out
    }

    /// The nearest front-facing posed mesh under `point` in the displayed camera.
    fn pick_mesh(
        &self,
        world: &World,
        point: Vec2,
        size: Vec2,
        geometry: &[crate::talk::Geometry],
    ) -> Option<Picked> {
        let camera = self.camera?;
        let (o, d) = camera.screen_ray(point, size);
        let (t, g) = geometry
            .iter()
            .filter_map(|g| g.hit(o, d).map(|t| (t, g)))
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        let model = text(&world.component(&g.id, MODEL)?.fields, "model");
        let vertex = g
            .mesh
            .raycast_vertex(
                g.inverse.transform_point3(o),
                g.inverse.transform_vector3(d),
            )
            .map(|(_, v)| v);
        let part = self
            .assets
            .parts
            .get(model)
            .into_iter()
            .flatten()
            .find(|r| vertex.is_some_and(|v| r.vertices.contains(&v)))
            .map(|r| r.name.clone());
        Some(Picked {
            t,
            id: g.id.clone(),
            part,
        })
    }

    /// The last anchored declaration (`ui.hover` or `ui.target`) accepted by `want` whose
    /// thing is under `point`: its model's nearest surface, or the 44px box of a
    /// model-less anchor that no nearer geometry hides.
    fn anchored_at(
        &self,
        world: &World,
        alpha: f32,
        point: Vec2,
        size: Vec2,
        picked: Option<&Picked>,
        want: impl Fn(&galaxy_script::screens::Widget) -> bool,
    ) -> Option<Anchored> {
        let camera = self.camera?;
        let (o, d) = camera.screen_ray(point, size);
        for b in self
            .screens
            .all()
            .into_iter()
            .rev()
            .filter(|b| anchored(&b.widget.kind) && want(&b.widget))
        {
            let a = b.widget.anchor.as_deref().unwrap_or("");
            let Some(at) = self.screens.anchors.get(a) else {
                continue;
            };
            let (entity, part) = binding(world, a);
            let model = entity
                .as_ref()
                .is_some_and(|id| self.hover_uses_mesh(world, id, part.as_deref()));
            let matches = if model {
                picked.is_some_and(|p| {
                    Some(p.id.as_str()) == entity.as_deref()
                        && (part.is_none() || p.part == part || a.contains('#'))
                })
            } else {
                let behind = self
                    .hover_anchor_world(world, a, alpha)
                    .is_some_and(|position| {
                        picked.is_some_and(|p| p.t + 0.001 < (position - o).dot(d))
                    });
                inside(b, point)
                    && !behind
                    && b.clip.is_none_or(|r| {
                        point.x >= r.x
                            && point.y >= r.y
                            && point.x < r.x + r.w
                            && point.y < r.y + r.h
                    })
            };
            if matches {
                return Some(Anchored {
                    widget: b.widget.clone(),
                    anchor: a.to_owned(),
                    entity,
                    part,
                    model,
                    at: *at,
                });
            }
        }
        None
    }

    /// The `ui.target` under `point` in the shown world, as a click finds it: a painted
    /// widget, a gesture in progress or a busy travel cut blocks it.
    pub(crate) fn target_at(
        &self,
        world: &World,
        alpha: f32,
        size: Vec2,
        point: Vec2,
    ) -> Option<Anchored> {
        if !point.is_finite()
            || point.x < 0.0
            || point.y < 0.0
            || point.x >= size.x
            || point.y >= size.y
            || self.screens.presentation.drag.is_some()
            || self.screens.presentation.scroll_grab.is_some()
            || self.screens.hit(point).is_some()
            || !self.screens.all().iter().any(|b| b.widget.kind == "target")
            || crate::travel::Cut::step(world, alpha).is_some_and(|c| c.busy())
        {
            return None;
        }
        let geometry = self.geometry(world, alpha);
        let picked = self.pick_mesh(world, point, size, &geometry);
        self.anchored_at(world, alpha, point, size, picked.as_ref(), |w| {
            w.kind == "target"
        })
    }

    /// A left click on a `ui.target`'s thing: its declared event and payload, source `ui`,
    /// like a button. Uses the last drawn frame's camera and layout.
    pub fn target_click(&self, world: &World, point: Vec2, tick: u64) -> Option<Input> {
        let (alpha, size) = self.talk_frame;
        let shown = crate::rooms::screen_world(world, crate::travel::place(world, alpha));
        let found = self.target_at(&shown, alpha, size, point)?;
        let input = Input {
            tick,
            source: "ui".into(),
            name: found.widget.on.clone()?,
            payload: found.widget.payload.clone(),
            via: None,
        };
        self.screens
            .ui_cue(galaxy_api::ui::UiAction::Press, &found.widget);
        Some(input)
    }

    /// A bot `[[click]] widget = "<target id>"`: a point on the target's visible thing,
    /// clicked through the same picking as the pointer. `None` when no target has that
    /// name.
    pub fn bot_target_click(
        &self,
        world: &World,
        name: &str,
        tick: u64,
    ) -> Option<Result<Input, String>> {
        let targets: Vec<_> = self
            .screens
            .all()
            .into_iter()
            .filter(|b| {
                b.widget.kind == "target"
                    && (b.widget.id.as_deref() == Some(name) || b.widget.path == name)
            })
            .map(|b| (b.widget.clone(), b.rect))
            .collect();
        let (w, rect) = match targets.as_slice() {
            [] => return None,
            [one] => one.clone(),
            _ => {
                return Some(Err(format!(
                    "target `{name}` is ambiguous; use its full widget path"
                )));
            }
        };
        let point = self.target_point(world, &w, rect);
        Some(
            point
                .and_then(|p| self.target_click(world, p, tick))
                .ok_or_else(|| format!("{}: target is covered or not visible", w.path)),
        )
    }

    /// A point where the pointer finds this target: on its visible model, or in its box.
    pub(crate) fn target_point(
        &self,
        world: &World,
        w: &galaxy_script::screens::Widget,
        rect: galaxy_render::Rect,
    ) -> Option<Vec2> {
        let (alpha, size) = self.talk_frame;
        let shown = crate::rooms::screen_world(world, crate::travel::place(world, alpha));
        let world = &*shown;
        let anchor = w.anchor.as_deref().unwrap_or("");
        let (entity, part) = binding(world, anchor);
        let mut candidates = vec![rect.center()];
        if let Some(id) = entity.filter(|id| self.hover_uses_mesh(world, id, part.as_deref()))
            && let Some(camera) = self.camera
        {
            candidates.clear();
            for g in self.geometry(world, alpha).iter().filter(|g| g.id == id) {
                for tri in g.mesh.indices.as_chunks::<3>().0 {
                    let local = tri
                        .iter()
                        .map(|i| Vec3::from(g.mesh.vertices[*i as usize].position))
                        .sum::<Vec3>()
                        / 3.0;
                    candidates
                        .extend(camera.world_to_screen(g.transform.transform_point3(local), size));
                }
            }
        }
        candidates.into_iter().find(|p| {
            self.target_at(world, alpha, size, *p)
                .is_some_and(|f| f.widget.path == w.path)
        })
    }

    pub(crate) fn draw_hover(
        &mut self,
        world: &World,
        alpha: f32,
        size: Vec2,
        overlay: &mut Overlay,
    ) {
        if self.screens.presentation.drag.is_some()
            || self.screens.presentation.scroll_grab.is_some()
        {
            return;
        }
        let Some(point) = self.screens.presentation.pointer.map(Vec2::from) else {
            return;
        };
        let result = self.look(world, alpha, size, point);
        if let Some(label) = result["hover"]["text"].as_str() {
            let origin = result["anchor"]
                .as_str()
                .and_then(|a| self.screens.anchors.get(a))
                .copied()
                .unwrap_or(point);
            let (report, layer) = self.screens.hover_label(label, origin, size);
            overlay.widgets.push(layer);
            for p in report["problems"].as_array().unwrap() {
                self.screens.problems.push(format!(
                    "{}: {}",
                    result["widget"].as_str().unwrap_or("hover"),
                    p.as_str().unwrap()
                ));
            }
        }
    }

    /// Every staged screen uses this too, including each room and conversation line.
    pub(crate) fn check_hovers(&mut self, world: &World, alpha: f32, size: Vec2) {
        self.collect_hovers(world, alpha, size);
        let original = self.screens.presentation.clone();
        let boxes = self.screens.boxes.clone();
        let problems = self.screens.problems.clone();
        let presented = self.screens.presented.clone();
        let jobs = check::scroll_states(&boxes, &original);
        let focus_live = self.focus_live;
        self.focus_live = false;
        for scroll in jobs {
            self.screens.presentation = original.clone();
            self.screens.presentation.scroll = scroll;
            let checks = self.hover_checks.take();
            self.scene(world, alpha, size, 0.0);
            self.hover_checks = checks;
            self.collect_hovers(world, alpha, size);
        }
        self.focus_live = focus_live;
        self.screens.presentation = original;
        self.screens.presented = presented;
        self.screens.boxes = boxes;
        self.screens.problems = problems;
    }
}
