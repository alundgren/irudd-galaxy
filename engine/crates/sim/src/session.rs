//! A running world: loaded modules, their scripts and the world store, stepped one fixed
//! tick at a time.
//!
//! Each tick, in this order:
//!
//! 1. the tick number goes up by one;
//! 2. each input for this tick records its event (cause: the input). After
//!    [`Session::announce_resume`], the first one is `engine.resumed`;
//! 3. events: every fact not handled yet (those recorded during the previous tick, and
//!    this tick's inputs), in sequence order, runs the handlers for its kind: modules in
//!    ID order, scripts in manifest order. Facts recorded now are events next tick, so one
//!    tick never chains without end. Inputs are handled before the services, so a key
//!    acts on the tick it arrives;
//! 4. engine services run: flight, motion, walking, collide, then ground ([`crate::services`]);
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
use galaxy_api::engine::{
    CADENCE_TICKS, DEBUG_SOURCE, EDITED, HOT_LOADED, RESUME_SOURCE, RESUMED, SCRIPT_FAILED, START,
};
use galaxy_module::ids;
use galaxy_module::module::Module;
use galaxy_script::vm::Code;
use galaxy_script::{Abort, Event, ScriptSet, Target};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::edit::{Action, Edit};
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
    /// How it reached the game when that was not the window itself: `live` for input
    /// `galaxy-gm live input` sent. Recorded and shown, never part of the world: the
    /// event, its `source` and the hash are those the window's own key or pointer makes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
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
    /// How inputs that did not come from the window itself arrived, by the cause they
    /// made ([`Input::via`]). Outside the world, so never in its hash.
    vias: BTreeMap<u64, String>,
    /// The next tick starts with an `engine.resumed` input ([`Session::announce_resume`]).
    resume_pending: bool,
}

/// A module set worked out once for many new sessions ([`Session::prepared`]): the
/// modules, their models' solid outlines and a bytecode cache their scripts compile into.
/// Outlines and bytecode are pure functions of the module files, so a session started
/// from here is the one [`Session::new`] starts; sessions share nothing else. `galaxy-gm
/// test` starts every scenario of a run from one.
#[derive(Clone)]
pub struct Prepared {
    modules: Vec<Module>,
    shapes: Shapes,
    code: Code,
}

impl Prepared {
    pub fn new(modules: Vec<Module>) -> Prepared {
        Prepared {
            shapes: Shapes::of(&modules),
            modules,
            code: Code::default(),
        }
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }
}

