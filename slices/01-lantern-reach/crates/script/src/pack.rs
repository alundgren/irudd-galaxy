//! Reading a pack folder: the manifest, its files and its revision.
//!
//! The revision is a content hash (BLAKE3) over every file under the pack root with
//! line endings normalised, so the same pack checked out on Windows or macOS has the
//! same revision. Files are read once; the VM and the checks work on the in-memory
//! copy, so what runs is exactly what was hashed and checked.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use galaxy_contract::api;
use galaxy_contract::data::NpcsFile;
use galaxy_contract::events::EventKind;
use galaxy_contract::pack::{Diagnostic, Limits, Location, PackManifest, Severity};
use galaxy_contract::world::Npc;

/// Engine maxima for [`Limits`]; a manifest may ask for less.
pub const MAX_LIMITS: Limits = Limits {
    steps_per_call: 1_000_000,
    memory_bytes: 64 * 1024 * 1024,
    quarantine_after: 10,
};

/// Largest file a pack may contain.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// A pack read from disk.
#[derive(Clone, Debug)]
pub struct Pack {
    pub root: PathBuf,
    pub manifest: PackManifest,
    /// Content hash of all files.
    pub revision: String,
    /// Every text file by `/`-separated path relative to the root.
    pub files: BTreeMap<String, String>,
    /// Records the pack's data files define (created at install if missing).
    pub data: PackData,
    /// Manifest warnings (the manifest step reports them).
    pub warnings: Vec<Diagnostic>,
}

/// Authored records a pack ships. Only people for now; other data types are an
/// extension point (they need their `Put*` commands wired in [`PackData::commands`]).
#[derive(Clone, Debug, Default)]
pub struct PackData {
    pub npcs: Vec<Npc>,
}

impl Pack {
    /// Every Luau file in the pack, sorted.
    pub fn luau_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .filter(|(p, _)| p.ends_with(".luau"))
            .map(|(p, s)| (p.as_str(), s.as_str()))
    }

    /// The folders scripts load code from: `scripts/`, and `lib/` if the manifest
    /// sets `lib = true`. Never `tests/`.
    pub fn script_roots(&self) -> &'static [&'static str] {
        if self.manifest.lib {
            &["scripts", "lib"]
        } else {
            &["scripts"]
        }
    }

    /// Can a script load `path`? It must be a `.luau` file under a script root, every
    /// segment a plain name (`[A-Za-z0-9_-]`, plus the extension), and in the pack.
    /// `require` loads only these, and the typecheck, the directive check and the scan
    /// run over exactly these, so nothing that runs escapes a check.
    pub fn loadable(&self, path: &str) -> bool {
        let mut segs = path.split('/');
        let root_ok = segs
            .next()
            .is_some_and(|r| self.script_roots().contains(&r));
        root_ok && loadable_name(path) && path.contains('/') && self.files.contains_key(path)
    }

    /// The Luau files scripts can load (see [`Pack::loadable`]), sorted.
    pub fn loadable_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.luau_files().filter(|(p, _)| self.loadable(p))
    }

    pub fn test_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.luau_files().filter(|(p, _)| p.starts_with("tests/"))
    }

    /// The manifest's limits, clamped to the engine maxima.
    pub fn limits(&self) -> Limits {
        let l = &self.manifest.limits;
        Limits {
            steps_per_call: l.steps_per_call.clamp(1, MAX_LIMITS.steps_per_call),
            memory_bytes: l.memory_bytes.clamp(1024 * 1024, MAX_LIMITS.memory_bytes),
            quarantine_after: l.quarantine_after.clamp(1, MAX_LIMITS.quarantine_after),
        }
    }

    /// IDs this pack creates: its data records and the manifest's `creates`.
    pub fn created_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self
            .manifest
            .creates
            .iter()
            .map(|i| i.to_string())
            .collect();
        ids.extend(self.data.npcs.iter().map(|n| n.id.to_string()));
        ids
    }
}

/// A diagnostic at a file (and maybe line).
pub fn diag(
    severity: Severity,
    code: &str,
    message: impl Into<String>,
    file: Option<&str>,
    line: Option<u32>,
) -> Diagnostic {
    Diagnostic {
        severity,
        code: code.to_owned(),
        message: message.into(),
        location: file.map(|f| Location {
            file: f.to_owned(),
            line,
            column: None,
        }),
    }
}

pub fn error(
    code: &str,
    message: impl Into<String>,
    file: Option<&str>,
    line: Option<u32>,
) -> Diagnostic {
    diag(Severity::Error, code, message, file, line)
}

