//! Drawing a laid-out screen into overlay layers, and the hit areas for input.
//!
//! Everything here runs every frame in Rust: hover, pressed looks, the dragged ghost, the
//! drop verdicts the screen gave at the start of a drag, bubbles and signs following
//! their scene anchors. No Luau runs here.

use std::collections::HashMap;

use render::glam::Vec2;
use render::{Color, Panel, PanelShape, Rect, Text, UiLayer, palette};
use serde_json::Value as Json;

use super::layout::{self, Ctx, Measure};
use super::rules::{self, Problems};
use super::style::{self, Shape, safe};
use super::tree::{Kind, Node, Role, Sign, StateKind, TextStyle};

/// A point in the scene the UI can attach to, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub at: Vec2,
    /// How far the thing reaches around `at`.
    pub radius: f32,
}

pub type Anchors = HashMap<String, Anchor>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Area {
    Rect(Rect),
    Circle(Vec2, f32),
}

impl Area {
    pub fn contains(&self, p: Vec2) -> bool {
        match *self {
            Area::Rect(r) => p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h,
            Area::Circle(c, r) => c.distance(p) <= r,
        }
    }

    pub fn center(&self) -> Vec2 {
        match *self {
            Area::Rect(r) => r.center(),
            Area::Circle(c, _) => c,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Sends `on` with `args`.
    Click { on: String, args: Json },
    /// Can be picked up.
    Drag {
        on: Option<String>,
        args: Json,
        cancel: Option<String>,
    },
    /// Somewhere to let go; `ok` is the screen's verdict for the current drag.
    Drop {
        on: Option<String>,
        args: Json,
        ok: bool,
    },
    /// Shows a hint only.
    Hint,
    /// A surface: stops the pointer reaching the scene below.
    Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub area: Area,
    /// The widget's path: hover and press are remembered by it across screen runs.
    pub path: String,
    pub act: Act,
    pub hint: Option<String>,
    /// The scene anchor it belongs to (a hovered module lifts).
    pub anchor: Option<String>,
}

/// What the pointer is doing.
#[derive(Clone, Debug, Default)]
pub struct Ix {
    pub pointer: Option<Vec2>,
    pub hover: Option<String>,
    pub pressed: Option<String>,
    pub drag: Option<Dragging>,
}

#[derive(Clone, Debug)]
pub struct Dragging {
    /// The drag widget's path.
    pub path: String,
    /// The drop target under the pointer.
    pub over: Option<String>,
}

pub struct Out {
    /// 0: scene-attached (signs, socket labels) and placed layers; 1: bubbles and
    /// refusals; 2: the dragged ghost, previews, hints and the lab bar.
    pub layers: [UiLayer; 3],
    pub hits: Vec<Hit>,
    pub problems: Problems,
}

pub struct Drawer<'a> {
    pub m: &'a mut dyn Measure,
    pub ix: &'a Ix,
    pub anchors: &'a Anchors,
    pub screen: Vec2,
    /// The placed layers' rectangles: bubbles and signs keep clear of them.
    pub avoid: Vec<Rect>,
    /// Each hotspot's sign this frame, by anchor: a bubble rises from its speaker's sign.
    signs: HashMap<String, Rect>,
    /// Scene-attached work (socket rings and their labels): bubbles keep clear of it.
    work: Vec<Rect>,
    /// The widget being drawn, for rule reports.
    cur: String,
    pub out: Out,
}

const SKY_RING: f32 = 4.0;

/// A line of a sign: its text and font (size, weight).
type SignLine = (String, (f32, u16));

impl<'a> Drawer<'a> {
    pub fn new(
        m: &'a mut dyn Measure,
        ix: &'a Ix,
        anchors: &'a Anchors,
        screen: Vec2,
        avoid: Vec<Rect>,
    ) -> Self {
        Drawer {
            m,
            ix,
            anchors,
            screen,
            avoid,
            signs: HashMap::new(),
            work: Vec::new(),
            cur: String::new(),
            out: Out {
                layers: Default::default(),
                hits: Vec::new(),
                problems: Problems::default(),
            },
        }
    }

    fn panel(&mut self, layer: usize, p: Panel) {
        self.out.layers[layer].panels.push(p);
    }

    fn text(
        &mut self,
        layer: usize,
        s: &str,
        pos: Vec2,
        (size, weight): (f32, u16),
        color: Color,
        wrap: Option<f32>,
    ) {
        let missing = self.m.missing(s);
        if !missing.is_empty() {
            let chars: String = missing.iter().collect();
            let path = self.cur.clone();
            self.out
                .problems
                .add(&path, format!("the font can't draw {chars:?} in {s:?}"));
        }
        let mut t = Text::new(s, pos, size).weight(weight).color(color);
        if let Some(w) = wrap {
            // A hair of slack so a run measured to fit doesn't wrap when drawn.
            t = t.wrap(w + 1.0);
        }
        self.out.layers[layer].texts.push(t);
    }

    fn hovered(&self, path: &str) -> bool {
        self.ix.hover.as_deref() == Some(path)
    }

    fn hit(&mut self, area: Area, n: &Node, act: Act) {
        self.out.hits.push(Hit {
            area,
            path: n.path.clone(),
            act,
            hint: n.hint.clone().or_else(|| n.rest.clone()),
            anchor: n.anchor.clone(),
        });
    }

    /// The whole screen: scene-attached widgets first, then the placed layers, then
    /// bubbles.
    pub fn screen(&mut self, root: &Node) {
        for c in &root.children {
            match c.kind {
                Kind::Hotspot => self.hotspot(c),
                Kind::Drop if c.anchor.is_some() => self.socket(c),
                Kind::Drag if c.anchor.is_some() => self.anchored_drag(c),
                _ => {}
            }
        }
        for c in &root.children {
            if !layout::anchored(c) {
                // The surface stops the pointer first; its widgets sit above it.
                self.hit(Area::Rect(c.rect), c, Act::Block);
                self.node(c, style::sky(), 0);
            }
        }
        for c in &root.children {
            if c.kind == Kind::Bubble {
                self.bubble(c);
            }
        }
    }