/// Loads every module's scripts against `registry`, compiling into `code`. Problems are
/// listed with their module.
fn load_scripts(
    modules: Vec<Module>,
    registry: &Registry,
    code: &Code,
) -> Result<BTreeMap<String, Loaded>, String> {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    for module in modules {
        match ScriptSet::load_with(&module, registry, code) {
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
        Session::start(modules, seed, None, &Code::default())
    }

    /// [`Session::new`] with `prepared`'s modules, without working out their outlines or
    /// compiling their scripts again.
    pub fn prepared(prepared: &Prepared, seed: u64) -> Result<Session, String> {
        Session::start(
            prepared.modules.clone(),
            seed,
            Some(prepared.shapes.clone()),
            &prepared.code,
        )
    }

    fn start(
        modules: Vec<Module>,
        seed: u64,
        shapes: Option<Shapes>,
        code: &Code,
    ) -> Result<Session, String> {
        let registry = Registry::new(&modules.iter().collect::<Vec<_>>())?;
        let modules = load_scripts(modules, &registry, code)?;
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
        crate::rooms::hydrate(&mut world, modules.values().map(|l| &l.module), &registry)?;
        let cause = world.service_cause("start");
        world.record(cause, START, Fields::new());
        crate::places::validate(&world)?;
        crate::talk::validate(&world)?;
        crate::life::validate(&world)?;
        crate::life::update(&mut world, &registry)?;
        if world
            .entities
            .values()
            .any(|c| c.contains_key(galaxy_api::engine::LIFE))
        {
            services::ground(&mut world);
        }
        Ok(Session {
            world,
            shapes: shapes.unwrap_or_else(|| Shapes::of(modules.values().map(|l| &l.module))),
            modules,
            registry,
            log: Vec::new(),
            recording: None,
            vias: BTreeMap::new(),
            resume_pending: false,
        })
    }

    /// Carries on from a saved world with `modules`. Each module the world was made with
    /// must be loaded; their versions may have changed, as long as every stored component
    /// still fits its schema (new fields get their defaults).
    pub fn resume(modules: Vec<Module>, mut world: World) -> Result<Session, String> {
        let registry = Registry::new(&modules.iter().collect::<Vec<_>>())?;
        let modules = load_scripts(modules, &registry, &Code::default())?;
        world.conform(&registry, |w| {
            w.cause(Cause::Engine {
                service: "load".into(),
            })
        })?;
        crate::places::validate(&world)?;
        crate::talk::validate(&world)?;
        crate::life::validate(&world)?;
        Ok(Session {
            world,
            shapes: Shapes::of(modules.values().map(|l| &l.module)),
            modules,
            registry,
            log: Vec::new(),
            recording: None,
            vias: BTreeMap::new(),
            resume_pending: false,
        })
    }

    /// Tells scripts a save resumed: the next tick starts with an `engine.resumed` input
    /// (source `resume`, no payload), recorded like any input, so a replay applies it from
    /// the recording and never announces again. A resumed world therefore ends as an
    /// uninterrupted one does given that same input on the tick after the save.
    /// [`Session::resume`] alone (as a replay starts) announces nothing.
    pub fn announce_resume(&mut self) {
        self.resume_pending = true;
    }

    /// How the input that made cause `cause` arrived, when not from the window itself
    /// (`live`).
    pub fn via(&self, cause: u64) -> Option<&str> {
        self.vias.get(&cause).map(String::as_str)
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// The loaded modules, in ID order.
    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.modules.values().map(|l| &l.module)
    }

    /// Whether `event` is one of a module's debug-only events (`debug` in its manifest).
    pub fn is_debug(&self, event: &str) -> bool {
        ids::split(event).is_some_and(|(ns, name)| {
            self.modules
                .get(ns)
                .is_some_and(|l| l.module.manifest.debug.iter().any(|d| d == name))
        })
    }

    /// Checks an input can be run: its name is a declared event kind and its payload fits,
    /// and a debug-only event comes with source `debug`, the only thing that source sends.
    pub fn check_input(&self, input: &Input) -> Result<(), String> {
        let debug = self.is_debug(&input.name);
        if debug && input.source != DEBUG_SOURCE {
            return Err(format!(
                "`{}` is a debug event: send it with source = \"{DEBUG_SOURCE}\" (or galaxy-gm \
                 live debug), not from {}",
                input.name, input.source
            ));
        }
        if !debug && input.source == DEBUG_SOURCE {
            return Err(format!(
                "source {DEBUG_SOURCE} sends only debug events (`debug` in a manifest), and \
                 `{}` is not one",
                input.name
            ));
        }
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
        let before = crate::travel::position(&self.world);
        self.world.tick += 1;
        let tick = self.world.tick;
        let resumed = std::mem::take(&mut self.resume_pending).then(|| Input {
            tick,
            source: RESUME_SOURCE.to_owned(),
            name: RESUMED.to_owned(),
            payload: Fields::new(),
            via: None,
        });
        for input in resumed.iter().chain(inputs) {
            // Keys can't be bound to a debug event, but a screen's button names its event
            // at run time: one that names a debug event does nothing.
            if input.source != DEBUG_SOURCE && self.is_debug(&input.name) {
                continue;
            }
            let input = Input {
                tick,
                ..input.clone()
            };
            let mut commands = Vec::new();
            if let Some(finish) = crate::travel::finish(&self.world, &input)? {
                commands.push(finish);
            }
            let talk = crate::talk::input(&self.world, &input)?;
            // A stale answer (a double click, or one queued behind a room change) stays
            // in the recording but is no event: scripts only ever see answers that took.
            let stale = input.name == galaxy_api::engine::TALK_CHOSEN && talk.is_empty();
            commands.extend(talk);
            if !stale {
                commands.push(Command::Record {
                    kind: input.name.clone(),
                    payload: input.payload.clone(),
                });
            }
            let batch = Batch {
                cause: Cause::Input {
                    source: input.source.clone(),
                    name: input.name.clone(),
                },
                commands,
            };
            let cause = self.world.causes.len();
            self.world
                .apply(&batch, &self.registry)
                .map_err(|e| format!("input `{}` on tick {tick}: {e}", input.name))?;
            if let Some(via) = &input.via
                && self.world.causes.len() > cause
            {
                self.vias.insert(cause as u64, via.clone());
            }
            if let Some(rec) = &mut self.recording {
                rec.events.push(RecEvent::Input(input));
            }
        }
        self.dispatch_events();
        crate::flight::flight(&mut self.world, &self.shapes);
        services::motion(&mut self.world);
        services::spin(&mut self.world);
        crate::places::advance_walks(&mut self.world);
        crate::collide::collide(&mut self.world, &self.shapes);
        services::ground(&mut self.world);
        if tick.is_multiple_of(CADENCE_TICKS) {
            self.run_cadence();
        }
        crate::life::update(&mut self.world, &self.registry)?;
        crate::flight::settle_holds(&mut self.world, &self.shapes);
        if self
            .world
            .entities
            .values()
            .any(|c| c.contains_key(galaxy_api::engine::LIFE))
        {
            services::ground(&mut self.world);
        }
        crate::travel::start(&mut self.world, before, &self.registry)?;
        crate::talk::interrupt(&mut self.world, &self.registry)?;
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
        let style_only = self.modules.get(module.id()).is_some_and(|old| {
            old.module.files.get("style.toml") != module.files.get("style.toml")
                && old
                    .module
                    .files
                    .0
                    .iter()
                    .filter(|(p, _)| p.as_str() != "style.toml")
                    .eq(module
                        .files
                        .0
                        .iter()
                        .filter(|(p, _)| p.as_str() != "style.toml"))
        });
        if style_only {
            return self.hot_load_style(module);
        }
        self.hot_load_world(module)
    }

    /// Style files are presentation assets: replacing one must not emit a sim event.
    pub(crate) fn hot_load_style(&mut self, module: Module) -> Result<(), String> {
        let id = module.id().to_owned();
        let loaded = self
            .modules
            .get_mut(&id)
            .ok_or("style module is not running")?;
        if !loaded
            .module
            .files
            .0
            .iter()
            .filter(|(p, _)| p.as_str() != "style.toml")
            .eq(module
                .files
                .0
                .iter()
                .filter(|(p, _)| p.as_str() != "style.toml"))
        {
            return Err("a presentation-only hot-load may change only style.toml".into());
        }
        loaded.module = module.clone();
        if let Some(rec) = &mut self.recording {
            rec.events.push(RecEvent::HotLoad {
                tick: self.world.tick,
                module: id,
                files: module.files,
                presentation_only: true,
            });
        }
        Ok(())
    }

    // Old recordings retain their original hot-load event semantics.
    pub(crate) fn hot_load_world(&mut self, module: Module) -> Result<(), String> {
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
        let modules = load_scripts(list, &registry, &Code::default())?;
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
                presentation_only: false,
            });
        }
        Ok(())
    }

    /// Checks one edit against the world as it is now and returns its command: full IDs in
    /// a loaded module's namespace (or the engine's), a declared component type, an entity
    /// that exists for everything but `put`, a component it has for `set`, and fields the
    /// type declares with values of their types. Each refusal names what was wrong.
    /// In a batch, what the `earlier` edits put counts as there.
    pub fn check_edit(&self, edit: &Edit, earlier: &[Edit]) -> Result<Command, String> {
        let put = |entity: &str, component: Option<&str>| {
            earlier.iter().any(|e| {
                e.entity == entity
                    && e.put.is_some()
                    && component.is_none_or(|c| e.component.as_deref() == Some(c))
            })
        };
        let has = |entity: &str, component: &str| {
            self.world.component(entity, component).is_some() || put(entity, Some(component))
        };
        let action = edit.action()?;
        let known = |id: &str, what: &str| -> Result<(), String> {
            let Some((ns, _)) = ids::split(id) else {
                return Err(format!(
                    "`{id}` is not a full {what} ID (<module>.<name>, such as example.door)"
                ));
            };
            if ns != ids::ENGINE && !self.modules.contains_key(ns) {
                return Err(format!(
                    "`{id}`: no loaded module `{ns}` (loaded: {})",
                    self.modules.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
            Ok(())
        };
        known(&edit.entity, "entity")?;
        if let Some(component) = &edit.component {
            known(component, "component type")?;
            self.registry.schema(component)?;
        }
        let exists = self.world.entities.contains_key(&edit.entity) || put(&edit.entity, None);
        if !exists && !matches!(action, Action::Put(..)) {
            return Err(format!(
                "there is no entity `{}` (put a component on it to make it)",
                edit.entity
            ));
        }
        match action {
            Action::Set(component, _) if !has(&edit.entity, component) => {
                return Err(format!(
                    "`{}` has no `{component}` to set (put adds a whole component)",
                    edit.entity
                ));
            }
            Action::Remove(Some(component)) if !has(&edit.entity, component) => {
                return Err(format!("`{}` has no `{component}` to remove", edit.entity));
            }
            Action::Place(place, spot) => {
                for id in std::iter::once(place).chain(spot) {
                    known(id, "entity")?;
                    if !self.world.entities.contains_key(id) && !put(id, None) {
                        return Err(format!("there is no entity `{id}` to move to"));
                    }
                }
            }
            _ => {}
        }
        crate::edit::refuse_kept(&edit.entity, &action, &self.world)?;
        let command = edit.command()?;
        self.registry.check(galaxy_api::Actor::Engine, &command)?;
        Ok(command)
    }

    /// Edits the world between two ticks, from outside its rules: `edits` apply in order
    /// as one batch with an `edit` cause from `source` (`scenario`, `inputs`, `live`),
    /// whole or not at all. The engine's own checks run as for any batch (places, talk and
    /// lives stay consistent), and the batch records an `engine.edited` fact, an event on
    /// the next tick, naming the entities and components edited, so scripts can react. The
    /// recording keeps the edit on this tick; a replay makes it again at the same point.
    pub fn edit(&mut self, source: &str, edits: &[Edit]) -> Result<(), String> {
        if edits.is_empty() {
            return Err("an edit names at least one change".into());
        }
        let tick = self.world.tick;
        let mut commands = Vec::new();
        let mut entities: Vec<&str> = Vec::new();
        let mut components: Vec<&str> = Vec::new();
        for (i, edit) in edits.iter().enumerate() {
            let command = self
                .check_edit(edit, &edits[..i])
                .map_err(|e| format!("edit {} (`{}`): {e}", i + 1, edit.text()))?;
            commands.push(command);
            for id in std::iter::once(edit.entity.as_str())
                .chain(edit.place.as_deref())
                .chain(edit.spot.as_deref())
            {
                if !entities.contains(&id) {
                    entities.push(id);
                }
            }
            if let Some(c) = edit.component.as_deref()
                && !components.contains(&c)
            {
                components.push(c);
            }
        }
        let Value::Object(payload) = json!({
            "source": source,
            "entities": entities,
            "components": components,
        }) else {
            unreachable!()
        };
        commands.push(Command::Record {
            kind: EDITED.to_owned(),
            payload,
        });
        let batch = Batch {
            cause: Cause::Edit {
                source: source.to_owned(),
            },
            commands,
        };
        self.world
            .apply(&batch, &self.registry)
            .map_err(|e| format!("the edit after tick {tick} was refused, nothing changed: {e}"))?;
        if let Some(rec) = &mut self.recording {
            rec.events.push(RecEvent::Edit {
                tick,
                source: source.to_owned(),
                edits: edits.to_vec(),
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

    /// Records engine widget state without changing the simulation.
    pub fn record_presentation(&mut self, state: serde_json::Value) {
        if let Some(rec) = &mut self.recording {
            if rec.events.iter().rev().find_map(|e| match e {
                RecEvent::Presentation { state, .. } => Some(state),
                _ => None,
            }) == Some(&state)
            {
                return;
            }
            let tick = self.world.tick;
            if let Some(RecEvent::Presentation {
                tick: t,
                state: old,
            }) = rec.events.last_mut()
                && *t == tick
            {
                *old = state;
                return;
            }
            rec.events.push(RecEvent::Presentation { tick, state });
        }
    }

    /// Records a UI sound cue without changing the simulation, stamped with this tick.
    pub fn record_ui_sound(&mut self, mut cue: galaxy_api::ui::UiCue) {
        cue.tick = self.world.tick;
        if let Some(rec) = &mut self.recording {
            rec.events.push(RecEvent::UiSound(cue));
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
