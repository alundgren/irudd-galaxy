//! Bubbles share screen layout, paint and input; focus uses the same posed meshes as play.
mod focus;
pub(crate) use focus::{FocusCache, Posed};
#[cfg(test)]
mod measure;

use crate::{View, rooms};
use galaxy_api::engine::*;
use galaxy_mesh::MeshData;
use galaxy_render::{
    Camera,
    glam::{Mat4, Quat, Vec2, Vec3},
    screen_style::ScreenStyle,
};
use galaxy_script::screens::Widget;
use galaxy_sim::{
    Input, World,
    scenario::{Click, ClickTarget},
    talk::{self, text},
};
use std::collections::BTreeMap;

fn part(kind: &str, path: String, text: &str, size: f32) -> Widget {
    Widget {
        hover: None,
        kind: kind.into(),
        id: None,
        path,
        text: text.into(),
        width: None,
        height: None,
        x: None,
        y: None,
        size,
        pad: 0.0,
        explicit_pad: false,
        gap: 12.0,
        align: None,
        anchor: None,
        on: None,
        payload: Default::default(),
        children: vec![],
        columns: 1,
        value: String::new(),
        delta: 0.0,
        accepts: BTreeMap::new(),
        notes: BTreeMap::new(),
        weight: 500,
        trailing: false,
        yields: false,
        look: None,
        spans: Vec::new(),
        icon: None,
        icon_at: None,
        cue: None,
        resting: None,
    }
}

/// The active line's bubble: a heading (the talk's title, else the speaker's sign name),
/// an optional quieter subtitle and the line, then either a small Continue/Done marker
/// or the line's answer buttons. Without answers the whole bubble is one click target.
pub fn widgets(world: &World, style: &ScreenStyle) -> Vec<Widget> {
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
            let path = format!("{id}/talk");
            let title = Some(text(f, "title"))
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    world
                        .component(text(f, "speaker"), LIFE)
                        .map(|l| text(&l.fields, "name"))
                })
                .unwrap_or("");
            let mut children = Vec::new();
            if !title.is_empty() {
                let mut w = part("text", format!("{path}/title"), title, style.bubble_title);
                w.weight = style.bubble_title_weight.round() as u16;
                children.push(w);
            }
            let subtitle = text(f, "subtitle");
            if !subtitle.is_empty() {
                let mut w = part(
                    "text",
                    format!("{path}/subtitle"),
                    subtitle,
                    style.bubble_subtitle,
                );
                w.weight = 400;
                children.push(w);
            }
            children.push(part(
                "text",
                format!("{path}/line"),
                text(line, "text"),
                style.bubble_text,
            ));
            let choices = talk::choices(world, line_id);
            if choices.is_empty() {
                let mut w = part(
                    "text",
                    format!("{path}/marker"),
                    if text(line, "next").is_empty() {
                        "Done"
                    } else {
                        "Continue"
                    },
                    style.bubble_marker,
                );
                w.weight = 400;
                w.trailing = true;
                children.push(w);
            } else {
                let mut row = part("row", format!("{path}/choices"), "", style.bubble_text);
                row.gap = 8.0;
                for choice in choices {
                    let cf = &world.component(choice, TALK_CHOICE)?.fields;
                    let mut b = part(
                        "button",
                        format!("{path}/choices/{choice}"),
                        text(cf, "text"),
                        style.bubble_text,
                    );
                    b.id = Some(format!("choice.{choice}"));
                    b.pad = 12.0;
                    b.explicit_pad = true;
                    b.on = Some(TALK_CHOSEN.into());
                    b.payload = serde_json::json!({"talk": id, "line": line_id, "choice": choice})
                        .as_object()
                        .unwrap()
                        .clone();
                    row.children.push(b);
                }
                children.push(row);
            }
            let mut bubble = part("bubble", path, "", style.bubble_text);
            bubble.id = Some(format!("talk.{id}"));
            bubble.width = Some(f["width"].as_f64().unwrap_or(320.0) as f32);
            bubble.x = Some(f["x"].as_f64().unwrap_or(0.0) as f32);
            bubble.y = Some(f["y"].as_f64().unwrap_or(-12.0) as f32);
            bubble.pad = 16.0;
            bubble.gap = 6.0;
            bubble.anchor = Some(talk::anchor(f));
            if children.iter().all(|c| c.kind != "row") {
                bubble.on = Some(TALK_NEXT.into());
                bubble.payload = serde_json::json!({"talk": id, "line": line_id})
                    .as_object()
                    .unwrap()
                    .clone();
            }
            bubble.children = children;
            Some(bubble)
        })
        .collect()
}

