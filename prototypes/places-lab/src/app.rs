//! The lab: keeps the place model, runs the screen when something changed, composes the
//! view from the four chosen variants every frame, and turns the pointer into clicks,
//! drags and drops. Shared by the window and the shot run.

use std::path::PathBuf;
use std::time::Instant;

use mlua::{LuaSerdeExt, Table, Value};
use render::glam::Vec2;
use render::{Align, Color, Panel, Rect, Renderer, RgbaImage, Scene, Text, UiLayer, palette};
use serde_json::{Value as Json, json};

use crate::combo::Combo;
use crate::notes::{self, Draft};
use crate::places::{Kind, Places};
use crate::script::{Cmd, Host};
use crate::stage::Stage;
use crate::talk::Line;
use crate::ui::draw::{Act, Anchors, Area, Dragging, Drawer, Hit, Ix};
use crate::ui::layout::Measure;
use crate::ui::rules::{self, Problems};
use crate::ui::tree::Node;
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

/// Something the pointer can act on, and a point where it is the top hit.
#[derive(Clone, Debug)]
pub struct Target {
    pub path: String,
    pub id: Option<String>,
    pub label: String,
    pub does: String,
    pub hint: Option<String>,
    pub at: Vec2,
}

/// Points to try inside an area: the centre first, then a spread.
fn samples(area: Area) -> Vec<Vec2> {
    match area {
        Area::Rect(r) => {
            let mut v = vec![r.center()];
            for fy in [0.25, 0.5, 0.75] {
                for fx in [0.15, 0.5, 0.85] {
                    v.push(Vec2::new(r.x + r.w * fx, r.y + r.h * fy));
                }
            }
            v
        }
        Area::Circle(c, rad) => {
            let mut v = vec![c];
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                v.push(c + Vec2::new(dx, dy) * rad * 0.6);
            }
            v
        }
    }
}

/// A widget's words: its text, its sign's title, or its first texts inside.
fn label(n: &Node) -> String {
    if let Some(t) = &n.text {
        return t.clone();
    }
    if let Some(s) = &n.sign {
        return s.title.clone();
    }
    let mut words = Vec::new();
    collect_texts(n, &mut words);
    words.truncate(3);
    words.join(" · ")
}

fn collect_texts(n: &Node, out: &mut Vec<String>) {
    for c in &n.children {
        if let Some(t) = &c.text {
            out.push(t.clone());
        }
        collect_texts(c, out);
    }
}

fn does(act: &Act) -> String {
    match act {
        Act::Click { on, args } if args.is_null() => format!("click: {on}"),
        Act::Click { on, args } => format!("click: {on} {args}"),
        Act::Drag { .. } => "drag".into(),
        Act::Drop { ok: true, .. } => "drop: accepts what is held".into(),
        Act::Drop { .. } => "drop: refuses what is held, or not a target".into(),
        Act::Hint => "hover: hint".into(),
        Act::Block => "surface".into(),
    }
}

#[derive(Clone, Debug)]
struct Press {
    path: String,
    act: Act,
    at: Vec2,
}

/// What the lab measures: the screen model's costs, and what a player feels.
#[derive(Clone, Debug, Default)]
pub struct Measures {
    /// Screen runs while the last drag was held, from pick-up to let-go.
    pub runs_last_drag: u64,
    pub drags: u64,
    pub last_layout_ms: f64,
    pub max_layout_ms: f64,
    pub frame_ms: f32,
    /// Clicks that did something (sent an event), all told.
    pub clicks: u64,
    /// Every move: from, to, and how long its view took to land, seconds.
    pub moves: Vec<(String, String, f32)>,
    /// Frames a line was shown, and frames its speaker's face was off screen or covered.
    pub line_frames: u64,
    pub speaker_hidden: u64,
}

