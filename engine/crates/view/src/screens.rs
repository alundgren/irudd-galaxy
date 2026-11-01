//! Widget layout, readability and hit testing. All dimensions are logical pixels.
use std::collections::BTreeMap;

use galaxy_api::Registry;
use galaxy_module::module::Module;
use galaxy_render::{Color, Overlay, Panel, Rect, Text, UiLayer, glam::Vec2, text::TextMeasure};
use galaxy_script::{
    ScriptSet,
    screens::{Screen, Widget},
};
use galaxy_sim::scenario::{Click, ClickTarget};
use galaxy_sim::{Input, World};

#[derive(Clone, Debug)]
pub struct Boxed {
    pub widget: Widget,
    pub rect: Rect,
    pub text: Option<Text>,
    pub children: Vec<Boxed>,
}

#[derive(Default)]
pub struct Screens {
    functions: Vec<Screen>,
    load_problems: Vec<String>,
    measure: TextMeasure,
    pub boxes: Vec<Boxed>,
    pub problems: Vec<String>,
}

impl Screens {
    pub fn load(modules: &[&Module]) -> Self {
        let mut out = Self::default();
        let registry = match Registry::new(modules) {
            Ok(r) => r,
            Err(e) => {
                out.load_problems.push(e);
                return out;
            }
        };
        for module in modules {
            match ScriptSet::load(module, &registry) {
                Ok(set) => {
                    for script in set.scripts.iter().filter(|s| s.screen) {
                        out.functions
                            .push(Screen::new(module, &script.file, &registry));
                    }
                }
                Err(errors) => out
                    .load_problems
                    .extend(errors.iter().map(|e| format!("{}: {e}", module.id()))),
            }
        }
        out
    }

    pub fn draw(
        &mut self,
        world: &World,
        size: Vec2,
        anchors: &BTreeMap<String, Vec2>,
        overlay: &mut Overlay,
    ) {
        self.problems = self.load_problems.clone();
        self.boxes.clear();
        for screen in &mut self.functions {
            let calls = screen.calls;
            screen.refresh(world);
            if calls != screen.calls {
                self.measure.clear();
            }
            self.problems.extend(screen.problems.clone());
            if let Some(tree) = &screen.tree {
                let mut boxed = layout(tree, size.x - 40.0, &mut self.measure);
                place(
                    &mut boxed,
                    Vec2::splat(20.0),
                    size,
                    anchors,
                    &mut self.problems,
                );
                check(
                    &boxed,
                    Rect::new(0.0, 0.0, size.x, size.y),
                    &mut self.measure,
                    &mut self.problems,
                );
                draw(&boxed, overlay, None);
                self.boxes.push(boxed);
            }
        }
        // Compare painted descendants, not just root bounds: columns have no surface,
        // and an anchored child can lie outside its root.
        let mut earlier: Vec<&Boxed> = Vec::new();
        for root in &self.boxes {
            let mut current = Vec::new();
            painted(root, &mut current);
            for b in &current {
                for other in &earlier {
                    if overlaps(b.rect, other.rect) {
                        self.problems
                            .push(format!("{}: overlaps {}", b.widget.path, other.widget.path));
                    }
                }
            }
            earlier.extend(current);
        }
    }

    /// Topmost painted surface wins, including a non-clickable panel covering a button.
    pub fn click(&self, point: Vec2, tick: u64) -> Option<Input> {
        let w = self.hit(point)?;
        w.on.as_ref().map(|name| Input {
            tick,
            source: "ui".into(),
            name: name.clone(),
            payload: w.payload.clone(),
        })
    }

