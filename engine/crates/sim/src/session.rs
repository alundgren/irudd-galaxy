//! A running world: loaded modules, their scripts and the world store, stepped one fixed
//! tick at a time.
//!
//! Each tick, in this order:
//!
//! 1. the tick number goes up by one;
//! 2. each input for this tick records its event (cause: the input);
//! 3. events: every fact not handled yet (those recorded during the previous tick, and
//!    this tick's inputs), in sequence order, runs the handlers for its kind: modules in
//!    ID order, scripts in manifest order. Facts recorded now are events next tick, so one
//!    tick never chains without end. Inputs are handled before the services, so a key
//!    acts on the tick it arrives;
//! 4. engine services run: flight, motion, collide, then ground ([`crate::services`]);
//! 5. on every [`CADENCE_TICKS`]th tick, each script's `cadence` handler runs, in the same
//!    order.
//!
//! A handler's commands apply as one batch right after it returns, so the next handler
//! reads the world with them. A handler that fails (an error, over budget, a write the
//! world refuses) changes nothing, and the engine records an `engine.script_failed` fact.
//! Nothing here reads a clock or the OS: the same modules, seed and inputs give the same
//! world, bit for bit.

use std::cell::Cell;
use std::collections::BTreeMap;

use galaxy_api::Registry;
use galaxy_api::command::{Batch, Cause, Command, Fields};
use galaxy_api::engine::{CADENCE_TICKS, HOT_LOADED, SCRIPT_FAILED, START};
use galaxy_module::module::Module;
use galaxy_script::{Abort, Event, ScriptSet, Target};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::recording::{BuildStamp, RecEvent, Recording};
use crate::services;
use crate::shape::Shapes;
use crate::world::World;

/// An input: something from outside the sim (a key, a click, a scenario step) on a tick.
/// It becomes a fact of kind `name`, which handlers see as an event that same tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub tick: u64,
    /// Where it came from: `keyboard`, `ui`, `scenario`, `cli` ...
    pub source: String,
    /// A declared event kind, as a full ID.
    pub name: String,
    #[serde(default)]
    pub payload: Fields,
}

/// A line a script printed or the host reported. Not part of the world.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LogLine {
    pub tick: u64,
    pub module: String,
    pub text: String,
}

/// Lines a session keeps.
const LOG_CAP: usize = 1000;

struct Loaded {
    module: Module,
    scripts: ScriptSet,
}

pub struct Session {
    pub world: World,
    modules: BTreeMap<String, Loaded>,
    registry: Registry,
    /// Solid outlines of the modules' models, for collision and steering.
    shapes: Shapes,
    pub log: Vec<LogLine>,
    recording: Option<Recording>,
}

/// Loads every module's scripts against `registry`. Problems are listed with their
/// module.
fn load_scripts(
    modules: Vec<Module>,
    registry: &Registry,
) -> Result<BTreeMap<String, Loaded>, String> {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    for module in modules {
        match ScriptSet::load(&module, registry) {
            Ok(scripts) => {
                out.insert(module.id().to_owned(), Loaded { module, scripts });
            }
            Err(list) => problems.extend(list.iter().map(|a| format!("{}: {a}", module.id()))),
        }
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(problems.join("\n"))
    }
}

impl Session {
    /// A new world from `modules`' start states.
    pub fn new(modules: Vec<Module>, seed: u64) -> Result<Session, String> {
        let registry = Registry::new(&modules.iter().collect::<Vec<_>>())?;
        let modules = load_scripts(modules, &registry)?;
        let mut world = World::new(seed);
        for (id, loaded) in &modules {
            let mut batch = Batch::new(Cause::Start { module: id.clone() });
            for (entity, components) in &loaded.module.start {
                for (component, fields) in components {
                    batch.commands.push(Command::Put {
                        entity: entity.clone(),
                        component: component.clone(),
                        fields: fields.clone(),
                    });
                }
            }
            world
                .apply(&batch, &registry)
                .map_err(|e| format!("{id}: start.toml: {e}"))?;
        }
        let cause = world.service_cause("start");
        world.record(cause, START, Fields::new());
        Ok(Session {
            world,
            shapes: Shapes::of(modules.values().map(|l| &l.module)),
            modules,
            registry,
            log: Vec::new(),
            recording: None,
        })
    }

