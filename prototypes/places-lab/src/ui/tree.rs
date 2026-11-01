//! The widget tree a Luau screen function returns, read into engine-owned widgets.
//!
//! A screen builds plain tables with the `ui.*` constructors (`lab/prelude.luau`). Every
//! widget kind and property is known here; anything else (a typo, a `color` on a text) is
//! a rule break, reported with the widget's path, so a screen can't quietly bypass the
//! engine's look.

use mlua::{Table, Value};
use serde_json::Value as Json;

use super::rules::Problems;

/// The widget kinds the engine owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The root: the scene to show and the layers placed on it.
    Screen,
    Column,
    Row,
    /// A surface in one of the style roles.
    Panel,
    Text,
    Button,
    /// A square button with an icon, a label and an optional ribbon (the station key).
    Tile,
    /// A state word with its colour and shape (gain, loss, new, ...).
    State,
    /// A labelled bar for a real quantity.
    Meter,
    Icon,
    /// Speech over a scene anchor; a click is its event.
    Bubble,
    /// A clickable place in the scene (a module), with its sign.
    Hotspot,
    /// Something that can be picked up and dragged.
    Drag,
    /// Somewhere a dragged thing can be let go, with the verdict for the current drag.
    Drop,
    Spacer,
    Divider,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "screen" => Kind::Screen,
            "column" => Kind::Column,
            "row" => Kind::Row,
            "panel" => Kind::Panel,
            "text" => Kind::Text,
            "button" => Kind::Button,
            "tile" => Kind::Tile,
            "state" => Kind::State,
            "meter" => Kind::Meter,
            "icon" => Kind::Icon,
            "bubble" => Kind::Bubble,
            "hotspot" => Kind::Hotspot,
            "drag" => Kind::Drag,
            "drop" => Kind::Drop,
            "spacer" => Kind::Spacer,
            "divider" => Kind::Divider,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Screen => "screen",
            Kind::Column => "column",
            Kind::Row => "row",
            Kind::Panel => "panel",
            Kind::Text => "text",
            Kind::Button => "button",
            Kind::Tile => "tile",
            Kind::State => "state",
            Kind::Meter => "meter",
            Kind::Icon => "icon",
            Kind::Bubble => "bubble",
            Kind::Hotspot => "hotspot",
            Kind::Drag => "drag",
            Kind::Drop => "drop",
            Kind::Spacer => "spacer",
            Kind::Divider => "divider",
        }
    }

    /// The properties this kind takes, besides `kind`, `id` and its children.
    fn props(self) -> &'static [&'static str] {
        match self {
            Kind::Screen => &["back", "ship"],
            Kind::Column | Kind::Row => &["gap", "pad", "align", "grow", "width", "place"],
            Kind::Panel => &["role", "pad", "gap", "width", "grow", "place", "align"],
            Kind::Text => &["text", "style", "grow", "align", "width"],
            Kind::Button => &["text", "on", "args", "rest", "hint", "icon", "grow"],
            Kind::Tile => &["text", "icon", "on", "args", "ribbon", "hint", "selected"],
            Kind::State => &["state", "text"],
            Kind::Meter => &["text", "value", "max", "over", "grow"],
            Kind::Icon => &["icon", "size"],
            Kind::Bubble => &[
                "anchor",
                "speaker",
                "place_name",
                "text",
                "more",
                "on",
                "args",
            ],
            Kind::Hotspot => &["anchor", "on", "args", "sign", "selected"],
            Kind::Drag => &["on", "args", "cancel", "anchor", "grow"],
            Kind::Drop => &[
                "on", "args", "anchor", "ok", "reason", "text", "preview", "side", "grow", "pad",
                "selected",
            ],
            Kind::Spacer => &["grow", "size"],
            Kind::Divider => &[],
        }
    }
}

/// The style roles: each kind of thing on screen has its own look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A room's own panel, beside its module.
    Room,
    /// What someone says (a bubble).
    Speech,
    /// The current goal.
    Goal,
    /// Credits, hold and other readouts.
    Status,
    /// Something you can press.
    Action,
    /// A module's hanging sign.
    Sign,
    /// The radio.
    Radio,
    /// One clear receipt for a transaction.
    Receipt,
    /// A group inside a panel, with no surface of its own.
    Plain,
}

