//! `ui.map`: the place around a followed thing or a fixed point, painted in Rust
//! every frame from the world, so the screen's Luau never runs again because something
//! moved. Explored cells (`engine.explored`, [`galaxy_api::explored`]) are clear, the rest
//! is fog, marks show only on explored ground unless `always`, and the followed thing is
//! a heading marker. North (+y) is up, as in the scene.
//!
//! [`frame`] works out what to paint, relative to the map's top-left corner, once per
//! frame; [`paint`] draws it wherever the box ends up (scrolled, sliding in, fading out).
use std::sync::Arc;

use galaxy_api::engine::{EXPLORED, EXPLORER, POSITION};
use galaxy_api::explored::{self, DEFAULT_CELL, Memory};
use galaxy_render::{Color, Panel, PanelShape};
use galaxy_script::screens::MapSpec;
use serde_json::Value;

use super::*;

/// Decoded memories by entity, with the text each came from: decoded again only when the
/// world's text changes.
pub(super) type Memories = BTreeMap<String, (String, Arc<Memory>)>;

/// One map's paint for this frame, in pixels from the map's top-left corner.
#[derive(Clone, Debug, Default)]
pub(super) struct Frame {
    /// The ground as tiles that never overlap and together cover the map: explored
    /// (`true`) or under fog. Clipped to the map.
    pub tiles: Vec<(Rect, bool)>,
    pub marks: Vec<Mark>,
    /// The followed thing: where, and its turn on screen (radians clockwise from up).
    pub me: Option<(Vec2, f32)>,
}

#[derive(Clone, Debug)]
pub(super) struct Mark {
    pub at: Vec2,
    pub glyph: Option<String>,
    pub image: Option<Arc<galaxy_render::vector::Icon>>,
    pub painted: bool,
    pub label: Option<String>,
    /// The label's measured width.
    pub label_width: f32,
    pub look: Option<String>,
}

/// Pixel size of a mark's dot and the halo round it.
const DOT: f32 = 10.0;
const HALO: f32 = 3.0;
/// A mark's icon, and the heading marker's width and length: a long dart, so its tip
/// and its notched tail can't be mistaken for each other.
const ICON: f32 = 20.0;
const ME: Vec2 = Vec2::new(18.0, 26.0);
/// Map labels and glyph icons are the smallest readable text.
const LABEL: f32 = 16.0;