/// Reads a pack. On failure returns the manifest-step diagnostics.
pub fn read_pack(root: &Path) -> Result<Pack, Vec<Diagnostic>> {
    let files = read_files(root).map_err(|e| vec![error("io", e, None, None)])?;
    let Some(text) = files.get("manifest.toml") else {
        return Err(vec![error(
            "missing_manifest",
            "no manifest.toml at the pack root",
            Some("manifest.toml"),
            None,
        )]);
    };
    let manifest: PackManifest = toml::from_str(text).map_err(|e| {
        let line = e.span().map(|s| line_of(text, s.start));
        vec![error(
            "manifest_schema",
            e.message().to_owned(),
            Some("manifest.toml"),
            line,
        )]
    })?;
    let mut diags = check_manifest(&manifest, &files);
    let mut data = PackData::default();
    for file in &manifest.data {
        let Some(text) = files.get(file.as_str()) else {
            diags.push(error(
                "missing_file",
                format!("data file `{file}` doesn't exist"),
                Some("manifest.toml"),
                None,
            ));
            continue;
        };
        match Path::new(file).file_name().and_then(|n| n.to_str()) {
            Some("npcs.toml") => match toml::from_str::<NpcsFile>(text) {
                Ok(f) => data.npcs.extend(f.npc),
                Err(e) => diags.push(error(
                    "data_schema",
                    e.message().to_owned(),
                    Some(file),
                    e.span().map(|s| line_of(text, s.start)),
                )),
            },
            // The core pack's world tables: `galaxy_core::load` builds the world from
            // them at new game, so the host has nothing to install. Its config files
            // (`mind.toml`) belong to the crates that read them.
            Some(name)
                if manifest.id.as_str() == "core"
                    && (galaxy_contract::data::DATA_FILES.contains(&name)
                        || galaxy_contract::data::CONFIG_FILES.contains(&name)) => {}
            _ => diags.push(error(
                "unsupported_data",
                format!("pack data `{file}`: only `npcs.toml` can be installed by a pack so far"),
                Some(file),
                None,
            )),
        }
    }
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(diags);
    }
    let revision = revision(&files);
    Ok(Pack {
        root: root.to_path_buf(),
        manifest,
        revision,
        files,
        data,
        warnings: diags,
    })
}

/// 1-based line of a byte offset.
pub fn line_of(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    u32::try_from(text[..end].matches('\n').count() + 1).unwrap_or(u32::MAX)
}

fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_lowercase())
        && c.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

fn check_manifest(m: &PackManifest, files: &BTreeMap<String, String>) -> Vec<Diagnostic> {
    let at = Some("manifest.toml");
    let mut d = Vec::new();
    let mut seen_uses = BTreeSet::new();
    for u in &m.uses {
        if !seen_uses.insert(u) {
            d.push(diag(
                Severity::Warning,
                "duplicate_use",
                format!("`{u}` is listed twice in `uses`"),
                at,
                None,
            ));
        }
        match api::find_use(u) {
            None => d.push(error(
                "unknown_use",
                format!("unknown capability `{u}` in `uses` (see crates/contract/api/HOOKS.md)"),
                at,
                None,
            )),
            Some(def) if !def.available => d.push(error(
                "reserved_use",
                format!(
                    "capability `{u}` is reserved and not implemented yet: {}",
                    def.summary
                ),
                at,
                None,
            )),
            Some(_) => {}
        }
    }
    let mut ids = BTreeSet::new();
    for s in &m.scripts {
        if !is_name(&s.id) {
            d.push(error(
                "bad_script_id",
                format!("script id `{}` must be [a-z][a-z0-9_]*", s.id),
                at,
                None,
            ));
        }
        if !ids.insert(&s.id) {
            d.push(error(
                "duplicate_script",
                format!("script id `{}` is used twice", s.id),
                at,
                None,
            ));
        }
        // The same name rule `require` and the checks use, so a declared script can
        // never be one the checks skip.
        if !(s.file.starts_with("scripts/") && loadable_name(&s.file)) {
            d.push(error(
                "bad_script_file",
                format!(
                    "script `{}`: file must be `scripts/<name>.luau`, got `{}`",
                    s.id, s.file
                ),
                at,
                None,
            ));
        } else if !files.contains_key(&s.file) {
            d.push(error(
                "missing_file",
                format!("script `{}`: `{}` doesn't exist", s.id, s.file),
                at,
                None,
            ));
        }
        for h in &s.handlers {
            match h.split_once('.') {
                Some(("on", ev)) if EventKind::ALL.iter().any(|k| k.as_str() == ev) => {}
                Some(("on", ev)) => d.push(error(
                    "unknown_event",
                    format!("script `{}`: no event `{ev}` (handler `{h}`)", s.id),
                    at,
                    None,
                )),
                Some(("after", name)) if is_name(name) => {}
                _ => d.push(error(
                    "bad_handler",
                    format!(
                        "script `{}`: handler `{h}` must be `on.<event>` or `after.<name>`",
                        s.id
                    ),
                    at,
                    None,
                )),
            }
        }
        for (store, keys) in &s.stores {
            for name in std::iter::once(store).chain(keys.keys()) {
                if !is_name(name) {
                    d.push(error(
                        "bad_store",
                        format!(
                            "script `{}`: store name `{name}` must be [a-z][a-z0-9_]*",
                            s.id
                        ),
                        at,
                        None,
                    ));
                }
            }
        }
    }
    for path in files.keys().filter(|p| p.ends_with(".luau")) {
        let root = path.split('/').next().unwrap_or("");
        let reachable = root == "scripts" || root == "tests" || (root == "lib" && m.lib);
        if !reachable {
            d.push(diag(
                Severity::Warning,
                "unloadable_file",
                format!(
                    "`{path}` is outside the script roots, so nothing can load it: scripts load \
                     modules from `scripts/` (and `lib/` with `lib = true` in the manifest)"
                ),
                Some(path),
                None,
            ));
        }
    }
    let l = &m.limits;
    let max = MAX_LIMITS;
    if l.steps_per_call > max.steps_per_call
        || l.memory_bytes > max.memory_bytes
        || l.quarantine_after > max.quarantine_after
    {
        d.push(diag(
            Severity::Warning,
            "limits_clamped",
            format!(
                "limits are clamped to the engine maxima: {} steps per call, {} bytes, quarantine after {}",
                max.steps_per_call, max.memory_bytes, max.quarantine_after
            ),
            at,
            None,
        ));
    }
    d
}

