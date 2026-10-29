//! Pack manifests, validation reports, and the per-pack state a world keeps.
//!
//! A pack is a folder: `manifest.toml` ([`PackManifest`]), Luau scripts under
//! `scripts/`, TOML data under `data/` and optional Luau unit tests under `tests/`.
//! Its revision is a content hash of those files. Every validation step writes into
//! one [`ValidationReport`]; later steps (sandbox run, legibility, stage) append to the
//! same report without changing its shape.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

/// `manifest.toml` at the root of a pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// `core`, or namespaced for game-master packs: `gm.kettle_growth`.
    pub id: PackId,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Hook capabilities the pack's scripts may use, e.g. `events.dock`,
    /// `stations.write`. See `crates/contract/api/HOOKS.md`. Undeclared namespaces are
    /// absent from `ctx` at runtime.
    #[serde(default)]
    pub uses: Vec<String>,
    /// The pack's scripts, run in this order.
    #[serde(default, rename = "script")]
    pub scripts: Vec<ScriptDecl>,
    /// Data files, relative to the pack root. Records they define are created when
    /// the pack is installed, if the world doesn't have them yet.
    #[serde(default)]
    pub data: Vec<String>,
    #[serde(default)]
    pub depends: Vec<PackId>,
    /// IDs the scripts create at runtime (so the reference check knows them).
    #[serde(default)]
    pub creates: Vec<AnyId>,
    /// Changes that make something worse for the player. Each names its paired
    /// opportunity.
    #[serde(default, rename = "shock")]
    pub shocks: Vec<Shock>,
    #[serde(default, rename = "opportunity")]
    pub opportunities: Vec<Opportunity>,
    /// What a person says when this pack's changes are rolled back in-world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<RollbackLine>,
    #[serde(default)]
    pub limits: Limits,
}

impl PackManifest {
    pub fn script(&self, id: &str) -> Option<&ScriptDecl> {
        self.scripts.iter().find(|s| s.id == id)
    }
}

/// One script in a pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptDecl {
    /// `[a-z][a-z0-9_]*`, unique in the pack. The script's `G.script { id = ... }`.
    pub id: String,
    /// Relative to the pack root, e.g. `scripts/kettle_growth.luau`.
    pub file: String,
    /// What the script handles: `on.<event>` and `after.<trigger>`.
    #[serde(default)]
    pub handlers: Vec<String>,
    /// Persistent per-script stores: store name -> key -> value type. Saved with the
    /// world, so a reload keeps them.
    #[serde(default)]
    pub stores: BTreeMap<String, BTreeMap<String, ValueType>>,
}

/// The type of a value in a script store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Boolean,
    Integer,
    Number,
    String,
}

/// A value in a script store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ScriptValue {
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
}

impl ScriptValue {
    pub fn value_type(&self) -> ValueType {
        match self {
            ScriptValue::Boolean(_) => ValueType::Boolean,
            ScriptValue::Integer(_) => ValueType::Integer,
            ScriptValue::Number(_) => ValueType::Number,
            ScriptValue::String(_) => ValueType::String,
        }
    }
}

/// A declared shock: a change that makes something worse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shock {
    pub id: String,
    pub summary: String,
    /// The `id` of the opportunity it opens. Required.
    pub opportunity: String,
}

/// A declared opportunity the player can reach.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Opportunity {
    pub id: String,
    pub summary: String,
}

/// The in-world line said when the pack's changes are rolled back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RollbackLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<StationId>,
    pub text: String,
}

/// Per-pack runtime limits. The engine clamps them to its own maxima.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// VM interrupt checks (loop back-edges and calls) per handler call.
    pub steps_per_call: u64,
    /// Memory limit for the pack's Luau state.
    pub memory_bytes: u64,
    /// Aborted handler calls (per script and revision) before the script is
    /// quarantined.
    pub quarantine_after: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            steps_per_call: 100_000,
            memory_bytes: 16 * 1024 * 1024,
            quarantine_after: 3,
        }
    }
}