    fn node(&mut self, n: &Node, bg: Color, layer: usize) {
        self.cur = n.path.clone();
        match n.kind {
            Kind::Column | Kind::Row | Kind::Screen => self.children(n, bg, layer),
            Kind::Panel => {
                let bg = match style::surface(n.role, n.rect) {
                    Some(p) => {
                        let fill = p.fill;
                        self.panel(layer, p);
                        if n.role == Role::Sign {
                            self.hangers(layer, n.rect);
                        }
                        if fill.to_array()[3] > 0.5 { fill } else { bg }
                    }
                    None => bg,
                };
                self.children(n, bg, layer);
            }
            Kind::Text => {
                let s = n.text.clone().unwrap_or_default();
                self.text(
                    layer,
                    &s,
                    Vec2::new(n.rect.x, n.rect.y),
                    n.style.font(),
                    style::text_on(bg),
                    n.wrap,
                );
            }
            Kind::Button => self.button(n, bg, layer),
            Kind::Tile => self.tile(n, layer),
            Kind::State => {
                let kind = n.state.unwrap_or(StateKind::Info);
                let word = n.text.clone().unwrap_or_default();
                self.state(
                    layer,
                    kind,
                    &word,
                    Vec2::new(n.rect.x, n.rect.y),
                    bg,
                    n.wrap,
                );
            }
            Kind::Meter => self.meter(n, bg, layer),
            Kind::Icon => {
                let name = n.icon.clone().unwrap_or_default();
                self.icon(layer, &name, n.rect, style::ink_on(bg), &n.path);
            }
            Kind::Divider => {
                let r = n.rect;
                self.panel(
                    layer,
                    style::fill(
                        Rect::new(r.x, r.y + r.h * 0.5 - 1.0, r.w, 2.0),
                        style::ink_on(bg).with_alpha(0.35),
                        1.0,
                    ),
                );
            }
            Kind::Spacer => {}
            Kind::Drag => self.inline_drag(n, bg, layer),
            Kind::Drop => self.inline_drop(n, bg, layer),
            Kind::Bubble | Kind::Hotspot => {}
        }
    }

    fn children(&mut self, n: &Node, bg: Color, layer: usize) {
        for c in &n.children {
            if !layout::anchored(c) {
                self.node(c, bg, layer);
            }
        }
    }

    /// Two short hangers above a sign.
    fn hangers(&mut self, layer: usize, r: Rect) {
        for x in [r.x + 14.0, r.x + r.w - 16.0] {
            self.panel(
                layer,
                style::fill(Rect::new(x, r.y - 7.0, 2.5, 8.0), palette::INK, 1.0),
            );
        }
    }

    fn button(&mut self, n: &Node, bg: Color, layer: usize) {
        let r = n.rect;
        let label = n.text.clone().unwrap_or_default();
        let hovered = self.hovered(&n.path);
        let pressed = hovered && self.ix.pressed.as_deref() == Some(n.path.as_str());
        let text_color;
        if n.rest.is_some() {
            // Resting: dashed, never greyed; the label stays pure black or white.
            self.panel(layer, style::ring(r, style::ink_on(bg), 2.5, 12.0, 12.0));
            text_color = style::text_on(bg);
            self.hit(Area::Rect(r), n, Act::Hint);
        } else {
            let mut p = style::surface(Role::Action, r).expect("action has a surface");
            if hovered {
                p.fill = safe::orange().lerp(Color::WHITE, 0.3);
                p.border_px = 4.0;
            }
            if pressed {
                p.rect.y += 1.5;
            }
            text_color = style::text_on(p.fill);
            self.panel(layer, p);
            match &n.on {
                Some(on) => self.hit(
                    Area::Rect(r),
                    n,
                    Act::Click {
                        on: on.clone(),
                        args: n.args.clone(),
                    },
                ),
                None => self
                    .out
                    .problems
                    .add(&n.path, "a button needs `on` (or `rest` with a reason)"),
            }
        }
        let mut x = r.x + layout::BUTTON_PAD.x;
        if let Some(icon) = n.icon.clone() {
            let s = layout::DEFAULT_ICON;
            self.icon(
                layer,
                &icon,
                Rect::new(x, r.y + (r.h - s) * 0.5, s, s),
                text_color,
                &n.path,
            );
            x += s + 8.0;
        }
        let s = self
            .m
            .text(&label, layout::BUTTON_FONT.0, layout::BUTTON_FONT.1, None);
        let y = r.y + (r.h - s.y) * 0.5 + if pressed { 1.5 } else { 0.0 };
        self.text(
            layer,
            &label,
            Vec2::new(x, y),
            layout::BUTTON_FONT,
            text_color,
            None,
        );
    }

    fn tile(&mut self, n: &Node, layer: usize) {
        let r = n.rect;
        let hovered = self.hovered(&n.path);
        let fill = if n.selected {
            safe::orange()
        } else if hovered {
            Color::hex(0x4a3a5c)
        } else {
            Color::hex(style::DARK_PLUM)
        };
        self.panel(
            layer,
            Panel {
                fill,
                border: if n.selected {
                    palette::INK
                } else {
                    safe::orange()
                },
                border_px: if hovered { 4.0 } else { 3.0 },
                radius: 14.0,
                shadow: true,
                ..Panel::new(r)
            },
        );
        let ink = style::text_on(fill);
        let s = 28.0;
        let icon = n.icon.clone().unwrap_or_default();
        self.icon(
            layer,
            &icon,
            Rect::new(r.x + (r.w - s) * 0.5, r.y + 10.0, s, s),
            ink,
            &n.path,
        );
        let label = n.text.clone().unwrap_or_default();
        let ls = self.m.text(&label, TextStyle::Small.font().0, 600, None);
        self.text(
            layer,
            &label,
            Vec2::new(r.x + (r.w - ls.x) * 0.5, r.y + r.h - ls.y - 9.0),
            (TextStyle::Small.font().0, 600),
            ink,
            None,
        );
        if let Some((kind, word)) = n.ribbon.clone() {
            self.ribbon(layer, kind, &word, Vec2::new(r.x + r.w - 6.0, r.y - 10.0));
        }
        match &n.on {
            Some(on) => self.hit(
                Area::Rect(r),
                n,
                Act::Click {
                    on: on.clone(),
                    args: n.args.clone(),
                },
            ),
            None => self.out.problems.add(&n.path, "a tile needs `on`"),
        }
    }

