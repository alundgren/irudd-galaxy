//! The world store: entities, their components, the history of facts and the causes of
//! every change.
//!
//! Entities are IDs with a set of components; a component is a set of typed fields of a
//! component type some module (or the engine) declared. Every map is ordered by key, so
//! walking the world, serializing it and hashing it give the same result on every run and
//! every OS. A change goes through [`World::apply`]: a [`Batch`] of commands with one
//! cause, applied whole or not at all.

use std::collections::BTreeMap;

use galaxy_api::command::{Actor, Batch, Cause, Command, Fields};
use galaxy_api::rng::RngBank;
use galaxy_api::{Registry, WorldView};
use serde::{Deserialize, Serialize};

/// One component on one entity: its fields and what last changed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub fields: Fields,
    /// The cause that last changed it (an index into [`World::causes`]).
    pub cause: u64,
    /// The tick it last changed on.
    pub tick: u64,
}

/// A recorded fact. Facts are history, and each is an event on the tick after it was
/// recorded (an input's fact on its own tick).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub seq: u64,
    pub tick: u64,
    pub kind: String,
    pub cause: u64,
    pub payload: Fields,
}

/// A cause as stored, with the tick it happened on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CauseRecord {
    pub id: u64,
    pub tick: u64,
    pub cause: Cause,
}

/// The whole simulation state. Everything that decides what happens next is in here, so
/// saving it, or hashing it, captures the run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct World {
    pub seed: u64,
    /// The last tick that ran (0 before the first).
    pub tick: u64,
    pub entities: BTreeMap<String, BTreeMap<String, Component>>,
    pub facts: Vec<Fact>,
    pub causes: Vec<CauseRecord>,
    pub rng: RngBank,
    /// Facts from this sequence number on have not been handled as events yet.
    pub next_event: u64,
    /// The cause record of each engine service, made the first time it changes something.
    pub services: BTreeMap<String, u64>,
}

/// One change, to undo a batch that fails half way.
enum Undo {
    Component {
        entity: String,
        component: String,
        before: Option<Component>,
    },
    Fact,
}

impl World {
    pub fn new(seed: u64) -> World {
        World {
            seed,
            tick: 0,
            entities: BTreeMap::new(),
            facts: Vec::new(),
            causes: Vec::new(),
            rng: RngBank::new(seed),
            next_event: 0,
            services: BTreeMap::new(),
        }
    }

