//! JSON Schema generation. `cargo xtask contract` writes these into
//! `crates/contract/schemas/`; a test fails when the committed files are stale.

use schemars::{JsonSchema, Schema, schema_for};

use crate::assets::AssetManifest;
use crate::command::{CommandBatch, JournalEntry, RecordedCause, Rejection};
use crate::data::*;
use crate::events::EventEnvelope;
use crate::history::{Fact, HistoryQuery};
use crate::input::PlayerInput;
use crate::pack::{PackManifest, ValidationReport};
use crate::talk::TalkLine;
use crate::verbs::Order;
use crate::world::WorldState;

fn one<T: JsonSchema>(name: &'static str) -> (&'static str, Schema) {
    (name, schema_for!(T))
}

/// Every schema by name. The file is `<name>.json`.
pub fn all() -> Vec<(&'static str, Schema)> {
    vec![
        one::<WorldState>("world"),
        one::<EventEnvelope>("events"),
        one::<Order>("verbs"),
        one::<PlayerInput>("input"),
        one::<CommandBatch>("commands"),
        one::<JournalEntry>("journal"),
        one::<Rejection>("rejection"),
        one::<RecordedCause>("cause"),
        one::<Fact>("fact"),
        one::<HistoryQuery>("history-query"),
        one::<PackManifest>("manifest"),
        one::<ValidationReport>("report"),
        one::<AssetManifest>("assets"),
        one::<TalkLine>("talk"),
        one::<WaresFile>("data-wares"),
        one::<HullsFile>("data-hulls"),
        one::<FactionsFile>("data-factions"),
        one::<RegionsFile>("data-regions"),
        one::<LanesFile>("data-lanes"),
        one::<StationsFile>("data-stations"),
        one::<RocksFile>("data-rocks"),
        one::<NpcsFile>("data-npcs"),
        one::<StartFile>("data-start"),
    ]
}

/// `(file name, contents)` for every schema, as written to disk (LF, trailing newline).
pub fn render() -> Vec<(String, String)> {
    all()
        .into_iter()
        .map(|(name, schema)| {
            let mut text = serde_json::to_string_pretty(&schema).expect("schema serializes");
            text.push('\n');
            (format!("{name}.json"), text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn committed_schemas_are_fresh() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas");
        let mut stale = Vec::new();
        let expected = super::render();
        for (file, text) in &expected {
            let on_disk = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
            if on_disk.replace("\r\n", "\n") != *text {
                stale.push(file.clone());
            }
        }
        let known: Vec<_> = expected.iter().map(|(f, _)| f.as_str()).collect();
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && !known.contains(&name.as_str()) {
                stale.push(format!("{name} (no longer generated)"));
            }
        }
        assert!(
            stale.is_empty(),
            "schemas are stale, run `cargo xtask contract`: {stale:?}"
        );
    }
}