    /// A news ribbon hanging off a tile's top-right corner: colour, shape and word.
    fn ribbon(&mut self, layer: usize, kind: StateKind, word: &str, top_right: Vec2) {
        let (color, shape) = style::state_look(kind);
        let font = (13.0, 700);
        let s = self.m.text(word, font.0, font.1, None);
        let w = s.x + 30.0;
        let r = Rect::new(top_right.x - w, top_right.y, w, s.y + 6.0);
        self.panel(
            layer,
            Panel {
                fill: color,
                border: palette::INK,
                border_px: 2.0,
                radius: 6.0,
                shadow: false,
                ..Panel::new(r)
            },
        );
        let ink = style::text_on(color);
        // The shape sits on a small white disc so it reads on its own colour.
        let c = Vec2::new(r.x + 12.0, r.y + r.h * 0.5);
        self.panel(
            layer,
            style::fill(
                Rect::new(c.x - 8.0, c.y - 8.0, 16.0, 16.0),
                Color::WHITE,
                8.0,
            ),
        );
        let mut shapes = Vec::new();
        style::shape(&mut shapes, shape, c, 11.0, color);
        self.out.layers[layer].panels.extend(shapes);
        self.text(
            layer,
            word,
            Vec2::new(r.x + 23.0, r.y + 3.0),
            font,
            ink,
            None,
        );
    }

    fn state(
        &mut self,
        layer: usize,
        kind: StateKind,
        word: &str,
        at: Vec2,
        bg: Color,
        wrap: Option<f32>,
    ) {
        let (color, shape) = style::state_look(kind);
        let s = layout::STATE_SHAPE;
        let mut shapes = Vec::new();
        style::shape(
            &mut shapes,
            shape,
            Vec2::new(at.x + s * 0.5, at.y + 10.0),
            s,
            color,
        );
        self.out.layers[layer].panels.extend(shapes);
        self.text(
            layer,
            word,
            Vec2::new(at.x + s + 6.0, at.y),
            (15.0, 700),
            style::text_on(bg),
            wrap,
        );
    }

    fn meter(&mut self, n: &Node, bg: Color, layer: usize) {
        let r = n.rect;
        let label = n.text.clone().unwrap_or_default();
        let ink = style::text_on(bg);
        let (size, weight) = TextStyle::Small.font();
        self.text(
            layer,
            &label,
            Vec2::new(r.x, r.y),
            (size, 600.max(weight)),
            ink,
            n.wrap,
        );
        let bar = Rect::new(r.x, r.y + r.h - layout::METER_BAR, r.w, layout::METER_BAR);
        let total = n.value.max(n.max).max(f32::EPSILON);
        let free_w = bar.w * n.value.min(n.max) / total;
        let over_w = bar.w * (n.value - n.max).max(0.0) / total;
        self.panel(
            layer,
            style::fill(bar, style::ink_on(bg).with_alpha(0.18), 6.0),
        );
        if free_w > 0.5 {
            self.panel(
                layer,
                style::fill(
                    Rect::new(bar.x, bar.y, free_w, bar.h),
                    style::ink_on(bg),
                    6.0,
                ),
            );
        }
        if over_w > 0.5 {
            let o = Rect::new(bar.x + free_w, bar.y, over_w, bar.h);
            self.panel(
                layer,
                Panel {
                    fill: safe::vermilion(),
                    border: style::ink_on(bg),
                    border_px: 1.5,
                    radius: 6.0,
                    shadow: false,
                    ..Panel::new(o)
                },
            );
        }
        // A tick where the free allowance ends.
        if n.max > 0.0 && n.value > n.max {
            let x = bar.x + bar.w * n.max / total;
            self.panel(
                layer,
                style::fill(
                    Rect::new(x - 1.5, bar.y - 4.0, 3.0, bar.h + 8.0),
                    style::ink_on(bg),
                    1.0,
                ),
            );
        }
    }

    fn inline_drag(&mut self, n: &Node, bg: Color, layer: usize) {
        let r = n.rect;
        let dragging = self.ix.drag.as_ref().is_some_and(|d| d.path == n.path);
        if dragging {
            // It stays where it was, outlined, while its ghost follows the pointer.
            self.panel(
                layer,
                style::ring(grow(r, 3.0), style::ink_on(bg), 2.0, 10.0, 10.0),
            );
        } else if self.hovered(&n.path) && self.ix.drag.is_none() {
            self.panel(
                layer,
                style::ring(grow(r, 3.0), safe::orange(), 3.0, 10.0, 0.0),
            );
        }
        self.children(n, bg, layer);
        self.hit(
            Area::Rect(r),
            n,
            Act::Drag {
                on: n.on.clone(),
                args: n.args.clone(),
                cancel: n.cancel.clone(),
            },
        );
    }

    fn inline_drop(&mut self, n: &Node, bg: Color, layer: usize) {
        let r = n.rect;
        self.children(n, bg, layer);
        let ok = n.ok.unwrap_or(false);
        // No verdict: not a target for this drag.
        let target = n.ok.is_some();
        if self.ix.drag.is_some() && target {
            let over =
                self.ix.drag.as_ref().and_then(|d| d.over.as_deref()) == Some(n.path.as_str());
            self.verdict_ring(layer, r, 12.0, ok, over);
            if !ok {
                let reason = n.reason.clone().unwrap_or_default();
                if reason.trim().is_empty() {
                    self.out
                        .problems
                        .add(&n.path, "a refusing drop target needs its reason");
                }
                let at = Vec2::new(r.x, r.y + r.h + 6.0);
                self.callout(
                    1,
                    &reason,
                    at,
                    r.w.max(220.0),
                    Some(StateKind::Refused),
                    &n.path,
                );
            }
        }
        if target {
            self.hit(
                Area::Rect(r),
                n,
                Act::Drop {
                    on: n.on.clone(),
                    args: n.args.clone(),
                    ok,
                },
            );
        }
    }