impl Role {
    fn parse(s: &str) -> Option<Role> {
        Some(match s {
            "room" => Role::Room,
            "speech" => Role::Speech,
            "goal" => Role::Goal,
            "status" => Role::Status,
            "action" => Role::Action,
            "sign" => Role::Sign,
            "radio" => Role::Radio,
            "receipt" => Role::Receipt,
            "plain" => Role::Plain,
            _ => return None,
        })
    }
}

/// Text sizes. A screen picks one; the engine owns the numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextStyle {
    Title,
    Heading,
    #[default]
    Body,
    /// Numbers that matter (credits, prices).
    Figure,
    Small,
    /// Column headings and quiet labels.
    Caption,
}

impl TextStyle {
    fn parse(s: &str) -> Option<TextStyle> {
        Some(match s {
            "title" => TextStyle::Title,
            "heading" => TextStyle::Heading,
            "body" => TextStyle::Body,
            "figure" => TextStyle::Figure,
            "small" => TextStyle::Small,
            "caption" => TextStyle::Caption,
            _ => return None,
        })
    }

    /// Size in logical pixels at the 1280x720 design size, and weight.
    pub fn font(self) -> (f32, u16) {
        match self {
            TextStyle::Title => (26.0, 700),
            TextStyle::Heading => (20.0, 600),
            TextStyle::Body => (18.0, 500),
            TextStyle::Figure => (19.0, 600),
            TextStyle::Small => (15.0, 500),
            TextStyle::Caption => (14.0, 600),
        }
    }
}

/// The state words, each a colour from the colourblind-safe set and a shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateKind {
    Gain,
    Loss,
    Ok,
    Refused,
    New,
    Changed,
    Closed,
    Info,
}

impl StateKind {
    pub fn parse(s: &str) -> Option<StateKind> {
        Some(match s {
            "gain" => StateKind::Gain,
            "loss" => StateKind::Loss,
            "ok" => StateKind::Ok,
            "refused" => StateKind::Refused,
            "new" => StateKind::New,
            "changed" => StateKind::Changed,
            "closed" => StateKind::Closed,
            "info" => StateKind::Info,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    /// Fill the cross axis (columns only).
    Stretch,
}

/// A hotspot's sign: always its title; hovered, the keeper and one line too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sign {
    pub title: String,
    pub keeper: Option<String>,
    pub line: Option<String>,
    pub state: Option<(StateKind, String)>,
}

/// One widget, read and checked.
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    /// Where the widget sits in the tree, for rule reports (`screen/panel[2]/button[1]`).
    pub path: String,
    pub id: Option<String>,
    pub text: Option<String>,
    pub style: TextStyle,
    pub role: Role,
    pub place: Option<String>,
    pub anchor: Option<String>,
    pub gap: f32,
    pub pad: f32,
    pub width: Option<f32>,
    pub grow: f32,
    pub align: Align,
    pub on: Option<String>,
    pub args: Json,
    pub cancel: Option<String>,
    /// A resting button's reason. Resting buttons are dashed, never greyed.
    pub rest: Option<String>,
    pub hint: Option<String>,
    pub icon: Option<String>,
    pub size: f32,
    pub ribbon: Option<(StateKind, String)>,
    pub state: Option<StateKind>,
    pub selected: bool,
    pub value: f32,
    pub max: f32,
    pub over: f32,
    pub speaker: Option<String>,
    pub place_name: Option<String>,
    pub more: bool,
    pub sign: Option<Sign>,
    pub ok: Option<bool>,
    pub reason: Option<String>,
    pub side: Option<String>,
    pub preview: Option<Box<Node>>,
    // Screen root only.
    pub scene: Option<String>,
    pub focus: Option<String>,
    pub back: Option<String>,
    pub ship: Json,
    pub children: Vec<Node>,
    /// Where layout put it, logical pixels.
    pub rect: render::Rect,
    /// The wrap width its text was laid out at, if it wraps.
    pub wrap: Option<f32>,
}

impl Node {
    fn new(kind: Kind, path: String) -> Self {
        Node {
            kind,
            path,
            id: None,
            text: None,
            style: TextStyle::Body,
            role: Role::Plain,
            place: None,
            anchor: None,
            gap: 0.0,
            pad: 0.0,
            width: None,
            grow: 0.0,
            align: Align::Start,
            on: None,
            args: Json::Null,
            cancel: None,
            rest: None,
            hint: None,
            icon: None,
            size: 0.0,
            ribbon: None,
            state: None,
            selected: false,
            value: 0.0,
            max: 0.0,
            over: 0.0,
            speaker: None,
            place_name: None,
            more: false,
            sign: None,
            ok: None,
            reason: None,
            side: None,
            preview: None,
            scene: None,
            focus: None,
            back: None,
            ship: Json::Null,
            children: Vec::new(),
            rect: render::Rect::default(),
            wrap: None,
        }
    }