pub struct App {
    pub host: Host,
    pub world: World,
    pub model: Places,
    pub combo: Combo,
    built: Built,
    dirty: bool,
    pub ix: Ix,
    press: Option<Press>,
    drag_src: Option<(Option<String>, Json)>,
    drag_runs_from: u64,
    hits: Vec<Hit>,
    frame_problems: Problems,
    /// Problems from turning the screen's talk into widgets.
    lower_problems: Problems,
    floats: Vec<(usize, Rect, String)>,
    pub anchors: Anchors,
    pub show_stats: bool,
    pub measures: Measures,
    pub events: Vec<String>,
    /// The line being said, as the talk variant was given it.
    pub line: Option<Line>,
    /// Seconds since the lab started (the frames' own clock).
    pub time: f32,
    /// A move is on its way: its start time, for the measures.
    moving: Option<f32>,
    /// F8: the note being written, with its shot.
    pub note: Option<Draft>,
    /// F8 was pressed: the window grabs a shot of the next frame for the note.
    pub want_note: bool,
    /// The last saved note, shown for a moment.
    pub noted: Option<(String, f32)>,
}

fn read_places(host: &mut Host) -> Result<Places, String> {
    Places::read(&host.places()?)
}

impl App {
    pub fn new(r: &mut Renderer, dir: PathBuf, combo: Combo) -> Result<App, String> {
        let mut host = Host::new(dir);
        let model = read_places(&mut host)?;
        let world = World::new(r, &model);
        let mut combo = combo;
        combo.setup(r, &world, &model);
        Ok(App {
            host,
            world,
            model,
            combo,
            built: Built::empty(),
            dirty: true,
            ix: Ix::default(),
            press: None,
            drag_src: None,
            drag_runs_from: 0,
            hits: Vec::new(),
            frame_problems: Problems::default(),
            lower_problems: Problems::default(),
            floats: Vec::new(),
            anchors: Anchors::new(),
            show_stats: false,
            measures: Measures::default(),
            events: Vec::new(),
            line: None,
            time: 0.0,
            moving: None,
            note: None,
            want_note: false,
            noted: None,
        })
    }

    /// Every rule break: from reading and layout, and from this frame's placing.
    pub fn problems(&self) -> Problems {
        let mut p = self.built.problems.clone();
        p.extend(&self.lower_problems);
        p.extend(&self.frame_problems);
        p
    }

    /// The view's move is still on its way: the screen is hidden.
    pub fn busy(&self) -> bool {
        self.combo.travel().busy()
    }

    fn rebuild(&mut self, r: &mut Renderer) {
        self.dirty = false;
        self.host.set_world(self.model.snapshot());
        let Some(value) = self.host.view() else {
            return;
        };
        let value = self.lower(value);
        let viewport = r.logical_size();
        let t = Instant::now();
        let built = ui::build(self.host.lua(), &value, &mut RMeasure(r), viewport);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.measures.last_layout_ms = ms;
        self.measures.max_layout_ms = self.measures.max_layout_ms.max(ms);
        self.built = built;
        self.world.apply_ship(r, &self.built.root.ship);
    }