    /// A drop target's look during a drag: sky blue and dashed if it takes the part
    /// (solid when the part is over it), vermilion and dashed with a cross if it refuses.
    fn verdict_ring(&mut self, layer: usize, r: Rect, radius: f32, ok: bool, over: bool) {
        let rr = grow(r, 4.0);
        if ok {
            self.panel(
                layer,
                style::ring(rr, Color::WHITE, SKY_RING + 3.0, radius, 0.0),
            );
            self.panel(
                layer,
                style::ring(
                    rr,
                    safe::sky(),
                    SKY_RING + if over { 2.0 } else { 0.0 },
                    radius,
                    if over { 0.0 } else { 16.0 },
                ),
            );
        } else {
            self.panel(
                layer,
                style::ring(rr, Color::WHITE, SKY_RING + 3.0, radius, 0.0),
            );
            self.panel(
                layer,
                style::ring(rr, safe::vermilion(), SKY_RING, radius, 16.0),
            );
            let mut shapes = Vec::new();
            let c = Vec2::new(rr.x + rr.w - 2.0, rr.y + 2.0);
            shapes.push(style::fill(
                Rect::new(c.x - 11.0, c.y - 11.0, 22.0, 22.0),
                Color::WHITE,
                11.0,
            ));
            style::shape(&mut shapes, Shape::Cross, c, 14.0, safe::vermilion());
            self.out.layers[layer].panels.extend(shapes);
        }
    }

    /// A small dark note with an optional state shape: refusal reasons, hints.
    fn callout(
        &mut self,
        layer: usize,
        s: &str,
        at: Vec2,
        max_w: f32,
        state: Option<StateKind>,
        path: &str,
    ) -> Rect {
        let font = (15.0, 600);
        let shape_w = if state.is_some() {
            layout::STATE_SHAPE + 8.0
        } else {
            0.0
        };
        let (size, wrap, _) =
            layout::text_size(self.m, s, font, (max_w - 20.0 - shape_w).max(40.0));
        let mut r = Rect::new(at.x, at.y, size.x + 20.0 + shape_w, size.y + 12.0);
        r = self.clamp(r, path);
        let fill = Color::hex(0x1b1424);
        self.panel(
            layer,
            Panel {
                fill,
                border: Color::WHITE,
                border_px: 1.5,
                radius: 8.0,
                shadow: true,
                ..Panel::new(r)
            },
        );
        if let Some(k) = state {
            let (color, shape) = style::state_look(k);
            let mut shapes = Vec::new();
            style::shape(
                &mut shapes,
                shape,
                Vec2::new(r.x + 10.0 + layout::STATE_SHAPE * 0.5, r.y + 16.0),
                layout::STATE_SHAPE,
                color,
            );
            self.out.layers[layer].panels.extend(shapes);
        }
        self.text(
            layer,
            s,
            Vec2::new(r.x + 10.0 + shape_w, r.y + 6.0),
            font,
            Color::WHITE,
            wrap,
        );
        r
    }

    /// Keeps `r` on screen; reports it if it had to move a long way.
    fn clamp(&mut self, mut r: Rect, _path: &str) -> Rect {
        let m = 6.0;
        r.x = r.x.min(self.screen.x - m - r.w).max(m);
        r.y = r.y.min(self.screen.y - m - r.h).max(m);
        r
    }

    fn anchor(&mut self, n: &Node) -> Option<Anchor> {
        let name = n.anchor.clone()?;
        match self.anchors.get(&name) {
            Some(a) => Some(*a),
            None => {
                self.out
                    .problems
                    .add(&n.path, format!("no scene anchor {name:?}"));
                None
            }
        }
    }

    /// A sign's lines and size: the title (and state) always; hovered or selected, the
    /// keeper and one line too.
    fn sign_layout(&mut self, sign: &Sign, big: bool) -> (Vec<SignLine>, Vec2, f32) {
        let title_font = if big { (19.0, 700) } else { (16.0, 700) };
        let mut lines = vec![(sign.title.clone(), title_font)];
        if big {
            if let Some(k) = &sign.keeper {
                lines.push((k.clone(), (15.0, 500)));
            }
            if let Some(l) = &sign.line {
                lines.push((l.clone(), (16.0, 500)));
            }
        }
        let title_w = self.m.text(&sign.title, title_font.0, title_font.1, None).x;
        let state_w = sign.state.as_ref().map_or(0.0, |(_, word)| {
            self.m.text(word, 15.0, 700, None).x + layout::STATE_SHAPE + 14.0
        });
        let (mut w, mut h) = (title_w + state_w, 0.0);
        for (s, f) in &lines {
            let sz = self.m.text(s, f.0, f.1, None);
            w = w.max(sz.x);
            h += sz.y + 2.0;
        }
        (lines, Vec2::new(w + 18.0, h + 16.0), title_w)
    }