    /// Carries on from a saved world with `modules`. Each module the world was made with
    /// must be loaded; their versions may have changed, as long as every stored component
    /// still fits its schema (new fields get their defaults).
    pub fn resume(modules: Vec<Module>, mut world: World) -> Result<Session, String> {
        let registry = Registry::new(&modules.iter().collect::<Vec<_>>())?;
        let modules = load_scripts(modules, &registry)?;
        world.conform(&registry, |w| {
            w.cause(Cause::Engine {
                service: "load".into(),
            })
        })?;
        Ok(Session {
            world,
            shapes: Shapes::of(modules.values().map(|l| &l.module)),
            modules,
            registry,
            log: Vec::new(),
            recording: None,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The loaded modules, in ID order.
    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.modules.values().map(|l| &l.module)
    }

    /// Checks an input can be run: its name is a declared event kind and its payload fits.
    pub fn check_input(&self, input: &Input) -> Result<(), String> {
        let command = Command::Record {
            kind: input.name.clone(),
            payload: input.payload.clone(),
        };
        self.registry
            .check(galaxy_api::Actor::Engine, &command)
            .map(|_| ())
    }

    /// Runs one tick with these inputs (their `tick` is ignored: they happen now).
    pub fn step(&mut self, inputs: &[Input]) -> Result<(), String> {
        self.world.tick += 1;
        let tick = self.world.tick;
        for input in inputs {
            let input = Input {
                tick,
                ..input.clone()
            };
            let batch = Batch {
                cause: Cause::Input {
                    source: input.source.clone(),
                    name: input.name.clone(),
                },
                commands: vec![Command::Record {
                    kind: input.name.clone(),
                    payload: input.payload.clone(),
                }],
            };
            self.world
                .apply(&batch, &self.registry)
                .map_err(|e| format!("input `{}` on tick {tick}: {e}", input.name))?;
            if let Some(rec) = &mut self.recording {
                rec.events.push(RecEvent::Input(input));
            }
        }
        self.dispatch_events();
        crate::flight::flight(&mut self.world, &self.shapes);
        services::motion(&mut self.world);
        crate::collide::collide(&mut self.world, &self.shapes);
        services::ground(&mut self.world);
        if tick.is_multiple_of(CADENCE_TICKS) {
            self.run_cadence();
        }
        Ok(())
    }

    fn dispatch_events(&mut self) {
        let end = self.world.facts.len() as u64;
        let from = self.world.next_event;
        self.world.next_event = end;
        for seq in from..end {
            let fact = self.world.facts[seq as usize].clone();
            let source = match &self.world.causes.get(fact.cause as usize).map(|c| &c.cause) {
                Some(Cause::Input { source, .. }) => Some(source.clone()),
                _ => None,
            };
            let event = Event {
                kind: &fact.kind,
                tick: fact.tick,
                fact: fact.seq,
                source: source.as_deref(),
                payload: &fact.payload,
            };
            let calls: Vec<(String, usize)> = self
                .modules
                .iter()
                .flat_map(|(id, loaded)| {
                    loaded
                        .scripts
                        .scripts
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.events.contains(&fact.kind))
                        .map(move |(i, _)| (id.clone(), i))
                })
                .collect();
            for (module, script) in calls {
                self.call(
                    &module,
                    script,
                    Target::Event(&event),
                    &fact.kind,
                    Some(fact.seq),
                );
            }
        }
    }

    fn run_cadence(&mut self) {
        let calls: Vec<(String, usize)> = self
            .modules
            .iter()
            .flat_map(|(id, loaded)| {
                loaded
                    .scripts
                    .scripts
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.cadence)
                    .map(move |(i, _)| (id.clone(), i))
            })
            .collect();
        for (module, script) in calls {
            self.call(&module, script, Target::Cadence, "cadence", None);
        }
    }

