//! Bubbles share screen layout, paint and input; focus uses the same posed meshes as play.
use crate::{View, rooms};
use galaxy_api::engine::*;
use galaxy_mesh::MeshData;
use galaxy_render::{
    Camera,
    glam::{Mat4, Quat, Vec2, Vec3},
};
use galaxy_script::screens::Widget;
use galaxy_sim::{
    Input, World,
    scenario::{Click, ClickTarget},
    talk::{self, text},
};
use std::collections::BTreeMap;

pub fn widgets(world: &World) -> Vec<Widget> {
    world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let f = &c.get(TALK)?.fields;
            if f["active"] != true || !talk::available(world, f) {
                return None;
            }
            let line_id = text(f, "current");
            let line = &world.component(line_id, TALK_LINE)?.fields;
            Some(Widget {
                kind: "bubble".into(),
                id: Some(format!("talk.{id}")),
                path: format!("{id}/talk"),
                text: format!(
                    "{}{}\n\n{}",
                    world
                        .component(text(f, "speaker"), LIFE)
                        .map(|l| text(&l.fields, "name"))
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("{s}\n"))
                        .unwrap_or_default(),
                    text(line, "text"),
                    if text(line, "next").is_empty() {
                        "Done"
                    } else {
                        "Continue"
                    }
                ),
                width: Some(f["width"].as_f64().unwrap_or(320.0) as f32),
                height: None,
                x: Some(f["x"].as_f64().unwrap_or(0.0) as f32),
                y: Some(f["y"].as_f64().unwrap_or(-12.0) as f32),
                size: 20.0,
                pad: 16.0,
                explicit_pad: false,
                gap: 12.0,
                anchor: Some(talk::anchor(f)),
                on: Some(TALK_NEXT.into()),
                payload: serde_json::json!({"talk": id, "line": line_id})
                    .as_object()
                    .unwrap()
                    .clone(),
                children: vec![],
                columns: 1,
                value: String::new(),
                delta: 0.0,
                accepts: BTreeMap::new(),
            })
        })
        .collect()
}

struct Geometry {
    id: String,
    mesh: MeshData,
    transform: Mat4,
    inverse: Mat4,
}
impl Geometry {
    fn hit(&self, origin: Vec3, direction: Vec3) -> Option<f32> {
        self.mesh.raycast(
            self.inverse.transform_point3(origin),
            self.inverse.transform_vector3(direction),
        )
    }
}

