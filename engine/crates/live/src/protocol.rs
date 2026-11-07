//! What travels over the live socket: newline-delimited JSON, one request and one reply a
//! line, and the discovery file a running game writes so clients can find it.
//!
//! A request is `{"id": 1, "token": "...", "op": "state", "args": {"entity": "walker"}}`;
//! its reply is `{"id": 1, "ok": true, "result": {...}}` or
//! `{"id": 1, "ok": false, "error": {"code": 4, "kind": "bad_input", "message": "...",
//! "fields": {...}}}`, where `code` and `kind` are the CLI's exit code and error kind, so
//! the client prints the error as its own.

use std::io;
use std::path::{Path, PathBuf};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{Name, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// Bump when requests or replies change incompatibly.
pub const PROTOCOL: u32 = 1;
/// The longest request line the game reads.
pub const MAX_REQUEST: u64 = 64 * 1024;
/// The longest text argument (an ID, a kind, a path).
pub const MAX_TEXT: usize = 4096;
/// The most ticks one `watch` covers: five minutes.
pub const MAX_WATCH_TICKS: u64 = 9000;
/// The most samples one `watch` returns.
pub const MAX_SAMPLES: u64 = 1000;
/// The most facts one `history` returns.
pub const MAX_HISTORY: usize = 500;
/// The most ticks one `input` runs: five minutes.
pub const MAX_INPUT_TICKS: u64 = 9000;
/// The most steps one `input` takes.
pub const MAX_STEPS: usize = 1000;
/// The most edits one `edit` makes.
pub const MAX_EDITS: usize = 1000;
/// The left mouse button, as `input` names it beside the manifest key names (which name
/// the right one `mouse_right`).
pub const MOUSE_LEFT: &str = "mouse_left";
/// The farthest a pointer step reaches, in logical pixels.
const MAX_POINT: f32 = 100_000.0;

/// What kind of local socket an endpoint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// A Windows named pipe; `name` is the pipe name without `\\.\pipe\`.
    NamedPipe,
    /// A Unix-domain socket; `name` is its path.
    UnixSocket,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub kind: EndpointKind,
    pub name: String,
}

impl Endpoint {
    /// A blocking connection.
    pub fn connect(&self) -> io::Result<Stream> {
        Stream::connect(self.to_name()?)
    }

    pub(crate) fn to_name(&self) -> io::Result<Name<'_>> {
        match self.kind {
            EndpointKind::NamedPipe => {
                self.name
                    .as_str()
                    .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            }
            EndpointKind::UnixSocket => {
                self.name
                    .as_str()
                    .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            }
        }
    }
}

/// A module the game runs: its ID and the folder it was read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningModule {
    pub id: String,
    pub dir: PathBuf,
}

/// What a running game writes to the live folder while it serves, so clients find it.
/// Only the user can read it (the user's own data folder; mode 0600 on Unix): the token
/// in it is what lets a client in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    pub protocol: u32,
    pub endpoint: Endpoint,
    /// Sent with every request. Never printed or logged.
    pub token: String,
    pub pid: u32,
    /// The session folder's name (`2026-11-01-142233-walk`): what `--game` picks it by.
    pub session: String,
    pub session_dir: PathBuf,
    /// The build that is playing: its installed name, or its version.
    pub build: String,
    pub modules: Vec<RunningModule>,
    /// Seconds since the Unix epoch when it started serving. Wall-clock, read only by
    /// clients choosing between games, never by the sim.
    pub started: u64,
}

impl Discovery {
    pub fn read(path: &Path) -> Result<Discovery, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The game as a listing shows it: everything but the token and the endpoint.
    pub fn public(&self) -> Value {
        json!({
            "pid": self.pid,
            "session": self.session,
            "session_dir": self.session_dir.display().to_string(),
            "build": self.build,
            "modules": self.modules.iter().map(|m| json!({
                "id": m.id,
                "dir": m.dir.display().to_string(),
            })).collect::<Vec<_>>(),
            "started": self.started,
        })
    }
}