    /// The screen's `ui.talk{...}` becomes the talk variant's widgets, and the life
    /// variant adds its own.
    fn lower(&mut self, value: Value) -> Value {
        self.lower_problems = Problems::default();
        self.line = None;
        let Value::Table(t) = &value else {
            return value;
        };
        let mut talk_lines: Vec<(usize, Line)> = Vec::new();
        let mut children: Vec<Value> = Vec::new();
        let read: mlua::Result<()> = (|| {
            for k in 1..=t.raw_len() {
                let v: Value = t.raw_get(k)?;
                if let Value::Table(c) = &v
                    && c.raw_get::<Option<String>>("kind")?.as_deref() == Some("talk")
                {
                    let line = self.read_line(c, &format!("screen/talk[{k}]"))?;
                    talk_lines.push((children.len(), line));
                    continue;
                }
                children.push(v);
            }
            Ok(())
        })();
        if let Err(e) = read {
            self.lower_problems.add("screen", format!("talk: {e}"));
            return value;
        }
        let result: mlua::Result<Value> = (|| {
            let lua = self.host.lua();
            let out = lua.create_table()?;
            for pair in t.clone().pairs::<Value, Value>() {
                let (k, v) = pair?;
                if let Value::String(_) = k {
                    out.raw_set(k, v)?;
                }
            }
            let mut extra: Vec<Vec<Table>> = vec![Vec::new(); children.len() + 1];
            // The room's own widgets first, so the screen's draw over them.
            let here = self.model.here();
            if here.kind == Kind::Interior {
                for w in self.combo.room().widgets(here, &self.model) {
                    extra[0].push(w.to_lua(lua)?);
                }
            }
            for (at, line) in &talk_lines {
                for w in self.combo.talk().widgets(line) {
                    extra[*at].push(w.to_lua(lua)?);
                }
            }
            for w in self.combo.life().widgets(&self.model) {
                extra[children.len()].push(w.to_lua(lua)?);
            }
            let mut i = 1;
            for (n, slot) in extra.into_iter().enumerate() {
                for w in slot {
                    out.raw_set(i, w)?;
                    i += 1;
                }
                if let Some(c) = children.get(n) {
                    out.raw_set(i, c.clone())?;
                    i += 1;
                }
            }
            Ok(Value::Table(out))
        })();
        self.line = talk_lines.pop().map(|(_, l)| l);
        match result {
            Ok(v) => v,
            Err(e) => {
                self.lower_problems.add("screen", format!("talk: {e}"));
                value
            }
        }
    }

    fn read_line(&mut self, t: &Table, path: &str) -> mlua::Result<Line> {
        let lua = self.host.lua();
        let who: String = t.raw_get::<Option<String>>("who")?.unwrap_or_default();
        let person = self.model.person(&who);
        if person.is_none() {
            self.lower_problems
                .add(path, format!("talk: no person {who:?} in the place model"));
        }
        let before = match t.raw_get::<Value>("before")? {
            v @ Value::Table(_) => lua.from_value::<Vec<String>>(v).unwrap_or_default(),
            _ => Vec::new(),
        };
        Ok(Line {
            name: person.map(|p| p.name.clone()).unwrap_or_else(|| who.clone()),
            role: person.map(|p| p.role.clone()).unwrap_or_default(),
            who,
            text: t.raw_get::<Option<String>>("text")?.unwrap_or_default(),
            more: t.raw_get::<Option<bool>>("more")?.unwrap_or(false),
            on: t.raw_get::<Option<String>>("on")?,
            args: lua
                .from_value::<Json>(t.raw_get::<Value>("args")?)
                .unwrap_or(Json::Null),
            before,
        })
    }

    pub fn resized(&mut self) {
        self.dirty = true;
    }

    /// F5: starts again, the place model and the scripts both.
    pub fn reset(&mut self, r: &mut Renderer) {
        self.host.reset();
        match read_places(&mut self.host) {
            Ok(m) => {
                self.world = World::new(r, &m);
                self.model = m;
                self.combo.setup(r, &self.world, &self.model);
            }
            Err(e) => self.host.error = Some(e),
        }
        self.combo.travel_mut().finish();
        self.moving = None;
        self.dirty = true;
    }