    /// Resolve bot targets against the current layout, then use ordinary pointer hit testing.
    pub fn bot_click(&self, click: &Click) -> Result<Input, String> {
        let point = match &click.target {
            ClickTarget::Point { x, y } => Vec2::new(*x, *y),
            ClickTarget::Widget(name) => {
                let mut widgets = Vec::new();
                for root in &self.boxes {
                    painted(root, &mut widgets);
                }
                let matches: Vec<_> = widgets
                    .into_iter()
                    .filter(|b| b.widget.path == *name || b.widget.id.as_deref() == Some(name))
                    .collect();
                let b = match matches.as_slice() {
                    [] => {
                        return Err(format!(
                            "widget `{name}` is missing from the current screen"
                        ));
                    }
                    [b] => *b,
                    _ => {
                        return Err(format!(
                            "widget `{name}` is ambiguous; use a full widget path"
                        ));
                    }
                };
                if b.widget.on.is_none() {
                    return Err(format!("{}: widget is not a button", b.widget.path));
                }
                let point = b.rect.center();
                if let Some(cover) = self.hit(point)
                    && cover.path != b.widget.path
                {
                    return Err(format!(
                        "{}: widget centre is covered by {}",
                        b.widget.path, cover.path
                    ));
                }
                point
            }
        };
        self.click(point, click.tick).ok_or_else(|| {
            format!(
                "click at {},{} before tick {} hit no button",
                point.x, point.y, click.tick
            )
        })
    }

    fn hit(&self, point: Vec2) -> Option<&Widget> {
        fn hit(b: &Boxed, p: Vec2) -> Option<&Widget> {
            for child in b.children.iter().rev() {
                if let Some(w) = hit(child, p) {
                    return Some(w);
                }
            }
            let r = b.rect;
            (p.x >= r.x
                && p.y >= r.y
                && p.x < r.x + r.w
                && p.y < r.y + r.h
                && ["panel", "button", "text"].contains(&b.widget.kind.as_str()))
            .then_some(&b.widget)
        }
        for b in self.boxes.iter().rev() {
            if let Some(w) = hit(b, point) {
                return Some(w);
            }
        }
        None
    }

    pub fn calls(&self) -> u64 {
        self.functions.iter().map(|s| s.calls).sum()
    }
}

fn layout(w: &Widget, available: f32, measure: &mut TextMeasure) -> Boxed {
    let width = w.width.unwrap_or(available).max(1.0);
    let inner = (width - 2.0 * w.pad).max(1.0);
    let mut text = None;
    let mut children = Vec::new();
    let height;
    if w.kind == "text" || w.kind == "button" {
        let t = Text::new(&w.text, Vec2::splat(w.pad), w.size).wrap(inner);
        height = measure.measure(&t, 1.0).y + 2.0 * w.pad;
        text = Some(t);
    } else {
        let row = w.kind == "row";
        let flowing = w
            .children
            .iter()
            .filter(|c| c.anchor.is_none() && c.x.is_none() && c.y.is_none())
            .count();
        let fixed: f32 = w
            .children
            .iter()
            .filter(|c| c.anchor.is_none() && c.x.is_none() && c.y.is_none())
            .filter_map(|c| c.width)
            .sum();
        let flexible = w
            .children
            .iter()
            .filter(|c| c.anchor.is_none() && c.x.is_none() && c.y.is_none() && c.width.is_none())
            .count();
        let share = if row {
            ((inner - fixed - w.gap * flowing.saturating_sub(1) as f32) / flexible.max(1) as f32)
                .max(1.0)
        } else {
            inner
        };
        let mut cursor = 0.0f32;
        let mut cross = 0.0f32;
        for child in &w.children {
            let mut b = layout(child, share, measure);
            let detached = child.anchor.is_some() || child.x.is_some() || child.y.is_some();
            let p = if detached {
                Vec2::ZERO
            } else if row {
                Vec2::new(cursor, 0.0)
            } else {
                Vec2::new(0.0, cursor)
            };
            b.rect.x = w.pad + p.x;
            b.rect.y = w.pad + p.y;
            if !detached {
                cursor += if row { b.rect.w } else { b.rect.h };
                cursor += w.gap;
                cross = cross.max(b.rect.h);
            }
            children.push(b);
        }
        height = 2.0 * w.pad
            + if row {
                cross
            } else {
                (cursor - w.gap).max(0.0)
            };
    }
    Boxed {
        widget: w.clone(),
        rect: Rect::new(0.0, 0.0, width, w.height.unwrap_or(height)),
        text,
        children,
    }
}