    fn hotspot(&mut self, n: &Node) {
        self.cur = n.path.clone();
        let Some(a) = self.anchor(n) else { return };
        let hovered = self.hovered(&n.path);
        let Some(sign) = n.sign.clone() else {
            self.out.problems.add(&n.path, "a hotspot needs its sign");
            return;
        };
        let screen = Rect::new(4.0, 4.0, self.screen.x - 8.0, self.screen.y - 8.0);
        let fits = |r: &Rect, avoid: &[Rect]| {
            rules::contains(screen, *r) && !avoid.iter().any(|o| rules::overlaps(*o, *r))
        };
        // The small sign hangs above the module, or below if a panel is in the way. Its
        // place never moves on hover, so the hover area is steady.
        let (_, small, _) = self.sign_layout(&sign, false);
        let lift = a.radius * 0.35 + 10.0;
        let above = Rect::new(
            a.at.x - small.x * 0.5,
            a.at.y - lift - small.y,
            small.x,
            small.y,
        );
        let below = Rect {
            y: a.at.y + lift,
            ..above
        };
        let small_r = if fits(&above, &self.avoid) {
            above
        } else {
            below
        };
        let small_r = self.clamp(small_r, &n.path);
        if let Some(anchor) = &n.anchor {
            self.signs.insert(anchor.clone(), small_r);
        }
        let big = hovered || n.selected;
        let (lines, size, title_w) = self.sign_layout(&sign, big);
        // A hovered sign grows in front of the others: keeping its bottom edge, or moved
        // aside or below when a panel is in the way.
        let r = if big {
            let bottom = small_r.y + small_r.h;
            let x = a.at.x - size.x * 0.5;
            let candidates = [
                Rect::new(x, bottom - size.y, size.x, size.y),
                Rect::new(a.at.x - size.x + 30.0, bottom - size.y, size.x, size.y),
                Rect::new(a.at.x - 30.0, bottom - size.y, size.x, size.y),
                Rect::new(x, small_r.y, size.x, size.y),
            ];
            let pick = candidates
                .iter()
                .copied()
                .find(|c| fits(c, &self.avoid))
                .unwrap_or(candidates[0]);
            self.clamp(pick, &n.path)
        } else {
            small_r
        };
        let layer = if big { 1 } else { 0 };
        let mut p = style::surface(Role::Sign, r).expect("sign has a surface");
        if hovered {
            p.border = safe::orange();
            p.border_px = 4.0;
        }
        let fill = p.fill;
        self.panel(layer, p);
        self.hangers(layer, r);
        let ink = style::text_on(fill);
        let pad = 9.0;
        let mut y = r.y + pad;
        for (i, (s, f)) in lines.iter().enumerate() {
            self.text(layer, s, Vec2::new(r.x + pad, y), *f, ink, None);
            if i == 0
                && let Some((k, word)) = &sign.state
            {
                self.state(
                    layer,
                    *k,
                    word,
                    Vec2::new(r.x + pad + title_w + 10.0, y + 1.0),
                    fill,
                    None,
                );
            }
            y += self.m.text(s, f.0, f.1, None).y + 2.0;
        }
        for o in self.avoid.clone() {
            if rules::overlaps(o, r) {
                self.out.problems.at(
                    &n.path,
                    r,
                    format!("sign {:?} is under a panel", sign.title),
                );
            }
        }
        match &n.on {
            Some(on) => {
                let act = Act::Click {
                    on: on.clone(),
                    args: n.args.clone(),
                };
                self.hit(Area::Circle(a.at, a.radius), n, act.clone());
                self.hit(Area::Rect(small_r), n, act);
            }
            None => self.out.problems.add(&n.path, "a hotspot needs `on`"),
        }
    }

    /// A drop target on a scene anchor (a socket): a ring, a label beside it, and the
    /// verdict during a drag.
    fn socket(&mut self, n: &Node) {
        self.cur = n.path.clone();
        let Some(a) = self.anchor(n) else { return };
        let ring = Rect::new(
            a.at.x - a.radius,
            a.at.y - a.radius,
            a.radius * 2.0,
            a.radius * 2.0,
        );
        let ok = n.ok.unwrap_or(false);
        let target = n.ok.is_some();
        let dragging = self.ix.drag.is_some() && target;
        let over = self.ix.drag.as_ref().and_then(|d| d.over.as_deref()) == Some(n.path.as_str());
        if dragging {
            self.verdict_ring(0, ring, a.radius, ok, over);
        } else if n.selected {
            self.panel(0, style::ring(ring, palette::INK, 7.0, a.radius, 0.0));
            self.panel(
                0,
                style::ring(grow(ring, -1.5), Color::hex(0xf2c14e), 4.0, a.radius, 0.0),
            );
        } else {
            self.panel(0, style::ring(ring, Color::WHITE, 3.0, a.radius, 10.0));
        }
        // The label beside the ring.
        let label = n.text.clone().unwrap_or_default();
        let left = n.side.as_deref() != Some("right");
        let reason = (dragging && !ok).then(|| n.reason.clone().unwrap_or_default());
        if reason.as_deref().is_some_and(|r| r.trim().is_empty()) {
            self.out
                .problems
                .add(&n.path, "a refusing drop target needs its reason");
        }
        let font = (15.0, 700);
        let max_w = 300.0;
        let (ls, lwrap, cut) = layout::text_size(self.m, &label, font, max_w);
        if cut {
            self.out
                .problems
                .add(&n.path, format!("{label:?} is cut off"));
        }
        let mut w = ls.x;
        let mut h = ls.y;
        let mut rs = None;
        if let Some(reason) = &reason {
            let (s, wrap, _) = layout::text_size(
                self.m,
                reason,
                (15.0, 600),
                max_w - layout::STATE_SHAPE - 8.0,
            );
            w = w.max(s.x + layout::STATE_SHAPE + 8.0);
            h += s.y + 4.0;
            rs = Some((s, wrap));
        }
        let pad = 8.0;
        let bw = w + pad * 2.0;
        let bh = h + pad * 2.0;
        let x = if left {
            a.at.x - a.radius - 14.0 - bw
        } else {
            a.at.x + a.radius + 14.0
        };
        let r = Rect::new(x, a.at.y - bh * 0.5, bw, bh);
        if !rules::contains(Rect::new(0.0, 0.0, self.screen.x, self.screen.y), r) {
            self.out.problems.at(
                &n.path,
                r,
                format!("socket label {label:?} runs off the screen"),
            );
        }
        self.work.push(ring);
        self.work.push(r);
        let layer = if reason.is_some() { 1 } else { 0 };
        let fill = style::surface(Role::Status, r).expect("status").fill;
        self.panel(
            layer,
            Panel {
                border: if over {
                    safe::sky()
                } else {
                    Color::TRANSPARENT
                },
                border_px: if over { 3.0 } else { 0.0 },
                ..style::surface(Role::Status, r).expect("status")
            },
        );
        // A short leader from the label to the ring.
        let lx = if left { r.x + r.w } else { a.at.x + a.radius };
        self.panel(
            layer,
            style::fill(Rect::new(lx, a.at.y - 1.0, 14.0, 2.0), Color::WHITE, 1.0),
        );
        let ink = style::text_on(fill);
        self.text(
            layer,
            &label,
            Vec2::new(r.x + pad, r.y + pad),
            font,
            ink,
            lwrap,
        );
        if let (Some(reason), Some((_, wrap))) = (&reason, rs) {
            let y = r.y + pad + ls.y + 4.0;
            self.state(
                layer,
                StateKind::Refused,
                reason,
                Vec2::new(r.x + pad, y),
                fill,
                wrap,
            );
        }
        if target {
            self.hit(
                Area::Circle(a.at, a.radius.max(28.0)),
                n,
                Act::Drop {
                    on: n.on.clone(),
                    args: n.args.clone(),
                    ok,
                },
            );
            self.hit(
                Area::Rect(r),
                n,
                Act::Drop {
                    on: n.on.clone(),
                    args: n.args.clone(),
                    ok,
                },
            );
        }
    }

