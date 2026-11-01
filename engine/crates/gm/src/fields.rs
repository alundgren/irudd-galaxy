//! `galaxy-gm fields [topic]`: the keys and fields files and components take, with types,
//! defaults and ranges, from the engine's own definitions (each next to its parser, and
//! tested against it there).

use std::path::Path;

use galaxy_module::cli::{self, Output};
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover, manifest};
use serde_json::{Map, Value, json};

pub enum Error {
    Usage(String),
    Failure(Failure),
}

impl From<Failure> for Error {
    fn from(f: Failure) -> Self {
        Error::Failure(f)
    }
}

const TOPICS: &[(&str, &str)] = &[
    (
        "style",
        "a module's style.toml: ink, light, sky, course and looks",
    ),
    (
        "components",
        "the engine's components (engine.position, engine.flight, ...): fields, types, defaults; with --modules, the modules' own too",
    ),
    ("manifest", "a module's manifest.toml"),
    (
        "inputs",
        "an inputs file (--inputs): inputs, notes, hot-loads and zooms by tick",
    ),
    (
        "spec",
        "a motion check spec: what to film and the expectations",
    ),
];

pub fn run(args: &[String]) -> Result<Output, Error> {
    let mut topic = None;
    let mut modules = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--modules" => {
                modules = Some(
                    it.next()
                        .ok_or_else(|| Error::Usage("--modules needs a folder".into()))?,
                )
            }
            o if o.starts_with("--") => {
                return Err(Error::Usage(format!("fields has no option {o}")));
            }
            t if topic.is_none() => topic = Some(t),
            t => {
                return Err(Error::Usage(format!(
                    "fields takes one topic, not also `{t}`"
                )));
            }
        }
    }
    if modules.is_some() && topic != Some("components") {
        return Err(Error::Usage(
            "--modules goes with the components topic".into(),
        ));
    }
    let out = match topic {
        None => json!({
            "topics": TOPICS.iter().map(|(t, about)| (t.to_string(), json!(about))).collect::<Map<_, _>>(),
            "next": "galaxy-gm fields <topic>",
        }),
        Some("style") => style(),
        Some("components") => components(modules)?,
        Some("manifest") => keyed("manifest.toml", manifest::FILE_KEYS),
        Some("inputs") => keyed("an inputs file", galaxy_sim::scenario::INPUTS_KEYS),
        Some("spec") => {
            let mut v = keyed("a motion check spec", galaxy_motion::check::SPEC_KEYS);
            let tools: Map<String, Value> = galaxy_motion::TOOLS
                .iter()
                .map(|t| {
                    let opts = galaxy_motion::options(t).unwrap_or_default();
                    (t.to_string(), json!(opts.join(", ")))
                })
                .collect();
            v["tool_options"] = Value::Object(tools);
            v
        }
        Some(other) => {
            let names: Vec<&str> = TOPICS.iter().map(|t| t.0).collect();
            return Err(Error::Usage(format!(
                "no fields topic `{other}`; the topics are {}",
                names.join(", ")
            )));
        }
    };
    Ok(Output::Json(out))
}

/// `"inner_px": "0 to 8; default 2.0: inner line width, logical pixels"`, a line a key.
fn style() -> Value {
    let mut tables = Map::new();
    for (table, keys) in galaxy_render::style::defaults() {
        let mut rows = Map::new();
        for (key, default) in keys {
            let (range, about) = galaxy_render::style::KEYS
                .iter()
                .find(|k| k.0 == table && k.1 == key)
                .map_or(("", ""), |k| (k.2, k.3));
            let kind = if default.starts_with("\"#") {
                "colour \"#rrggbb\"".to_owned()
            } else {
                range.to_owned()
            };
            rows.insert(key, json!(format!("{kind}; default {default}: {about}")));
        }
        tables.insert(table.to_owned(), Value::Object(rows));
    }
    json!({
        "topic": "style.toml",
        "about": "Every key is optional. ink, light, sky and course are scene-wide and may come from one module only; [looks.<name>] is a look models wear as look = \"<module>.<name>\".",
        "tables": tables,
    })
}

/// The engine's components, and with `--modules` the modules' own.
fn components(modules: Option<&String>) -> Result<Value, Error> {
    let mut all = Map::new();
    for (name, fields) in galaxy_api::engine::components() {
        let mut rows = Map::new();
        for (field, decl) in fields {
            let about = galaxy_api::engine::FIELDS
                .iter()
                .find(|f| f.0 == name && f.1 == field)
                .map_or("", |f| f.2);
            rows.insert(
                field,
                json!(format!(
                    "{}; default {}: {about}",
                    decl.ty.name(),
                    decl.default
                )),
            );
        }
        all.insert(format!("engine.{name}"), Value::Object(rows));
    }
    if let Some(dir) = modules {
        let dir = discover::resolve(&cli::current_dir()?, Path::new(dir));
        let dirs = discover::modules_in(&dir);
        if dirs.is_empty() {
            return Err(Error::Failure(
                Failure::bad_input(format!("no modules in {}", dir.display()))
                    .with("path", dir.display().to_string()),
            ));
        }
        for d in dirs {
            let m = Module::read(&d)?;
            for (name, decl) in &m.manifest.components {
                let rows: Map<String, Value> = decl
                    .fields
                    .iter()
                    .map(|(f, d)| {
                        (
                            f.clone(),
                            json!(format!("{}; default {}", d.ty.name(), d.default)),
                        )
                    })
                    .collect();
                all.insert(format!("{}.{name}", m.id()), Value::Object(rows));
            }
        }
    }
    Ok(json!({
        "topic": "components",
        "about": "A component is a type of data an entity can have, written in start.toml or by scripts. The engine's services act on engine.*; a module writes another module's components only if its manifest lists them in writes.",
        "components": all,
    }))
}

/// A file's keys by table: `"tick": "10: the tick it runs on ..."`.
fn keyed(topic: &str, keys: &[(&str, &str, &str, &str)]) -> Value {
    let mut tables = Map::new();
    for (table, key, example, about) in keys {
        let name = if table.is_empty() { "top level" } else { table };
        let entry = tables
            .entry(name.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        entry[*key] = json!(format!("{about}. Example: {key} = {example}"));
    }
    json!({ "topic": topic, "tables": tables })
}
