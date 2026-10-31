//! The module loader: a module folder read into memory, and what it declares.
//!
//! The engine reads a module's text files once ([`ModuleFiles::read`]) and works only on
//! that copy from then on, so a recording can store exactly the files a session ran and a
//! replay can run them again without the folder. Line endings are normalised to `\n`, so
//! a checkout with CRLF files hashes and runs the same as one with LF.
//!
//! ```text
//! <module>/
//!   manifest.toml     id, versions, scripts, events, writes, components (crate::manifest)
//!   start.toml        optional: the entities a new world starts with
//!   scripts/*.luau    scripts (the manifest lists the entry scripts)
//!   tests/*.toml      scenario tests
//! ```
//!
//! `start.toml`:
//!
//! ```toml
//! [entities.door]                              # the entity example.door
//! door = { open = false }                      # this module's component type
//! "engine.position" = { x = 1.0, y = 2.0 }     # another namespace's, by full ID
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::failure::Failure;
use crate::manifest::{self, FILE, Manifest};
use crate::{ids, version};

/// The start file in a module folder.
pub const START_FILE: &str = "start.toml";

/// File extensions the engine reads from a module. Other files (notes, later assets) are
/// not part of what it runs, records or hashes yet.
pub const EXTENSIONS: [&str; 2] = ["toml", "luau"];

/// Largest module file the engine reads.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// A module's text files, by path inside the module (`scripts/main.luau`, with `/`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ModuleFiles(pub BTreeMap<String, String>);

impl ModuleFiles {
    /// Reads the module folder's `.toml` and `.luau` files, skipping names that start
    /// with `.`.
    pub fn read(dir: &Path) -> Result<ModuleFiles, Failure> {
        let mut files = BTreeMap::new();
        walk(dir, dir, &mut files)?;
        Ok(ModuleFiles(files))
    }

    pub fn get(&self, path: &str) -> Option<&str> {
        self.0.get(path).map(String::as_str)
    }

    /// SHA-256 over every path and its text, in path order, as hex.
    pub fn hash(&self) -> String {
        let mut h = Sha256::new();
        for (path, text) in &self.0 {
            h.update((path.len() as u64).to_le_bytes());
            h.update(path.as_bytes());
            h.update((text.len() as u64).to_le_bytes());
            h.update(text.as_bytes());
        }
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<(), Failure> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        Failure::bad_input(format!("can't read {}: {e}", dir.display()))
            .with("path", dir.display().to_string())
    })?;
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out)?;
            continue;
        }
        let wanted = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.contains(&e));
        if !wanted {
            continue;
        }
        let bad =
            |message: String| Failure::bad_input(message).with("path", path.display().to_string());
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE_BYTES {
            return Err(bad(format!(
                "{} is {size} bytes; module files are at most {MAX_FILE_BYTES}",
                path.display()
            )));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| bad(format!("can't read {} as text: {e}", path.display())))?;
        let relative: Vec<String> = path
            .strip_prefix(root)
            .expect("walk stays inside the root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        out.insert(relative.join("/"), text.replace("\r\n", "\n"));
    }
    Ok(())
}

/// The components an entity starts with: full component type ID to its fields.
pub type StartEntity = BTreeMap<String, Map<String, Value>>;

/// A module read and parsed: its manifest, its files and its start state.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub manifest: Manifest,
    pub files: ModuleFiles,
    /// Entities a new world starts with, by full ID.
    pub start: BTreeMap<String, StartEntity>,
}

/// An ID a module declares, for the core-ID scan and `galaxy-gm check`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeclaredId {
    /// `module.name`.
    pub full: String,
    /// `name`.
    pub local: String,
    /// `component`, `event` or `entity`.
    pub what: &'static str,
}

impl Module {
    /// Reads a module folder from disk. Run the version gate first.
    pub fn read(dir: &Path) -> Result<Module, Failure> {
        Module::from_files(dir, ModuleFiles::read(dir)?)
    }

