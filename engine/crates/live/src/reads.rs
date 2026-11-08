//! Reads of a world: what `galaxy-gm live state`, `history` and `watch` print, and what
//! `galaxy sim run --state` prints. Every read takes the world by shared reference, so
//! reading can't change it or its hash.

use std::collections::BTreeMap;

use galaxy_api::command::Cause;
use galaxy_api::engine::{MODEL, MOTION, POSITION};
use galaxy_sim::session::LogLine;
use galaxy_sim::{Session, World};
use serde_json::{Map, Value, json};

/// What a state read prints: a line an entity, or fields.
#[derive(Clone, Debug, Default)]
pub struct StateQuery {
    /// A full entity ID, or a bare name one module has (`walker` for `stroll.walker`).
    pub entity: Option<String>,
    /// A full component name, or a bare one.
    pub component: Option<String>,
    /// Every field, the facts and causes.
    pub all: bool,
}

/// `want` among `names`: itself, or the one name ending in `.want`. The error says what
/// there is, or that the name is ambiguous.
pub fn pick(want: &str, names: &[&String], what: &str) -> Result<String, String> {
    if names.iter().any(|n| *n == want) {
        return Ok(want.to_owned());
    }
    let suffix = format!(".{want}");
    let found: Vec<&&String> = names.iter().filter(|n| n.ends_with(&suffix)).collect();
    match found.as_slice() {
        [one] => Ok((**one).clone()),
        [] => Err(format!(
            "no {what} `{want}` in the world; there are {}",
            names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        many => Err(format!(
            "`{want}` could be {}; write the full ID",
            many.iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(" or ")
        )),
    }
}

/// The entity `want` names.
pub fn entity(world: &World, want: &str) -> Result<String, String> {
    pick(want, &world.entities.keys().collect::<Vec<_>>(), "entity")
}

/// The component `want` names, among those some entity has.
pub fn component(world: &World, want: &str) -> Result<String, String> {
    let mut names: Vec<&String> = world.entities.values().flat_map(|cs| cs.keys()).collect();
    names.sort();
    names.dedup();
    pick(want, &names, "component")
}

/// The world: a line an entity (where it is, its speed, its model and its components) and
/// the facts counted by kind. With an entity or component, those entities' and
/// components' fields, without the facts; with `all`, every field, the facts and causes.
/// An error is a name that isn't in the world.
pub fn state(world: &World, query: &StateQuery) -> Result<Value, String> {
    let entity = query
        .entity
        .as_deref()
        .map(|e| entity(world, e))
        .transpose()?;
    let component = query
        .component
        .as_deref()
        .map(|c| component(world, c))
        .transpose()?;
    let entities: Map<String, Value> = world
        .entities
        .iter()
        .filter(|(id, _)| entity.as_ref().is_none_or(|e| e == *id))
        .filter_map(|(id, components)| {
            let components: Map<String, Value> = components
                .iter()
                .filter(|(name, _)| component.as_ref().is_none_or(|c| c == *name))
                .map(|(name, c)| (name.clone(), Value::Object(c.fields.clone())))
                .collect();
            (!components.is_empty()).then(|| (id.clone(), Value::Object(components)))
        })
        .collect();
    if entity.is_some() || component.is_some() {
        return Ok(json!({
            "entities": entities,
            "facts": format!(
                "{} fact(s) left out with --entity/--component; without them the facts are counted by kind",
                world.facts.len()
            ),
        }));
    }
    if query.all {
        return Ok(json!({
            "entities": entities,
            "facts": world.facts,
            "causes": world.causes,
        }));
    }
    let summary: Map<String, Value> = world
        .entities
        .keys()
        .map(|id| (id.clone(), Value::String(line(world, id))))
        .collect();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &world.facts {
        *kinds.entry(f.kind.as_str()).or_default() += 1;
    }
    Ok(json!({
        "entities": summary,
        "facts": { "count": world.facts.len(), "kinds": kinds },
        "causes": world.causes.len(),
        "compact": "a line an entity: where it is, its speed, its model and its components. \
                    --entity <id> for one entity's fields, --component <name> for one \
                    component on every entity, --all for the whole world (every field, the \
                    facts and causes)",
    }))
}

/// One entity in a line: where it is and how fast it goes, the model it wears, and its
/// components by name.
fn line(world: &World, id: &str) -> String {
    let components = &world.entities[id];
    let number = |c: Option<&galaxy_sim::world::Component>, k: &str| {
        c.and_then(|c| c.fields.get(k)).and_then(Value::as_f64)
    };
    let mut parts = Vec::new();
    let at = components.get(POSITION);
    if let (Some(x), Some(y)) = (number(at, "x"), number(at, "y")) {
        parts.push(format!(
            "at ({x:.2}, {y:.2}) angle {:.2}",
            number(at, "angle").unwrap_or(0.0)
        ));
    }
    let moving = components.get(MOTION);
    if let (Some(vx), Some(vy)) = (number(moving, "vx"), number(moving, "vy")) {
        parts.push(format!("speed {:.2}", (vx * vx + vy * vy).sqrt()));
    }
    if let Some(m) = components.get(MODEL) {
        let text = |k: &str| m.fields.get(k).and_then(Value::as_str).unwrap_or("");
        let mut worn = format!("wears {}", text("model"));
        if let Some(s) = number(Some(m), "scale").filter(|s| *s != 1.0) {
            worn.push_str(&format!(" at scale {s}"));
        }
        if !text("look").is_empty() {
            worn.push_str(&format!(" with look {}", text("look")));
        }
        parts.push(worn);
    }
    let names: Vec<&str> = components.keys().map(String::as_str).collect();
    if parts.is_empty() {
        names.join(", ")
    } else {
        format!("{}; {}", parts.join(", "), names.join(", "))
    }
}

/// What history shows.
#[derive(Clone, Debug, Default)]
pub struct HistoryQuery {
    /// The most recent this many matching facts; 20 when not given.
    pub last: Option<usize>,
    /// Only facts from this tick on.
    pub since: Option<u64>,
    /// Only facts whose kind contains this text.
    pub kind: Option<String>,
    /// Only the facts inputs made (keys, clicks, bot steps).
    pub inputs: bool,
}

/// Facts the query shows when it names no count.
pub const HISTORY_LAST: usize = 20;

/// Who made a fact, in a few words: `input from keyboard`, `script stroll
/// scripts/stroll.luau on stroll.turn`, `engine flight`, `start of stroll`, `hot-load of
/// stroll 0.1.0`.
pub fn cause_text(cause: &Cause) -> String {
    match cause {
        Cause::Start { module } => format!("start of {module}"),
        Cause::Input { source, .. } => format!("input from {source}"),
        Cause::Script {
            module,
            script,
            handler,
            ..
        } => format!("script {module} {script} on {handler}"),
        Cause::Engine { service } => format!("engine {service}"),
        Cause::HotLoad {
            module, version, ..
        } => format!("hot-load of {module} {version}"),
        Cause::Edit { source } => format!("edit from {source}"),
    }
}

/// Recent facts, oldest first, each with who made it (and for an input that did not come
/// from the window itself, how it came: `via` live), and what scripts printed since the
/// first one shown.
pub fn history(session: &Session, query: &HistoryQuery) -> Value {
    let (world, log) = (&session.world, &session.log);
    let cause_of = |id: u64| world.causes.get(id as usize).map(|c| &c.cause);
    let matching: Vec<&galaxy_sim::world::Fact> = world
        .facts
        .iter()
        .filter(|f| query.since.is_none_or(|t| f.tick >= t))
        .filter(|f| query.kind.as_deref().is_none_or(|k| f.kind.contains(k)))
        .filter(|f| !query.inputs || matches!(cause_of(f.cause), Some(Cause::Input { .. })))
        .collect();
    let last = query.last.unwrap_or(HISTORY_LAST);
    let shown = &matching[matching.len().saturating_sub(last)..];
    let from = shown
        .first()
        .map_or(query.since.unwrap_or(world.tick), |f| f.tick);
    let facts: Vec<Value> = shown
        .iter()
        .map(|f| {
            let via = session.via(f.cause);
            let by = cause_of(f.cause).map(|c| match via {
                Some(via) => format!("{} via {via}", cause_text(c)),
                None => cause_text(c),
            });
            let mut fact = json!({
                "seq": f.seq,
                "tick": f.tick,
                "kind": f.kind,
                "by": by,
                "payload": f.payload,
            });
            if let Some(via) = via {
                fact["via"] = json!(via);
            }
            fact
        })
        .collect();
    let printed: Vec<&LogLine> = log.iter().filter(|l| l.tick >= from).collect();
    let printed = &printed[printed.len().saturating_sub(last)..];
    json!({
        "tick": world.tick,
        "matching": matching.len(),
        "shown": facts.len(),
        "facts": facts,
        "log": printed,
    })
}

/// What one watch samples: an entity's component, or one field of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Watched {
    pub entity: String,
    pub component: String,
    pub field: Option<String>,
}

impl Watched {
    /// Resolves bare names against the world. The field must be one the component has.
    pub fn resolve(
        world: &World,
        entity: &str,
        component: &str,
        field: Option<&str>,
    ) -> Result<Watched, String> {
        let entity = self::entity(world, entity)?;
        let have: Vec<&String> = world.entities[&entity].keys().collect();
        let component = pick(component, &have, &format!("component on {entity}"))?;
        if let Some(f) = field {
            let fields = &world.entities[&entity][&component].fields;
            if !fields.contains_key(f) {
                return Err(format!(
                    "{component} on {entity} has no field `{f}`; it has {}",
                    fields.keys().cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        Ok(Watched {
            entity,
            component,
            field: field.map(str::to_owned),
        })
    }

    /// The value now: the field, or the component's fields; null once it is gone.
    pub fn sample(&self, world: &World) -> Value {
        let Some(c) = world.component(&self.entity, &self.component) else {
            return Value::Null;
        };
        match &self.field {
            Some(f) => c.fields.get(f).cloned().unwrap_or(Value::Null),
            None => Value::Object(c.fields.clone()),
        }
    }
}

/// A watch's report from its samples, `[tick, value]` in order.
pub fn watch_report(watched: &Watched, samples: &[(u64, Value)]) -> Value {
    let changes = samples.windows(2).filter(|w| w[0].1 != w[1].1).count();
    let numbers: Vec<f64> = samples.iter().filter_map(|(_, v)| v.as_f64()).collect();
    let range = (numbers.len() == samples.len() && !numbers.is_empty()).then(|| {
        let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
        let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        json!({ "min": min, "max": max })
    });
    json!({
        "entity": watched.entity,
        "component": watched.component,
        "field": watched.field,
        "from_tick": samples.first().map(|s| s.0),
        "to_tick": samples.last().map(|s| s.0),
        "changes": changes,
        "range": range,
        "first": samples.first().map(|s| s.1.clone()),
        "last": samples.last().map(|s| s.1.clone()),
        "samples": samples.iter().map(|(t, v)| json!({ "tick": t, "value": v })).collect::<Vec<_>>(),
    })
}