    /// One frame: run the screen if something changed, move time on, compose the view
    /// from the variants, draw the screen over it.
    pub fn frame(&mut self, r: &mut Renderer, dt: f32) -> Scene {
        if self.host.poll_reload() {
            match read_places(&mut self.host) {
                Ok(m) => self.model.reload(m),
                Err(e) => self.host.error = Some(e),
            }
            self.dirty = true;
        }
        if self.dirty {
            self.rebuild(r);
        }
        self.time += dt;
        let version = self.model.version;
        self.combo.life_mut().step(&mut self.model, dt);
        self.combo.travel_mut().step(dt);
        self.combo.room_mut().step(dt, &self.model);
        if let Some(t0) = self.moving
            && !self.busy()
        {
            self.arrived(t0);
        }
        if self.model.version != version {
            self.dirty = true;
        }
        if self.dirty {
            self.rebuild(r);
        }
        self.world.step(dt, None);
        let viewport = r.logical_size();
        let busy = self.busy();
        let mut shown = {
            let stage = Stage {
                world: &self.world,
                model: &self.model,
                room: self.combo.room(),
                life: self.combo.life(),
                viewport,
                speaking: if busy {
                    None
                } else {
                    self.line.as_ref().map(|l| l.who.as_str())
                },
            };
            self.combo.travel().compose(&stage)
        };
        let line = if busy { None } else { self.line.clone() };
        self.combo.talk_mut().frame(&mut shown, line.as_ref(), dt);
        self.anchors = shown.anchors(&self.world.hull, viewport);
        // The ghost part on the socket the dragged part is over.
        let ghost = self.ghost();
        self.world
            .set_ghost(r, ghost.as_ref().map(|(l, s)| (l.as_str(), s.as_str())));
        let mut scene = shown.scene;
        if busy {
            self.hits.clear();
            self.floats.clear();
            self.frame_problems = Problems::default();
            self.ix.hover = None;
            self.world.hover = None;
        } else {
            let out = {
                let mut m = RMeasure(r);
                let mut d = Drawer::new(
                    &mut m,
                    &self.ix,
                    &self.anchors,
                    viewport,
                    self.built.placed.clone(),
                );
                d.screen(&self.built.root);
                d.pointer_layer(&self.built.root);
                d.out
            };
            self.hits = out.hits;
            self.frame_problems = out.problems;
            self.floats = out.floats;
            self.refresh_hover();
            let [base, mid, top] = out.layers;
            // The travel variant's underlay goes beneath the screen's panels.
            let mut under = UiLayer::default();
            {
                let stage = Stage {
                    world: &self.world,
                    model: &self.model,
                    room: self.combo.room(),
                    life: self.combo.life(),
                    viewport,
                    speaking: None,
                };
                self.combo.travel().underlay(&stage, &mut under);
            }
            under.panels.extend(base.panels);
            scene.overlay.panels = under.panels;
            scene.overlay.texts = base.texts;
            scene.overlay.upper = [mid, top];
            self.check_speaker(viewport);
        }
        {
            let stage = Stage {
                world: &self.world,
                model: &self.model,
                room: self.combo.room(),
                life: self.combo.life(),
                viewport,
                speaking: None,
            };
            self.combo
                .travel()
                .overlay(&stage, &mut scene.overlay.upper[1]);
        }
        self.lab_layer(&mut scene, r);
        scene
    }

    /// The move's view has landed: the life variant and the scripts hear of it.
    fn arrived(&mut self, t0: f32) {
        self.moving = None;
        let from = self.model.was.clone().unwrap_or_default();
        let here = self.model.here.clone();
        self.measures
            .moves
            .push((from, here.clone(), self.time - t0));
        self.combo.life_mut().arrived(&mut self.model, &here);
        if self.host.handles("arrive") {
            self.send("arrive", &json!({ "place": here }));
        }
        self.dirty = true;
    }

