//! The core-ID scan: the engine knows no game nouns.
//!
//! Every ID a module declares ([`crate::module::Module::declared_ids`]) is looked for in
//! source text as a whole word: the full ID (`example.door`) and the local name (`door`).
//! A word is a run of ASCII letters, digits and `_`; matching is case-sensitive, like the
//! IDs. The module's own name is never looked for (the engine says "module" and "core"
//! freely), nor is a local name equal to it.
//!
//! Two users:
//! - The engine check scans engine and GM source for the IDs `game/modules/core` declares
//!   and fails on a match (`crates/gm/tests/core_ids.rs`).
//! - `cargo xtask install` ships an index of the words in engine and GM program source
//!   ([`SourceIndex`], `source-ids.json` in the build folder), and `galaxy-gm check`
//!   compares a module's declared IDs with it, so a module change that introduces a
//!   clashing ID is caught on that change, before the next engine check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::module::DeclaredId;

/// The index file in an installed build.
pub const INDEX_FILE: &str = "source-ids.json";

/// The module whose IDs the engine check scans for: the base game, `game/modules/core`.
/// `galaxy-gm check` rejects its clashes and only warns about other modules', since only
/// its IDs fail the engine check.
pub const SCANNED_MODULE: &str = "core";

/// File types the scan reads.
const TEXT: [&str; 6] = ["rs", "toml", "luau", "md", "json", "txt"];

fn word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The words in `text`.
pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
}

/// Whether `needle` occurs in `line` as a whole word (no word character or `.` before it,
/// no word character after it).
fn has_whole(line: &str, needle: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(at) = line[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        let before = start == 0 || !(word_char(bytes[start - 1]) || bytes[start - 1] == b'.');
        let after = end == bytes.len() || !word_char(bytes[end]);
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

/// One declared ID found in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub id: DeclaredId,
    /// The text that matched: the full ID or the local name.
    pub matched: String,
    pub file: PathBuf,
    pub line: usize,
}

/// The IDs worth looking for: all but the module's own name.
fn wanted<'a>(module: &'a str, declared: &'a [DeclaredId]) -> impl Iterator<Item = &'a DeclaredId> {
    declared.iter().filter(move |d| d.local != module)
}

/// Text files under `dir`, skipping hidden folders and `target`.
pub fn text_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| TEXT.contains(&e))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Every place under `dirs` where a declared ID of `module` appears.
pub fn scan(dirs: &[PathBuf], module: &str, declared: &[DeclaredId]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for file in dirs.iter().flat_map(|d| text_files(d)) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let in_line: BTreeSet<&str> = words(line).collect();
            for id in wanted(module, declared) {
                let matched = if has_whole(line, &id.full) {
                    &id.full
                } else if if id.local.contains('.') {
                    has_whole(line, &id.local)
                } else {
                    in_line.contains(id.local.as_str())
                } {
                    &id.local
                } else {
                    continue;
                };
                hits.push(Hit {
                    id: id.clone(),
                    matched: matched.clone(),
                    file: file.clone(),
                    line: n + 1,
                });
            }
        }
    }
    hits
}

/// The words in engine and GM program source, shipped with an installed build.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceIndex {
    pub engine: String,
    pub words: BTreeSet<String>,
}

impl SourceIndex {
    /// The words in every text file under `dirs`.
    pub fn build(dirs: &[PathBuf]) -> SourceIndex {
        let mut words_seen = BTreeSet::new();
        for file in dirs.iter().flat_map(|d| text_files(d)) {
            if let Ok(text) = std::fs::read_to_string(&file) {
                words_seen.extend(words(&text).map(str::to_owned));
            }
        }
        SourceIndex {
            engine: crate::version::engine().to_string(),
            words: words_seen,
        }
    }

    /// The declared IDs of `module` that appear in the index. A full ID appears only where
    /// its local name does, so the local name decides. The index holds single words, so a
    /// local name with dots (an entity such as `row.03`) is left to the engine check.
    pub fn clashes<'a>(&self, module: &'a str, declared: &'a [DeclaredId]) -> Vec<&'a DeclaredId> {
        wanted(module, declared)
            .filter(|d| self.words.contains(d.local.as_str()))
            .collect()
    }
}

/// The program source the index covers: `src` of every crate under `crates`.
pub fn program_source(crates: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(crates) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path().join("src")))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(local: &str) -> DeclaredId {
        DeclaredId {
            full: format!("demo.{local}"),
            local: local.into(),
            what: "component",
        }
    }

    #[test]
    fn matches_whole_words_only() {
        assert!(has_whole("let x = demo.door;", "demo.door"));
        assert!(!has_whole("let x = demo.doorway;", "demo.door"));
        assert!(!has_whole("x.demo.door", "demo.door"));
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("a.rs"),
            "// a door\nfn doorway() {}\nconst X: &str = \"demo.door\";\nmod demo;\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("b.bin"), "door").unwrap();
        let declared = [id("door"), id("demo"), id("hinge")];
        let hits = scan(&[tmp.path().to_path_buf()], "demo", &declared);
        let found: Vec<(usize, &str)> = hits.iter().map(|h| (h.line, h.matched.as_str())).collect();
        assert_eq!(found, [(1, "door"), (3, "demo.door")]);

        let index = SourceIndex::build(&[tmp.path().to_path_buf()]);
        let clashes: Vec<&str> = index
            .clashes("demo", &declared)
            .iter()
            .map(|d| d.local.as_str())
            .collect();
        assert_eq!(clashes, ["door"]);
    }
}