    /// An empty screen (shown when a script fails).
    pub fn empty_screen() -> Node {
        Node::new(Kind::Screen, "screen".into())
    }

    /// The first node with this id.
    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id.as_deref() == Some(id) {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }
}

fn string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.to_string_lossy()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Number(n) => Some(format!("{n}")),
        _ => None,
    }
}

fn number(v: &Value) -> Option<f32> {
    match v {
        Value::Integer(i) => Some(*i as f32),
        Value::Number(n) => Some(*n as f32),
        _ => None,
    }
}

/// Reads a widget table. `path` names it in problems.
pub fn read(lua: &mlua::Lua, value: &Value, path: &str, problems: &mut Problems) -> Option<Node> {
    let Value::Table(t) = value else {
        problems.add(
            path,
            format!("expected a widget, got {}", value.type_name()),
        );
        return None;
    };
    let kind_name: Option<String> = t.raw_get::<Value>("kind").ok().as_ref().and_then(string);
    let Some(kind) = kind_name.as_deref().and_then(Kind::parse) else {
        problems.add(
            path,
            format!(
                "not a widget (kind {:?}); build widgets with ui.*",
                kind_name.unwrap_or_default()
            ),
        );
        return None;
    };
    let path = if path.is_empty() {
        kind.name().to_string()
    } else {
        path.to_string()
    };
    let mut node = Node::new(kind, path.clone());
    let mut counts: std::collections::HashMap<&'static str, usize> = Default::default();
    // Array part: children, or the text for text-like widgets.
    let len = t.raw_len();
    for i in 1..=len {
        let v: Value = t.raw_get(i).unwrap_or(Value::Nil);
        if let Some(s) = string(&v) {
            if i == 1 && node.text.is_none() {
                node.text = Some(s);
            } else {
                problems.add(&path, format!("stray text {s:?} among the children"));
            }
            continue;
        }
        if let Value::Table(_) = &v {
            let child_kind: Option<String> = match &v {
                Value::Table(ct) => ct.raw_get::<Value>("kind").ok().as_ref().and_then(string),
                _ => None,
            };
            let name = child_kind
                .as_deref()
                .and_then(Kind::parse)
                .map_or("?", |k| k.name());
            let n = counts.entry(name).or_default();
            *n += 1;
            let child_path = format!("{path}/{name}[{n}]");
            if let Some(child) = read(lua, &v, &child_path, problems) {
                node.children.push(child);
            }
        } else if !v.is_nil() {
            problems.add(&path, format!("child {i} is a {}", v.type_name()));
        }
    }
    for pair in t.clone().pairs::<Value, Value>() {
        let Ok((k, v)) = pair else { continue };
        let Value::String(k) = k else { continue };
        let key = k.to_string_lossy();
        if key == "kind" {
            continue;
        }
        if key != "id" && key != "text" && !kind.props().contains(&key.as_str()) {
            problems.add(&path, format!("{} has no property `{key}`", kind.name()));
            continue;
        }
        prop(lua, &mut node, &key, &v, problems);
    }
    Some(node)
}

