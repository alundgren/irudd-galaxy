//! A module's `manifest.toml`.
//!
//! ```toml
//! id = "example"
//! version = "0.1.0"
//! min_engine_version = "0.2.0"
//!
//! scripts = ["scripts/main.luau"]   # entry scripts, run on events and the cadence
//! events = ["opened", "pressed"]    # event kinds this module records or takes as input
//! writes = ["engine.position"]      # other modules' component types its scripts may write
//!
//! [components.door]                 # a component type: example.door
//! open = "bool"                     # a field and its type
//! pushes = { type = "int", default = 0 }
//! ```
//!
//! Field types are `number`, `int`, `bool`, `text` and `id` (an entity ID, or none).
//!
//! The minimum is read on its own first ([`read_minimum`]), so a manifest written for a
//! newer engine, with fields this one doesn't know, still reaches the version gate and is
//! refused for the right reason. Only then is the whole file read strictly ([`load`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::failure::Failure;
use crate::ids;
use crate::version::Version;

/// The manifest's file name in a module folder.
pub const FILE: &str = "manifest.toml";

/// A module's manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub dir: PathBuf,
    pub id: String,
    pub version: Version,
    pub min_engine_version: Version,
    /// Entry scripts, as paths inside the module (`scripts/main.luau`).
    pub scripts: Vec<String>,
    /// Event kinds, as local names (`opened` is `<id>.opened`).
    pub events: Vec<String>,
    /// Component types of other modules (or the engine) its scripts may write, as full IDs.
    pub writes: Vec<String>,
    /// Component types it declares, by local name.
    pub components: BTreeMap<String, ComponentDecl>,
}

/// A component type a module declares: its fields, by name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComponentDecl {
    pub fields: BTreeMap<String, FieldDecl>,
}

/// One field of a component type.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub ty: FieldType,
    /// The value a new component gets when `put` leaves the field out.
    pub default: Value,
}

/// The type of a component field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldType {
    /// A number (64-bit float).
    Number,
    /// A whole number (64-bit).
    Int,
    Bool,
    Text,
    /// An entity ID, or none.
    Id,
}

impl FieldType {
    pub const ALL: [FieldType; 5] = [
        FieldType::Number,
        FieldType::Int,
        FieldType::Bool,
        FieldType::Text,
        FieldType::Id,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FieldType::Number => "number",
            FieldType::Int => "int",
            FieldType::Bool => "bool",
            FieldType::Text => "text",
            FieldType::Id => "id",
        }
    }

    pub fn parse(name: &str) -> Option<FieldType> {
        FieldType::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The value a field of this type has when nothing set it.
    pub fn zero(self) -> Value {
        match self {
            FieldType::Number => Value::from(0.0),
            FieldType::Int => Value::from(0),
            FieldType::Bool => Value::Bool(false),
            FieldType::Text => Value::String(String::new()),
            FieldType::Id => Value::Null,
        }
    }

    /// `value` as this type, or why not. Whole numbers are accepted for `number`, and
    /// numbers that are whole for `int`. Numbers must be finite.
    pub fn coerce(self, value: &Value) -> Result<Value, String> {
        let wrong = || format!("expected {}, got {}", self.name(), describe(value));
        match (self, value) {
            (FieldType::Number, Value::Number(n)) => n
                .as_f64()
                .filter(|f| f.is_finite())
                .map(Value::from)
                .ok_or_else(wrong),
            (FieldType::Int, Value::Number(n)) => {
                if let Some(i) = n.as_i64() {
                    return Ok(Value::from(i));
                }
                // Whole and within 2^53, so exact.
                n.as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_992.0)
                    .map(|f| Value::from(f as i64))
                    .ok_or_else(wrong)
            }
            (FieldType::Bool, Value::Bool(_)) | (FieldType::Text, Value::String(_)) => {
                Ok(value.clone())
            }
            (FieldType::Id, Value::Null) => Ok(Value::Null),
            (FieldType::Id, Value::String(s)) if s.is_empty() => Ok(Value::Null),
            (FieldType::Id, Value::String(s)) if ids::is_full_id(s) => Ok(value.clone()),
            (FieldType::Id, Value::String(s)) => Err(format!(
                "`{s}` is not an entity ID (module.name, such as example.door)"
            )),
            _ => Err(wrong()),
        }
    }
}