    /// Runs one handler and applies what it wrote, or records why it failed.
    fn call(
        &mut self,
        module: &str,
        script: usize,
        target: Target<'_>,
        handler: &str,
        fact: Option<u64>,
    ) {
        let loaded = &self.modules[module];
        let file = loaded.scripts.scripts[script].file.clone();
        let key = format!("script/{module}");
        let mut rng = self.world.rng.stream(&key).clone();
        let outcome = loaded
            .scripts
            .call(script, target, &self.world, &self.registry, &mut rng);
        *self.world.rng.stream(&key) = rng;
        for text in outcome.log {
            self.push_log(module, text);
        }
        let result = outcome.result.and_then(|commands| {
            let batch = Batch {
                cause: Cause::Script {
                    module: module.to_owned(),
                    script: file.clone(),
                    handler: handler.to_owned(),
                    fact,
                },
                commands,
            };
            self.world
                .apply(&batch, &self.registry)
                .map_err(|message| Abort {
                    code: "bad_command".into(),
                    message,
                    location: None,
                })
        });
        if let Err(abort) = result {
            self.push_log(module, format!("{file} {handler}: {abort}"));
            let cause = self.world.service_cause("script_host");
            let payload = json!({
                "module": module,
                "script": file,
                "handler": handler,
                "code": abort.code,
                "error": abort.message,
                "at": abort.location.map(|l| l.to_string()).unwrap_or_default(),
            });
            let Value::Object(payload) = payload else {
                unreachable!()
            };
            self.world.record(cause, SCRIPT_FAILED, payload);
        }
    }

    fn push_log(&mut self, module: &str, text: String) {
        if self.log.len() < LOG_CAP {
            self.log.push(LogLine {
                tick: self.world.tick,
                module: module.to_owned(),
                text,
            });
        }
    }

    /// Swaps in a changed module between ticks: its schemas, events and scripts. Every
    /// module's scripts are checked against the new set, and every stored component must
    /// still fit (new fields get their defaults). On failure nothing changes. The engine
    /// records an `engine.hot_loaded` fact, an event on the next tick.
    pub fn hot_load(&mut self, module: Module) -> Result<(), String> {
        let id = module.id().to_owned();
        if !self.modules.contains_key(&id) {
            return Err(format!(
                "module `{id}` is not running; hot-loading changes a running module"
            ));
        }
        let mut list: Vec<Module> = self
            .modules
            .values()
            .filter(|l| l.module.id() != id)
            .map(|l| l.module.clone())
            .collect();
        list.push(module.clone());
        let registry = Registry::new(&list.iter().collect::<Vec<_>>())?;
        let modules = load_scripts(list, &registry)?;
        let cause = Cause::HotLoad {
            module: id.clone(),
            version: module.manifest.version.to_string(),
            files: module.files.hash(),
        };
        let made = Cell::new(None);
        self.world.conform(&registry, |w| {
            let c = w.cause(cause.clone());
            made.set(Some(c));
            c
        })?;
        let cause = made.get().unwrap_or_else(|| self.world.cause(cause));
        let Value::Object(payload) = json!({
            "module": id,
            "version": module.manifest.version.to_string(),
        }) else {
            unreachable!()
        };
        self.world.record(cause, HOT_LOADED, payload);
        self.shapes = Shapes::of(modules.values().map(|l| &l.module));
        self.modules = modules;
        self.registry = registry;
        if let Some(rec) = &mut self.recording {
            rec.events.push(RecEvent::HotLoad {
                tick: self.world.tick + 1,
                module: id,
                files: module.files,
            });
        }
        Ok(())
    }

    /// Notes a remark on the current tick, in the recording, with the world's hash, and
    /// from a played session the camera and the frame that was shown.
    pub fn note(&mut self, text: &str, camera: Option<Value>, frame: Option<Value>) {
        if self.recording.is_some() {
            let hash = self.world.hash();
            let tick = self.world.tick;
            if let Some(rec) = &mut self.recording {
                rec.events.push(RecEvent::Note {
                    tick,
                    text: text.to_owned(),
                    hash: Some(hash),
                    camera,
                    frame,
                });
            }
        }
    }

    /// Notes in the recording that the player zoomed the view to `view` on this tick.
    /// Presentation only: the world is not touched.
    pub fn record_zoom(&mut self, view: f32) {
        let tick = self.world.tick;
        if let Some(rec) = &mut self.recording {
            // Many wheel events on one tick keep only where the wheel ended.
            if let Some(RecEvent::Zoom { tick: t, view: v }) = rec.events.last_mut()
                && *t == tick
            {
                *v = view;
                return;
            }
            rec.events.push(RecEvent::Zoom { tick, view });
        }
    }

    /// Starts recording from the world as it is now.
    pub fn start_recording(&mut self, build: BuildStamp) {
        self.recording = Some(Recording::start(
            build,
            &self.world,
            self.modules.values().map(|l| &l.module),
        ));
    }

    /// The recording so far, ended at the current tick.
    pub fn recording(&self) -> Option<Recording> {
        self.recording.as_ref().map(|r| r.ended(&self.world))
    }
}
