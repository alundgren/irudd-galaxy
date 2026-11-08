//! Widget layout, readability and hit testing. All dimensions are logical pixels.
mod hovers;
mod icons;
mod interaction;
mod map;
mod motion;
mod paint;
mod scrolling;
mod yielding;
pub use interaction::{Drag, Presentation};
pub use motion::Motion;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use galaxy_api::Registry;
use galaxy_api::ui::{UiAction, UiCue};
use galaxy_module::module::Module;
use galaxy_render::{
    Align, Overlay, Rect, Text, TextSpan, UiImage, UiLayer, glam::Vec2, screen_style::ScreenStyle,
    text::TextMeasure, vector::Icon,
};
use galaxy_script::{
    ScriptSet,
    screens::{Screen, Widget, anchored},
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
    pub style: Arc<ScreenStyle>,
    /// A module image icon and its box, placed like the text.
    pub icon: Option<IconBox>,
    /// Each span with a look, one rectangle per line, from the text's origin.
    pub span_rects: Vec<Vec<Rect>>,
    /// The whole label's lines, from the text's origin (for an underlined look).
    pub line_rects: Vec<Rect>,
    /// The width the words and icon take, for a tag that hugs them.
    pub content_width: f32,
    /// A drop target's height for its verdicts: the tallest label, or the height kept for
    /// them while a value is held. Zero for other widgets.
    pub verdicts: f32,
    /// A yielding widget not drawn this frame, with everything in it (`yield`).
    pub hidden: bool,
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
    icons: icons::Icons,
    pub clock: f32,
    pub anchors: BTreeMap<String, Vec2>,
    pub style: ScreenStyle,
    /// The player's reduced-motion setting: recorded into the presentation.
    pub reduced_motion: bool,
    /// UI sound cues since the game last took them; `None` unless the game listens, so
    /// staged scenes in check, notes and films make none.
    cues: RefCell<Option<Vec<UiCue>>>,
    /// The clickable thing the pointer was over in the last frame, for hover cues.
    hovering: Option<String>,
    /// Each `ui.map`'s paint this frame, by widget path, and the decoded memories.
    maps: BTreeMap<String, map::Frame>,
    memories: map::Memories,
}

/// At most this many cues wait to be taken; later ones are dropped.
const MAX_CUES: usize = 256;

#[derive(Clone, Debug)]
pub struct IconBox {
    pub rect: Rect,
    pub icon: Arc<Icon>,
    pub painted: bool,
}

