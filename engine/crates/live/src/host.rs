//! The game's side of the live socket above the wire: [`Live`] answers each request from
//! a [`Host`] (the running game), between frames.
//!
//! Reads (`status`, `state`, `history`, `watch`, `shot`) take the session by shared
//! reference, so they can't change the world or its hash. `load` is the one change: it
//! hot-loads through the host, exactly like a `--watch` reload, so the session records it
//! and still replays.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use galaxy_api::engine::{HOT_LOADED, TICKS_PER_SECOND};
use galaxy_module::Failure;
use galaxy_module::module::Module;
use galaxy_render::RgbaImage;
use galaxy_sim::{Session, World};
use serde_json::{Map, Value, json};

use crate::protocol::{Discovery, Op, WireError};
use crate::reads::{self, HistoryQuery, StateQuery, Watched};
use crate::server::{Incoming, Server, ServerOptions, Wake};

/// The running game, as the live socket sees it.
pub trait Host {
    fn session(&self) -> &Session;
    /// What only the window knows: `fps`, `window` (logical size), `paused` (a note is
    /// being written) and the like. Merged into `status`.
    fn presentation(&self) -> Map<String, Value>;
    /// Why the world can't take a hot-load now, if it can't: an F8 note is open, and a
    /// note kept after a load on the same tick would hold a world its replay can't reach
    /// at that note.
    fn holds_loads(&self) -> Option<&'static str> {
        None
    }
    /// The frame the window shows now, drawn again at the same moment.
    fn frame(&mut self) -> Result<RgbaImage, String>;
    /// Hot-loads these modules between ticks, in order, as `--watch` would: each one's
    /// result. The host refreshes what it shows (keys, sounds, models) afterwards.
    fn hot_load(&mut self, modules: Vec<Module>) -> Vec<Result<(), String>>;
    /// Writes the recording so far into the session folder, after a hot-load.
    fn write_recording(&mut self) -> Option<PathBuf>;
}

/// A watch waiting for its ticks.
struct Pending {
    incoming: Incoming,
    watched: Watched,
    every: u64,
    end: u64,
    start: u64,
    samples: Vec<(u64, Value)>,
}

/// The live socket in a running game: the server, and the watches it is running.
pub struct Live {
    server: Server,
    watches: Vec<Pending>,
    /// Where each module was last loaded from, for `load` with no folder.
    dirs: BTreeMap<String, PathBuf>,
}

impl Live {
    /// Starts serving. `wake` is called when a request comes in, so a sleeping window loop
    /// wakes to answer it.
    pub fn start(opts: ServerOptions, wake: Option<Wake>) -> io::Result<Live> {
        let dirs = opts
            .modules
            .iter()
            .map(|m| (m.id.clone(), m.dir.clone()))
            .collect();
        Ok(Live {
            server: Server::start(opts, wake)?,
            watches: Vec::new(),
            dirs,
        })
    }

    pub fn discovery(&self) -> &Discovery {
        self.server.discovery()
    }

    /// Answers every request waiting. Call between ticks.
    pub fn serve(&mut self, host: &mut impl Host) {
        while let Some(incoming) = self.server.poll() {
            self.answer(host, incoming);
        }
    }

    /// Whether a watch is waiting for ticks.
    pub fn watching(&self) -> bool {
        !self.watches.is_empty()
    }

    /// Samples the watches after a tick, and answers those that are done.
    pub fn after_tick(&mut self, world: &World) {
        if self.watches.is_empty() {
            return;
        }
        let tick = world.tick;
        let mut still = Vec::new();
        for mut w in std::mem::take(&mut self.watches) {
            let due = tick > w.start && (tick - w.start).is_multiple_of(w.every) || tick == w.end;
            if due && w.samples.last().is_none_or(|s| s.0 != tick) {
                w.samples.push((tick, w.watched.sample(world)));
            }
            if tick >= w.end {
                let report = reads::watch_report(&w.watched, &w.samples);
                w.incoming.respond(Ok(report));
            } else {
                still.push(w);
            }
        }
        self.watches = still;
    }

