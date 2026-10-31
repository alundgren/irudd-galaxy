//! The lab: runs the screen when something changed, draws it every frame, and turns the
//! pointer into clicks, drags and drops. Shared by the window and the shot run.

use std::path::PathBuf;
use std::time::Instant;

use render::glam::Vec2;
use render::{Color, Panel, Rect, Renderer, Scene, Text, palette};
use serde_json::Value as Json;

use crate::script::Host;
use crate::ui::draw::{Act, Dragging, Drawer, Hit, Ix};
use crate::ui::layout::Measure;
use crate::ui::rules::Problems;
use crate::ui::{self, Built, style};
use crate::world::World;

pub struct RMeasure<'a>(pub &'a mut Renderer);

impl Measure for RMeasure<'_> {
    fn text(&mut self, text: &str, size: f32, weight: u16, wrap: Option<f32>) -> Vec2 {
        let mut t = Text::new(text, Vec2::ZERO, size).weight(weight);
        if let Some(w) = wrap {
            t = t.wrap(w);
        }
        self.0.measure_text(&t)
    }

    fn missing(&mut self, text: &str) -> Vec<char> {
        self.0.missing_glyphs(&Text::new(text, Vec2::ZERO, 16.0))
    }
}

#[derive(Clone, Debug)]
struct Press {
    path: String,
    act: Act,
    at: Vec2,
}

/// What the lab measures about the screen model.
#[derive(Clone, Debug, Default)]
pub struct Measures {
    /// Screen runs while the last drag was held, from pick-up to let-go.
    pub runs_last_drag: u64,
    pub drags: u64,
    pub last_layout_ms: f64,
    pub max_layout_ms: f64,
    pub frame_ms: f32,
}

pub struct App {
    pub host: Host,
    pub world: World,
    built: Built,
    dirty: bool,
    pub ix: Ix,
    press: Option<Press>,
    drag_src: Option<(Option<String>, Json)>,
    drag_runs_from: u64,
    hits: Vec<Hit>,
    frame_problems: Problems,
    pub show_stats: bool,
    pub measures: Measures,
    pub events: Vec<String>,
}

impl App {
    pub fn new(r: &mut Renderer, dir: PathBuf) -> App {
        App {
            host: Host::new(dir),
            world: World::new(r),
            built: Built::empty(),
            dirty: true,
            ix: Ix::default(),
            press: None,
            drag_src: None,
            drag_runs_from: 0,
            hits: Vec::new(),
            frame_problems: Problems::default(),
            show_stats: false,
            measures: Measures::default(),
            events: Vec::new(),
        }
    }

    /// Every rule break: from reading and layout, and from this frame's placing.
    pub fn problems(&self) -> Problems {
        let mut p = self.built.problems.clone();
        p.extend(&self.frame_problems);
        p
    }

    fn rebuild(&mut self, r: &mut Renderer) {
        self.dirty = false;
        let Some(value) = self.host.view() else {
            return;
        };
        let viewport = r.logical_size();
        let t = Instant::now();
        let built = ui::build(self.host.lua(), &value, &mut RMeasure(r), viewport);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.measures.last_layout_ms = ms;
        self.measures.max_layout_ms = self.measures.max_layout_ms.max(ms);
        self.built = built;
        let root = &self.built.root;
        self.world.apply(
            r,
            root.scene.as_deref(),
            root.focus.as_deref(),
            &root.ship,
            viewport,
        );
    }

    pub fn resized(&mut self) {
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.host.reset();
        self.dirty = true;
    }

