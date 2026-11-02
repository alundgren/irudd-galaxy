//! Widget layout, readability and hit testing. All dimensions are logical pixels.
mod hovers;
mod icons;
mod interaction;
mod scrolling;
pub use interaction::{Drag, Presentation};

use std::collections::BTreeMap;

use galaxy_api::Registry;
use galaxy_module::module::Module;
use galaxy_render::{
    Overlay, Rect, Text, UiLayer, glam::Vec2, screen_style::ScreenStyle, text::TextMeasure,
};
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
    pub clip: Option<Rect>,
    pub content_height: f32,
    pub style: ScreenStyle,
}

#[derive(Default)]
pub struct Screens {
    functions: Vec<Screen>,
    load_problems: Vec<String>,
    measure: TextMeasure,
    pub boxes: Vec<Boxed>,
    pub problems: Vec<String>,
    pub presentation: Presentation,
    pub presented: Presentation,
    icons: BTreeMap<String, String>,
    pub clock: f32,
    pub anchors: BTreeMap<String, Vec2>,
    pub style: ScreenStyle,
}

impl Screens {
    pub fn load(modules: &[&Module]) -> Self {
        let mut out = Self::default();
        let (icons, problems) = icons::load(modules, &mut out.measure);
        out.icons = icons;
        out.load_problems.extend(problems);
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
        self.anchors = anchors.clone();
        for screen in &mut self.functions {
            let calls = screen.calls;
            screen.refresh(world);
            if calls != screen.calls {
                self.measure.clear();
            }
            self.problems.extend(screen.problems.clone());
            if let Some(tree) = &screen.tree {
                let mut tree = tree.clone();
                icons::resolve(&mut tree, &self.icons, &mut self.problems);
                let mut boxed = layout(&tree, size.x - 40.0, &mut self.measure, &self.style);
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
                present(&mut boxed, &mut self.presentation, None);
                self.boxes.push(boxed);
            }
        }
        for tree in crate::life::widgets(world)
            .into_iter()
            .chain(crate::talk::widgets(world))
        {
            let mut boxed = layout(&tree, size.x - 40.0, &mut self.measure, &self.style);
            place(&mut boxed, Vec2::ZERO, size, anchors, &mut self.problems);
            check(
                &boxed,
                Rect::new(0.0, 0.0, size.x, size.y),
                &mut self.measure,
                &mut self.problems,
            );
            self.boxes.push(boxed);
        }
        self.finish_motion();
        for boxed in &self.boxes {
            draw(boxed, overlay, None, &self.presentation, &self.style);
        }
        self.draw_drag(overlay);
        self.presented = self.presentation.clone();
        // Compare painted descendants, not just root bounds: columns have no surface,
        // and an anchored child can lie outside its root.
        let mut earlier: Vec<&Boxed> = Vec::new();
        for root in &self.boxes {
            let mut current = Vec::new();
            painted(root, &mut current);
            for b in &current {
                for other in &earlier {
                    if visible_overlap(b, other) {
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
        if !matches!(w.kind.as_str(), "button" | "bubble") {
            return None;
        }
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
                if !matches!(b.widget.kind.as_str(), "button" | "bubble") {
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

    pub(crate) fn hit(&self, point: Vec2) -> Option<&Widget> {
        self.hit_surface(point, false)
    }

    pub(crate) fn hover_hit(&self, point: Vec2) -> Option<&Widget> {
        self.hit_surface(point, true)
    }

    fn hit_surface(&self, point: Vec2, include_hovers: bool) -> Option<&Widget> {
        fn hit(b: &Boxed, p: Vec2, include_hovers: bool) -> Option<&Widget> {
            if b.clip.is_some_and(|r| !point_in(r, p)) {
                return None;
            }
            for child in b.children.iter().rev() {
                if let Some(w) = hit(child, p, include_hovers) {
                    return Some(w);
                }
            }
            let r = b.rect;
            (p.x >= r.x
                && p.y >= r.y
                && p.x < r.x + r.w
                && p.y < r.y + r.h
                && (painted_kind(&b.widget.kind) || (include_hovers && b.widget.hover.is_some())))
            .then_some(&b.widget)
        }
        for b in self.boxes.iter().rev() {
            if let Some(w) = hit(b, point, include_hovers) {
                return Some(w);
            }
        }
        None
    }

    pub fn calls(&self) -> u64 {
        self.functions.iter().map(|s| s.calls).sum()
    }
}

fn layout(w: &Widget, available: f32, measure: &mut TextMeasure, style: &ScreenStyle) -> Boxed {
    let mut padded = w.clone();
    if !w.explicit_pad
        && matches!(
            w.kind.as_str(),
            "panel" | "button" | "bubble" | "drag_source" | "drop_target"
        )
    {
        padded.pad = style.frame_padding;
    }
    if w.kind == "scroll" {
        padded.pad = w.pad.max(12.0);
    }
    if w.kind == "hover" {
        padded.width = Some(44.0);
        padded.height = Some(44.0);
    }
    let w = &padded;
    let width = w.width.unwrap_or(available).max(1.0);
    let inner = (width
        - 2.0 * w.pad
        - if w.kind == "scroll" {
            style.handle_width + 8.0
        } else {
            0.0
        })
    .max(1.0);
    let mut text = None;
    let mut children = Vec::new();
    let height;
    if leaf(&w.kind) {
        let labels = if w.kind == "delta" {
            vec![format!("{:+}", w.delta)]
        } else if w.kind == "drop_target" {
            verdict_labels(w)
        } else {
            vec![w.text.clone()]
        };
        let mut measured: Vec<_> = labels
            .into_iter()
            .map(|label| {
                let t = Text::new(label, Vec2::splat(w.pad), w.size).wrap(inner);
                let h = measure.measure(&t, 1.0).y;
                (t, h)
            })
            .collect();
        measured.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (t, h) = measured.pop().expect("at least one label");
        let mut t = t;
        let mut y = w.pad;
        for child in &w.children {
            let mut b = layout(child, inner, measure, style);
            b.rect.x = w.pad;
            b.rect.y = y;
            y += b.rect.h + w.gap;
            children.push(b);
        }
        t.pos.y = y;
        height = h + y + w.pad;
        text = Some(t);
    } else if w.kind == "grid" {
        let cell = ((inner - w.gap * (w.columns - 1) as f32) / w.columns as f32).max(1.0);
        let mut y = w.pad;
        for row in w.children.chunks(w.columns) {
            let mut boxes: Vec<_> = row
                .iter()
                .map(|c| layout(c, cell, measure, style))
                .collect();
            let h = boxes.iter().map(|b| b.rect.h).fold(0.0, f32::max);
            for (i, b) in boxes.iter_mut().enumerate() {
                b.rect.x = w.pad + i as f32 * (cell + w.gap);
                b.rect.y = y;
            }
            y += h + w.gap;
            children.extend(boxes);
        }
        height = if children.is_empty() {
            2.0 * w.pad
        } else {
            y - w.gap + w.pad
        };
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
            let mut b = layout(child, share, measure, style);
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
        clip: None,
        content_height: height,
        style: *style,
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
    if w.kind == "hover" {
        if let Some(p) = w.anchor.as_ref().and_then(|a| anchors.get(a)) {
            b.rect.x = p.x - 22.0;
            b.rect.y = p.y - 22.0;
        } else {
            problems.push(format!("{}: missing hover anchor", w.path));
        }
        return;
    }
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
    if b.widget.kind == "hover" {
        return;
    }
    let mut problem = |message: &str| problems.push(format!("{}: {message}", b.widget.path));
    if !contains(viewport, b.rect) {
        problem("outside the viewport");
    }
    if let Some(t) = &b.text {
        if b.widget.size < 16.0 {
            problem("text is smaller than 16 logical pixels");
        }
        if t.text.trim().is_empty()
            || (b.widget.kind == "drop_target" && b.widget.text.trim().is_empty())
        {
            problem("empty text or button label");
        }
        if !measure.missing(t, 1.0).is_empty() {
            problem("the bundled font has missing glyphs");
        }
        let size = measure.measure(t, 1.0);
        if !contains(b.rect, Rect::new(t.pos.x, t.pos.y, size.x, size.y)) {
            problem("text is clipped by its widget");
        }
        if matches!(
            b.widget.kind.as_str(),
            "button" | "bubble" | "drag_source" | "drop_target"
        ) && (b.rect.h < b.style.target_min || b.rect.w < b.style.target_min)
        {
            problem(&format!(
                "target is smaller than [screen] target_min ({} pixels)",
                b.style.target_min
            ));
        }
    }
    if b.widget.kind == "scroll"
        && b.content_height > b.rect.h
        && scrolling::viewport(b).h < b.style.handle_min
    {
        problems.push(format!(
            "{}: scroll viewport is smaller than [screen] handle_min ({} pixels)",
            b.widget.path, b.style.handle_min
        ));
    }
    let parent = if b.widget.kind == "scroll" {
        Rect::new(b.rect.x, b.rect.y, b.rect.w, b.content_height.max(b.rect.h))
    } else {
        b.rect
    };
    let child_viewport = if b.widget.kind == "scroll" {
        parent
    } else {
        viewport
    };
    if b.widget.kind == "drop_target"
        && let Some(base) = &b.text
    {
        for label in verdict_labels(&b.widget) {
            let mut t = base.clone();
            t.text = label;
            let size = measure.measure(&t, 1.0);
            if !contains(b.rect, Rect::new(t.pos.x, t.pos.y, size.x, size.y)) {
                problems.push(format!("{}: drop verdict is clipped", b.widget.path));
            }
            if !measure.missing(&t, 1.0).is_empty() {
                problems.push(format!(
                    "{}: drop verdict has missing glyphs",
                    b.widget.path
                ));
            }
        }
    }
    for (i, c) in b.children.iter().enumerate() {
        if b.widget.kind == "grid"
            && (c.widget.anchor.is_some() || c.widget.x.is_some() || c.widget.y.is_some())
        {
            problems.push(format!(
                "{}: grid cells must flow, without anchors or x/y",
                c.widget.path
            ));
        }
        if c.widget.anchor.is_none() && !contains(parent, c.rect) {
            problems.push(format!(
                "{}: outside its parent {}",
                c.widget.path, b.widget.path
            ));
        }
        for other in &b.children[..i] {
            if c.widget.kind != "hover"
                && other.widget.kind != "hover"
                && overlaps(c.rect, other.rect)
            {
                problems.push(format!("{}: overlaps {}", c.widget.path, other.widget.path));
            }
        }
        check(c, child_viewport, measure, problems);
    }
}

fn painted<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
    if painted_kind(&b.widget.kind) {
        out.push(b);
    }
    for child in &b.children {
        painted(child, out);
    }
}

fn draw(
    b: &Boxed,
    overlay: &mut Overlay,
    backing: Option<Rect>,
    state: &Presentation,
    style: &ScreenStyle,
) {
    if b.clip.is_some_and(|r| !overlaps(r, b.rect)) {
        return;
    }
    let mut layer = UiLayer {
        clip: b.clip,
        ..Default::default()
    };
    let mut backing = backing;
    let lifted = state.lifted(&b.widget);
    if matches!(
        b.widget.kind.as_str(),
        "panel" | "button" | "bubble" | "scroll" | "drag_source" | "drop_target"
    ) {
        let mut panel = style.surface(
            b.rect,
            if b.widget.kind == "bubble" {
                style.bubble
            } else {
                style.panel
            },
        );
        panel.radius = panel.radius.min(b.widget.pad * 2.0);
        if b.widget.kind == "button" {
            panel.fill = style.button;
        }
        if b.widget.kind == "drag_source" {
            panel.fill = style.button;
            if lifted {
                panel.fill = style.socket;
                panel.shadow = false;
                panel.dash = style.socket_dash;
                panel.border_px = style.border_px;
            }
        }
        if b.widget.kind == "drop_target"
            && let Some(drag) = &state.drag
        {
            panel.fill = if b
                .widget
                .accepts
                .get(&drag.value)
                .is_some_and(String::is_empty)
            {
                style.accept
            } else {
                style.refuse
            };
        }
        if b.widget.kind == "drop_target" && state.drag.is_some() {
            panel.border_px = style.hint_border_px;
        }
        panel.border_px = panel.border_px.min(b.widget.pad);
        layer.panels.push(panel);
        backing = Some(b.rect);
    } else if b.text.is_some() && backing.is_none_or(|r| !contains(r, b.rect)) {
        // A flat paper surface covers every glyph, including text with zero padding.
        let fill = if b.widget.kind == "sign" {
            style.sign
        } else {
            style.paper
        };
        let mut panel = style.surface(b.rect, fill);
        panel.radius = 0.0;
        panel.border_px = 0.0;
        panel.shadow = false;
        layer.panels.push(panel);
    }
    if let Some(t) = &b.text {
        let mut t = t.clone();
        t.color = style.text;
        if b.widget.kind == "delta" {
            t.color = if b.widget.delta < 0.0 {
                style.loss
            } else {
                style.gain
            };
        }
        if b.widget.kind == "drop_target" {
            t.text = match &state.drag {
                None => b.widget.text.clone(),
                Some(drag) => match b.widget.accepts.get(&drag.value) {
                    Some(reason) if reason.is_empty() => format!("{}\nDrop here", b.widget.text),
                    reason => format!(
                        "{}\nCannot drop: {}",
                        b.widget.text,
                        reason.map(String::as_str).unwrap_or("Not accepted")
                    ),
                },
            };
        }
        if !lifted {
            layer.texts.push(t);
        }
    }
    if !layer.panels.is_empty() || !layer.texts.is_empty() {
        overlay.widgets.push(layer);
    }
    for child in &b.children {
        draw(child, overlay, backing, state, style);
    }
    scrolling::draw(b, overlay, state);
}

fn leaf(kind: &str) -> bool {
    matches!(
        kind,
        "text" | "sign" | "button" | "bubble" | "delta" | "icon" | "drag_source" | "drop_target"
    )
}
fn painted_kind(kind: &str) -> bool {
    leaf(kind) || matches!(kind, "panel" | "scroll")
}
fn point_in(r: Rect, p: Vec2) -> bool {
    p.x >= r.x && p.y >= r.y && p.x < r.x + r.w && p.y < r.y + r.h
}
fn intersection(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect::new(
        x,
        y,
        (a.x + a.w).min(b.x + b.w).max(x) - x,
        (a.y + a.h).min(b.y + b.h).max(y) - y,
    )
}
fn visible_overlap(a: &Boxed, b: &Boxed) -> bool {
    overlaps(
        a.clip.map_or(a.rect, |r| intersection(r, a.rect)),
        b.clip.map_or(b.rect, |r| intersection(r, b.rect)),
    )
}
fn translate(b: &mut Boxed, dy: f32) {
    b.rect.y += dy;
    if let Some(t) = &mut b.text {
        t.pos.y += dy;
    }
    for c in &mut b.children {
        translate(c, dy);
    }
}
fn present(b: &mut Boxed, state: &mut Presentation, clip: Option<Rect>) {
    b.clip = clip;
    let child_clip = if b.widget.kind == "scroll" {
        let offset = state.scroll.entry(b.widget.path.clone()).or_default();
        *offset = offset.clamp(0.0, (b.content_height - b.rect.h).max(0.0));
        for child in &mut b.children {
            translate(child, -*offset);
        }
        let viewport = scrolling::viewport(b);
        Some(clip.map_or(viewport, |r| intersection(r, viewport)))
    } else {
        clip
    };
    for child in &mut b.children {
        present(child, state, child_clip);
    }
}

fn verdict_labels(w: &Widget) -> Vec<String> {
    let mut out = vec![
        w.text.clone(),
        format!("{}\nDrop here", w.text),
        format!("{}\nCannot drop: Not accepted", w.text),
    ];
    out.extend(
        w.accepts
            .values()
            .filter(|s| !s.is_empty())
            .map(|r| format!("{}\nCannot drop: {}", w.text, r)),
    );
    out
}