fn number(fields: &serde_json::Map<String, Value>, key: &str, default: f64) -> f64 {
    fields.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// Works out the map in box `b` for this frame. What can't be shown is a problem at its
/// path, as a missing anchor is.
pub(super) fn frame(
    b: &Boxed,
    spec: &MapSpec,
    world: &World,
    memories: &mut Memories,
    icons: &icons::Icons,
    measure: &mut TextMeasure,
    problems: &mut Vec<String>,
) -> Frame {
    let path = b.widget.path.as_str();
    let size = Vec2::new(b.rect.w, b.rect.h);
    let mut out = Frame::default();
    let place_of = |id: &str| galaxy_sim::places::place_of(world, id);
    let memory_place = |id: &str| match world.component(id, EXPLORED) {
        Some(c) => galaxy_sim::explore::memory_place(&c.fields),
        None => place_of(id),
    };
    // The space the map shows: the followed thing's place, else the memory's, else the
    // open plane. Only memory, marks and things in that space are drawn.
    let space = match (&spec.follow, &spec.memory) {
        (Some(id), _) => place_of(id),
        (None, Some(id)) => memory_place(id),
        (None, None) => "",
    };
    let open = |id: &str| place_of(id) == space;
    let position = |id: &str| world.component(id, POSITION).map(|c| &c.fields);
    let (cx, cy) = match (&spec.follow, spec.center) {
        (Some(id), _) => match position(id) {
            Some(p) => (number(p, "x", 0.0), number(p, "y", 0.0)),
            None => {
                problems.push(format!(
                    "{path}: map follows `{id}`, which has no engine.position"
                ));
                (0.0, 0.0)
            }
        },
        (None, Some(c)) => c,
        (None, None) => (0.0, 0.0),
    };
    let scale = f64::from(size.x) / spec.span;
    let to_px = |x: f64, y: f64| {
        Vec2::new(
            ((x - cx) * scale) as f32 + size.x * 0.5,
            size.y * 0.5 - ((y - cy) * scale) as f32,
        )
    };
    let memory = spec.memory.as_ref().and_then(|id| {
        match world.component(id, EXPLORED) {
            // Memory of another place: everything here is unexplored.
            Some(_) if memory_place(id) != space => {
                Some((Arc::new(Memory::default()), DEFAULT_CELL))
            }
            Some(c) => {
                let text = c.fields.get("cells").and_then(Value::as_str).unwrap_or("");
                let cell = number(&c.fields, "cell", DEFAULT_CELL);
                let fresh = memories.get(id).is_none_or(|(kept, _)| kept != text);
                if fresh {
                    let Ok(m) = Memory::parse(text) else {
                        problems.push(format!("{path}: `{id}`'s engine.explored is malformed"));
                        return None;
                    };
                    memories.insert(id.clone(), (text.to_owned(), Arc::new(m)));
                }
                Some((memories[id].1.clone(), cell))
            }
            None if world.component(id, EXPLORER).is_some() => {
                // An explorer before its first tick has explored nothing yet.
                Some((Arc::new(Memory::default()), DEFAULT_CELL))
            }
            None => {
                problems.push(format!(
                    "{path}: map memory `{id}` has no engine.explored or engine.explorer"
                ));
                None
            }
        }
    });
    let whole = Rect::new(0.0, 0.0, size.x, size.y);
    let explored_at = |x: f64, y: f64| match (&spec.memory, &memory) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(_), Some((m, cell))) => {
            explored::cell_of(x, y, *cell).is_some_and(|(i, j)| m.contains(i, j))
        }
    };
    match (&spec.memory, &memory) {
        (None, _) => out.tiles.push((whole, true)),
        (Some(_), Some((m, cell))) if explored::check_cell(*cell).is_ok() => {
            let half_w = spec.span * 0.5;
            let half_h = f64::from(size.y) / scale * 0.5;
            let range = |lo: f64, hi: f64| {
                let lo = explored::cell_of(lo, 0.0, *cell).map(|c| c.0);
                let hi = explored::cell_of(hi, 0.0, *cell).map(|c| c.0);
                lo.zip(hi)
            };
            match (
                range(cx - half_w, cx + half_w),
                range(cy - half_h, cy + half_h),
            ) {
                (Some((x0, x1)), Some((y0, y1))) => {
                    for row in y0..=y1 {
                        // The row's band, its explored runs, and fog in the gaps. Shared
                        // edges come from the same sums, so neighbours meet exactly.
                        let top = to_px(0.0, (row + 1) as f64 * cell).y.max(0.0);
                        let bottom = to_px(0.0, row as f64 * cell).y.min(size.y);
                        if bottom <= top {
                            continue;
                        }
                        let mut x = 0.0f32;
                        for (s, e) in m.runs(row, x0, x1 + 1) {
                            let a = to_px(s as f64 * cell, 0.0).x.max(0.0);
                            let b = to_px(e as f64 * cell, 0.0).x.min(size.x);
                            if a > x {
                                out.tiles
                                    .push((Rect::new(x, top, a - x, bottom - top), false));
                            }
                            if b > a {
                                out.tiles
                                    .push((Rect::new(a, top, b - a, bottom - top), true));
                            }
                            x = x.max(b);
                        }
                        if x < size.x {
                            out.tiles
                                .push((Rect::new(x, top, size.x - x, bottom - top), false));
                        }
                    }
                }
                _ => out.tiles.push((whole, false)),
            }
        }
        _ => out.tiles.push((whole, false)),
    }
    if let Some(id) = &spec.follow
        && let Some(p) = position(id)
        && open(id)
    {
        let angle = number(p, "angle", 0.0);
        out.me = Some((
            to_px(number(p, "x", 0.0), number(p, "y", 0.0)),
            (std::f64::consts::FRAC_PI_2 - angle) as f32,
        ));
    }
    let module = path.split(':').next().unwrap_or_default();
    for (i, m) in spec.marks.iter().enumerate() {
        let at_mark = format!("{path}/marks[{}]", i + 1);
        let Some(p) = position(&m.entity) else {
            let why = if world.entities.contains_key(&m.entity) {
                "has no engine.position"
            } else {
                "doesn't exist"
            };
            problems.push(format!("{at_mark}: mark `{}` {why}", m.entity));
            continue;
        };
        if !open(&m.entity) {
            continue;
        }
        let (x, y) = (number(p, "x", 0.0), number(p, "y", 0.0));
        if !m.always && !explored_at(x, y) {
            continue;
        }
        let mut at = to_px(x, y);
        let inset = DOT * 0.5 + HALO;
        if !point_in(whole, at) {
            if !m.always {
                continue;
            }
            at = at.clamp(
                Vec2::splat(inset),
                (size - Vec2::splat(inset)).max(Vec2::splat(inset)),
            );
        }
        if let Some(name) = &m.look {
            match b.style.looks.get(name) {
                None => problems.push(format!("{at_mark}: unknown look `{name}`")),
                Some(look) => {
                    let ink = look.fill.or(look.text).unwrap_or(b.style.map_mark);
                    if galaxy_render::screen_style::contrast(ink, b.style.map_clear) < 3.0 {
                        problems.push(format!(
                            "{at_mark}: look `{name}` needs contrast at least 3:1 on [screen] map_clear"
                        ));
                    }
                }
            }
        }
        let mut mark = Mark {
            at,
            glyph: None,
            image: None,
            painted: false,
            label: m.text.clone(),
            label_width: 0.0,
            look: m.look.clone(),
        };
        if let Some(name) = &m.icon {
            match icons.get(&format!("{module}.{name}")) {
                Some(icons::Asset::Glyph(g)) => mark.glyph = Some(g.clone()),
                Some(icons::Asset::Vector { icon, painted }) => {
                    mark.image = Some(icon.clone());
                    mark.painted = *painted;
                }
                None => problems.push(format!("{at_mark}: missing module icon `{name}`")),
            }
        }
        for words in mark.label.iter().chain(&mark.glyph) {
            if !measure
                .missing(&Text::new(words.clone(), Vec2::ZERO, LABEL), 1.0)
                .is_empty()
            {
                problems.push(format!("{at_mark}: the bundled font has missing glyphs"));
            }
        }
        if let Some(label) = &mark.label {
            mark.label_width = measure
                .measure(&Text::new(label.clone(), Vec2::ZERO, LABEL), 1.0)
                .x;
        }
        out.marks.push(mark);
    }
    out
}