fn every_tick() -> u64 {
    1
}

/// One request's operation and its arguments: `{"op": "...", "args": {...}}` on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "args", rename_all = "snake_case")]
pub enum Op {
    /// Who is running: build, modules, tick, hash, the player's place, frame rate.
    Status {},
    /// The world, a line an entity, or the fields of one entity or component.
    State {
        #[serde(default)]
        entity: Option<String>,
        #[serde(default)]
        component: Option<String>,
        #[serde(default)]
        all: bool,
    },
    /// Recent facts (inputs among them) and script prints, oldest first.
    History {
        #[serde(default)]
        last: Option<usize>,
        #[serde(default)]
        since: Option<u64>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        inputs: bool,
    },
    /// A component, or one field of it, sampled after each tick for `ticks` ticks.
    Watch {
        entity: String,
        component: String,
        #[serde(default)]
        field: Option<String>,
        ticks: u64,
        #[serde(default = "every_tick")]
        every: u64,
    },
    /// A PNG of what the window shows, written to `out` (or the session's `shots/`).
    Shot {
        #[serde(default)]
        out: Option<PathBuf>,
    },
    /// Hot-load modules: these module folders, or with none the game's own folders whose
    /// files changed.
    Load {
        #[serde(default)]
        dirs: Vec<PathBuf>,
    },
    /// Keys and the pointer, held and moved over the next `ticks` ticks through the same
    /// path as the window's own: answered once they have passed. What the connection
    /// still holds when it closes is let go.
    Input { steps: Vec<Step>, ticks: u64 },
    /// Edits the world between two ticks, as one batch: names may be written as `state`
    /// takes them (a unique part of an ID), the game makes them full. Recorded, so the
    /// session still replays.
    Edit { edits: Vec<galaxy_sim::edit::Edit> },
    /// Sends one of a module's debug-only events on the next tick, as an input with source
    /// `debug` (the event may be named by a unique last part). Recorded like any input.
    Debug {
        event: String,
        #[serde(default)]
        payload: serde_json::Map<String, Value>,
    },
}

/// One step of an `input`: what happens, `at` ticks after the input started. A step at
/// `at` takes effect on the tick `at + 1` after the one the input started on, so a step
/// at 0 reaches the very next tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub at: u64,
    #[serde(flatten)]
    pub act: Act,
}

/// What a step does: `{"do": "key", "key": "w", "down": true}` or
/// `{"do": "move", "x": 400, "y": 300}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Act {
    /// A key goes down or up: a manifest key name (`w`, `space`, `mouse_right`) or
    /// [`MOUSE_LEFT`].
    Key { key: String, down: bool },
    /// The pointer moves to this point of the window, in logical pixels from its top
    /// left.
    Move { x: f32, y: f32 },
}

impl Act {
    /// The step as `galaxy-gm live input` writes it: `down w`, `up w`, `move 400 300`.
    pub fn text(&self) -> String {
        match self {
            Act::Key { key, down: true } => format!("down {key}"),
            Act::Key { key, down: false } => format!("up {key}"),
            Act::Move { x, y } => format!("move {x} {y}"),
        }
    }
}

/// Whether `key` is one `input` can hold: a manifest key name or [`MOUSE_LEFT`].
pub fn is_key(key: &str) -> bool {
    key == MOUSE_LEFT || galaxy_module::manifest::KEYS.contains(&key)
}

/// Why `key` is not a key, with the names that are.
pub fn not_a_key(key: &str) -> String {
    let mut names: Vec<&str> = galaxy_module::manifest::KEYS.to_vec();
    names.push(MOUSE_LEFT);
    format!("`{key}` is not a key; keys are {}", names.join(", "))
}