fn prop(lua: &mlua::Lua, node: &mut Node, key: &str, v: &Value, problems: &mut Problems) {
    let path = node.path.clone();
    let bad = |problems: &mut Problems, what: &str| {
        problems.add(
            &path,
            format!("`{key}` should be {what}, got {}", v.type_name()),
        );
    };
    let s = string(v);
    let n = number(v);
    match key {
        "id" => node.id = s,
        "text" => match s {
            Some(s) => node.text = Some(s),
            None => bad(problems, "a string"),
        },
        "style" => match s.as_deref().and_then(TextStyle::parse) {
            Some(st) => node.style = st,
            None => problems.add(&path, format!("unknown text style {s:?}")),
        },
        "role" => match s.as_deref().and_then(Role::parse) {
            Some(r) => node.role = r,
            None => problems.add(&path, format!("unknown role {s:?}")),
        },
        "place" => node.place = s,
        "anchor" => node.anchor = s,
        "scene" => node.scene = s,
        "focus" => node.focus = s,
        "back" => node.back = s,
        "on" => node.on = s,
        "cancel" => node.cancel = s,
        "hint" => node.hint = s,
        "icon" => node.icon = s,
        "speaker" => node.speaker = s,
        "place_name" => node.place_name = s,
        "reason" => node.reason = s,
        "side" => node.side = s,
        "gap" => node.gap = n.unwrap_or(0.0),
        "pad" => node.pad = n.unwrap_or(0.0),
        "width" => node.width = n,
        "grow" => node.grow = n.unwrap_or(0.0),
        "size" => node.size = n.unwrap_or(0.0),
        "value" => node.value = n.unwrap_or(0.0),
        "max" => node.max = n.unwrap_or(0.0),
        "over" => node.over = n.unwrap_or(0.0),
        "align" => {
            node.align = match s.as_deref() {
                Some("start") => Align::Start,
                Some("center") => Align::Center,
                Some("end") => Align::End,
                Some("stretch") => Align::Stretch,
                _ => {
                    problems.add(&path, format!("unknown align {s:?}"));
                    Align::Start
                }
            }
        }
        "more" => node.more = matches!(v, Value::Boolean(true)),
        "selected" => node.selected = matches!(v, Value::Boolean(true)),
        "ok" => match v {
            Value::Boolean(b) => node.ok = Some(*b),
            Value::Nil => {}
            _ => bad(problems, "true or false"),
        },
        "rest" => match v {
            Value::Boolean(false) | Value::Nil => {}
            Value::String(_) => node.rest = s,
            _ => problems.add(
                &path,
                "a resting button needs its reason: rest = \"Hold full\"".to_string(),
            ),
        },
        "state" => match s.as_deref().and_then(StateKind::parse) {
            Some(k) => node.state = Some(k),
            None => problems.add(&path, format!("unknown state {s:?}")),
        },
        "ribbon" => match v {
            Value::Nil | Value::Boolean(false) => {}
            Value::Table(t) => {
                let kind: Option<String> = t.raw_get::<Value>(1).ok().as_ref().and_then(string);
                let word: Option<String> = t.raw_get::<Value>(2).ok().as_ref().and_then(string);
                match (kind.as_deref().and_then(StateKind::parse), word) {
                    (Some(k), Some(w)) if !w.trim().is_empty() => node.ribbon = Some((k, w)),
                    (Some(_), _) => problems.add(
                        &path,
                        "a ribbon needs its word as well as its colour: {\"new\", \"New\"}"
                            .to_string(),
                    ),
                    _ => problems.add(&path, format!("unknown ribbon {kind:?}")),
                }
            }
            _ => bad(problems, "{kind, word}"),
        },
        "sign" => match v {
            Value::Table(t) => node.sign = Some(sign(t)),
            _ => bad(problems, "a table"),
        },
        "args" | "ship" => {
            let json = mlua::LuaSerdeExt::from_value::<Json>(lua, v.clone());
            match json {
                Ok(j) if key == "args" => node.args = j,
                Ok(j) => node.ship = j,
                Err(e) => problems.add(&path, format!("`{key}` can't be passed on: {e}")),
            }
        }
        "preview" => {
            let child_path = format!("{path}/preview");
            node.preview = read(lua, v, &child_path, problems).map(Box::new);
        }
        _ => {}
    }
}

fn sign(t: &Table) -> Sign {
    let get = |k: &str| -> Option<String> { t.raw_get::<Value>(k).ok().as_ref().and_then(string) };
    let state = match t.raw_get::<Value>("state") {
        Ok(Value::Table(s)) => {
            let kind: Option<String> = s.raw_get::<Value>(1).ok().as_ref().and_then(string);
            let word: Option<String> = s.raw_get::<Value>(2).ok().as_ref().and_then(string);
            kind.as_deref().and_then(StateKind::parse).zip(word)
        }
        _ => None,
    };
    Sign {
        title: get("title").unwrap_or_default(),
        keeper: get("keeper"),
        line: get("line"),
        state,
    }
}