/// A round mark: `fill` inside a ring of `ring` `ring_px` wide, as one panel, so it fades
/// as one.
fn disc(centre: Vec2, size: f32, fill: Color, ring: Color, ring_px: f32, alpha: f32) -> Panel {
    let mut p = Panel::new(Rect::new(
        centre.x - size * 0.5,
        centre.y - size * 0.5,
        size,
        size,
    ));
    p.radius = size * 0.5;
    p.fill = fill;
    p.border = ring;
    p.border_px = ring_px;
    p.shadow = false;
    p.opacity = alpha;
    p
}

/// Where a mark's label tag of `size` goes: right of the mark, `gap` from its centre,
/// or left of it when that would cross the map's right edge; kept inside `inner` so it
/// is never clipped (a tag wider than the map starts at its left edge).
fn label_at(at: Vec2, gap: f32, size: Vec2, inner: Rect) -> (f32, f32) {
    let right = at.x + gap;
    let x = if right + size.x <= inner.x + inner.w {
        right
    } else {
        at.x - gap - size.x
    };
    let x = x.min(inner.x + inner.w - size.x).max(inner.x);
    let y = (at.y - LABEL * 0.7 - 1.0)
        .min(inner.y + inner.h - size.y)
        .max(inner.y);
    (x, y)
}