    /// One frame: run the screen if something changed, move the scene on, draw the UI.
    pub fn frame(&mut self, r: &mut Renderer, dt: f32) -> Scene {
        if self.host.poll_reload() {
            self.dirty = true;
        }
        if self.dirty {
            self.rebuild(r);
        }
        self.world.step(dt);
        let viewport = r.logical_size();
        let anchors = self.world.anchors(viewport);
        // The ghost part on the socket the dragged part is over.
        let ghost = self.ghost();
        self.world
            .set_ghost(r, ghost.as_ref().map(|(l, s)| (l.as_str(), s.as_str())));
        let out = {
            let mut m = RMeasure(r);
            let mut d = Drawer::new(
                &mut m,
                &self.ix,
                &anchors,
                viewport,
                self.built.placed.clone(),
            );
            d.screen(&self.built.root);
            d.pointer_layer(&self.built.root);
            d.out
        };
        self.hits = out.hits;
        self.frame_problems = out.problems;
        self.refresh_hover();
        let mut scene = self.world.scene(viewport);
        let [base, mid, top] = out.layers;
        scene.overlay.panels = base.panels;
        scene.overlay.texts = base.texts;
        scene.overlay.upper = [mid, top];
        self.lab_layer(&mut scene, r);
        scene
    }

    fn ghost(&self) -> Option<(String, String)> {
        let d = self.ix.drag.as_ref()?;
        let over = d.over.as_ref()?;
        let target = ui::draw::find_path(&self.built.root, over)?;
        if target.ok != Some(true) {
            return None;
        }
        let socket = target.anchor.as_deref()?.strip_prefix("socket.")?;
        let (_, args) = self.drag_src.as_ref()?;
        let look = args.get("look")?.as_str()?;
        Some((look.to_string(), socket.to_string()))
    }

