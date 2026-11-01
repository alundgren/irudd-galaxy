//! The one command set every change to the world goes through, and the cause each change
//! carries.
//!
//! The primitive commands are: `put` a component on an entity (creating the entity if it is
//! new), `patch` some fields of a component, `remove` a component or a whole entity, and
//! `record` a fact. A recorded fact goes into the world's history and is also an event:
//! handlers for its kind run on the next tick. Commands always travel in a [`Batch`] with
//! one [`Cause`]; the batch applies whole or not at all, and everything it changes
//! remembers that cause. Place moves, walks and clock advances are high-level commands
//! expanded into checked primitive writes by the world.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Field values by name.
pub type Fields = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// Sets a whole component: fields left out get their defaults.
    Put {
        entity: String,
        component: String,
        fields: Fields,
    },
    /// Changes the given fields of an existing component.
    Patch {
        entity: String,
        component: String,
        fields: Fields,
    },
    /// Removes one component, or the whole entity when `component` is `None`.
    Remove {
        entity: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        component: Option<String>,
    },
    /// Records a fact of a declared kind; it is an event on the next tick.
    Record { kind: String, payload: Fields },
    /// Moves a located entity immediately; an omitted spot uses the place's arrival.
    Move {
        entity: String,
        place: String,
        spot: Option<String>,
        state: Option<String>,
    },
    /// Starts a straight walk to a spot in the entity's current place.
    Walk {
        entity: String,
        spot: String,
        speed: f64,
        state: Option<String>,
    },
    /// Advances one explicitly named world clock, in module-defined units.
    AdvanceClock { entity: String, amount: i64 },
}

impl Command {
    pub fn op(&self) -> &'static str {
        match self {
            Command::Put { .. } => "put",
            Command::Patch { .. } => "patch",
            Command::Remove { .. } => "remove",
            Command::Record { .. } => "record",
            Command::Move { .. } => "move",
            Command::Walk { .. } => "walk",
            Command::AdvanceClock { .. } => "advance_clock",
        }
    }
}

/// Why a batch of commands happened.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// A module's start state, when a new world begins.
    Start { module: String },
    /// A recorded input (a key, a click, a scenario step): it records the input's event.
    Input { source: String, name: String },
    /// A script handler: `handler` is the event kind it ran for, or `cadence`; `fact` is
    /// the fact that was the event.
    Script {
        module: String,
        script: String,
        handler: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fact: Option<u64>,
    },
    /// An engine service (motion, ground, the script host reporting a failure).
    Engine { service: String },
    /// A module hot-loaded into the running world: its version and the hash of its files.
    HotLoad {
        module: String,
        version: String,
        files: String,
    },
}

/// Who acts for a cause, for capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor<'a> {
    Engine,
    Module(&'a str),
}

impl Cause {
    pub fn actor(&self) -> Actor<'_> {
        match self {
            Cause::Start { module } | Cause::Script { module, .. } => Actor::Module(module),
            Cause::Input { .. } | Cause::Engine { .. } | Cause::HotLoad { .. } => Actor::Engine,
        }
    }
}

/// Commands with their one cause.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub cause: Cause,
    pub commands: Vec<Command>,
}

impl Batch {
    pub fn new(cause: Cause) -> Batch {
        Batch {
            cause,
            commands: Vec::new(),
        }
    }
}
