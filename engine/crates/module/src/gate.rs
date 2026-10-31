//! The engine version gate: an engine refuses modules that need a newer engine.
//!
//! It runs before a command does any work, and it only reads files: it never builds or
//! installs anything. It tells the person how to install a new enough engine instead.

use crate::failure::{Failure, code};
use crate::manifest::{self, Manifest, Minimum};
use crate::version::{self, Version};

/// The one-line install instruction the refusal carries.
pub const HOW_TO_INSTALL: &str = "in an engine checkout at a version that is new enough, run \
                                  `cargo xtask install` in engine/ (see \
                                  engine/docs/modules-and-install.md)";

/// The module with the highest minimum engine version (the first one, on a tie).
pub fn highest(modules: &[Minimum]) -> Option<&Minimum> {
    modules.iter().reduce(|best, m| {
        if m.min_engine_version > best.min_engine_version {
            m
        } else {
            best
        }
    })
}

/// Passes if this engine is at least every module's minimum.
pub fn check(modules: &[Minimum]) -> Result<(), Failure> {
    check_against(version::engine(), modules)
}

/// Passes if `engine` is at least every module's minimum. A refusal exits 2 with kind
/// `engine_version` and names both versions and how to install.
pub fn check_against(engine: Version, modules: &[Minimum]) -> Result<(), Failure> {
    let Some(need) = highest(modules) else {
        return Ok(());
    };
    let required = need.min_engine_version;
    if required <= engine {
        return Ok(());
    }
    let module = need
        .id
        .clone()
        .unwrap_or_else(|| need.dir.display().to_string());
    Err(Failure::new(
        code::CANNOT_START,
        "engine_version",
        format!(
            "module {module} needs engine {required} or newer, but this is engine {engine}. \
             Install engine {required} or newer: {HOW_TO_INSTALL}. Then run the command again."
        ),
    )
    .with("engine", engine.to_string())
    .with("required", required.to_string())
    .with("module", module)
    .with("module_dir", need.dir.display().to_string())
    .with("install", HOW_TO_INSTALL))
}

/// Reads each module's minimum, runs the gate, then reads each manifest in full.
pub fn open(dirs: &[std::path::PathBuf]) -> Result<Vec<Manifest>, Failure> {
    let minimums = dirs
        .iter()
        .map(|dir| manifest::read_minimum(dir))
        .collect::<Result<Vec<_>, _>>()?;
    check(&minimums)?;
    dirs.iter().map(|dir| manifest::load(dir)).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn needs(id: &str, version: &str) -> Minimum {
        Minimum {
            dir: PathBuf::from(id),
            id: Some(id.into()),
            min_engine_version: version.parse().unwrap(),
        }
    }

    #[test]
    fn passes_when_the_engine_is_new_enough() {
        let engine = "0.4.2".parse().unwrap();
        assert!(check_against(engine, &[]).is_ok());
        assert!(check_against(engine, &[needs("a", "0.4"), needs("b", "0.4.2")]).is_ok());
    }

    #[test]
    fn refuses_naming_both_versions_and_how_to_install() {
        let engine = "0.4.2".parse().unwrap();
        let failure = check_against(
            engine,
            &[needs("a", "0.3"), needs("b", "0.5"), needs("c", "0.4")],
        )
        .unwrap_err();
        assert_eq!((failure.code, failure.kind), (2, "engine_version"));
        let json = failure.to_json();
        assert_eq!(json["engine"], "0.4.2");
        assert_eq!(json["required"], "0.5.0");
        assert_eq!(json["module"], "b");
        assert!(failure.message.contains("0.4.2") && failure.message.contains("0.5.0"));
        assert!(failure.message.contains("cargo xtask install"));
    }
}