    fn top_hit(&self, p: Vec2) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.area.contains(p))
    }

    fn refresh_hover(&mut self) {
        let Some(p) = self.ix.pointer else {
            self.ix.hover = None;
            self.world.hover = None;
            return;
        };
        if let Some(d) = &mut self.ix.drag {
            d.over = self
                .hits
                .iter()
                .rev()
                .find(|h| h.area.contains(p) && matches!(h.act, Act::Drop { .. }))
                .map(|h| h.path.clone());
            self.ix.hover = None;
            self.world.hover = None;
            return;
        }
        let hit = self.top_hit(p).filter(|h| h.act != Act::Block).cloned();
        self.world.hover = hit
            .as_ref()
            .and_then(|h| h.anchor.as_deref())
            .and_then(|a| a.strip_prefix("module."))
            .map(str::to_string);
        self.ix.hover = hit.map(|h| h.path);
    }

    fn send(&mut self, on: &str, args: &Json) {
        self.host.event(on, args);
        self.events.push(format!("{on} {args}"));
        self.dirty = true;
    }

    pub fn pointer_move(&mut self, p: Vec2) {
        self.ix.pointer = Some(p);
        if self.ix.drag.is_none()
            && let Some(press) = self.press.clone()
            && matches!(press.act, Act::Drag { .. })
            && p.distance(press.at) > 5.0
        {
            self.begin_drag(press);
        }
        self.refresh_hover();
    }

    pub fn pointer_left(&mut self) {
        self.ix.pointer = None;
        self.refresh_hover();
    }

    pub fn pointer_down(&mut self) {
        if self.world.gliding() {
            // A click during a glide lands it.
            self.world.land();
        }
        let Some(p) = self.ix.pointer else { return };
        self.press = self.top_hit(p).map(|h| Press {
            path: h.path.clone(),
            act: h.act.clone(),
            at: p,
        });
        self.ix.pressed = self.press.as_ref().map(|p| p.path.clone());
    }

    fn begin_drag(&mut self, press: Press) {
        let Act::Drag { on, args, cancel } = press.act else {
            return;
        };
        self.ix.drag = Some(Dragging {
            path: press.path,
            over: None,
        });
        self.ix.pressed = None;
        self.drag_src = Some((cancel, args.clone()));
        self.drag_runs_from = self.host.stats.view_runs;
        self.measures.drags += 1;
        if let Some(on) = on {
            self.send(&on, &args);
        }
    }

    pub fn pointer_up(&mut self) {
        let press = self.press.take();
        self.ix.pressed = None;
        if let Some(drag) = self.ix.drag.take() {
            self.end_drag(drag);
            return;
        }
        let (Some(press), Some(p)) = (press, self.ix.pointer) else {
            return;
        };
        if let Some(h) = self.top_hit(p).cloned()
            && h.path == press.path
            && let Act::Click { on, args } = h.act
        {
            self.send(&on, &args);
        }
    }

    fn end_drag(&mut self, drag: Dragging) {
        let (cancel, src_args) = self.drag_src.take().unwrap_or((None, Json::Null));
        // Runs while held: the pick-up's own run, and any since.
        self.measures.runs_last_drag =
            self.host.stats.view_runs - self.drag_runs_from + u64::from(self.dirty);
        let target = drag
            .over
            .and_then(|o| self.hits.iter().rev().find(|h| h.path == o).cloned());
        match target.map(|t| t.act) {
            Some(Act::Drop {
                on: Some(on),
                args,
                ok: true,
            }) => {
                let mut merged = src_args.as_object().cloned().unwrap_or_default();
                if let Json::Object(a) = args {
                    merged.extend(a);
                }
                self.send(&on, &Json::Object(merged));
            }
            _ => {
                if let Some(c) = cancel {
                    self.send(&c, &src_args);
                }
            }
        }
    }

    /// Esc or right-click: drops a held part where it was, else the screen's `back`.
    pub fn back(&mut self) {
        if let Some(drag) = self.ix.drag.take() {
            let mut d = drag;
            d.over = None;
            self.end_drag(d);
            return;
        }
        if let Some(b) = self.built.root.back.clone() {
            self.send(&b, &Json::Null);
        }
    }

    /// Rule breaks and script errors along the top, and the measures (F3).
    fn lab_layer(&mut self, scene: &mut Scene, r: &mut Renderer) {
        let problems = self.problems();
        let top = &mut scene.overlay.upper[1];
        for p in &problems.0 {
            if let Some(rect) = p.rect {
                top.panels
                    .push(style::ring(rect, style::safe::vermilion(), 3.0, 6.0, 8.0));
            }
        }
        let mut lines: Vec<String> = Vec::new();
        if let Some(e) = &self.host.error {
            lines.push(format!("Script: {e}"));
        }
        for p in problems.0.iter().take(6) {
            lines.push(format!("{}: {}", p.path, p.what));
        }
        if problems.len() > 6 {
            lines.push(format!("... and {} more", problems.len() - 6));
        }
        if self.show_stats {
            let s = &self.host.stats;
            let m = &self.measures;
            lines.push(format!(
                "Screen runs {} (last {:.2} ms, max {:.2} ms) · events {} · layout {:.2} ms (max {:.2}) · frame {:.1} ms",
                s.view_runs, s.last_view_ms, s.max_view_ms, s.event_runs, m.last_layout_ms, m.max_layout_ms, m.frame_ms
            ));
            lines.push(format!(
                "Drags {} · screen runs while the last drag was held: {}",
                m.drags, m.runs_last_drag
            ));
        }
        if lines.is_empty() {
            return;
        }
        let viewport = r.logical_size();
        let w = (viewport.x - 40.0).min(900.0);
        let mut y = 8.0;
        let mut texts = Vec::new();
        let mut h = 0.0;
        for l in &lines {
            let t = Text::new(l.clone(), Vec2::new(0.0, 0.0), 14.0)
                .weight(600)
                .wrap(w - 20.0);
            h += r.measure_text(&t).y + 2.0;
            texts.push(t);
        }
        let bar = Rect::new((viewport.x - w) * 0.5, y, w, h + 12.0);
        top.panels.push(Panel {
            fill: Color::hex(0x1b1424).with_alpha(0.94),
            border: if problems.is_empty() && self.host.error.is_none() {
                palette::GLASS
            } else {
                style::safe::vermilion()
            },
            border_px: 2.0,
            radius: 8.0,
            shadow: false,
            ..Panel::new(bar)
        });
        y += 6.0;
        for mut t in texts {
            t.pos = Vec2::new(bar.x + 10.0, y);
            t.color = Color::WHITE;
            y += r.measure_text(&t).y + 2.0;
            top.texts.push(t);
        }
    }

    /// The centre of the widget with this id: its hit area, or its rectangle.
    pub fn locate(&self, id: &str) -> Option<Vec2> {
        let n = self.built.root.find(id)?;
        if let Some(h) = self.hits.iter().rev().find(|h| h.path == n.path) {
            return Some(h.area.center());
        }
        Some(n.rect.center())
    }
}