    /// Parses a module from its files. `dir` names where they came from, for messages.
    pub fn from_files(dir: &Path, files: ModuleFiles) -> Result<Module, Failure> {
        let text = files.get(FILE).ok_or_else(|| {
            Failure::bad_input(format!(
                "{} has no {FILE}: a module is a folder with a {FILE}",
                dir.display()
            ))
            .with("path", dir.display().to_string())
        })?;
        let minimum = manifest::minimum_of(dir, text)?;
        crate::gate::check_against(version::engine(), &[minimum])?;
        let manifest = manifest::parse(dir, text)?;
        let bad = |file: &str, message: String| {
            let path = dir.join(file);
            Failure::bad_input(format!("{}: {message}", path.display()))
                .with("path", path.display().to_string())
        };
        for script in &manifest.scripts {
            if files.get(script).is_none() {
                return Err(bad(
                    FILE,
                    format!("scripts: `{script}` is not in the module"),
                ));
            }
        }
        let start = match files.get(START_FILE) {
            None => BTreeMap::new(),
            Some(text) => parse_start(&manifest.id, text).map_err(|e| bad(START_FILE, e))?,
        };
        Ok(Module {
            manifest,
            files,
            start,
        })
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// The full ID of one of this module's component types or event kinds.
    pub fn full(&self, local: &str) -> String {
        format!("{}.{local}", self.manifest.id)
    }

    /// Every ID the module declares: its component types, event kinds and start entities.
    pub fn declared_ids(&self) -> Vec<DeclaredId> {
        let id = &self.manifest.id;
        let entry = |local: &str, what| DeclaredId {
            full: format!("{id}.{local}"),
            local: local.to_owned(),
            what,
        };
        let mut out: Vec<DeclaredId> = self
            .manifest
            .components
            .keys()
            .map(|c| entry(c, "component"))
            .chain(self.manifest.events.iter().map(|e| entry(e, "event")))
            .chain(self.start.keys().filter_map(|full| {
                let (_, local) = ids::split(full)?;
                Some(entry(local, "entity"))
            }))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// `start.toml`, with names made full.
fn parse_start(module: &str, text: &str) -> Result<BTreeMap<String, StartEntity>, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("{e}"))?;
    let mut out = BTreeMap::new();
    for (key, value) in table {
        if key != "entities" {
            return Err(format!(
                "unknown key `{key}`; the start file has [entities.<name>]"
            ));
        }
        let toml::Value::Table(entities) = value else {
            return Err("`entities` must be a table of entities".into());
        };
        for (name, components) in entities {
            let entity = ids::qualify_entity(module, &name)
                .ok_or_else(|| format!("entities: {}", ids::local_rule(&name)))?;
            let toml::Value::Table(components) = components else {
                return Err(format!("entities.{name} must be a table of components"));
            };
            let mut start = StartEntity::new();
            for (component, fields) in components {
                let full = ids::qualify_type(module, &component).ok_or_else(|| {
                    format!("entities.{name}: `{component}` is not a component type name")
                })?;
                let Value::Object(fields) = manifest::toml_to_json(&fields)
                    .map_err(|e| format!("entities.{name}.{component}: {e}"))?
                else {
                    return Err(format!(
                        "entities.{name}.{component} must be a table of fields"
                    ));
                };
                start.insert(full, fields);
            }
            if out.insert(entity.clone(), start).is_some() {
                return Err(format!("entities: `{entity}` is listed twice"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, path: &str, text: &str) {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_text_files_with_lf_and_parses_the_start() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        write(
            dir,
            FILE,
            "id = \"demo\"\r\nversion = \"0.1.0\"\r\nmin_engine_version = \"0.1.0\"\r\n\
             scripts = [\"scripts/main.luau\"]\r\nevents = [\"opened\"]\r\n\
             [components.door]\r\nopen = \"bool\"\r\n",
        );
        write(dir, "scripts/main.luau", "return {}\r\n");
        write(dir, "notes.md", "not read");
        write(dir, ".hidden/x.luau", "not read");
        write(
            dir,
            START_FILE,
            "[entities.front]\ndoor = { open = true }\n\"engine.position\" = { x = 1.5 }\n",
        );
        let module = Module::read(dir).unwrap();
        let paths: Vec<&String> = module.files.0.keys().collect();
        assert_eq!(paths, [FILE, "scripts/main.luau", START_FILE]);
        assert_eq!(module.files.get("scripts/main.luau"), Some("return {}\n"));
        let front = &module.start["demo.front"];
        assert_eq!(front["demo.door"]["open"], Value::Bool(true));
        assert_eq!(front["engine.position"]["x"], Value::from(1.5));
        let declared: Vec<(String, &str)> = module
            .declared_ids()
            .into_iter()
            .map(|d| (d.full, d.what))
            .collect();
        assert_eq!(
            declared,
            [
                ("demo.door".to_owned(), "component"),
                ("demo.front".to_owned(), "entity"),
                ("demo.opened".to_owned(), "event"),
            ]
        );

        // The same files on another line ending hash the same.
        let lf = module.files.hash();
        write(dir, "scripts/main.luau", "return {}\n");
        assert_eq!(ModuleFiles::read(dir).unwrap().hash(), lf);
    }

    #[test]
    fn a_missing_script_or_a_bad_start_is_bad_input() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let head = "id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n";
        write(
            dir,
            FILE,
            &format!("{head}scripts = [\"scripts/none.luau\"]\n"),
        );
        assert_eq!(Module::read(dir).unwrap_err().kind, "bad_input");
        write(dir, FILE, head);
        for start in [
            "[entities.Bad]\n",
            "[things.x]\n",
            "[entities.x]\n\"Bad.y\" = {}\n",
            "[entities.x]\nc = 3\n",
        ] {
            write(dir, START_FILE, start);
            assert_eq!(Module::read(dir).unwrap_err().kind, "bad_input", "{start}");
        }
    }
}