    fn answer(&mut self, host: &mut impl Host, incoming: Incoming) {
        let world = &host.session().world;
        let result = match &incoming.op {
            Op::Status {} => Ok(self.status(host)),
            Op::State {
                entity,
                component,
                all,
            } => reads::state(
                world,
                &StateQuery {
                    entity: entity.clone(),
                    component: component.clone(),
                    all: *all,
                },
            )
            .map(|state| json!({ "tick": world.tick, "hash": world.hash(), "state": state }))
            .map_err(WireError::usage),
            Op::History {
                last,
                since,
                kind,
                inputs,
            } => Ok(reads::history(
                world,
                &host.session().log,
                &HistoryQuery {
                    last: *last,
                    since: *since,
                    kind: kind.clone(),
                    inputs: *inputs,
                },
            )),
            Op::Watch {
                entity,
                component,
                field,
                ticks,
                every,
            } => match Watched::resolve(world, entity, component, field.as_deref()) {
                Ok(watched) => {
                    let start = world.tick;
                    let samples = vec![(start, watched.sample(world))];
                    let (every, end) = (*every, start + ticks);
                    self.watches.push(Pending {
                        incoming,
                        watched,
                        every,
                        end,
                        start,
                        samples,
                    });
                    return;
                }
                Err(e) => Err(WireError::usage(e)),
            },
            Op::Shot { out } => {
                let out = out.clone();
                return self.shot(host, incoming, out);
            }
            Op::Load { dirs } => {
                let dirs = dirs.clone();
                self.load(host, &dirs)
            }
        };
        incoming.respond(result);
    }

    fn status(&self, host: &impl Host) -> Value {
        let session = host.session();
        let world = &session.world;
        let d = self.server.discovery();
        let place = galaxy_sim::travel::player(world)
            .map(|(being, place)| json!({ "being": being, "place": place }));
        let mut out = json!({
            "pid": d.pid,
            "build": d.build,
            "session": d.session,
            "session_dir": d.session_dir.display().to_string(),
            "modules": session.modules().map(|m| json!({
                "id": m.id(),
                "version": m.manifest.version.to_string(),
                "dir": self.dirs.get(m.id()).map(|d| d.display().to_string()),
            })).collect::<Vec<_>>(),
            "tick": world.tick,
            "seconds": world.tick as f64 / f64::from(TICKS_PER_SECOND),
            "hash": world.hash(),
            "place": place,
            "entities": world.entities.len(),
            "facts": world.facts.len(),
            "hot_loads": world.facts.iter().filter(|f| f.kind == HOT_LOADED).count(),
        });
        if let Value::Object(map) = &mut out {
            map.extend(host.presentation());
        }
        out
    }

