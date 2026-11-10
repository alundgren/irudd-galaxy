//! The explore service: every thing with an `engine.explorer` marks the grid cells within
//! its `radius` into its `engine.explored` memory, once positions are final for the tick.
//!
//! The memory is world state (canonical text, [`galaxy_api::explored`]), so it is hashed,
//! saved, replayed and edited like anything else. Decoding and re-encoding it every tick
//! would cost more as the memory grows, so the session keeps a decoded copy per explorer
//! ([`Explored`]) with the text it came from. The copy is used only while the text in the
//! world is still exactly that text; after a load, an edit, a script write or a hot-load
//! the text differs and the copy is decoded again. The component is written only on ticks
//! that uncover a new cell.
//!
//! A memory belongs to one coordinate space: `place` names it (empty is the open plane).
//! The service makes the memory in the place the explorer is in, and marks it only while
//! the explorer is in that place, so cells from one place never show in another. A memory
//! with no cells yet (new, or cleared by an edit) moves to wherever its explorer is.

use std::collections::BTreeMap;

use galaxy_api::Batch;
use galaxy_api::command::Command;
use galaxy_api::engine::{EXPLORED, EXPLORER, POSITION};
use galaxy_api::explored::{self, DEFAULT_CELL, Memory};
use serde_json::Value;

use crate::world::{Component, World};

fn number(fields: &serde_json::Map<String, Value>, name: &str, default: f64) -> f64 {
    fields.get(name).and_then(Value::as_f64).unwrap_or(default)
}

/// Decoded memories kept beside the world, by entity: the text each was decoded from
/// (or last written as) and its cells.
#[derive(Clone, Debug, Default)]
pub struct Explored {
    memories: BTreeMap<String, (String, Memory)>,
}

/// The place a memory is of: empty is the open plane.
pub fn memory_place(fields: &serde_json::Map<String, Value>) -> &str {
    fields.get("place").and_then(Value::as_str).unwrap_or("")
}

/// Marks what each explorer can see this tick, in the place it is in.
pub fn explore(world: &mut World, cache: &mut Explored) {
    let explorers: Vec<(String, f64, f64, f64, String)> = world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let radius = number(&c.get(EXPLORER)?.fields, "radius", 0.0);
            let p = &c.get(POSITION)?.fields;
            let place = crate::places::place_of(world, id);
            Some((
                id.clone(),
                radius,
                number(p, "x", 0.0),
                number(p, "y", 0.0),
                place.to_owned(),
            ))
        })
        .collect();
    cache
        .memories
        .retain(|id, _| explorers.iter().any(|e| e.0 == *id));
    for (id, radius, x, y, place) in explorers {
        let stored = world.component(&id, EXPLORED);
        let (text, cell) = match stored {
            Some(c) => (
                c.fields.get("cells").and_then(Value::as_str).unwrap_or(""),
                number(&c.fields, "cell", DEFAULT_CELL),
            ),
            None => ("", DEFAULT_CELL),
        };
        // A memory with cells stays in its own place; an empty one follows the explorer.
        let moves = stored.is_some_and(|c| memory_place(&c.fields) != place);
        if moves && !text.is_empty() {
            continue;
        }
        // Validation keeps bad numbers out of the world; this is a last guard.
        if explored::check_cell(cell).is_err() || explored::check_radius(radius, cell).is_err() {
            continue;
        }
        let fresh = match cache.memories.get(&id) {
            Some((kept, _)) => kept != text,
            None => true,
        };
        if fresh {
            let Ok(memory) = Memory::parse(text) else {
                continue;
            };
            cache.memories.insert(id.clone(), (text.to_owned(), memory));
        }
        let (kept, memory) = cache.memories.get_mut(&id).expect("kept above");
        let had = stored.is_some();
        if !explored::mark(memory, x, y, radius, cell) && had && !moves {
            continue;
        }
        *kept = memory.text();
        let cause = world.service_cause("explore");
        let tick = world.tick;
        let mut fields = serde_json::Map::new();
        fields.insert("cell".into(), Value::from(cell));
        fields.insert("cells".into(), Value::from(kept.clone()));
        let place = if place.is_empty() {
            Value::Null
        } else {
            Value::from(place)
        };
        fields.insert("place".into(), place.clone());
        let components = world.entities.get_mut(&id).expect("an explorer");
        match components.get_mut(EXPLORED) {
            Some(c) => {
                c.fields.insert("cells".into(), Value::from(kept.clone()));
                c.fields.insert("place".into(), place);
                c.cause = cause;
                c.tick = tick;
            }
            None => {
                components.insert(
                    EXPLORED.to_owned(),
                    Component {
                        fields,
                        cause,
                        tick,
                    },
                );
            }
        }
    }
}

/// Checks one entity's explorer and memory: numbers in range and canonical cells text.
fn check(world: &World, id: &str) -> Result<(), String> {
    let memory = world.component(id, EXPLORED);
    let cell = memory.map_or(DEFAULT_CELL, |c| number(&c.fields, "cell", DEFAULT_CELL));
    if let Some(m) = memory {
        explored::check_cell(cell).map_err(|e| format!("`{id}`'s {EXPLORED}: {e}"))?;
        check_place(world, id, &m.fields)?;
        let text = m.fields.get("cells").and_then(Value::as_str).unwrap_or("");
        Memory::parse(text).map_err(|e| format!("`{id}`'s {EXPLORED}: {e}"))?;
    }
    if let Some(e) = world.component(id, EXPLORER) {
        let radius = number(&e.fields, "radius", 0.0);
        explored::check_radius(radius, cell).map_err(|e| format!("`{id}`'s {EXPLORER}: {e}"))?;
    }
    Ok(())
}

/// Checks that a memory's place is an `engine.place`.
fn check_place(
    world: &World,
    id: &str,
    fields: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let place = memory_place(fields);
    if !place.is_empty() && world.component(place, galaxy_api::engine::PLACE).is_none() {
        return Err(format!(
            "`{id}`'s {EXPLORED}: place `{place}` is not an engine.place"
        ));
    }
    Ok(())
}

/// Checks every explorer and memory in the world (start states, a resumed save).
pub fn validate(world: &World) -> Result<(), String> {
    for (id, c) in &world.entities {
        if c.contains_key(EXPLORER) || c.contains_key(EXPLORED) {
            check(world, id)?;
        }
    }
    Ok(())
}

/// Checks the explorers and memories a batch wrote, so a long memory is read again only
/// when something changed it.
pub fn validate_batch(world: &World, batch: &Batch) -> Result<(), String> {
    for command in &batch.commands {
        if let Command::Put {
            entity, component, ..
        }
        | Command::Patch {
            entity, component, ..
        } = command
            && (component == EXPLORER || component == EXPLORED)
        {
            check(world, entity)?;
        }
        // A place a memory names can't be removed from under it.
        if let Command::Remove { component, .. } = command
            && component
                .as_deref()
                .is_none_or(|c| c == galaxy_api::engine::PLACE)
        {
            for (id, c) in &world.entities {
                if let Some(m) = c.get(EXPLORED) {
                    check_place(world, id, &m.fields)?;
                }
            }
        }
    }
    Ok(())
}