    fn anchored_drag(&mut self, n: &Node) {
        let Some(a) = self.anchor(n) else { return };
        if self.ix.drag.is_none() && self.hovered(&n.path) {
            let ring = Rect::new(
                a.at.x - a.radius - 4.0,
                a.at.y - a.radius - 4.0,
                a.radius * 2.0 + 8.0,
                a.radius * 2.0 + 8.0,
            );
            self.panel(
                0,
                style::ring(ring, safe::orange(), 3.0, a.radius + 4.0, 0.0),
            );
        }
        self.hit(
            Area::Circle(a.at, a.radius.max(24.0)),
            n,
            Act::Drag {
                on: n.on.clone(),
                args: n.args.clone(),
                cancel: n.cancel.clone(),
            },
        );
    }

    fn bubble(&mut self, n: &Node) {
        self.cur = n.path.clone();
        let Some(a) = self.anchor(n) else { return };
        let speaker = n.speaker.clone().unwrap_or_default();
        let place = n.place_name.clone();
        let text = n.text.clone().unwrap_or_default();
        let pad = 14.0;
        let max_w = 340.0;
        let name_font = (17.0, 700);
        let place_font = (14.0, 400);
        let body_font = (18.0, 500);
        let name_s = self.m.text(&speaker, name_font.0, name_font.1, None);
        let place_s = place
            .as_deref()
            .map(|p| self.m.text(p, place_font.0, place_font.1, None));
        let (body_s, wrap, cut) = layout::text_size(self.m, &text, body_font, max_w - pad * 2.0);
        if cut {
            self.out.problems.add(&n.path, "bubble text is cut off");
        }
        let more_w = if n.more { 22.0 } else { 0.0 };
        let w = (body_s.x + more_w).max(name_s.x + place_s.map_or(0.0, |p| p.x + 10.0)) + pad * 2.0;
        let h = name_s.y + 4.0 + body_s.y + pad * 2.0;
        let tail = 14.0;
        // Above the speaker (above their sign, if they have one) if there is room clear of
        // the panels, else beside or below.
        let top = match n.anchor.as_ref().and_then(|k| self.signs.get(k)) {
            Some(sign) => sign.y - 8.0,
            None => a.at.y - a.radius * 0.4,
        };
        let above = top - tail - h;
        let below = a.at.y + a.radius * 0.4 + tail;
        let candidates = [
            Rect::new(a.at.x - w * 0.5, above, w, h),
            Rect::new(a.at.x - w + 40.0, above, w, h),
            Rect::new(a.at.x - 40.0, above, w, h),
            Rect::new(a.at.x - w * 0.5, below, w, h),
            Rect::new(a.at.x - w - a.radius, a.at.y - h * 0.5, w, h),
        ];
        let screen = Rect::new(4.0, 4.0, self.screen.x - 8.0, self.screen.y - 8.0);
        let fits = |r: &Rect, avoid: &[Rect]| {
            rules::contains(screen, *r) && !avoid.iter().any(|o| rules::overlaps(*o, *r))
        };
        // Clear of the panels, the work in the scene and the other signs.
        let mut avoid = self.avoid.clone();
        avoid.extend(self.work.iter().copied());
        avoid.extend(
            self.signs
                .iter()
                .filter(|(k, _)| n.anchor.as_ref() != Some(*k))
                .map(|(_, r)| *r),
        );
        // Slid sideways onto the screen before testing, so a speaker near an edge still
        // gets a bubble above them.
        let sx = self.screen.x;
        let candidates = candidates.map(|r| Rect {
            x: r.x.clamp(6.0, (sx - 6.0 - r.w).max(6.0)),
            ..r
        });
        let pick = candidates.iter().position(|r| fits(r, &avoid));
        let mut r = match pick {
            Some(i) => candidates[i],
            None => {
                let r = self.clamp(candidates[0], &n.path);
                self.out.problems.at(
                    &n.path,
                    r,
                    "the bubble has no room clear of the panels and the work",
                );
                r
            }
        };
        r = self.clamp(r, &n.path);
        let hovered = self.hovered(&n.path);
        let mut p = style::surface(Role::Speech, r).expect("speech");
        if hovered {
            p.border_px = 4.0;
        }
        self.panel(1, p);
        // The tail points at the speaker from whichever side the bubble ended up on.
        let (centre, rot, seam) = if r.y + r.h <= a.at.y {
            let tx = a.at.x.clamp(r.x + 24.0, r.x + r.w - 24.0);
            (
                Vec2::new(tx, r.y + r.h - 3.0 + tail * 0.5),
                std::f32::consts::PI,
                Rect::new(tx - 8.5, r.y + r.h - 5.0, 17.0, 4.0),
            )
        } else if r.y >= a.at.y {
            let tx = a.at.x.clamp(r.x + 24.0, r.x + r.w - 24.0);
            (
                Vec2::new(tx, r.y + 3.0 - tail * 0.5),
                0.0,
                Rect::new(tx - 8.5, r.y + 1.0, 17.0, 4.0),
            )
        } else if r.x + r.w <= a.at.x {
            let ty = a.at.y.clamp(r.y + 20.0, r.y + r.h - 20.0);
            (
                Vec2::new(r.x + r.w - 3.0 + tail * 0.5, ty),
                std::f32::consts::FRAC_PI_2,
                Rect::new(r.x + r.w - 5.0, ty - 8.5, 4.0, 17.0),
            )
        } else {
            let ty = a.at.y.clamp(r.y + 20.0, r.y + r.h - 20.0);
            (
                Vec2::new(r.x + 3.0 - tail * 0.5, ty),
                -std::f32::consts::FRAC_PI_2,
                Rect::new(r.x + 1.0, ty - 8.5, 4.0, 17.0),
            )
        };
        self.panel(
            1,
            Panel {
                shape: PanelShape::Triangle,
                rotate: rot,
                fill: Color::WHITE,
                border: palette::INK,
                border_px: 3.0,
                radius: 1.0,
                shadow: false,
                ..Panel::new(Rect::new(
                    centre.x - 11.0,
                    centre.y - tail * 0.5,
                    22.0,
                    tail,
                ))
            },
        );
        // Cover the tail's ink where it meets the bubble.
        self.panel(1, style::fill(seam, Color::WHITE, 0.0));
        let mut x = r.x + pad;
        let y = r.y + pad;
        self.text(1, &speaker, Vec2::new(x, y), name_font, Color::BLACK, None);
        // An orange underline marks the speaker on their own.
        self.panel(
            1,
            style::fill(
                Rect::new(x, y + name_s.y + 0.5, name_s.x, 3.0),
                safe::orange(),
                1.5,
            ),
        );
        x += name_s.x + 10.0;
        if let (Some(p), Some(ps)) = (&place, place_s) {
            self.text(
                1,
                p,
                Vec2::new(x, y + (name_s.y - ps.y) * 0.5 + 1.0),
                place_font,
                Color::BLACK,
                None,
            );
        }
        self.text(
            1,
            &text,
            Vec2::new(r.x + pad, y + name_s.y + 4.0),
            body_font,
            Color::BLACK,
            wrap,
        );
        if n.more {
            // A small marker: there is more.
            let c = Vec2::new(r.x + r.w - pad - 6.0, r.y + r.h - pad - 7.0);
            self.panel(
                1,
                Panel {
                    shape: PanelShape::Triangle,
                    rotate: std::f32::consts::FRAC_PI_2,
                    fill: safe::orange(),
                    border: palette::INK,
                    border_px: 1.5,
                    radius: 1.0,
                    shadow: false,
                    ..Panel::new(Rect::new(c.x - 7.0, c.y - 6.0, 14.0, 12.0))
                },
            );
        }
        match &n.on {
            Some(on) => self.hit(
                Area::Rect(r),
                n,
                Act::Click {
                    on: on.clone(),
                    args: n.args.clone(),
                },
            ),
            None => self.hit(Area::Rect(r), n, Act::Block),
        }
    }