/// Paints `frame` into the map box `rect`: its ground as tiles, its frame, marks and the
/// heading marker, in one layer clipped to the map. The ground and frame are hard-edged
/// tiles that never overlap, so the map fades evenly; each mark and the marker is one
/// panel with its halo as a ring.
pub(super) fn paint(
    frame: Option<&Frame>,
    rect: Rect,
    clip: Option<Rect>,
    style: &ScreenStyle,
    alpha: f32,
    overlay: &mut Overlay,
) {
    let clip = clip.map_or(rect, |c| intersection(c, rect));
    let mut layer = UiLayer {
        clip: Some(clip),
        ..Default::default()
    };
    let tile = |r: Rect, fill: Color| {
        let mut p = Panel::new(r);
        p.shape = PanelShape::Tile;
        p.fill = fill;
        p.border_px = 0.0;
        p.shadow = false;
        p.opacity = alpha;
        p
    };
    // The frame is the outer ring of tiles; the ground fills what is inside it.
    let e = style.border_px.max(1.0).min(rect.w * 0.5).min(rect.h * 0.5);
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let inner = Rect::new(x0 + e, y0 + e, rect.w - 2.0 * e, rect.h - 2.0 * e);
    for r in [
        Rect::new(x0, y0, rect.w, e),
        Rect::new(x0, y1 - e, rect.w, e),
        Rect::new(x0, y0 + e, e, rect.h - 2.0 * e),
        Rect::new(x1 - e, y0 + e, e, rect.h - 2.0 * e),
    ] {
        layer.panels.push(tile(r, style.map_edge));
    }
    let origin = Vec2::new(rect.x, rect.y);
    let empty = Frame::default();
    let frame = frame.unwrap_or(&empty);
    let ground = if frame.tiles.is_empty() {
        vec![(Rect::new(0.0, 0.0, rect.w, rect.h), false)]
    } else {
        frame.tiles.clone()
    };
    for (r, clear) in ground {
        let r = intersection(Rect::new(r.x + origin.x, r.y + origin.y, r.w, r.h), inner);
        if r.w > 0.0 && r.h > 0.0 {
            let fill = if clear {
                style.map_clear
            } else {
                style.map_fog
            };
            layer.panels.push(tile(r, fill));
        }
    }
    for m in &frame.marks {
        let at = origin + m.at;
        let look = style.look(m.look.as_deref());
        let ink = look
            .and_then(|l| l.fill.or(l.text))
            .unwrap_or(style.map_mark);
        let has_icon = m.image.is_some() || m.glyph.is_some();
        let size = if has_icon { ICON } else { DOT };
        if has_icon {
            // A plain backing the icon is drawn on, as text is on a panel.
            layer.panels.push(disc(
                at,
                size + 2.0 * HALO,
                style.map_clear,
                style.map_clear,
                0.0,
                alpha,
            ));
        } else {
            layer.panels.push(disc(
                at,
                DOT + 2.0 * HALO,
                ink,
                style.map_clear,
                HALO,
                alpha,
            ));
        }
        if let Some(icon) = &m.image {
            layer.images.push(UiImage {
                rect: Rect::new(at.x - ICON * 0.5, at.y - ICON * 0.5, ICON, ICON),
                icon: icon.clone(),
                tint: (!m.painted).then_some(ink),
                opacity: alpha,
            });
        } else if let Some(glyph) = &m.glyph {
            let mut t = Text::new(glyph.clone(), Vec2::new(at.x, at.y - LABEL * 0.65), LABEL)
                .align(Align::Center)
                .color(ink.with_alpha(alpha));
            t.weight = 600;
            layer.texts.push(t);
        }
        if let Some(label) = &m.label {
            let fill = look.and_then(|l| l.fill).unwrap_or(style.paper);
            // A look's text is checked on its own fill only, so it needs that fill.
            let words = look
                .filter(|l| l.fill.is_some())
                .and_then(|l| l.text)
                .unwrap_or(style.text);
            let pad = 4.0;
            let (w, h) = (m.label_width + 2.0 * pad, LABEL * 1.4 + 2.0);
            let (x, top) = label_at(at, size * 0.5 + HALO + 2.0, Vec2::new(w, h), inner);
            let y = top + 1.0;
            let mut tag = style.surface(Rect::new(x, top, w, h), fill);
            tag.radius = style.radius.min(6.0);
            tag.border_px = 0.0;
            tag.opacity = alpha;
            layer.panels.push(tag);
            layer.texts.push(
                Text::new(label.clone(), Vec2::new(x + pad, y), LABEL)
                    .color(words.with_alpha(alpha)),
            );
        }
    }
    if let Some((at, turn)) = frame.me {
        let at = origin + at;
        let size = ME + Vec2::splat(2.0 * HALO);
        let mut p = Panel::new(Rect::new(
            at.x - size.x * 0.5,
            at.y - size.y * 0.5,
            size.x,
            size.y,
        ));
        p.shape = PanelShape::Dart;
        p.rotate = turn;
        p.radius = 0.5;
        p.fill = style.map_self;
        p.border = style.map_clear;
        p.border_px = HALO;
        p.shadow = false;
        p.opacity = alpha;
        // Its own layer, over the marks' labels, so a label never hides the heading.
        let mut top = UiLayer {
            clip: Some(clip),
            ..Default::default()
        };
        top.panels.push(p);
        overlay.widgets.push(layer);
        overlay.widgets.push(top);
    } else {
        overlay.widgets.push(layer);
    }
}