impl View {
    fn geometry(&self, world: &World, alpha: f32) -> Vec<Geometry> {
        let place = crate::travel::place(world, alpha);
        world
            .entities
            .iter()
            .filter_map(|(id, c)| {
                if !rooms::visible(world, id, place) {
                    return None;
                }
                let f = &c.get(MODEL)?.fields;
                let pose = self.pose(world, id, alpha)?;
                let scale = f["scale"].as_f64().unwrap_or(1.0) as f32;
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let transform = Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    Quat::from_rotation_z(pose.angle),
                    pose.point(),
                );
                Some(Geometry {
                    id: id.clone(),
                    mesh: self
                        .assets
                        .models
                        .get(text(f, "model"))?
                        .morphed(self.morph(world, id, alpha)),
                    transform,
                    inverse: transform.inverse(),
                })
            })
            .collect()
    }

    /// Uses the displayed camera and interpolated poses. Painted screens block picking.
    pub fn talk_click(&self, world: &World, point: Vec2, tick: u64) -> Option<Input> {
        if self.screens.hit(point).is_some() || self.screens.presentation.drag.is_some() {
            return None;
        }
        let camera = self.camera?;
        let (alpha, size) = self.talk_frame;
        let (o, d) = camera.screen_ray(point, size);
        let geometry = self.geometry(world, alpha);
        let closest = geometry
            .iter()
            .filter_map(|g| g.hit(o, d).map(|t| (t, g.id.as_str())))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let speaker = closest.and_then(|(_, id)| talk::for_speaker(world, id).map(|_| id));
        let conversation = if let Some(speaker) = speaker {
            talk::for_speaker(world, speaker)?
        } else {
            // An anchor-only publisher has a 44px target centred on its projected anchor.
            // Model speakers always use their actual visible geometry.
            let mut anchors = BTreeMap::new();
            self.place_anchors(world, alpha, size, camera, &mut anchors);
            world.entities.iter().find_map(|(id, c)| {
                let f = &c.get(TALK)?.fields;
                let speaker = text(f, "speaker");
                if world.component(speaker, MODEL).is_some() || !talk::available(world, f) {
                    return None;
                }
                let name = talk::anchor(f);
                let point_at = if let Some((publisher, _)) = name.split_once('#') {
                    let a = &world.component(publisher, ANCHOR)?.fields;
                    let target = text(a, "target");
                    let pose = self.pose(
                        world,
                        if target.is_empty() { publisher } else { target },
                        alpha,
                    )?;
                    let n = |k| a[k].as_f64().unwrap_or(0.0) as f32;
                    let p = pose.point()
                        + Quat::from_rotation_z(pose.angle)
                            * Vec3::new(n("x"), n("y"), n("height"));
                    if closest.is_some_and(|(t, _)| t + 0.001 < (p - o).dot(d)) {
                        return None;
                    }
                    camera.world_to_screen(p, size)?
                } else {
                    *anchors.get(&name)?
                };
                ((point - point_at).abs().max_element() <= 22.0
                    && talk::for_speaker(world, speaker) == Some(id.as_str()))
                .then_some(id.as_str())
            })?
        };
        Some(Input {
            tick,
            source: "talk".into(),
            name: TALK_START.into(),
            payload: serde_json::json!({"talk":conversation})
                .as_object()
                .unwrap()
                .clone(),
        })
    }

    /// `speaker.<full-id>` bot targets use the same picking path as a real click.
    pub fn bot_talk_click(&self, world: &World, click: &Click) -> Result<Input, String> {
        match &click.target {
            ClickTarget::Widget(name) if name.starts_with("speaker.") => {
                let id = name.trim_start_matches("speaker.");
                let camera = self.camera.ok_or("no displayed camera")?;
                let (alpha, size) = self.talk_frame;
                let geometry = self.geometry(world, alpha);
                let g = geometry.iter().find(|g| g.id == id).ok_or_else(|| {
                    format!("speaker `{id}` has no visible model; click its anchor coordinates")
                })?;
                let (lo, hi) = g.mesh.bounds().ok_or("speaker has empty geometry")?;
                let p = camera
                    .world_to_screen(g.transform.transform_point3((lo + hi) * 0.5), size)
                    .ok_or("speaker is behind the camera")?;
                let input = self
                    .talk_click(world, p, click.tick)
                    .ok_or_else(|| format!("speaker `{id}` is covered or has no enabled talk"))?;
                let actual = text(&input.payload, "talk");
                if world
                    .component(actual, TALK)
                    .is_none_or(|t| text(&t.fields, "speaker") != id)
                {
                    return Err(format!("speaker `{id}` is covered by another speaker"));
                }
                Ok(input)
            }
            ClickTarget::Point { x, y } => self.screens.bot_click(click).or_else(|_| {
                self.talk_click(world, Vec2::new(*x, *y), click.tick)
                    .ok_or_else(|| "click hit no widget or speaker".into())
            }),
            _ => self.screens.bot_click(click),
        }
    }

    /// The speakers of the lines on screen whose focus is covered or off screen in this
    /// frame, by the same rules as [`View::check_talk`]: a script can cover a face at
    /// runtime in a state the check never stages. Presentation only; the world is read.
    pub(crate) fn live_focus(
        &self,
        world: &World,
        alpha: f32,
        size: Vec2,
        camera: Camera,
    ) -> Vec<String> {
        world
            .entities
            .iter()
            .filter_map(|(id, c)| {
                let f = &c.get(TALK)?.fields;
                if f["active"] != true || !talk::available(world, f) {
                    return None;
                }
                let line = text(f, "current");
                self.focus(world, text(f, "speaker"), camera, size, alpha)
                    .err()
                    .map(|e| format!("talk `{id}`, line `{line}`: {e}"))
            })
            .collect()
    }

    /// Check every authored line with its place's conditional screens and room camera.
    /// Active walks are sampled at every tick and halfway between ticks, including their endpoint.
    pub fn check_talk(&mut self, world: &World, size: Vec2) -> Vec<String> {
        self.check_talk_at(world, size, 1.0, true)
    }

    pub(crate) fn check_talk_at(
        &mut self,
        world: &World,
        size: Vec2,
        alpha: f32,
        walks: bool,
    ) -> Vec<String> {
        // The check stages each line and runs focus itself, with its own wording.
        let live = std::mem::replace(&mut self.focus_live, false);
        let findings = self.check_talk_lines(world, size, alpha, walks);
        self.focus_live = live;
        findings
    }

    fn check_talk_lines(
        &mut self,
        world: &World,
        size: Vec2,
        alpha: f32,
        walks: bool,
    ) -> Vec<String> {
        let mut findings = Vec::new();
        for (id, c) in &world.entities {
            let Some(t) = c.get(TALK) else { continue };
            let f = &t.fields;
            let speaker = text(f, "speaker");
            let place = if world.component(speaker, PLACE).is_some() {
                speaker
            } else {
                galaxy_sim::places::place_of(world, speaker)
            };
            let mut staged = world.clone();
            if let Some((player, _)) = galaxy_sim::travel::player(world) {
                let c = staged.entities.get_mut(player).unwrap();
                c.get_mut(LOCATION)
                    .unwrap()
                    .fields
                    .insert("place".into(), place.into());
                if let Some(t) = c.get_mut(TRAVEL) {
                    t.fields.insert("active".into(), false.into());
                }
            }
            for c in staged.entities.values_mut() {
                if let Some(t) = c.get_mut(TALK) {
                    t.fields.insert("active".into(), false.into());
                }
            }
            let chain = match talk::lines(world, text(f, "first")) {
                Ok(c) => c,
                Err(e) => {
                    findings.push(e);
                    continue;
                }
            };
            // Stream poses through the check instead of retaining whole worlds for a walk.
            let mut check_pose = |pose: &World| {
                for line in &chain {
                    let mut test = pose.clone();
                    let t = &mut test
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .get_mut(TALK)
                        .unwrap()
                        .fields;
                    t.extend(
                        serde_json::json!({"enabled":true,"active":true,"current":line})
                            .as_object()
                            .unwrap()
                            .clone(),
                    );
                    let scene = self.scene(&test, alpha, size, 0.0);
                    let prefix = format!("place `{place}`, talk `{id}`, line `{line}`");
                    findings.extend(
                        self.screens
                            .problems
                            .iter()
                            .map(|p| format!("{prefix}: {p}")),
                    );
                    if let Err(e) = self.focus(&test, speaker, scene.camera, size, alpha) {
                        findings.push(format!("{prefix}: {e}"));
                    }
                }
            };
            check_pose(&staged);
            while walks
                && staged.entities.values().any(|c| {
                    c.get(WALK)
                        .is_some_and(|w| w.fields["active"] == true && w.fields["place"] == place)
                })
            {
                let before = staged.clone();
                staged.tick += 1;
                galaxy_sim::places::advance_walks(&mut staged);
                galaxy_sim::services::ground(&mut staged);
                let mut halfway = staged.clone();
                for (e, c) in &mut halfway.entities {
                    if let Some(p) = c.get_mut(POSITION)
                        && let Some(old) = before.component(e, POSITION)
                    {
                        for k in ["x", "y", "height"] {
                            p.fields.insert(
                                k.into(),
                                ((p.fields[k].as_f64().unwrap_or(0.0)
                                    + old.fields[k].as_f64().unwrap_or(0.0))
                                    * 0.5)
                                    .into(),
                            );
                        }
                    }
                }
                check_pose(&halfway);
                check_pose(&staged);
            }
        }
        findings.sort();
        findings.dedup();
        findings
    }

    fn focus(
        &self,
        world: &World,
        speaker: &str,
        camera: Camera,
        size: Vec2,
        alpha: f32,
    ) -> Result<(), String> {
        let part = world
            .component(speaker, FOCUS)
            .map(|c| text(&c.fields, "part"))
            .unwrap_or(if world.component(speaker, BEING).is_some() {
                "face"
            } else {
                ""
            });
        if part.is_empty() {
            return Ok(());
        }
        let geometry = self.geometry(world, alpha);
        let g = geometry
            .iter()
            .find(|g| g.id == speaker)
            .ok_or_else(|| format!("speaker `{speaker}` focus `{part}` has no visible model"))?;
        let model = text(&world.component(speaker, MODEL).unwrap().fields, "model");
        let range=self.assets.parts.get(model).and_then(|p|p.iter().find(|p|p.name==part))
            .ok_or_else(||format!("speaker `{speaker}` has no focus mesh part `{part}`; name a part or set engine.focus.part empty"))?;
        let mut focus = g.mesh.clone();
        focus
            .indices
            .retain(|i| range.vertices.contains(&(*i as usize)));
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for v in &focus.vertices[range.vertices.clone()] {
            let p = g.transform.transform_point3(Vec3::from(v.position));
            let clip = camera.view_proj(size.x / size.y) * p.extend(1.0);
            let screen = camera
                .world_to_screen(p, size)
                .ok_or_else(|| format!("speaker `{speaker}` focus `{part}` is off screen"))?;
            if clip.z < 0.0
                || clip.z > clip.w
                || screen.x < 0.0
                || screen.y < 0.0
                || screen.x >= size.x
                || screen.y >= size.y
            {
                return Err(format!("speaker `{speaker}` focus `{part}` is off screen"));
            }
            lo = lo.min(screen);
            hi = hi.max(screen);
        }
        let mut pixels = 0;
        for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
            for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let (o, d) = camera.screen_ray(point, size);
                let Some(t) = focus.raycast(
                    g.inverse.transform_point3(o),
                    g.inverse.transform_vector3(d),
                ) else {
                    continue;
                };
                pixels += 1;
                if let Some(w) = self.screens.hit(point) {
                    return Err(format!(
                        "speaker `{speaker}` focus `{part}` is covered by {}",
                        w.path
                    ));
                }
                for other in &geometry {
                    if other.hit(o, d).is_some_and(|near| near + 0.001 < t) {
                        return Err(format!(
                            "speaker `{speaker}` focus `{part}` is covered by geometry of `{}`",
                            other.id
                        ));
                    }
                }
            }
        }
        if pixels == 0 {
            return Err(format!(
                "speaker `{speaker}` focus `{part}` has no visible front-facing pixels"
            ));
        }
        Ok(())
    }
}