    /// Hex BLAKE3 of the whole state, serialized canonically (ordered maps, fields in
    /// declaration order, floats as their shortest round-trip decimal).
    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("the world serializes");
        blake3::hash(&bytes).to_hex().to_string()
    }

    fn add_cause(&mut self, cause: Cause) -> u64 {
        let id = self.causes.len() as u64;
        self.causes.push(CauseRecord {
            id,
            tick: self.tick,
            cause,
        });
        id
    }

    /// The cause record of an engine service, made on first use.
    pub fn service_cause(&mut self, service: &str) -> u64 {
        if let Some(id) = self.services.get(service) {
            return *id;
        }
        let id = self.add_cause(Cause::Engine {
            service: service.to_owned(),
        });
        self.services.insert(service.to_owned(), id);
        id
    }

    /// Records a fact directly, for the engine.
    pub fn record(&mut self, cause: u64, kind: &str, payload: Fields) {
        let seq = self.facts.len() as u64;
        self.facts.push(Fact {
            seq,
            tick: self.tick,
            kind: kind.to_owned(),
            cause,
            payload,
        });
    }

    /// Applies a batch whole, or changes nothing and says why. The commands are checked
    /// against the registry again (schemas and capabilities), then against the world: a
    /// patch needs the component, a new entity must be in the actor's namespace, and
    /// removing an entity needs the right to write each of its components. An empty batch
    /// changes nothing and records no cause.
    pub fn apply(&mut self, batch: &Batch, registry: &Registry) -> Result<(), String> {
        if batch.commands.is_empty() {
            return Ok(());
        }
        let actor = batch.cause.actor();
        let cause = self.causes.len() as u64;
        let mut undo = Vec::new();
        let result = batch
            .commands
            .iter()
            .enumerate()
            .try_for_each(|(i, command)| {
                self.apply_one(actor, registry, command, cause, &mut undo)
                    .map_err(|e| format!("command {} ({}): {e}", i + 1, command.op()))
            });
        let result = result.and_then(|()| {
            if matches!(batch.cause, Cause::Start { .. }) {
                Ok(())
            } else {
                crate::places::validate(self)
            }
        });
        if let Err(e) = result {
            self.undo(undo);
            return Err(e);
        }
        self.add_cause(batch.cause.clone());
        Ok(())
    }

    fn apply_one(
        &mut self,
        actor: Actor<'_>,
        registry: &Registry,
        command: &Command,
        cause: u64,
        undo: &mut Vec<Undo>,
    ) -> Result<(), String> {
        let command = registry.check(actor, command)?;
        let tick = self.tick;
        match command {
            command @ (Command::Move { .. }
            | Command::Walk { .. }
            | Command::AdvanceClock { .. }) => {
                for expanded in crate::places::expand(self, &command)? {
                    self.apply_one(actor, registry, &expanded, cause, undo)?;
                }
            }
            Command::Put {
                entity,
                component,
                fields,
            } => {
                if !self.entities.contains_key(&entity) && !registry.may_create(actor, &entity) {
                    return Err(format!(
                        "`{entity}` doesn't exist, and only its own module may create it"
                    ));
                }
                let before = self.set(
                    &entity,
                    &component,
                    Some(Component {
                        fields,
                        cause,
                        tick,
                    }),
                );
                undo.push(Undo::Component {
                    entity,
                    component,
                    before,
                });
            }
            Command::Patch {
                entity,
                component,
                fields,
            } => {
                let mut next = self
                    .component(&entity, &component)
                    .cloned()
                    .ok_or_else(|| format!("`{entity}` has no `{component}` to patch"))?;
                next.fields.extend(fields);
                next.cause = cause;
                next.tick = tick;
                let before = self.set(&entity, &component, Some(next));
                undo.push(Undo::Component {
                    entity,
                    component,
                    before,
                });
            }
            Command::Remove {
                entity,
                component: Some(component),
            } => {
                if self.component(&entity, &component).is_none() {
                    return Err(format!("`{entity}` has no `{component}` to remove"));
                }
                let before = self.set(&entity, &component, None);
                undo.push(Undo::Component {
                    entity,
                    component,
                    before,
                });
            }
            Command::Remove {
                entity,
                component: None,
            } => {
                let types: Vec<String> = self
                    .entities
                    .get(&entity)
                    .ok_or_else(|| format!("there is no entity `{entity}` to remove"))?
                    .keys()
                    .cloned()
                    .collect();
                if let Some(denied) = types.iter().find(|c| !registry.may_write(actor, c)) {
                    return Err(format!(
                        "removing `{entity}` would remove `{denied}`, which this module may \
                         not write"
                    ));
                }
                for component in types {
                    let before = self.set(&entity, &component, None);
                    undo.push(Undo::Component {
                        entity: entity.clone(),
                        component,
                        before,
                    });
                }
            }
            Command::Record { kind, payload } => {
                self.record(cause, &kind, payload);
                undo.push(Undo::Fact);
            }
        }
        Ok(())
    }

    /// Sets or removes a component and returns what was there. An entity with no
    /// components left is gone.
    fn set(
        &mut self,
        entity: &str,
        component: &str,
        value: Option<Component>,
    ) -> Option<Component> {
        match value {
            Some(value) => self
                .entities
                .entry(entity.to_owned())
                .or_default()
                .insert(component.to_owned(), value),
            None => {
                let components = self.entities.get_mut(entity)?;
                let before = components.remove(component);
                if components.is_empty() {
                    self.entities.remove(entity);
                }
                before
            }
        }
    }

    fn undo(&mut self, undo: Vec<Undo>) {
        for step in undo.into_iter().rev() {
            match step {
                Undo::Component {
                    entity,
                    component,
                    before,
                } => {
                    self.set(&entity, &component, before);
                }
                Undo::Fact => {
                    self.facts.pop();
                }
            }
        }
    }

    pub fn component(&self, entity: &str, component: &str) -> Option<&Component> {
        self.entities.get(entity)?.get(component)
    }

    /// Makes every stored component fit `registry`: each type must exist and each field
    /// must still be declared with a value of its type. Fields the registry added get their
    /// defaults, with `cause`. Used when modules change under a world (a hot-load, or
    /// loading a save made with other module versions). On failure nothing changes.
    pub fn conform(
        &mut self,
        registry: &Registry,
        cause: impl FnOnce(&mut World) -> u64,
    ) -> Result<(), String> {
        let mut additions = Vec::new();
        for (entity, components) in &self.entities {
            for (component, stored) in components {
                let schema = registry.schema(component).map_err(|_| {
                    format!(
                        "`{entity}` has a `{component}`, which no loaded module declares any more"
                    )
                })?;
                for (field, value) in &stored.fields {
                    let decl = schema.fields.get(field).ok_or_else(|| {
                        format!(
                            "`{entity}`'s `{component}` has a field `{field}` the module no \
                             longer declares"
                        )
                    })?;
                    if decl.ty.coerce(value).as_ref() != Ok(value) {
                        return Err(format!(
                            "`{entity}`'s `{component}.{field}` doesn't fit its new type `{}`",
                            decl.ty.name()
                        ));
                    }
                }
                for (field, decl) in &schema.fields {
                    if !stored.fields.contains_key(field) {
                        additions.push((
                            entity.clone(),
                            component.clone(),
                            field.clone(),
                            decl.default.clone(),
                        ));
                    }
                }
            }
        }
        if additions.is_empty() {
            return Ok(());
        }
        let cause = cause(self);
        let tick = self.tick;
        for (entity, component, field, value) in additions {
            let stored = self
                .entities
                .get_mut(&entity)
                .and_then(|c| c.get_mut(&component))
                .expect("found above");
            stored.fields.insert(field, value);
            stored.cause = cause;
            stored.tick = tick;
        }
        Ok(())
    }

    /// Records a cause and returns its ID, for changes the engine makes directly.
    pub fn cause(&mut self, cause: Cause) -> u64 {
        self.add_cause(cause)
    }
}

impl WorldView for World {
    fn tick(&self) -> u64 {
        self.tick
    }

    fn get(&self, entity: &str, component: &str) -> Option<&Fields> {
        self.component(entity, component).map(|c| &c.fields)
    }

    fn exists(&self, entity: &str) -> bool {
        self.entities.contains_key(entity)
    }

    fn with_component(&self, component: &str) -> Vec<&str> {
        self.entities
            .iter()
            .filter(|(_, c)| c.contains_key(component))
            .map(|(e, _)| e.as_str())
            .collect()
    }
}
