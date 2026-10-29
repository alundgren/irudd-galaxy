//! Pack manifests and validation reports.
//!
//! Placeholder owned by W1, filled in by W5. A pack is a folder with `manifest.toml`,
//! Luau scripts and TOML data. Every validation step writes into one
//! [`ValidationReport`], whose format is part of the contract.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

/// `manifest.toml` at the root of a pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PackManifest {
    pub id: PackId,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Hook namespaces the pack's scripts may use, e.g. `events.dock`, `stations.write`.
    #[serde(default)]
    pub uses: Vec<String>,
    /// Script files, relative to the pack root.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Data files, relative to the pack root.
    #[serde(default)]
    pub data: Vec<String>,
    #[serde(default)]
    pub depends: Vec<PackId>,
}

/// The result of validating (and maybe applying) a pack revision.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationReport {
    pub pack: PackId,
    pub revision: String,
    pub status: ReportStatus,
    pub steps: Vec<StepReport>,
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

/// The validation pipeline, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStep {
    Types,
    Canon,
    Invariants,
    Sandbox,
    Legibility,
    Stage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StepReport {
    pub step: ValidationStep,
    pub passed: bool,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub severity: Severity,
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
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}