fn place(
    b: &mut Boxed,
    origin: Vec2,
    size: Vec2,
    anchors: &BTreeMap<String, Vec2>,
    problems: &mut Vec<String>,
) {
    let w = &b.widget;
    let mut pos = origin + Vec2::new(b.rect.x, b.rect.y);
    if let Some(x) = w.x {
        pos.x = origin.x + x;
    }
    if let Some(y) = w.y {
        pos.y = origin.y + y;
    }
    if let Some(anchor) = &w.anchor {
        match anchors.get(anchor) {
            Some(p) => {
                pos = *p
                    + Vec2::new(
                        w.x.unwrap_or(0.0) - b.rect.w * 0.5,
                        w.y.unwrap_or(-12.0) - b.rect.h,
                    );
                pos = pos.clamp(
                    Vec2::splat(8.0),
                    (size - Vec2::new(b.rect.w, b.rect.h) - Vec2::splat(8.0)).max(Vec2::splat(8.0)),
                );
            }
            None => problems.push(format!("{}: missing anchor `{anchor}`", w.path)),
        }
    }
    b.rect.x = pos.x;
    b.rect.y = pos.y;
    if let Some(t) = &mut b.text {
        t.pos += pos;
    }
    for child in &mut b.children {
        place(child, pos, size, anchors, problems);
    }
}

fn contains(a: Rect, b: Rect) -> bool {
    b.x >= a.x - 0.5
        && b.y >= a.y - 0.5
        && b.x + b.w <= a.x + a.w + 0.5
        && b.y + b.h <= a.y + a.h + 0.5
}
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x + 1.0 < b.x + b.w && b.x + 1.0 < a.x + a.w && a.y + 1.0 < b.y + b.h && b.y + 1.0 < a.y + a.h
}

fn check(b: &Boxed, viewport: Rect, measure: &mut TextMeasure, problems: &mut Vec<String>) {
    let mut problem = |message: &str| problems.push(format!("{}: {message}", b.widget.path));
    if !contains(viewport, b.rect) {
        problem("outside the viewport");
    }
    if let Some(t) = &b.text {
        if b.widget.size < 16.0 {
            problem("text is smaller than 16 logical pixels");
        }
        if b.widget.text.trim().is_empty() {
            problem("empty text or button label");
        }
        if !measure.missing(t, 1.0).is_empty() {
            problem("the bundled font has missing glyphs");
        }
        let size = measure.measure(t, 1.0);
        if !contains(b.rect, Rect::new(t.pos.x, t.pos.y, size.x, size.y)) {
            problem("text is clipped by its widget");
        }
        if b.widget.kind == "button" && (b.rect.h < 44.0 || b.rect.w < 44.0) {
            problem("button target is smaller than 44 pixels");
        }
    }
    for (i, c) in b.children.iter().enumerate() {
        if c.widget.anchor.is_none() && !contains(b.rect, c.rect) {
            problems.push(format!(
                "{}: outside its parent {}",
                c.widget.path, b.widget.path
            ));
        }
        for other in &b.children[..i] {
            if overlaps(c.rect, other.rect) {
                problems.push(format!("{}: overlaps {}", c.widget.path, other.widget.path));
            }
        }
        check(c, viewport, measure, problems);
    }
}

fn painted<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
    if ["panel", "button", "text"].contains(&b.widget.kind.as_str()) {
        out.push(b);
    }
    for child in &b.children {
        painted(child, out);
    }
}

fn draw(b: &Boxed, overlay: &mut Overlay, backing: Option<Rect>) {
    let mut layer = UiLayer::default();
    let mut backing = backing;
    if b.widget.kind == "panel" || b.widget.kind == "button" {
        let mut panel = Panel::new(b.rect);
        if b.widget.kind == "button" {
            panel.fill = Color::hex(0xffce75);
        }
        layer.panels.push(panel);
        backing = Some(b.rect);
    } else if b.text.is_some() && backing.is_none_or(|r| !contains(r, b.rect)) {
        // A flat paper surface covers every glyph, including text with zero padding.
        let mut panel = Panel::new(b.rect);
        panel.radius = 0.0;
        panel.border_px = 0.0;
        panel.shadow = false;
        layer.panels.push(panel);
    }
    if let Some(t) = &b.text {
        layer.texts.push(t.clone());
    }
    if !layer.panels.is_empty() || !layer.texts.is_empty() {
        overlay.widgets.push(layer);
    }
    for child in &b.children {
        draw(child, overlay, backing);
    }
}