    /// Draws the ghost of what is being dragged, the preview for the target under it, and
    /// the hint for what is hovered.
    pub fn pointer_layer(&mut self, root: &Node) {
        let Some(p) = self.ix.pointer else { return };
        if let Some(d) = self.ix.drag.clone() {
            // A compact ghost just below and right of the pointer, so the target and its
            // verdict stay in sight; the preview under it.
            let mut below = p.y + 14.0;
            if let Some(src) = find_path(root, &d.path) {
                let mut ghost = src.clone();
                ghost.anchor = None;
                let mut problems = Problems::default();
                let w = if src.anchor.is_some() {
                    250.0
                } else {
                    src.rect.w.clamp(60.0, 250.0)
                };
                {
                    let mut cx = Ctx {
                        m: &mut *self.m,
                        problems: &mut problems,
                    };
                    layout::lay_out_at(&mut cx, &mut ghost, p + Vec2::new(16.0, 16.0), w);
                }
                let r = ghost.rect;
                self.panel(
                    2,
                    Panel {
                        fill: Color::WHITE,
                        border: safe::sky(),
                        border_px: 3.0,
                        radius: 10.0,
                        shadow: true,
                        ..Panel::new(grow(r, 4.0))
                    },
                );
                let mut sub = Vec::new();
                std::mem::swap(&mut sub, &mut self.out.hits);
                self.children(&ghost, Color::WHITE, 2);
                std::mem::swap(&mut sub, &mut self.out.hits);
                below = r.y + r.h + 12.0;
            }
            if let Some(over) = &d.over
                && let Some(target) = find_path(root, over)
                && target.ok == Some(true)
                && let Some(preview) = &target.preview
            {
                let mut pv = (**preview).clone();
                let mut problems = Problems::default();
                {
                    let mut cx = Ctx {
                        m: &mut *self.m,
                        problems: &mut problems,
                    };
                    let s = layout::measure(&mut cx, &mut pv, 300.0);
                    let mut at = Vec2::new(p.x + 12.0, below);
                    if at.x + s.x > self.screen.x - 8.0 {
                        at.x = self.screen.x - 8.0 - s.x;
                    }
                    if at.y + s.y > self.screen.y - 8.0 {
                        at.y = p.y - 16.0 - s.y;
                    }
                    layout::lay_out_at(&mut cx, &mut pv, at, 300.0);
                }
                self.out.problems.extend(&problems);
                let mut sub = Vec::new();
                std::mem::swap(&mut sub, &mut self.out.hits);
                self.node(&pv, style::sky(), 2);
                std::mem::swap(&mut sub, &mut self.out.hits);
            }
            return;
        }
        let hint = self
            .ix
            .hover
            .as_ref()
            .and_then(|h| self.out.hits.iter().rev().find(|x| &x.path == h))
            .and_then(|h| h.hint.clone());
        if let Some(hint) = hint {
            // Below the pointer, or above it in the lower part of the screen, so it never
            // covers the row it was asked about.
            let at = if p.y > self.screen.y * 0.6 {
                p + Vec2::new(16.0, -60.0)
            } else {
                p + Vec2::new(16.0, 22.0)
            };
            self.callout(2, &hint, at, 320.0, None, "hint");
        }
    }