/// Static invariant: every declared shock opens a declared opportunity.
pub fn unpaired_shocks(m: &PackManifest) -> Vec<Diagnostic> {
    let opportunities: BTreeSet<&str> = m.opportunities.iter().map(|o| o.id.as_str()).collect();
    m.shocks
        .iter()
        .filter(|s| !opportunities.contains(s.opportunity.as_str()))
        .map(|s| {
            error(
                "unpaired_shock",
                format!(
                    "shock `{}` names opportunity `{}`, which isn't declared in [[opportunity]]",
                    s.id, s.opportunity
                ),
                Some("manifest.toml"),
                None,
            )
        })
        .collect()
}

/// Reads every file under `root` (skipping dot-files and folders).
fn read_files(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let meta = entry.metadata().map_err(|e| e.to_string())?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if meta.len() > MAX_FILE_BYTES {
                return Err(format!("{rel} is larger than {MAX_FILE_BYTES} bytes"));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
            out.insert(rel, text.replace("\r\n", "\n"));
        }
    }
    Ok(out)
}

/// BLAKE3 over `path NUL contents NUL` for every file, sorted by path. 16 hex digits.
pub fn revision(files: &BTreeMap<String, String>) -> String {
    let mut h = blake3::Hasher::new();
    for (path, text) in files {
        h.update(path.as_bytes());
        h.update(&[0]);
        h.update(text.as_bytes());
        h.update(&[0]);
    }
    h.finalize().to_hex()[..16].to_owned()
}

/// A path every check covers and `require` can load: `.luau`, and each segment made
/// only of `[A-Za-z0-9_-]` (no dots, spaces or accents).
pub(crate) fn loadable_name(path: &str) -> bool {
    let Some(stem) = path.strip_suffix(".luau") else {
        return false;
    };
    stem.split('/').all(|seg| {
        !seg.is_empty()
            && seg
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
}

#[cfg(test)]
mod loadable_name_tests {
    use super::loadable_name;

    #[test]
    fn script_names_follow_the_require_rule() {
        assert!(loadable_name("scripts/kettle_growth.luau"));
        assert!(loadable_name("lib/a-b/c_d.luau"));
        // Dots, spaces and accents would be skipped by the checks, so they're refused.
        assert!(!loadable_name("scripts/probe.v2.luau"));
        assert!(!loadable_name("scripts/my script.luau"));
        assert!(!loadable_name("scripts/k\u{e4}ttle.luau"));
        assert!(!loadable_name("scripts/.luau"));
        assert!(!loadable_name("scripts/a.lua"));
    }
}
