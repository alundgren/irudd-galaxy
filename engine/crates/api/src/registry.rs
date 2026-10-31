//! What the loaded modules declare, put together: component schemas, event kinds and
//! capabilities. Commands are checked against it before they touch the world.
//!
//! Capabilities are granted per component type:
//! - a module writes (puts, patches, removes) its own component types;
//! - it writes another module's or the engine's types only if its manifest lists them in
//!   `writes`;
//! - it creates entities only in its own namespace, but may put its components on any
//!   entity that exists;
//! - it records only its own event kinds.
//!
//! The engine (inputs, hot-loads, its services) may do anything. Reading is free.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_module::ids;
use galaxy_module::manifest::{FieldDecl, FieldType};
use galaxy_module::module::Module;
use serde_json::Value;

use crate::command::{Actor, Command, Fields};
use crate::engine;

/// A component type: who declared it and its fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    /// The module that declared it, or `engine`.
    pub owner: String,
    pub fields: BTreeMap<String, FieldDecl>,
}

/// Schemas, event kinds and capabilities of a set of modules plus the engine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    pub components: BTreeMap<String, Schema>,
    /// Every event kind, by full ID.
    pub events: BTreeSet<String>,
    /// Each module's `writes`, by module.
    pub writes: BTreeMap<String, BTreeSet<String>>,
    /// Loaded module IDs and versions.
    pub modules: BTreeMap<String, String>,
}

impl Registry {
    /// The registry for `modules` (sorted by ID by the caller or not; order doesn't
    /// matter). Fails if a module writes a component type nobody declares.
    pub fn new(modules: &[&Module]) -> Result<Registry, String> {
        let mut reg = Registry::default();
        for (name, fields) in engine::components() {
            reg.components.insert(
                format!("{}.{name}", ids::ENGINE),
                Schema {
                    owner: ids::ENGINE.into(),
                    fields,
                },
            );
        }
        reg.events.extend(
            engine::EVENTS
                .iter()
                .map(|e| format!("{}.{e}", ids::ENGINE)),
        );
        for module in modules {
            let id = module.id().to_owned();
            if reg
                .modules
                .insert(id.clone(), module.manifest.version.to_string())
                .is_some()
            {
                return Err(format!("module `{id}` is loaded twice"));
            }
            for (name, decl) in &module.manifest.components {
                reg.components.insert(
                    module.full(name),
                    Schema {
                        owner: id.clone(),
                        fields: decl.fields.clone(),
                    },
                );
            }
            reg.events
                .extend(module.manifest.events.iter().map(|e| module.full(e)));
            reg.writes
                .insert(id, module.manifest.writes.iter().cloned().collect());
        }
        for (module, writes) in &reg.writes {
            for component in writes {
                if !reg.components.contains_key(component) {
                    return Err(format!(
                        "module `{module}` writes `{component}`, but no loaded module declares \
                         that component type"
                    ));
                }
            }
        }
        Ok(reg)
    }

    pub fn schema(&self, component: &str) -> Result<&Schema, String> {
        self.components
            .get(component)
            .ok_or_else(|| format!("there is no component type `{component}`"))
    }

    /// Whether `actor` may put, patch or remove `component`.
    pub fn may_write(&self, actor: Actor<'_>, component: &str) -> bool {
        match actor {
            Actor::Engine => true,
            Actor::Module(module) => {
                self.components
                    .get(component)
                    .is_some_and(|s| s.owner == module)
                    || self
                        .writes
                        .get(module)
                        .is_some_and(|w| w.contains(component))
            }
        }
    }

    /// Checks a command's shape and the actor's capabilities, and returns it normalised:
    /// a `put` has every field (defaults filled in), numbers are coerced to the field
    /// types. Whether the entity and component exist is the world's to check.
    pub fn check(&self, actor: Actor<'_>, command: &Command) -> Result<Command, String> {
        match command {
            Command::Put {
                entity,
                component,
                fields,
            } => {
                self.check_write(actor, entity, component)?;
                Ok(Command::Put {
                    entity: entity.clone(),
                    component: component.clone(),
                    fields: self.fill(component, fields)?,
                })
            }
            Command::Patch {
                entity,
                component,
                fields,
            } => {
                self.check_write(actor, entity, component)?;
                Ok(Command::Patch {
                    entity: entity.clone(),
                    component: component.clone(),
                    fields: self.coerce(component, fields)?,
                })
            }
            Command::Remove { entity, component } => {
                if !ids::is_full_id(entity) {
                    return Err(format!("`{entity}` is not an entity ID"));
                }
                if let Some(component) = component {
                    self.check_write(actor, entity, component)?;
                }
                Ok(command.clone())
            }
            Command::Record { kind, payload } => {
                if !self.events.contains(kind) {
                    return Err(format!(
                        "there is no event kind `{kind}`: a module lists its event kinds in \
                         its manifest's `events`"
                    ));
                }
                if let Actor::Module(module) = actor
                    && ids::split(kind).map(|(ns, _)| ns) != Some(module)
                {
                    return Err(format!(
                        "module `{module}` may only record its own event kinds, not `{kind}`"
                    ));
                }
                check_payload(payload, 0)?;
                Ok(command.clone())
            }
        }
    }

    fn check_write(&self, actor: Actor<'_>, entity: &str, component: &str) -> Result<(), String> {
        if !ids::is_full_id(entity) {
            return Err(format!(
                "`{entity}` is not an entity ID (module.name, such as example.door)"
            ));
        }
        self.schema(component)?;
        if !self.may_write(actor, component) {
            let Actor::Module(module) = actor else {
                unreachable!("the engine may write anything")
            };
            return Err(format!(
                "module `{module}` may not write `{component}`: list it in the manifest's \
                 `writes` to be granted it"
            ));
        }
        Ok(())
    }

