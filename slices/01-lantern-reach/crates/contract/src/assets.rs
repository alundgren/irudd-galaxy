//! The asset manifest: parts, stages and variants by ID.
//!
//! Placeholder owned by W1, filled in by W6 (presentation). The sim only ever holds
//! [`PartId`]s and [`AssetId`]s; how they look is the renderer's business.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

/// `assets/manifest.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssetManifest {
    #[serde(default)]
    pub parts: Vec<PartDef>,
}

/// A station kit part a station can attach.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PartDef {
    pub id: PartId,
    pub name: String,
    /// Free-form part kind, e.g. `ring`, `dome`, `dock_arm`.
    pub kind: String,
    /// The visual recipe or sprite, if not drawn from the kind alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<AssetId>,
    #[serde(default)]
    pub variants: Vec<String>,
}