    /// While a line shows, its speaker's face must be on screen and uncovered.
    fn check_speaker(&mut self, viewport: Vec2) {
        let Some(line) = &self.line else { return };
        self.measures.line_frames += 1;
        let path = format!("talk/{}", line.who);
        let face = self.anchors.get(&format!("person.{}.face", line.who)).copied();
        let broke = match face {
            None => Some(format!("{} is speaking but isn't in this view", line.name)),
            Some(a) => {
                let r = a.radius * 0.7;
                let face = Rect::new(a.at.x - r, a.at.y - r, r * 2.0, r * 2.0);
                let screen = Rect::new(0.0, 0.0, viewport.x, viewport.y);
                if !rules::contains(screen, face) {
                    Some(format!("{} is speaking off screen", line.name))
                } else if let Some((_, _, what)) = self
                    .floats
                    .iter()
                    .find(|(_, o, _)| rules::overlaps(*o, face))
                {
                    Some(format!("{}'s face is covered by {what}", line.name))
                } else if self.built.placed.iter().any(|o| rules::overlaps(*o, face)) {
                    Some(format!("{}'s face is under a panel", line.name))
                } else {
                    None
                }
            }
        };
        if let Some(what) = broke {
            self.measures.speaker_hidden += 1;
            match face {
                Some(a) => self.frame_problems.at(
                    &path,
                    Rect::new(a.at.x - 20.0, a.at.y - 20.0, 40.0, 40.0),
                    what,
                ),
                None => self.frame_problems.add(&path, what),
            }
        }
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
            .and_then(|a| a.strip_prefix("place."))
            .map(str::to_string);
        self.ix.hover = hit.map(|h| h.path);
    }

    pub fn send(&mut self, on: &str, args: &Json) {
        self.host.set_world(self.model.snapshot());
        self.host.event(on, args);
        self.events.push(format!("{on} {args}"));
        self.dirty = true;
        for cmd in self.host.take_cmds() {
            match cmd {
                Cmd::Go(place) => self.go(&place),
                Cmd::Move {
                    person,
                    place,
                    state,
                    spot,
                } => self.combo.life_mut().moved(
                    &mut self.model,
                    &person,
                    place.as_deref(),
                    state.as_deref(),
                    spot.as_deref(),
                ),
            }
        }
    }