impl Op {
    /// The wire name.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Status {} => "status",
            Op::State { .. } => "state",
            Op::History { .. } => "history",
            Op::Watch { .. } => "watch",
            Op::Shot { .. } => "shot",
            Op::Load { .. } => "load",
            Op::Input { .. } => "input",
            Op::Edit { .. } => "edit",
            Op::Debug { .. } => "debug",
        }
    }

    /// Parses `op` with `args` (`null` reads as `{}`).
    pub fn parse(op: &str, args: Value) -> Result<Op, String> {
        let args = if args.is_null() { json!({}) } else { args };
        let op: Op = serde_json::from_value(json!({ "op": op, "args": args }))
            .map_err(|e| format!("bad `{op}` request: {e}"))?;
        op.check()?;
        Ok(op)
    }

    /// Checks sizes and ranges, so the game never spends a frame on a request it would
    /// refuse anyway.
    pub fn check(&self) -> Result<(), String> {
        let text = |what: &str, s: &str| {
            if s.len() > MAX_TEXT {
                Err(format!("{what} is {} bytes; at most {MAX_TEXT}", s.len()))
            } else {
                Ok(())
            }
        };
        match self {
            Op::Status {} => Ok(()),
            Op::State {
                entity, component, ..
            } => {
                entity
                    .as_deref()
                    .map_or(Ok(()), |e| text("the entity", e))?;
                component
                    .as_deref()
                    .map_or(Ok(()), |c| text("the component", c))
            }
            Op::History { kind, last, .. } => {
                if last.is_some_and(|n| n == 0 || n > MAX_HISTORY) {
                    return Err(format!("--last takes 1 to {MAX_HISTORY}"));
                }
                kind.as_deref().map_or(Ok(()), |k| text("the kind", k))
            }
            Op::Watch {
                entity,
                component,
                field,
                ticks,
                every,
            } => {
                text("the entity", entity)?;
                text("the component", component)?;
                field.as_deref().map_or(Ok(()), |f| text("the field", f))?;
                if *ticks == 0 || *ticks > MAX_WATCH_TICKS {
                    return Err(format!(
                        "--ticks takes 1 to {MAX_WATCH_TICKS} (30 ticks a second)"
                    ));
                }
                if *every == 0 {
                    return Err("--every takes 1 or more ticks".into());
                }
                if ticks.div_ceil(*every) > MAX_SAMPLES {
                    return Err(format!(
                        "{ticks} ticks every {every} is more than {MAX_SAMPLES} samples; \
                         raise --every"
                    ));
                }
                Ok(())
            }
            Op::Shot { out } => out
                .as_deref()
                .map_or(Ok(()), |p| text("the path", &p.to_string_lossy())),
            Op::Load { dirs } => {
                if dirs.len() > 64 {
                    return Err("at most 64 module folders at once".into());
                }
                dirs.iter()
                    .try_for_each(|d| text("the module folder", &d.to_string_lossy()))
            }
            Op::Edit { edits } => {
                if edits.is_empty() || edits.len() > MAX_EDITS {
                    return Err(format!(
                        "an edit makes 1 to {MAX_EDITS} changes; this one makes {}",
                        edits.len()
                    ));
                }
                for edit in edits {
                    edit.action()?;
                    for name in [&edit.component, &edit.place, &edit.spot]
                        .into_iter()
                        .flatten()
                        .chain([&edit.entity])
                    {
                        text("a name", name)?;
                    }
                }
                Ok(())
            }
            Op::Debug { event, payload } => {
                text("an event", event)?;
                text("a payload", &Value::Object(payload.clone()).to_string())
            }
            Op::Input { steps, ticks } => {
                if *ticks == 0 || *ticks > MAX_INPUT_TICKS {
                    return Err(format!(
                        "an input runs 1 to {MAX_INPUT_TICKS} ticks (30 a second, so at most \
                         {} seconds); this one runs {ticks}",
                        MAX_INPUT_TICKS / 30
                    ));
                }
                if steps.len() > MAX_STEPS {
                    return Err(format!(
                        "an input takes at most {MAX_STEPS} steps; this one has {}",
                        steps.len()
                    ));
                }
                for step in steps {
                    if step.at >= *ticks {
                        return Err(format!(
                            "step `{}` is at tick {} of an input that runs {ticks}",
                            step.act.text(),
                            step.at
                        ));
                    }
                    match &step.act {
                        Act::Key { key, .. } if !is_key(key) => return Err(not_a_key(key)),
                        Act::Move { x, y }
                            if !(x.is_finite() && y.is_finite())
                                || !(0.0..=MAX_POINT).contains(x)
                                || !(0.0..=MAX_POINT).contains(y) =>
                        {
                            return Err(format!(
                                "a pointer point is logical pixels from the window's top left, \
                                 0 to {MAX_POINT}; not ({x}, {y})"
                            ));
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
        }
    }
}

/// A request line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub token: String,
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

/// A failed request, as the CLI's exit code, error kind, message and extra fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    pub code: u8,
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub fields: Map<String, Value>,
}

impl WireError {
    pub fn new(code: u8, kind: &str, message: impl Into<String>) -> Self {
        WireError {
            code,
            kind: kind.to_owned(),
            message: message.into(),
            fields: Map::new(),
        }
    }

    /// Exit 2, kind `usage`: the request names something that isn't there.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(2, "usage", message)
    }

    /// Exit 4, kind `bad_input`.
    pub fn input(message: impl Into<String>) -> Self {
        Self::new(4, "bad_input", message)
    }

    /// Exit 1, kind `runtime`.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new(1, "runtime", message)
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.fields.insert(key.to_owned(), value.into());
        self
    }
}