/// A value's kind, for messages.
pub fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "nothing",
        Value::Bool(_) => "a bool",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "a table",
    }
}

/// What the version gate needs from a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minimum {
    pub dir: PathBuf,
    /// The module's id, if the manifest has a readable one.
    pub id: Option<String>,
    pub min_engine_version: Version,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    id: String,
    version: String,
    min_engine_version: String,
    #[serde(default)]
    scripts: Vec<String>,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    writes: Vec<String>,
    #[serde(default)]
    components: BTreeMap<String, BTreeMap<String, toml::Value>>,
}

/// The module folder a path names: the folder itself, or the folder of a `manifest.toml`.
pub fn module_dir(path: &Path) -> PathBuf {
    if path.is_file() && path.file_name().is_some_and(|name| name == FILE) {
        path.parent().map_or_else(PathBuf::new, Path::to_path_buf)
    } else {
        path.to_path_buf()
    }
}

fn read(dir: &Path) -> Result<String, Failure> {
    let path = dir.join(FILE);
    if !dir.is_dir() {
        return Err(
            Failure::bad_input(format!("{} is not a folder", dir.display()))
                .with("path", dir.display().to_string()),
        );
    }
    std::fs::read_to_string(&path).map_err(|e| {
        Failure::bad_input(format!(
            "can't read {}: {e}. A module is a folder with a {FILE}",
            path.display()
        ))
        .with("path", path.display().to_string())
    })
}

fn invalid(dir: &Path, message: impl std::fmt::Display) -> Failure {
    let path = dir.join(FILE);
    Failure::bad_input(format!("{}: {message}", path.display()))
        .with("path", path.display().to_string())
}

/// Reads only the module's minimum engine version (and its id, if readable).
pub fn read_minimum(dir: &Path) -> Result<Minimum, Failure> {
    let text = read(dir)?;
    minimum_of(dir, &text)
}

/// The minimum engine version (and id) in a manifest's text.
pub fn minimum_of(dir: &Path, text: &str) -> Result<Minimum, Failure> {
    let table: toml::Table = text.parse().map_err(|e| invalid(dir, e))?;
    let minimum = match table.get("min_engine_version") {
        Some(toml::Value::String(text)) => text.parse().map_err(|e| invalid(dir, e))?,
        Some(_) => return Err(invalid(dir, "min_engine_version must be a string")),
        None => return Err(invalid(dir, "min_engine_version is missing")),
    };
    Ok(Minimum {
        dir: dir.to_path_buf(),
        id: table.get("id").and_then(|v| v.as_str()).map(str::to_owned),
        min_engine_version: minimum,
    })
}

/// Reads the whole manifest strictly. Run the gate on [`read_minimum`] first.
pub fn load(dir: &Path) -> Result<Manifest, Failure> {
    let text = read(dir)?;
    parse(dir, &text)
}