    /// Moves the player; the travel variant shows the way.
    pub fn go(&mut self, place: &str) {
        let from = self.model.here.clone();
        if let Some(to) = self.model.go(place) {
            self.press = None;
            self.ix.pressed = None;
            self.ix.drag = None;
            self.ix.hover = None;
            self.world.hover = None;
            self.combo.travel_mut().start(&from, &to);
            self.moving = Some(self.time);
            self.dirty = true;
        }
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
        if self.busy() {
            // A click during a move lands it.
            self.combo.travel_mut().finish();
            return;
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
            self.measures.clicks += 1;
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

    /// Esc or right-click: lands a move, drops a held part where it was, else the
    /// screen's `back`.
    pub fn back(&mut self) {
        if self.busy() {
            self.combo.travel_mut().finish();
            return;
        }
        if let Some(drag) = self.ix.drag.take() {
            // The button may still be down: forget the press too, or moving would pick
            // the part up again.
            self.press = None;
            self.ix.pressed = None;
            let mut d = drag;
            d.over = None;
            self.end_drag(d);
            return;
        }
        if let Some(b) = self.built.root.back.clone() {
            self.send(&b, &Json::Null);
        }
    }

    /// Keys 1-4: the next variant of a choice (Shift: the previous one).
    pub fn cycle(&mut self, choice: usize, back: bool) {
        self.combo.cycle(choice, back);
        self.dirty = true;
    }

    /// F8 with its shot taken: starts a note.
    pub fn begin_note(&mut self, image: Option<RgbaImage>) {
        self.note = Some(Draft {
            text: String::new(),
            image,
            combo: self.combo.spec(),
            place: self.model.here.clone(),
            play_s: self.time,
            at: std::time::SystemTime::now(),
        });
    }

    /// Saves the note being written.
    pub fn save_note(&mut self) {
        if let Some(d) = self.note.take() {
            match notes::save(&notes::folder(), &d) {
                Ok(p) => {
                    self.noted = Some((format!("Note saved: {}", p.display()), self.time));
                }
                Err(e) => self.host.error = Some(format!("note: {e}")),
            }
        }
    }

    /// The combination in the corner, rule breaks and script errors along the top, the
    /// measures (F3), and the note being written (F8).
    fn lab_layer(&mut self, scene: &mut Scene, r: &mut Renderer) {
        let viewport = r.logical_size();
        let problems = self.problems();
        {
            // The combination in use, small in the bottom right corner.
            let top = &mut scene.overlay.upper[1];
            let label = Text::new(self.combo.label(), Vec2::ZERO, 13.0).weight(600);
            let size = r.measure_text(&label);
            let pill = Rect::new(
                viewport.x - size.x - 16.0,
                viewport.y - size.y - 12.0,
                size.x + 12.0,
                size.y + 6.0,
            );
            top.panels.push(Panel {
                fill: Color::hex(0x1b1424).with_alpha(0.72),
                border: Color::TRANSPARENT,
                border_px: 0.0,
                radius: 6.0,
                shadow: false,
                ..Panel::new(pill)
            });
            top.texts.push(Text {
                pos: Vec2::new(pill.x + 6.0, pill.y + 3.0),
                color: palette::CREAM,
                ..label
            });
            for p in &problems.0 {
                if let Some(rect) = p.rect {
                    top.panels
                        .push(style::ring(rect, style::safe::vermilion(), 3.0, 6.0, 8.0));
                }
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
        if let Some((s, t)) = &self.noted
            && self.time - t < 4.0
        {
            lines.push(s.clone());
        }
        if self.show_stats {
            let s = &self.host.stats;
            let m = &self.measures;
            lines.push(format!(
                "Combination {} · keys 1-4 change it (Shift: back) · F8 writes a note",
                self.combo.spec()
            ));
            lines.push(format!(
                "Here: {} · last move {} · clicks {}",
                self.model.here,
                m.moves
                    .last()
                    .map(|(a, b, t)| format!("{a} to {b} in {t:.2} s"))
                    .unwrap_or_else(|| "none yet".into()),
                m.clicks
            ));
            lines.push(format!(
                "Screen runs {} (last {:.2} ms, max {:.2} ms) · events {} · layout {:.2} ms (max {:.2}) · frame {:.1} ms",
                s.view_runs, s.last_view_ms, s.max_view_ms, s.event_runs, m.last_layout_ms, m.max_layout_ms, m.frame_ms
            ));
            lines.push(format!(
                "Lines shown for {} frames, speaker hidden on {} · drags {} (screen runs while held: {})",
                m.line_frames, m.speaker_hidden, m.drags, m.runs_last_drag
            ));
        }
        if !lines.is_empty() {
            let top = &mut scene.overlay.upper[1];
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
        if let Some(text) = self.note.as_ref().map(|d| d.text.clone()) {
            self.note_box(scene, r, &text, viewport);
        }
    }

    /// The note being written: a box in the middle, what is typed, and how to finish.
    fn note_box(&self, scene: &mut Scene, r: &mut Renderer, text: &str, viewport: Vec2) {
        let top = &mut scene.overlay.upper[1];
        let w = 640.0f32.min(viewport.x - 40.0);
        let head = format!(
            "Note on {} at {} · type, then Enter saves it, Esc drops it",
            self.combo.spec(),
            self.model.here
        );
        let body = Text::new(format!("{text}_"), Vec2::ZERO, 18.0)
            .weight(500)
            .wrap(w - 28.0);
        let bh = r.measure_text(&body).y.max(24.0);
        let rect = Rect::new(
            (viewport.x - w) * 0.5,
            viewport.y * 0.5 - 60.0,
            w,
            bh + 54.0,
        );
        top.panels.push(Panel {
            fill: Color::hex(0x1b1424).with_alpha(0.96),
            border: palette::MUSTARD,
            border_px: 2.0,
            radius: 10.0,
            shadow: true,
            ..Panel::new(rect)
        });
        top.texts.push(
            Text::new(head, Vec2::new(rect.x + 14.0, rect.y + 10.0), 14.0)
                .weight(600)
                .color(palette::CREAM),
        );
        top.texts.push(Text {
            pos: Vec2::new(rect.x + 14.0, rect.y + 36.0),
            color: Color::WHITE,
            align: Align::Left,
            ..body
        });
    }

    /// Everything the pointer can act on now, each with a point where it is the top hit,
    /// and the paths of those it can't reach anywhere (covered by something else).
    pub fn targets(&self) -> (Vec<Target>, Vec<String>) {
        let mut seen: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        let mut unreachable = Vec::new();
        for h in &self.hits {
            if h.act == Act::Block || seen.contains(&h.path.as_str()) {
                continue;
            }
            seen.push(&h.path);
            let reach =
                |p: Vec2| self.top_hit(p).map(|t| t.path.as_str()) == Some(h.path.as_str());
            let points: Vec<Vec2> = self
                .hits
                .iter()
                .filter(|o| o.path == h.path)
                .flat_map(|o| samples(o.area))
                .filter(|p| reach(*p))
                .collect();
            // Prefer a point with room round it, so a neighbour that moves a little (a
            // speaker's bubble as they nod) doesn't take the hover from a resting pointer.
            let roomy = |p: &&Vec2| {
                [(8.0, 0.0), (-8.0, 0.0), (0.0, 8.0), (0.0, -8.0)]
                    .iter()
                    .all(|&(x, y)| reach(**p + Vec2::new(x, y)))
            };
            let at = points.iter().find(roomy).or(points.first()).copied();
            match at {
                Some(at) => {
                    let node = ui::draw::find_path(&self.built.root, &h.path);
                    out.push(Target {
                        path: h.path.clone(),
                        id: node.and_then(|n| n.id.clone()),
                        label: node.map(label).unwrap_or_default(),
                        does: does(&h.act),
                        hint: h.hint.clone(),
                        at,
                    });
                }
                None => unreachable.push(h.path.clone()),
            }
        }
        (out, unreachable)
    }

    /// What is on screen now, as text an agent can read beside the picture.
    pub fn describe(&self) -> Json {
        let node = |path: &str| ui::draw::find_path(&self.built.root, path);
        let who = |path: &str| {
            let n = node(path);
            serde_json::json!({
                "path": path,
                "id": n.and_then(|n| n.id.clone()),
                "label": n.map(label).unwrap_or_default(),
            })
        };
        let rect = |r: Rect| [r.x.round(), r.y.round(), r.w.round(), r.h.round()];
        let (targets, unreachable) = self.targets();
        serde_json::json!({
            "pointer": self.ix.pointer.map(|p| [p.x.round(), p.y.round()]),
            "hovered": self.ix.hover.as_deref().map(who),
            "dragging": self.ix.drag.as_ref().map(|d| serde_json::json!({
                "what": who(&d.path),
                "over": d.over.as_deref().map(who),
            })),
            "shown": self.floats.iter().map(|(layer, r, what)| serde_json::json!({
                "what": what, "layer": layer, "rect": rect(*r),
            })).collect::<Vec<_>>(),
            "targets": targets.iter().map(|t| serde_json::json!({
                "id": t.id, "path": t.path, "label": t.label, "does": t.does,
                "hint": t.hint, "at": [t.at.x.round(), t.at.y.round()],
            })).collect::<Vec<_>>(),
            "unreachable": unreachable,
            "problems": self.problems().0.iter()
                .map(|p| format!("{}: {}", p.path, p.what)).collect::<Vec<_>>(),
            "script_error": self.host.error,
            "combo": self.combo.spec(),
            "here": self.model.here,
            "moving": self.busy(),
            "line": self.line.as_ref().map(|l| serde_json::json!({
                "who": l.who, "text": l.text, "more": l.more,
            })),
            "people_here": self.model.people_in(&self.model.here)
                .map(|p| p.id.clone()).collect::<Vec<_>>(),
            "anchors": self.anchors.iter()
                .filter(|(k, _)| k.starts_with("person.") || k.starts_with("place."))
                .map(|(k, a)| (k.clone(), serde_json::json!([a.at.x.round(), a.at.y.round(), a.radius.round()])))
                .collect::<serde_json::Map<_, _>>(),
        })
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