pub(crate) struct Geometry {
    pub(crate) id: String,
    pub(crate) mesh: MeshData,
    pub(crate) transform: Mat4,
    pub(crate) inverse: Mat4,
    /// The mesh's bounds in its own space, padded well past rounding: a ray that misses
    /// them can't hit a triangle, so [`Geometry::reaches`] only skips sure misses.
    bounds: (Vec3, Vec3),
}
impl Geometry {
    pub(crate) fn new(id: String, mesh: MeshData, transform: Mat4) -> Self {
        let bounds = match mesh.bounds() {
            Some((lo, hi)) => {
                let pad = Vec3::splat((hi - lo).max_element() * 1e-3 + 1e-3);
                (lo - pad, hi + pad)
            }
            None => (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        };
        Self {
            id,
            mesh,
            transform,
            inverse: transform.inverse(),
            bounds,
        }
    }

    /// The ray in the mesh's own space, as every raycast here takes it.
    pub(crate) fn local(&self, origin: Vec3, direction: Vec3) -> (Vec3, Vec3) {
        (
            self.inverse.transform_point3(origin),
            self.inverse.transform_vector3(direction),
        )
    }

    /// False only when the local ray `origin + direction * t`, `t >= 0`, misses the
    /// padded bounds, so no triangle can be hit. Unsure cases (NaN) say true.
    pub(crate) fn reaches(&self, origin: Vec3, direction: Vec3) -> bool {
        let (lo, hi) = self.bounds;
        let (mut near, mut far) = (0.0f32, f32::INFINITY);
        for k in 0..3 {
            let (o, d) = (origin[k], direction[k]);
            if d == 0.0 {
                if o < lo[k] || o > hi[k] {
                    return false;
                }
                continue;
            }
            let (a, b) = ((lo[k] - o) / d, (hi[k] - o) / d);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        near.partial_cmp(&far) != Some(std::cmp::Ordering::Greater)
    }

    pub(crate) fn hit(&self, origin: Vec3, direction: Vec3) -> Option<f32> {
        self.hit_vertex(origin, direction).map(|(t, _)| t)
    }

    /// [`MeshData::raycast_vertex`] on the posed mesh, skipping sure misses.
    pub(crate) fn hit_vertex(&self, origin: Vec3, direction: Vec3) -> Option<(f32, usize)> {
        let (o, d) = self.local(origin, direction);
        if !self.reaches(o, d) {
            return None;
        }
        self.mesh.raycast_vertex(o, d)
    }
}

impl View {
    pub(crate) fn geometry(&self, world: &World, alpha: f32) -> Vec<Geometry> {
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
                Some(Geometry::new(
                    id.clone(),
                    self.assets
                        .models
                        .get(text(f, "model"))?
                        .morphed(self.morph(world, id, alpha)),
                    transform,
                ))
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
        self.screens.ui_cue_as(
            galaxy_api::ui::UiAction::Press,
            "speaker",
            None,
            conversation,
        );
        Some(Input {
            tick,
            source: "talk".into(),
            name: TALK_START.into(),
            payload: serde_json::json!({"talk":conversation})
                .as_object()
                .unwrap()
                .clone(),
            via: None,
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
            ClickTarget::Point { x, y } => self.screens.bot_click(click).or_else(|e| {
                if self.screens.bot_resting(click).is_some() {
                    return Err(e);
                }
                let point = Vec2::new(*x, *y);
                self.target_click(world, point, click.tick)
                    .or_else(|| self.talk_click(world, point, click.tick))
                    .ok_or_else(|| "click hit no widget, target or speaker".into())
            }),
            ClickTarget::Widget(name) => self
                .bot_target_click(world, name, click.tick)
                .unwrap_or_else(|| self.screens.bot_click(click)),
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
                // An active conversation hides its speaker's life sign. Check the
                // same pose without speech too, including intermediate walk poses.
                self.scene(pose, alpha, size, 0.0);
                findings.extend(
                    self.screens
                        .problems
                        .iter()
                        .map(|p| format!("place `{place}`, without talk: {p}")),
                );
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
        let key = focus::Key::new(
            speaker,
            part,
            range.vertices.clone(),
            camera,
            size,
            &geometry,
            &self.screens,
        );
        self.focus_cache.check(key, |scan| {
            let screens = &self.screens;
            let mesh = scan.mesh.get_or_insert_with(|| {
                let mut focus = g.mesh.clone();
                focus
                    .indices
                    .retain(|i| range.vertices.contains(&(*i as usize)));
                focus
            });
            let (lo, hi) = scan
                .bounds
                .get_or_insert_with(|| {
                    let mut lo = Vec2::splat(f32::MAX);
                    let mut hi = Vec2::splat(f32::MIN);
                    for v in &mesh.vertices[range.vertices.clone()] {
                        let p = g.transform.transform_point3(Vec3::from(v.position));
                        let clip = camera.view_proj(size.x / size.y) * p.extend(1.0);
                        let screen = camera.world_to_screen(p, size).ok_or_else(|| {
                            format!("speaker `{speaker}` focus `{part}` is off screen")
                        })?;
                        if clip.z < 0.0
                            || clip.z > clip.w
                            || screen.x < 0.0
                            || screen.y < 0.0
                            || screen.x >= size.x
                            || screen.y >= size.y
                        {
                            return Err(format!(
                                "speaker `{speaker}` focus `{part}` is off screen"
                            ));
                        }
                        lo = lo.min(screen);
                        hi = hi.max(screen);
                    }
                    Ok((lo, hi))
                })
                .clone()?;
            let covered = |point: Vec2, by: &Option<String>| {
                if let Some(w) = screens.hit(point) {
                    return Err(format!(
                        "speaker `{speaker}` focus `{part}` is covered by {}",
                        w.path
                    ));
                }
                if let Some(other) = by {
                    return Err(format!(
                        "speaker `{speaker}` focus `{part}` is covered by geometry of `{other}`"
                    ));
                }
                Ok(())
            };
            // Pixels an earlier test over these poses found, then the rest.
            for (point, by) in &scan.pixels {
                covered(*point, by)?;
            }
            // As the pixel loop always ran: `floor..=ceil` of the bounds, in i32.
            let (x0, x1) = (lo.x.floor() as i32 as i64, hi.x.ceil() as i32 as i64);
            let (y0, y1) = (lo.y.floor() as i32 as i64, hi.y.ceil() as i32 as i64);
            let width = (x1 - x0 + 1).max(0) as usize;
            let total = width * (y1 - y0 + 1).max(0) as usize;
            while scan.next < total {
                let i = scan.next;
                scan.next += 1;
                let point = Vec2::new(
                    (x0 + (i % width) as i64) as i32 as f32 + 0.5,
                    (y0 + (i / width) as i64) as i32 as f32 + 0.5,
                );
                let (o, d) = camera.screen_ray(point, size);
                let (local_o, local_d) = g.local(o, d);
                // The focus part is some of the speaker's triangles: inside its bounds.
                if !g.reaches(local_o, local_d) {
                    continue;
                }
                let Some(t) = mesh.raycast(local_o, local_d) else {
                    continue;
                };
                let by = geometry
                    .iter()
                    .find(|other| other.hit(o, d).is_some_and(|near| near + 0.001 < t))
                    .map(|other| other.id.clone());
                scan.pixels.push((point, by));
                let (point, by) = scan.pixels.last().unwrap();
                covered(*point, by)?;
            }
            if scan.pixels.is_empty() {
                return Err(format!(
                    "speaker `{speaker}` focus `{part}` has no visible front-facing pixels"
                ));
            }
            Ok(())
        })
    }
}