/// A reply line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

impl Response {
    pub fn line(id: u64, result: Result<Value, WireError>) -> String {
        let r = match result {
            Ok(v) => Response {
                id,
                ok: true,
                result: Some(v),
                error: None,
            },
            Err(e) => Response {
                id,
                ok: false,
                result: None,
                error: Some(e),
            },
        };
        serde_json::to_string(&r).expect("replies serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_parse_from_the_wire_and_are_checked() {
        let op = Op::parse("state", json!({ "entity": "walker" })).unwrap();
        assert_eq!(
            op,
            Op::State {
                entity: Some("walker".into()),
                component: None,
                all: false
            }
        );
        assert_eq!(Op::parse("status", Value::Null).unwrap(), Op::Status {});
        let watch = Op::parse(
            "watch",
            json!({ "entity": "a", "component": "b", "ticks": 30 }),
        )
        .unwrap();
        assert!(matches!(watch, Op::Watch { every: 1, .. }));
        assert!(Op::parse("nope", Value::Null).is_err());
        assert!(
            Op::parse(
                "watch",
                json!({ "entity": "a", "component": "b", "ticks": 0 })
            )
            .is_err()
        );
        assert!(
            Op::parse(
                "watch",
                json!({ "entity": "a", "component": "b", "ticks": 5000, "every": 1 })
            )
            .unwrap_err()
            .contains("samples")
        );
        assert!(Op::parse("history", json!({ "last": 0 })).is_err());
        let input = Op::parse(
            "input",
            json!({ "ticks": 2, "steps": [
                { "at": 0, "do": "key", "key": "w", "down": true },
                { "at": 1, "do": "move", "x": 400, "y": 300 },
            ] }),
        )
        .unwrap();
        let Op::Input { steps, .. } = &input else {
            panic!("{input:?}")
        };
        assert_eq!(steps[1].act, Act::Move { x: 400.0, y: 300.0 });
        assert_eq!(serde_json::to_value(&steps[0]).unwrap()["do"], "key");
        let step = |at: u64, key: &str| {
            json!({ "ticks": 2, "steps": [
            { "at": at, "do": "key", "key": key, "down": true }
        ] })
        };
        assert!(
            Op::parse("input", step(2, "w"))
                .unwrap_err()
                .contains("runs 2")
        );
        assert!(
            Op::parse("input", step(0, "jump"))
                .unwrap_err()
                .contains("not a key")
        );
        assert!(Op::parse("input", step(0, "mouse_left")).is_ok());
        assert!(Op::parse("input", json!({ "ticks": 0, "steps": [] })).is_err());
        assert!(Op::parse("state", json!({ "entity": "x".repeat(5000) })).is_err());
    }
}