/// What laying out a tree needs besides the tree.
struct Kit<'a> {
    measure: &'a mut TextMeasure,
    style: &'a Arc<ScreenStyle>,
    kept: &'a BTreeMap<String, f32>,
    icons: &'a icons::Icons,
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
        let previous = std::mem::take(&mut self.boxes);
        self.anchors = anchors.clone();
        let style = Arc::new(self.style.clone());
        // While a value is held, each drop target keeps the verdict height it had at
        // pickup, so a note the scripts rewrite moves no target. The drop ends it: the
        // drop's own handler may fill the target and change its verdicts.
        let kept = self
            .presentation
            .drag
            .as_ref()
            .map(|d| d.kept.clone())
            .unwrap_or_default();
        // Placed boxes as laid out (checked), and as shown (scrolled and clipped). Whether
        // a yielding widget shows is decided on what is shown, over every screen at once.
        let mut placed = Vec::new();
        let mut screens = 0;
        for screen in &mut self.functions {
            if screen.refresh(world) {
                self.measure.clear();
            }
            self.problems.extend(screen.problems.clone());
            if let Some(tree) = &screen.tree {
                let mut tree = tree.clone();
                icons::resolve(&mut tree, &self.icons, &mut self.problems);
                let mut kit = Kit {
                    measure: &mut self.measure,
                    style: &style,
                    kept: &kept,
                    icons: &self.icons,
                };
                let mut boxed = layout(&tree, size.x - 40.0, &mut kit);
                place(
                    &mut boxed,
                    Vec2::splat(20.0),
                    size,
                    anchors,
                    &mut self.problems,
                );
                placed.push(boxed);
                screens += 1;
            }
        }
        let signs = crate::life::widgets(world).into_iter().map(|mut tree| {
            if !style.sign_look.is_empty() {
                tree.look = Some(style.sign_look.clone());
            }
            tree
        });
        let bubbles = crate::talk::widgets(world, &style)
            .into_iter()
            .map(|mut tree| {
                if !style.bubble_look.is_empty() {
                    tree.look = Some(style.bubble_look.clone());
                }
                for part in &mut tree.children {
                    if part.path.ends_with("/title") && !style.bubble_title_look.is_empty() {
                        part.look = Some(style.bubble_title_look.clone());
                    }
                }
                tree
            });
        for tree in signs.chain(bubbles).collect::<Vec<_>>() {
            let mut kit = Kit {
                measure: &mut self.measure,
                style: &style,
                kept: &kept,
                icons: &self.icons,
            };
            let mut boxed = layout(&tree, size.x - 40.0, &mut kit);
            place(&mut boxed, Vec2::ZERO, size, anchors, &mut self.problems);
            placed.push(boxed);
        }
        let mut shown = placed.clone();
        for boxed in shown.iter_mut().take(screens) {
            present(boxed, &mut self.presentation, None);
        }
        yielding::resolve(&mut shown, size);
        for (boxed, shown) in placed.iter_mut().zip(&shown) {
            yielding::copy_hidden(shown, boxed);
            check(
                boxed,
                Rect::new(0.0, 0.0, size.x, size.y),
                paint::Ink::base(&self.style),
                &mut self.measure,
                &mut self.problems,
            );
        }
        self.boxes = shown;
        self.maps.clear();
        for root in &self.boxes {
            frames(
                root,
                world,
                &mut self.maps,
                &mut self.memories,
                &self.icons,
                &mut self.measure,
                &mut self.problems,
            );
        }
        if self.maps.is_empty() {
            self.memories.clear();
        }
        self.finish_motion();
        self.update_motion(&previous);
        let hovered = self.hovered_button();
        // Panels on their way out draw first, under what is on screen now.
        for (b, t) in self.leaving(size) {
            let fx = Fx {
                alpha: 1.0 - t,
                dy: t * self.motion_slide(),
            };
            draw(
                &b,
                overlay,
                paint::Ink::base(&self.style),
                fx,
                &self.painter(None),
            );
        }
        let painter = self.painter(hovered.as_deref());
        for boxed in &self.boxes {
            draw(
                boxed,
                overlay,
                paint::Ink::base(&self.style),
                Fx::NONE,
                &painter,
            );
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

    fn motion_slide(&self) -> f32 {
        if self.presentation.motion.reduced {
            0.0
        } else {
            self.style.motion_slide
        }
    }

    fn painter(&self, hovered: Option<&str>) -> Painter<'_> {
        Painter {
            screens: self,
            hovered: hovered.map(str::to_owned),
        }
    }

    /// The ready button under the pointer, by path, unless a gesture holds the pointer.
    fn hovered_button(&self) -> Option<String> {
        let p = &self.presentation;
        if p.drag.is_some() || p.settle.is_some() || p.scroll_grab.is_some() {
            return None;
        }
        let w = self.hit(Vec2::from(p.pointer?))?;
        (w.kind == "button" && w.resting.is_none()).then(|| w.path.clone())
    }

    /// Topmost painted surface wins, including a non-clickable panel covering a button.
    /// A resting button takes the click and records nothing.
    pub fn click(&self, point: Vec2, tick: u64) -> Option<Input> {
        let w = self.hit(point)?;
        if w.kind == "button" && w.resting.is_some() {
            self.ui_cue(UiAction::Deny, w);
            return None;
        }
        if !matches!(w.kind.as_str(), "button" | "bubble") {
            return None;
        }
        let input = w.on.as_ref().map(|name| Input {
            tick,
            source: "ui".into(),
            name: name.clone(),
            payload: w.payload.clone(),
            via: None,
        })?;
        self.ui_cue(UiAction::Press, w);
        Some(input)
    }

    /// Resolve bot targets against the current layout, then use ordinary pointer hit testing.
    /// A resting button is an error naming its reason; see [`Screens::bot_resting`].
    pub fn bot_click(&self, click: &Click) -> Result<Input, String> {
        let point = self.bot_point(click)?;
        if let Some((path, reason)) = self.resting_at(point) {
            return Err(format!("{path}: resting: {reason}"));
        }
        self.click(point, click.tick).ok_or_else(|| {
            format!(
                "click at {},{} before tick {} hit no button",
                point.x, point.y, click.tick
            )
        })
    }

    /// The resting button a bot click would land on, with its reason: the bot reports it
    /// instead of clicking, and nothing is recorded.
    pub fn bot_resting(&self, click: &Click) -> Option<(String, String)> {
        self.resting_at(self.bot_point(click).ok()?)
    }

    fn resting_at(&self, point: Vec2) -> Option<(String, String)> {
        let w = self.hit(point)?;
        (w.kind == "button").then_some((w.path.clone(), w.resting.clone()?))
    }

    /// Where a bot click lands: its point, or a named button's uncovered centre.
    fn bot_point(&self, click: &Click) -> Result<Vec2, String> {
        Ok(match &click.target {
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
            if b.hidden || b.clip.is_some_and(|r| !point_in(r, p)) {
                return None;
            }
            for child in b.children.iter().rev() {
                if let Some(w) = hit(child, p, include_hovers) {
                    // A bubble is one surface: its heading, line and marker click it.
                    // Only its answer buttons are separate targets.
                    if b.widget.kind == "bubble" && w.kind != "button" {
                        return Some(&b.widget);
                    }
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

    /// From now on, UI actions leave cues for [`Screens::take_cues`] (a played session).
    pub fn listen(&mut self, on: bool) {
        *self.cues.get_mut() = on.then(Vec::new);
    }

    pub fn listening(&self) -> bool {
        self.cues.borrow().is_some()
    }

    /// The UI sound cues since the last call, in the order they happened.
    pub fn take_cues(&mut self) -> Vec<UiCue> {
        self.cues
            .get_mut()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Puts cues taken with [`Screens::take_cues`] back, ahead of any newer ones.
    pub(crate) fn requeue(&mut self, mut cues: Vec<UiCue>) {
        if let Some(waiting) = self.cues.get_mut() {
            cues.append(waiting);
            *waiting = cues;
        }
    }

    /// Notes a UI action on `w` for the game to play and record.
    pub(crate) fn ui_cue(&self, action: UiAction, w: &Widget) {
        self.ui_cue_as(action, &w.kind, w.look.as_deref(), &w.path);
    }

    pub(crate) fn ui_cue_as(&self, action: UiAction, kind: &str, look: Option<&str>, widget: &str) {
        if let Some(cues) = self.cues.borrow_mut().as_mut()
            && cues.len() < MAX_CUES
        {
            cues.push(UiCue {
                tick: 0,
                time: self.clock,
                action,
                kind: kind.to_owned(),
                look: look.map(str::to_owned),
                widget: widget.to_owned(),
            });
        }
    }

    /// A UI action on the shown widget at `path`, such as a bot's click on a resting button.
    pub fn cue_path(&self, action: UiAction, path: &str) {
        if let Some(b) = self.all().into_iter().find(|b| b.widget.path == path) {
            self.ui_cue(action, &b.widget);
        }
    }

    /// The pointer is over `over` (a clickable widget) this frame: a hover cue when it is
    /// a different thing from last frame's. A widget with an id stays the same thing when
    /// its screen moves it to another parent.
    pub(crate) fn hover_over(&mut self, over: Option<&Widget>) {
        let key = over.map(motion::key);
        if key == self.hovering {
            return;
        }
        self.hovering = key;
        if let Some(w) = over {
            self.ui_cue(UiAction::Hover, w);
        }
    }
}

/// Works out the paint of every map in `b` for this frame.
fn frames(
    b: &Boxed,
    world: &World,
    maps: &mut BTreeMap<String, map::Frame>,
    memories: &mut map::Memories,
    icons: &icons::Icons,
    measure: &mut TextMeasure,
    problems: &mut Vec<String>,
) {
    if b.hidden {
        return;
    }
    if let Some(spec) = &b.widget.map {
        let frame = map::frame(b, spec, world, memories, icons, measure, problems);
        maps.insert(b.widget.path.clone(), frame);
    }
    for child in &b.children {
        frames(child, world, maps, memories, icons, measure, problems);
    }
}

/// `kit.kept` maps a drop target's path to the verdict height it keeps while a value is
/// held.
fn layout(w: &Widget, available: f32, kit: &mut Kit) -> Boxed {
    let style = kit.style;
    let look = style.look(w.look.as_deref());
    let mut padded = w.clone();
    if !w.explicit_pad {
        if matches!(
            w.kind.as_str(),
            "panel" | "button" | "bubble" | "drag_source" | "drop_target"
        ) {
            padded.pad = style.frame_padding;
        }
        if let Some(pad) = look.and_then(|l| l.pad) {
            padded.pad = pad;
        }
    }
    if let Some(weight) = look.and_then(|l| l.weight()) {
        padded.weight = weight;
    }
    if w.kind == "scroll" {
        padded.pad = w.pad.max(12.0);
    }
    if anchored(&w.kind) {
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
    let mut verdicts = 0.0;
    let mut icon = None;
    let mut span_rects = Vec::new();
    let mut line_rects = Vec::new();
    let mut content_width = 0.0;
    let height;
    let vector = w.icon.as_ref().and_then(|id| match kit.icons.get(id) {
        Some(icons::Asset::Vector { icon, painted }) => Some((icon.clone(), *painted)),
        _ => None,
    });
    if w.kind == "icon"
        && let Some((art, painted)) = vector.clone()
    {
        // An image icon is a square of `size` pixels.
        icon = Some(IconBox {
            rect: Rect::new(w.pad, w.pad, w.size, w.size),
            icon: art,
            painted,
        });
        content_width = w.size;
        height = w.size + 2.0 * w.pad;
    } else if leaf(&w.kind) {
        let side = (w.size * 1.25).round();
        let top = w.icon_at.as_deref() == Some("top");
        let beside = vector.is_some() && !top;
        let wrap = if beside { inner - side - 8.0 } else { inner }.max(1.0);
        let labels = if w.kind == "delta" {
            vec![format!("{:+}", w.delta)]
        } else if w.kind == "drop_target" {
            verdict_labels(w)
        } else {
            vec![w.text.clone()]
        };
        let spans: Vec<TextSpan> = paint::span_ranges(w)
            .into_iter()
            .map(|(range, name)| TextSpan {
                range,
                color: style.text,
                weight: style
                    .look(Some(&name))
                    .and_then(|l| l.weight())
                    .unwrap_or(w.weight),
            })
            .collect();
        let mut measured: Vec<_> = labels
            .into_iter()
            .map(|label| {
                let mut t = Text::new(label, Vec2::splat(w.pad), w.size)
                    .wrap(wrap)
                    .weight(w.weight);
                t.spans = spans.clone();
                if w.trailing {
                    t = t.align(Align::Right);
                    t.pos.x = w.pad + inner;
                }
                if vector.is_some() && top {
                    t = t.align(Align::Center);
                    t.pos.x = w.pad + inner * 0.5;
                }
                let h = kit.measure.measure(&t, 1.0).y;
                (t, h)
            })
            .collect();
        measured.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (mut t, mut h) = measured.pop().expect("at least one label");
        if w.kind == "drop_target" {
            // A held value keeps the space reserved at pickup; a verdict that outgrows it
            // is reported as clipped by check.
            h = kit.kept.get(&w.path).copied().unwrap_or(h);
            verdicts = h;
        }
        let mut y = w.pad;
        for child in &w.children {
            let mut b = layout(child, inner, kit);
            b.rect.x = w.pad;
            b.rect.y = y;
            y += b.rect.h + w.gap;
            children.push(b);
        }
        t.pos.y = y;
        // How wide the words run: the longest shaped line.
        let mut whole = t.clone();
        whole.spans = vec![TextSpan {
            range: 0..t.text.len(),
            color: style.text,
            weight: w.weight,
        }];
        let lines = kit
            .measure
            .span_rects(&whole, 1.0)
            .pop()
            .unwrap_or_default();
        let words = lines.iter().map(|r| r.x + r.w).fold(0.0, f32::max);
        if look.is_some_and(|l| l.underline.is_some()) {
            line_rects = lines;
        }
        if !t.spans.is_empty() {
            span_rects = kit.measure.span_rects(&t, 1.0);
        }
        content_width = words;
        match vector {
            Some((art, painted)) if top => {
                icon = Some(IconBox {
                    rect: Rect::new(w.pad + (inner - side) * 0.5, y, side, side),
                    icon: art,
                    painted,
                });
                t.pos.y = y + side + 6.0;
                height = h + side + 6.0 + y + w.pad;
                content_width = inner;
            }
            Some((art, painted)) => {
                let row = h.max(side);
                icon = Some(IconBox {
                    rect: Rect::new(w.pad, y + (row - side) * 0.5, side, side),
                    icon: art,
                    painted,
                });
                t.pos.x += side + 8.0;
                t.pos.y = y + (row - h) * 0.5;
                height = row + y + w.pad;
                content_width = words + side + 8.0;
            }
            None => height = h + y + w.pad,
        }
        text = Some(t);
    } else if w.kind == "map" {
        // A square unless given a height; it paints itself every frame.
        height = w.height.unwrap_or(width);
    } else if w.kind == "grid" {
        let cell = ((inner - w.gap * (w.columns - 1) as f32) / w.columns as f32).max(1.0);
        let mut y = w.pad;
        for row in w.children.chunks(w.columns) {
            let mut boxes: Vec<_> = row.iter().map(|c| layout(c, cell, kit)).collect();
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
        let flowing = w.children.iter().filter(|c| !detached(c)).count();
        let fixed: f32 = w
            .children
            .iter()
            .filter(|c| !detached(c))
            .filter_map(|c| c.width)
            .sum();
        let flexible = w
            .children
            .iter()
            .filter(|c| !detached(c) && c.width.is_none())
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
            let mut b = layout(child, share, kit);
            let detached = detached(child);
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
        style: kit.style.clone(),
        icon,
        span_rects,
        line_rects,
        content_width,
        verdicts,
        hidden: false,
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
    if anchored(&w.kind) {
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
    if let Some(align) = &w.align {
        pos = aligned(align, Vec2::new(b.rect.w, b.rect.h), w.x, w.y, size);
    }
    if let Some(anchor) = &w.anchor {
        match anchors.get(anchor) {
            Some(p) if w.yields => {
                // Never clamped: off screen, or in the way, it is hidden instead.
                pos = *p
                    + Vec2::new(
                        w.x.unwrap_or(0.0) - b.rect.w * 0.5,
                        w.y.unwrap_or(-12.0) - b.rect.h,
                    );
                b.hidden = !point_in(Rect::new(0.0, 0.0, size.x, size.y), *p);
            }
            None if w.yields => b.hidden = true,
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
    if let Some(i) = &mut b.icon {
        i.rect.x += pos.x;
        i.rect.y += pos.y;
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

fn check(
    b: &Boxed,
    viewport: Rect,
    ink: paint::Ink,
    measure: &mut TextMeasure,
    problems: &mut Vec<String>,
) {
    if anchored(&b.widget.kind) || b.hidden {
        return;
    }
    let (_, ink) = paint::resolve(b, ink, &b.style);
    paint::check(b, ink, &b.style, problems);
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
        if !contains(b.rect, text_rect(t, size)) {
            problem("text is clipped by its widget");
        }
    }
    if let Some(i) = &b.icon {
        if i.rect.w < 16.0 || i.rect.h < 16.0 {
            problem("icon is smaller than 16 logical pixels");
        }
        if !contains(b.rect, i.rect) {
            problem("icon is clipped by its widget");
        }
    }
    let target = matches!(
        b.widget.kind.as_str(),
        "button" | "drag_source" | "drop_target"
    ) && b.text.is_some()
        || b.widget.kind == "bubble" && b.widget.on.is_some();
    if target && (b.rect.h < b.style.target_min || b.rect.w < b.style.target_min) {
        problems.push(format!(
            "{}: target is smaller than [screen] target_min ({} pixels)",
            b.widget.path, b.style.target_min
        ));
    }
    // The frame is capped by the padding, and the frame is what marks a button resting.
    if b.widget.kind == "button" && b.widget.resting.is_some() && b.widget.pad < 1.0 {
        problems.push(format!(
            "{}: a resting button needs pad of at least 1 pixel to show its frame",
            b.widget.path
        ));
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
        if b.widget.kind == "grid" && detached(&c.widget) {
            problems.push(format!(
                "{}: grid cells must flow, without anchors, align or x/y",
                c.widget.path
            ));
        }
        if c.widget.anchor.is_none() && c.widget.align.is_none() && !contains(parent, c.rect) {
            problems.push(format!(
                "{}: outside its parent {}",
                c.widget.path, b.widget.path
            ));
        }
        for other in &b.children[..i] {
            if !anchored(&c.widget.kind)
                && !anchored(&other.widget.kind)
                && !c.hidden
                && !other.hidden
                && overlaps(c.rect, other.rect)
            {
                problems.push(format!("{}: overlaps {}", c.widget.path, other.widget.path));
            }
        }
        check(c, child_viewport, ink, measure, problems);
    }
}

fn painted<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
    if b.hidden {
        return;
    }
    if painted_kind(&b.widget.kind) {
        out.push(b);
    }
    for child in &b.children {
        painted(child, out);
    }
}

/// Motion applied to a widget and everything in it while drawing: presentation only.
#[derive(Clone, Copy, Debug)]
struct Fx {
    alpha: f32,
    dy: f32,
}
impl Fx {
    const NONE: Fx = Fx {
        alpha: 1.0,
        dy: 0.0,
    };
}

/// What drawing reads besides the box: the screens' presentation, style and clock.
struct Painter<'a> {
    screens: &'a Screens,
    hovered: Option<String>,
}

fn shifted(r: Rect, dy: f32) -> Rect {
    Rect::new(r.x, r.y + dy, r.w, r.h)
}

/// Where a text's shaped buffer starts: its spans and lines are measured from there.
fn origin(t: &Text) -> Vec2 {
    let wrap = t.max_width.unwrap_or(0.0);
    Vec2::new(
        t.pos.x
            - match t.align {
                Align::Left => 0.0,
                Align::Center => wrap * 0.5,
                Align::Right => wrap,
            },
        t.pos.y,
    )
}

fn draw(b: &Boxed, overlay: &mut Overlay, ink: paint::Ink, fx: Fx, p: &Painter) {
    if b.hidden || b.clip.is_some_and(|r| !overlaps(r, b.rect)) {
        return;
    }
    let screens = p.screens;
    let state = &screens.presentation;
    let style = &screens.style;
    let motion = !state.motion.reduced;
    let mut fx = fx;
    if matches!(b.widget.kind.as_str(), "panel" | "scroll" | "bubble") {
        let a = screens.arrival(b);
        fx.alpha *= a;
        fx.dy += (1.0 - a) * if motion { style.motion_slide } else { 0.0 };
    }
    let hovered = p.hovered.as_deref() == Some(b.widget.path.as_str());
    let pressed = state.motion.pressed.as_deref() == Some(b.widget.path.as_str());
    let mut dy = fx.dy;
    if motion && b.widget.kind == "button" && b.widget.resting.is_none() {
        if pressed {
            dy += style.press_sink;
        } else if hovered {
            dy -= style.hover_lift;
        }
    }
    let rect = shifted(b.rect, dy);
    if kind_is_map(&b.widget) {
        map::paint(
            screens.maps.get(&b.widget.path),
            rect,
            b.clip.map(|c| shifted(c, fx.dy)),
            style,
            fx.alpha,
            overlay,
        );
        return;
    }
    let mut layer = UiLayer {
        clip: b.clip.map(|c| shifted(c, fx.dy)),
        ..Default::default()
    };
    let lifted = state.lifted(&b.widget);
    let (surface, mut ink) = paint::resolve(b, ink, style);
    let look = style.look(b.widget.look.as_deref());
    let kind = b.widget.kind.as_str();
    let pad = b.widget.pad;
    // A tag hugs its words; other surfaces fill their box.
    let tag_rect = |r: Rect| {
        let w = (b.content_width + 2.0 * pad).min(r.w);
        let x = match b.text.as_ref().map(|t| t.align) {
            Some(Align::Right) => r.x + r.w - w,
            Some(Align::Center) => r.x + (r.w - w) * 0.5,
            _ => r.x,
        };
        Rect::new(x, r.y, w, r.h)
    };
    let mut fill_rect = rect;
    if let Some(s) = surface {
        let mut panel = style.surface(if s.tag { tag_rect(rect) } else { rect }, s.fill);
        fill_rect = panel.rect;
        if s.flat {
            panel.radius = 0.0;
            panel.border_px = 0.0;
        } else {
            panel.radius = s.radius.min(pad * 2.0);
            panel.border = s.border;
            panel.border_px = s.border_px;
            panel.shadow = s.shadow > 0.0;
            panel.shadow_px = s.shadow;
        }
        if kind == "button" {
            if b.widget.resting.is_some() {
                // Resting: its own fill and a dashed frame, with text at full contrast.
                panel.fill = style.resting_fill();
                panel.dash = style.resting_dash;
                panel.border_px = style.border_px.max(1.0);
                ink.text = style.text;
            } else if hovered {
                panel.fill = look
                    .and_then(|l| l.hover)
                    .or(style.button_hover)
                    .unwrap_or(panel.fill);
            }
        }
        if kind == "drag_source" && lifted {
            panel.fill = style.socket;
            panel.shadow = false;
            panel.dash = style.socket_dash;
            panel.border_px = style.border_px;
        }
        if kind == "drop_target"
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
            panel.border_px = style.hint_border_px;
            ink.text = style.text;
        }
        if !s.flat {
            panel.border_px = panel.border_px.min(pad);
        }
        panel.opacity = fx.alpha;
        layer.panels.push(panel);
    }
    let cue = screens.cue(b);
    if let Some((_, t)) = cue
        && b.widget.cue.as_deref() == Some("flash")
    {
        let mut flash = style.surface(fill_rect, ink.flash);
        flash.radius = style.radius.min(pad.max(4.0) * 2.0);
        flash.border_px = 0.0;
        flash.shadow = false;
        flash.opacity = (1.0 - t) * fx.alpha;
        layer.panels.push(flash);
    }
    if let Some(t) = &b.text {
        let mut t = t.clone();
        t.pos.y += dy;
        let fade = |c: galaxy_render::Color| c.with_alpha(c.a * fx.alpha);
        t.color = fade(ink.words(&b.widget));
        if kind == "delta"
            && let Some((cue, at)) = cue
            && b.widget.cue.as_deref() == Some("count")
        {
            t.text = motion::counted(cue, at);
        }
        if kind == "drop_target" {
            t.text = verdict(&b.widget, state.drag.as_ref().map(|d| d.value.as_str()));
        }
        let o = origin(&t);
        let line = |r: &Rect, size: f32| {
            Rect::new(
                o.x + r.x,
                o.y + r.y + r.h * 0.86,
                r.w,
                (size * 0.08).max(1.5),
            )
        };
        for ((range, name), rects) in paint::span_ranges(&b.widget).into_iter().zip(&b.span_rects) {
            let Some(span) = style.looks.get(&name) else {
                continue;
            };
            if let Some(span_text) = t.spans.iter_mut().find(|s| s.range == range) {
                span_text.color = fade(span.text.unwrap_or(ink.text));
            }
            for r in rects {
                if let Some(fill) = span.fill {
                    let mut tag =
                        style.surface(Rect::new(o.x + r.x - 4.0, o.y + r.y, r.w + 8.0, r.h), fill);
                    tag.radius = span.radius.unwrap_or(style.radius).min(r.h * 0.5);
                    tag.border_px = 0.0;
                    tag.shadow = false;
                    tag.opacity = fx.alpha;
                    layer.panels.push(tag);
                }
                if let Some(u) = span.underline {
                    let mut rule = style.surface(line(r, t.size), u);
                    rule.radius = 0.0;
                    rule.border_px = 0.0;
                    rule.shadow = false;
                    rule.opacity = fx.alpha;
                    layer.panels.push(rule);
                }
            }
        }
        if let Some(u) = look.and_then(|l| l.underline) {
            for r in &b.line_rects {
                let mut rule = style.surface(line(r, t.size), u);
                rule.radius = 0.0;
                rule.border_px = 0.0;
                rule.shadow = false;
                rule.opacity = fx.alpha;
                layer.panels.push(rule);
            }
        }
        if !lifted {
            layer.texts.push(t);
        }
    }
    if let Some(i) = &b.icon
        && !lifted
    {
        layer.images.push(UiImage {
            rect: shifted(i.rect, dy),
            icon: i.icon.clone(),
            tint: (!i.painted).then_some(ink.text),
            opacity: fx.alpha,
        });
    }
    if !layer.panels.is_empty() || !layer.texts.is_empty() || !layer.images.is_empty() {
        overlay.widgets.push(layer);
    }
    let child_fx = Fx {
        alpha: fx.alpha,
        dy,
    };
    for child in &b.children {
        draw(child, overlay, ink, child_fx, p);
    }
    scrolling::draw(b, overlay, state, fx.alpha, fx.dy);
}

/// Out of its parent's flow: anchored, aligned or explicitly placed.
fn detached(w: &Widget) -> bool {
    w.anchor.is_some() || w.align.is_some() || w.x.is_some() || w.y.is_some()
}

/// The top-left corner of a `size` box placed against the window's `align` edges, with
/// `x`/`y` as margins from those edges (or offsets from the middle on a centred axis).
fn aligned(align: &str, size: Vec2, x: Option<f32>, y: Option<f32>, window: Vec2) -> Vec2 {
    let (x, y) = (x.unwrap_or(0.0), y.unwrap_or(0.0));
    let (vertical, horizontal) = match align {
        "center" => ("middle", "center"),
        "left" | "right" => ("middle", align),
        "top" | "bottom" => (align, "center"),
        _ => align.split_once('-').unwrap_or(("top", "left")),
    };
    Vec2::new(
        match horizontal {
            "left" => x,
            "right" => window.x - size.x - x,
            _ => (window.x - size.x) * 0.5 + x,
        },
        match vertical {
            "top" => y,
            "bottom" => window.y - size.y - y,
            _ => (window.y - size.y) * 0.5 + y,
        },
    )
}

fn leaf(kind: &str) -> bool {
    matches!(
        kind,
        "text" | "sign" | "button" | "delta" | "icon" | "drag_source" | "drop_target"
    )
}
fn painted_kind(kind: &str) -> bool {
    leaf(kind) || matches!(kind, "panel" | "scroll" | "bubble" | "map")
}
fn kind_is_map(w: &Widget) -> bool {
    w.kind == "map" && w.map.is_some()
}
/// The box a measured text covers; a trailing text's position is its end edge.
fn text_rect(t: &Text, size: Vec2) -> Rect {
    let x = match t.align {
        Align::Left => t.pos.x,
        Align::Center => t.pos.x - size.x * 0.5,
        Align::Right => t.pos.x - size.x,
    };
    Rect::new(x, t.pos.y, size.x, size.y)
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
    if let Some(i) = &mut b.icon {
        i.rect.y += dy;
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

/// A drop target's label while `value` is held (its plain text with no drag): `Drop here`
/// and the value's note under it, or `Cannot drop:` and the reason.
fn verdict(w: &Widget, value: Option<&str>) -> String {
    let Some(value) = value else {
        return w.text.clone();
    };
    match w.accepts.get(value) {
        Some(reason) if reason.is_empty() => match w.notes.get(value) {
            Some(note) => format!("{}\nDrop here\n{note}", w.text),
            None => format!("{}\nDrop here", w.text),
        },
        reason => format!(
            "{}\nCannot drop: {}",
            w.text,
            reason.map_or("Not accepted", String::as_str)
        ),
    }
}

/// Every label a drop target can show, so layout reserves the tallest and check reads
/// each one: at rest, a bare accept, an unlisted value and every listed value.
fn verdict_labels(w: &Widget) -> Vec<String> {
    let mut out = vec![
        w.text.clone(),
        format!("{}\nDrop here", w.text),
        format!("{}\nCannot drop: Not accepted", w.text),
    ];
    out.extend(w.accepts.keys().map(|v| verdict(w, Some(v))));
    out
}