/// The result of validating (and maybe applying) a pack revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    pub pack: PackId,
    /// Content hash of the pack's files.
    pub revision: String,
    pub status: ReportStatus,
    /// In pipeline order. Later steps are appended as they run.
    pub steps: Vec<StepReport>,
}

impl ValidationReport {
    /// Rejected if any step failed.
    pub fn accepted(&self) -> bool {
        !matches!(self.status, ReportStatus::Rejected)
    }

    /// Every error diagnostic, in step order.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.steps
            .iter()
            .flat_map(|s| &s.diagnostics)
            .filter(|d| d.severity == Severity::Error)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReportStatus {
    Accepted,
    Rejected,
    /// Applied; `change` is the cause ID to roll back by.
    Applied {
        change: CauseId,
    },
}

/// The validation pipeline, in order. `pack check` and dev hot reload run the fast
/// steps (`manifest` to `smoke`); the game-master path adds the rest.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStep {
    /// `manifest.toml` and data files parse against their schemas; files exist.
    Manifest,
    /// Strict Luau typecheck against `galaxy.d.luau`.
    Types,
    /// Every hook a script uses is declared in `uses`; handlers match the manifest.
    Hooks,
    /// Every referenced ID exists or is created by this pack.
    References,
    /// Static invariants: no clock or timer APIs, shocks paired with opportunities.
    Invariants,
    /// Each script loads and each handler runs once on a sample event within budget.
    Smoke,
    Canon,
    Sandbox,
    Legibility,
    Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Passed,
    Failed,
    /// Not run (a tool is missing, or an earlier step failed); see the diagnostics.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StepReport {
    pub step: ValidationStep,
    pub outcome: StepOutcome,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine-readable reason, e.g. `undeclared_hook`, `clock_api`,
    /// `over_budget`, `uncited_line`, `type_error`.
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Location {
    /// Relative to the pack root, `/`-separated.
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

/// The result of `galaxy pack test`: the pack's `tests/*.luau` unit tests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TestReport {
    pub pack: PackId,
    pub revision: String,
    pub passed: bool,
    pub tests: Vec<TestResult>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TestResult {
    pub file: String,
    pub name: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// What a world keeps for one installed pack. Scripts hold no state in Luau; this is
/// where anything they remember lives, so a reload or a save loses nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PackState {
    /// Declared stores: script -> store -> key -> value.
    pub stores: BTreeMap<String, BTreeMap<String, BTreeMap<String, ScriptValue>>>,
    /// Activity triggers: `<script>.<trigger>` -> times fired.
    pub triggers: BTreeMap<String, u64>,
    /// Aborted handler calls and quarantine, per script.
    pub health: BTreeMap<String, ScriptHealth>,
    /// Revisions switched off by a rollback. Their handlers don't run.
    pub retired: BTreeSet<String>,
}

/// How a script's current revision has behaved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ScriptHealth {
    /// The revision these counts are for; a new revision starts clean.
    pub revision: String,
    pub aborts: u32,
    pub quarantined: bool,
    /// Earlier revisions of this script that were quarantined. Loading one of them
    /// again doesn't give it a clean start: it stays quarantined.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub quarantined_before: BTreeSet<String>,
}

impl ScriptHealth {
    /// The health `revision` starts with, given what was stored for this script.
    pub fn for_revision(stored: Option<&ScriptHealth>, revision: &str) -> ScriptHealth {
        let Some(stored) = stored else {
            return ScriptHealth {
                revision: revision.to_owned(),
                ..ScriptHealth::default()
            };
        };
        if stored.revision == revision {
            return stored.clone();
        }
        let mut before = stored.quarantined_before.clone();
        if stored.quarantined {
            before.insert(stored.revision.clone());
        }
        ScriptHealth {
            revision: revision.to_owned(),
            aborts: 0,
            quarantined: before.remove(revision),
            quarantined_before: before,
        }
    }
}
