//! A module's `manifest.toml`.
//!
//! ```toml
//! id = "example"
//! version = "0.1.0"
//! min_engine_version = "0.1.0"
//! ```
//!
//! The minimum is read on its own first ([`read_minimum`]), so a manifest written for a
//! newer engine, with fields this one doesn't know, still reaches the version gate and is
//! refused for the right reason. Only then is the whole file read strictly ([`load`]).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::failure::Failure;
use crate::version::Version;

/// The manifest's file name in a module folder.
pub const FILE: &str = "manifest.toml";

/// A module's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub dir: PathBuf,
    pub id: String,
    pub version: Version,
    pub min_engine_version: Version,
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
    let raw: Raw = toml::from_str(&text).map_err(|e| invalid(dir, e))?;
    if !valid_id(&raw.id) {
        return Err(invalid(
            dir,
            format!(
                "id `{}` must start with a lowercase letter and use only lowercase letters, \
                 digits, `-` and `_`",
                raw.id
            ),
        ));
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
    })
}

fn valid_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
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
        let dir = module("id = \"Demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n");
        assert_eq!(load(dir.path()).unwrap_err().kind, "bad_input");
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(read_minimum(empty.path()).unwrap_err().code, 4);
    }

    #[test]
    fn a_manifest_path_names_its_folder() {
        let dir = module("id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"0.1.0\"\n");
        assert_eq!(module_dir(&dir.path().join(FILE)), dir.path());
        assert_eq!(module_dir(dir.path()), dir.path());
    }
}