    /// Engine-drawn icons, made of shapes. Unknown names are a rule break.
    fn icon(&mut self, layer: usize, name: &str, r: Rect, ink: Color, path: &str) {
        let s = r.w.min(r.h);
        let o = Vec2::new(r.x + (r.w - s) * 0.5, r.y + (r.h - s) * 0.5);
        let at = |x: f32, y: f32, w: f32, h: f32| Rect::new(o.x + x * s, o.y + y * s, w * s, h * s);
        let line = (s * 0.1).max(2.0);
        let mut ps: Vec<Panel> = Vec::new();
        let f = |r: Rect, rad: f32| style::fill(r, ink, rad);
        match name {
            "market" => {
                // An awning over a counter.
                for i in 0..4 {
                    if i % 2 == 0 {
                        ps.push(f(at(0.05 + i as f32 * 0.225, 0.1, 0.225, 0.3), 1.0));
                    }
                }
                ps.push(style::ring(
                    at(0.05, 0.1, 0.9, 0.3),
                    ink,
                    line * 0.7,
                    2.0,
                    0.0,
                ));
                ps.push(f(at(0.12, 0.4, 0.08, 0.45), 1.0));
                ps.push(f(at(0.8, 0.4, 0.08, 0.45), 1.0));
                ps.push(f(at(0.05, 0.78, 0.9, 0.12), 2.0));
            }
            "office" => {
                // A door with a lamp above it.
                ps.push(style::ring(
                    at(0.25, 0.25, 0.5, 0.7),
                    ink,
                    line,
                    s * 0.08,
                    0.0,
                ));
                ps.push(f(at(0.58, 0.58, 0.1, 0.1), s));
                ps.push(f(at(0.42, 0.02, 0.16, 0.16), s));
            }
            "wharf" | "cradle" => {
                // A crane: mast, jib and hook.
                ps.push(f(at(0.12, 0.08, 0.12, 0.86), 1.0));
                ps.push(f(at(0.12, 0.08, 0.8, 0.12), 1.0));
                ps.push(f(at(0.78, 0.2, 0.05, 0.35), 1.0));
                ps.push(style::ring(at(0.68, 0.52, 0.25, 0.25), ink, line, s, 0.0));
                ps.push(f(at(0.02, 0.86, 0.45, 0.1), 1.0));
            }
            "notices" => {
                // A board with three notes.
                ps.push(style::ring(at(0.05, 0.08, 0.9, 0.84), ink, line, 3.0, 0.0));
                for i in 0..3 {
                    ps.push(f(
                        at(0.2, 0.25 + i as f32 * 0.2, 0.6 - i as f32 * 0.12, 0.09),
                        2.0,
                    ));
                }
            }
            "canteen" => {
                // A cup with steam.
                ps.push(style::ring(at(0.15, 0.4, 0.55, 0.5), ink, line, 4.0, 0.0));
                ps.push(style::ring(at(0.62, 0.5, 0.25, 0.25), ink, line, s, 0.0));
                ps.push(f(at(0.28, 0.08, 0.08, 0.24), 2.0));
                ps.push(f(at(0.48, 0.12, 0.08, 0.2), 2.0));
            }
            "log" => {
                // A book: a cover, a spine and lines. Not two bars (that's pause).
                ps.push(style::ring(at(0.12, 0.08, 0.76, 0.84), ink, line, 3.0, 0.0));
                ps.push(f(at(0.12, 0.08, 0.14, 0.84), 2.0));
                for i in 0..3 {
                    ps.push(f(at(0.36, 0.26 + i as f32 * 0.18, 0.4, 0.07), 2.0));
                }
            }
            "back" => ps.push(Panel {
                shape: PanelShape::Triangle,
                rotate: -std::f32::consts::FRAC_PI_2,
                ..f(at(0.2, 0.2, 0.6, 0.6), 1.0)
            }),
            "coin" => {
                ps.push(style::ring(at(0.12, 0.12, 0.76, 0.76), ink, line, s, 0.0));
                ps.push(f(at(0.44, 0.3, 0.12, 0.4), 2.0));
            }
            "hold" => {
                ps.push(style::ring(at(0.1, 0.18, 0.8, 0.68), ink, line, 3.0, 0.0));
                ps.push(f(at(0.1, 0.46, 0.8, 0.08), 1.0));
                ps.push(f(at(0.46, 0.18, 0.08, 0.68), 1.0));
            }
            "radio" => {
                ps.push(style::ring(at(0.1, 0.35, 0.8, 0.55), ink, line, 4.0, 0.0));
                ps.push(f(at(0.62, 0.05, 0.07, 0.34), 2.0));
                ps.push(style::ring(at(0.22, 0.48, 0.28, 0.28), ink, line, s, 0.0));
            }
            "goal" => {
                // A small flag.
                ps.push(f(at(0.18, 0.08, 0.1, 0.86), 1.0));
                ps.push(f(at(0.28, 0.1, 0.55, 0.36), 2.0));
            }
            "part" => {
                ps.push(style::ring(at(0.15, 0.15, 0.7, 0.7), ink, line, 4.0, 0.0));
                ps.push(f(at(0.4, 0.4, 0.2, 0.2), s));
            }
            _ => {
                self.out
                    .problems
                    .add(path, format!("unknown icon {name:?}"));
                ps.push(style::ring(at(0.1, 0.1, 0.8, 0.8), ink, line, 3.0, 4.0));
            }
        }
        self.out.layers[layer].panels.extend(ps);
    }
}

fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
}

pub fn find_path<'n>(n: &'n Node, path: &str) -> Option<&'n Node> {
    if n.path == path {
        return Some(n);
    }
    n.children.iter().find_map(|c| find_path(c, path))
}