    /// Draws the frame now and writes it off the game's thread; answers when written.
    fn shot(&mut self, host: &mut impl Host, incoming: Incoming, out: Option<PathBuf>) {
        let world = &host.session().world;
        let (tick, hash) = (world.tick, world.hash());
        let out = out.unwrap_or_else(|| {
            shots_dir(&self.server.discovery().session_dir).join(format!("tick-{tick}.png"))
        });
        let image = match host.frame() {
            Ok(i) => i,
            Err(e) => return incoming.respond(Err(WireError::runtime(e))),
        };
        let write = move || {
            if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| {
                    WireError::runtime(format!("can't make {}: {e}", parent.display()))
                        .with("path", parent.display().to_string())
                })?;
            }
            image.save_png(&out).map_err(|e| {
                WireError::runtime(e.to_string()).with("path", out.display().to_string())
            })?;
            Ok(json!({
                "out": out.display().to_string(),
                "size": [image.width, image.height],
                "tick": tick,
                "hash": hash,
            }))
        };
        let spawned = std::thread::Builder::new()
            .name("galaxy-live-shot".into())
            .spawn(move || incoming.respond(write()));
        if let Err(e) = spawned {
            eprintln!("galaxy: can't start writing a live shot: {e}");
        }
    }

    /// Hot-loads `dirs` (each a module folder), or with none every module whose files
    /// changed in the folder it was loaded from.
    fn load(&mut self, host: &mut impl Host, dirs: &[PathBuf]) -> Result<Value, WireError> {
        if let Some(why) = host.holds_loads() {
            return Err(WireError::new(5, "hot_load_refused", why).with("loaded", json!([])));
        }
        let running: BTreeMap<String, &Module> = host
            .session()
            .modules()
            .map(|m| (m.id().to_owned(), m))
            .collect();
        let named = !dirs.is_empty();
        let dirs: Vec<PathBuf> = if named {
            dirs.to_vec()
        } else {
            self.dirs.values().cloned().collect()
        };
        let mut modules = Vec::new();
        let mut unchanged = Vec::new();
        for dir in &dirs {
            galaxy_module::gate::open(std::slice::from_ref(dir)).map_err(wire)?;
            let module = Module::read(dir).map_err(wire)?;
            let Some(old) = running.get(module.id()) else {
                return Err(WireError::usage(format!(
                    "{} is module `{}`, which this game is not running; it runs {}",
                    dir.display(),
                    module.id(),
                    running.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
                .with("path", dir.display().to_string()));
            };
            if !named && old.files == module.files {
                unchanged.push(module.id().to_owned());
                continue;
            }
            let presentation_only = style_only(old, &module);
            modules.push((dir.clone(), presentation_only, module));
        }
        let ids: Vec<(PathBuf, bool, String, String)> = modules
            .iter()
            .map(|(d, p, m)| {
                (
                    d.clone(),
                    *p,
                    m.id().to_owned(),
                    m.manifest.version.to_string(),
                )
            })
            .collect();
        let results = if modules.is_empty() {
            Vec::new()
        } else {
            host.hot_load(modules.into_iter().map(|(_, _, m)| m).collect())
        };
        let mut loaded = Vec::new();
        let mut failed = Vec::new();
        for ((dir, presentation_only, id, version), result) in ids.into_iter().zip(results) {
            match result {
                Ok(()) => {
                    loaded.push(json!({
                        "module": id,
                        "version": version,
                        "dir": dir.display().to_string(),
                        "presentation_only": presentation_only,
                    }));
                    self.dirs.insert(id, dir);
                }
                Err(e) => failed.push(json!({ "module": id, "error": e })),
            }
        }
        let recording = if loaded.is_empty() {
            None
        } else {
            host.write_recording()
        };
        let world = &host.session().world;
        let out = json!({
            "loaded": loaded,
            "unchanged": unchanged,
            "failed": failed,
            "tick": world.tick,
            "hash": world.hash(),
            "recording": recording.map(|p| p.display().to_string()),
            "note": if loaded.is_empty() && failed.is_empty() {
                "nothing changed on disk since the game loaded these modules; name a module \
                 folder to load it anyway"
            } else {
                "a world hot-load runs before the next tick and is recorded like a --watch \
                 reload, so the session still replays; style-only loads change no world state"
            },
        });
        if failed.is_empty() {
            return Ok(out);
        }
        let mut e = WireError::new(
            5,
            "hot_load_refused",
            format!(
                "{} module(s) could not be hot-loaded; the game runs on with what it had",
                failed.len()
            ),
        );
        if let Value::Object(fields) = out {
            e.fields = fields;
        }
        Err(e)
    }
}

/// Whether `new` differs from `old` only in `style.toml` (a presentation-only load, as the
/// session decides it).
fn style_only(old: &Module, new: &Module) -> bool {
    let rest = |m: &Module| {
        m.files
            .0
            .iter()
            .filter(|(p, _)| p.as_str() != "style.toml")
            .map(|(p, t)| (p.clone(), t.clone()))
            .collect::<Vec<_>>()
    };
    old.files.get("style.toml") != new.files.get("style.toml") && rest(old) == rest(new)
}

fn wire(f: Failure) -> WireError {
    WireError {
        code: f.code,
        kind: f.kind.to_owned(),
        message: f.message,
        fields: f.fields,
    }
}

/// The folder a shot goes to when not told: `shots/` in the session folder.
pub fn shots_dir(session_dir: &Path) -> PathBuf {
    session_dir.join("shots")
}