    /// Whether `actor` may create the entity `entity` (its own namespace only).
    pub fn may_create(&self, actor: Actor<'_>, entity: &str) -> bool {
        match actor {
            Actor::Engine => true,
            Actor::Module(module) => ids::split(entity).is_some_and(|(ns, _)| ns == module),
        }
    }

    /// Every field of `component`: the given ones coerced, the rest defaults.
    pub fn fill(&self, component: &str, fields: &Fields) -> Result<Fields, String> {
        let schema = self.schema(component)?;
        let mut out = self.coerce(component, fields)?;
        for (name, decl) in &schema.fields {
            out.entry(name.clone())
                .or_insert_with(|| decl.default.clone());
        }
        Ok(out)
    }

    /// The given fields coerced to their types; unknown fields are an error.
    pub fn coerce(&self, component: &str, fields: &Fields) -> Result<Fields, String> {
        let schema = self.schema(component)?;
        let mut out = Fields::new();
        for (name, value) in fields {
            let decl = schema.fields.get(name).ok_or_else(|| {
                let known: Vec<&str> = schema.fields.keys().map(String::as_str).collect();
                format!(
                    "`{component}` has no field `{name}` (its fields: {})",
                    known.join(", ")
                )
            })?;
            let value = decl
                .ty
                .coerce(value)
                .map_err(|e| format!("`{component}.{name}`: {e}"))?;
            out.insert(name.clone(), value);
        }
        Ok(out)
    }

    /// The field type of `component.field`.
    pub fn field_type(&self, component: &str, field: &str) -> Option<FieldType> {
        self.components
            .get(component)?
            .fields
            .get(field)
            .map(|f| f.ty)
    }
}

/// Deepest nesting a fact payload may have.
const MAX_DEPTH: usize = 8;

/// A payload holds text, booleans, finite numbers, lists and tables, nested at most
/// [`MAX_DEPTH`] deep.
fn check_payload(fields: &Fields, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("a payload nests at most {MAX_DEPTH} tables deep"));
    }
    for value in fields.values() {
        check_value(value, depth)?;
    }
    Ok(())
}

fn check_value(value: &Value, depth: usize) -> Result<(), String> {
    match value {
        Value::Number(n) if n.as_f64().is_some_and(f64::is_finite) => Ok(()),
        Value::Number(_) => Err("payload numbers must be finite".into()),
        Value::Object(map) => check_payload(map, depth + 1),
        Value::Array(items) => items.iter().try_for_each(|v| check_value(v, depth + 1)),
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_module::module::ModuleFiles;
    use serde_json::json;
    use std::path::Path;

    fn module(id: &str, extra: &str) -> Module {
        let mut files = ModuleFiles::default();
        files.0.insert(
            "manifest.toml".into(),
            format!("id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n{extra}"),
        );
        Module::from_files(Path::new(id), files).unwrap()
    }

    fn fields(v: Value) -> Fields {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn grants_writes_per_component_type() {
        let a = module(
            "aa",
            "events = [\"rang\"]\nwrites = [\"engine.position\"]\n[components.bell]\nrings = \"int\"\n",
        );
        let b = module(
            "bb",
            "[components.knob]\nturns = { type = \"number\", default = 1 }\n",
        );
        let reg = Registry::new(&[&a, &b]).unwrap();
        let aa = Actor::Module("aa");
        let put = |entity: &str, component: &str, f: Value| Command::Put {
            entity: entity.into(),
            component: component.into(),
            fields: fields(f),
        };

        let filled = reg
            .check(aa, &put("aa.x", "aa.bell", json!({ "rings": 2.0 })))
            .unwrap();
        assert_eq!(filled, put("aa.x", "aa.bell", json!({ "rings": 2 })));
        assert!(
            reg.check(aa, &put("bb.y", "engine.position", json!({})))
                .is_ok()
        );
        let denied = reg
            .check(aa, &put("aa.x", "bb.knob", json!({})))
            .unwrap_err();
        assert!(denied.contains("writes"), "{denied}");
        assert!(
            reg.check(Actor::Engine, &put("aa.x", "bb.knob", json!({})))
                .is_ok()
        );
        assert!(
            reg.check(aa, &put("aa.x", "aa.bell", json!({ "rings": 1.5 })))
                .is_err()
        );
        assert!(
            reg.check(aa, &put("aa.x", "aa.bell", json!({ "nope": 1 })))
                .is_err()
        );
        assert!(reg.check(aa, &put("aa.x", "aa.none", json!({}))).is_err());
        assert!(reg.check(aa, &put("x", "aa.bell", json!({}))).is_err());

        let record = |kind: &str| Command::Record {
            kind: kind.into(),
            payload: fields(json!({ "n": 1, "deep": { "list": [1, "two"] } })),
        };
        assert!(reg.check(aa, &record("aa.rang")).is_ok());
        assert!(reg.check(aa, &record("aa.other")).is_err());
        assert!(reg.check(Actor::Module("bb"), &record("aa.rang")).is_err());
        assert!(reg.check(Actor::Engine, &record("aa.rang")).is_ok());

        assert!(reg.may_create(aa, "aa.new"));
        assert!(!reg.may_create(aa, "bb.new"));

        let bad = module("cc", "writes = [\"zz.none\"]\n");
        assert!(Registry::new(&[&bad]).is_err());
    }
}