/// Parses a manifest's text strictly. `dir` is where it came from, for messages.
pub fn parse(dir: &Path, text: &str) -> Result<Manifest, Failure> {
    let raw: Raw = toml::from_str(text).map_err(|e| invalid(dir, e))?;
    if !ids::valid_module_id(&raw.id) {
        return Err(invalid(
            dir,
            format!(
                "id `{}` must start with a lowercase letter and use only lowercase letters, \
                 digits, `-` and `_`",
                raw.id
            ),
        ));
    }
    if raw.id == ids::ENGINE {
        return Err(invalid(
            dir,
            format!(
                "id `{}` is the engine's own namespace; pick another",
                ids::ENGINE
            ),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for event in &raw.events {
        if !ids::valid_field(event) {
            return Err(invalid(dir, format!("events: {}", ids::name_rule(event))));
        }
        if !seen.insert(event) {
            return Err(invalid(dir, format!("events: `{event}` is listed twice")));
        }
    }
    for write in &raw.writes {
        match ids::split(write) {
            Some((module, _)) if module == raw.id => {
                return Err(invalid(
                    dir,
                    format!(
                        "writes: `{write}` is this module's own component type; a module \
                         always writes its own, so leave it out"
                    ),
                ));
            }
            Some(_) => {}
            None => {
                return Err(invalid(
                    dir,
                    format!(
                        "writes: `{write}` is not a component type ID (module.name, such \
                         as engine.position)"
                    ),
                ));
            }
        }
    }
    for script in &raw.scripts {
        if !script.ends_with(".luau") || script.contains('\\') || script.starts_with('/') {
            return Err(invalid(
                dir,
                format!(
                    "scripts: `{script}` must be a `.luau` file inside the module, written \
                     with `/` (such as scripts/main.luau)"
                ),
            ));
        }
    }
    let mut components = BTreeMap::new();
    for (name, fields) in raw.components {
        if !ids::valid_field(&name) {
            return Err(invalid(
                dir,
                format!("components: {}", ids::name_rule(&name)),
            ));
        }
        let mut decl = ComponentDecl::default();
        for (field, spec) in fields {
            let at = format!("components.{name}.{field}");
            if !ids::valid_field(&field) {
                return Err(invalid(
                    dir,
                    format!(
                        "{at}: a field name starts with a lowercase letter and uses \
                         lowercase letters, digits and `_`"
                    ),
                ));
            }
            decl.fields.insert(
                field,
                field_decl(&spec).map_err(|e| invalid(dir, format!("{at}: {e}")))?,
            );
        }
        components.insert(name, decl);
    }
    Ok(Manifest {
        dir: dir.to_path_buf(),
        version: raw
            .version
            .parse()
            .map_err(|e| invalid(dir, format!("version: {e}")))?,
        min_engine_version: raw
            .min_engine_version
            .parse()
            .map_err(|e| invalid(dir, format!("min_engine_version: {e}")))?,
        id: raw.id,
        scripts: raw.scripts,
        events: raw.events,
        writes: raw.writes,
        components,
    })
}

/// `"int"` or `{ type = "int", default = 3 }`.
fn field_decl(spec: &toml::Value) -> Result<FieldDecl, String> {
    let types = || {
        FieldType::ALL
            .iter()
            .map(|t| t.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let ty_of = |name: &str| {
        FieldType::parse(name)
            .ok_or_else(|| format!("unknown type `{name}`; use one of {}", types()))
    };
    match spec {
        toml::Value::String(name) => {
            let ty = ty_of(name)?;
            Ok(FieldDecl {
                ty,
                default: ty.zero(),
            })
        }
        toml::Value::Table(table) => {
            for key in table.keys() {
                if key != "type" && key != "default" {
                    return Err(format!(
                        "unknown key `{key}`; a field has `type` and `default`"
                    ));
                }
            }
            let ty = match table.get("type") {
                Some(toml::Value::String(name)) => ty_of(name)?,
                _ => return Err(format!("`type` is missing; use one of {}", types())),
            };
            let default = match table.get("default") {
                None => ty.zero(),
                Some(value) => ty.coerce(&toml_to_json(value)?)?,
            };
            Ok(FieldDecl { ty, default })
        }
        _ => Err(format!(
            "write the type (\"int\") or a table ({{ type = \"int\", default = 0 }}); types: {}",
            types()
        )),
    }
}

/// A TOML value as JSON. Dates have no JSON form and are refused.
pub fn toml_to_json(value: &toml::Value) -> Result<Value, String> {
    Ok(match value {
        toml::Value::String(s) => Value::String(s.clone()),
        toml::Value::Integer(i) => Value::from(*i),
        toml::Value::Float(f) if f.is_finite() => Value::from(*f),
        toml::Value::Float(_) => return Err("numbers must be finite".into()),
        toml::Value::Boolean(b) => Value::Bool(*b),
        toml::Value::Datetime(_) => return Err("dates are not supported".into()),
        toml::Value::Array(items) => {
            Value::Array(items.iter().map(toml_to_json).collect::<Result<_, _>>()?)
        }
        toml::Value::Table(table) => Value::Object(
            table
                .iter()
                .map(|(k, v)| Ok((k.clone(), toml_to_json(v)?)))
                .collect::<Result<_, String>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(text: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), text).unwrap();
        dir
    }

    #[test]
    fn reads_a_manifest_with_crlf_line_endings() {
        let dir =
            module("id = \"demo\"\r\nversion = \"0.2.0\"\r\nmin_engine_version = \"0.1\"\r\n");
        let manifest = load(dir.path()).unwrap();
        assert_eq!(manifest.id, "demo");
        assert_eq!(manifest.version.to_string(), "0.2.0");
        assert_eq!(manifest.min_engine_version.to_string(), "0.1.0");
    }

    #[test]
    fn the_minimum_reads_past_fields_a_newer_engine_added() {
        let text = "id = \"demo\"\nversion = \"1.0.0\"\nmin_engine_version = \"9.0.0\"\n\
                    [needs]\nsomething = true\n";
        let dir = module(text);
        let minimum = read_minimum(dir.path()).unwrap();
        assert_eq!(minimum.min_engine_version.to_string(), "9.0.0");
        assert_eq!(minimum.id.as_deref(), Some("demo"));
        assert_eq!(load(dir.path()).unwrap_err().kind, "bad_input");
    }

    #[test]
    fn bad_manifests_are_bad_input() {
        for text in [
            "id = \"demo\"\nversion = \"0.1.0\"\n",
            "id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = 1\n",
            "id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"latest\"\n",
            "not toml at all = = =",
        ] {
            let dir = module(text);
            let failure = read_minimum(dir.path()).unwrap_err();
            assert_eq!((failure.code, failure.kind), (4, "bad_input"), "{text}");
        }
        let head = "version = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n";
        for text in [
            format!("id = \"Demo\"\n{head}"),
            format!("id = \"engine\"\n{head}"),
            format!("id = \"demo\"\n{head}events = [\"Bad\"]\n"),
            format!("id = \"demo\"\n{head}events = [\"a\", \"a\"]\n"),
            format!("id = \"demo\"\n{head}writes = [\"demo.own\"]\n"),
            format!("id = \"demo\"\n{head}writes = [\"nodot\"]\n"),
            format!("id = \"demo\"\n{head}scripts = [\"main.lua\"]\n"),
            format!("id = \"demo\"\n{head}[components.c]\nf = \"float\"\n"),
            format!(
                "id = \"demo\"\n{head}[components.c]\nf = {{ type = \"int\", default = 1.5 }}\n"
            ),
            format!("id = \"demo\"\n{head}[components.c]\nf = {{ type = \"int\", min = 1 }}\n"),
        ] {
            let dir = module(&text);
            assert_eq!(load(dir.path()).unwrap_err().kind, "bad_input", "{text}");
        }
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(read_minimum(empty.path()).unwrap_err().code, 4);
    }

    #[test]
    fn reads_components_events_writes_and_scripts() {
        let dir = module(
            "id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.2.0\"\n\
             scripts = [\"scripts/main.luau\"]\nevents = [\"opened\"]\n\
             writes = [\"engine.position\"]\n\
             [components.door]\nopen = \"bool\"\npushes = { type = \"int\", default = 2 }\n\
             weight = { type = \"number\", default = 3 }\n",
        );
        let m = load(dir.path()).unwrap();
        assert_eq!(m.scripts, ["scripts/main.luau"]);
        assert_eq!(m.events, ["opened"]);
        assert_eq!(m.writes, ["engine.position"]);
        let door = &m.components["door"];
        assert_eq!(door.fields["open"].ty, FieldType::Bool);
        assert_eq!(door.fields["open"].default, Value::Bool(false));
        assert_eq!(door.fields["pushes"].default, Value::from(2));
        assert_eq!(door.fields["weight"].default, Value::from(3.0));
    }

    #[test]
    fn a_manifest_path_names_its_folder() {
        let dir = module("id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n");
        assert_eq!(module_dir(&dir.path().join(FILE)), dir.path());
        assert_eq!(module_dir(dir.path()), dir.path());
    }
}
