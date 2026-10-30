//! Authored canon and explicit, reversible revelations.
use crate::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanonNode {
    pub id: String,
    pub status: CanonStatus,
    pub statement: String,
    pub opener: Option<String>,
    pub requires: Option<BeatId>,
    pub during: Option<BeatId>,
    pub step: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanonStatus {
    Locked,
    Sealed,
    Open,
    Question,
    Forbidden,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CanonState {
    #[serde(default)]
    pub edges: Vec<CanonEdge>,
    pub nodes: BTreeMap<String, CanonNode>,
}
#[derive(Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanonFile {
    #[serde(default)]
    pub edge: Vec<CanonEdge>,
    pub node: Vec<CanonNode>,
}
impl CanonState {
    pub fn may_open(&self, id: &str, pack: &PackId, world: &WorldState) -> Result<(), String> {
        let n = self
            .nodes
            .get(id)
            .ok_or_else(|| format!("unknown canon node {id}"))?;
        if !matches!(n.status, CanonStatus::Sealed | CanonStatus::Open) {
            return Err(format!("{id} cannot be opened"));
        }
        let allowed = if pack.as_str() == "core" {
            n.opener.as_deref().is_some_and(|o| o.starts_with("core:"))
        } else {
            n.opener.as_deref() == Some("pack")
        };
        if !allowed {
            return Err(format!("{pack} is not the opener of {id}"));
        }
        if let Some(b) = &n.requires
            && !world
                .progress
                .beats
                .get(b)
                .is_some_and(|s| s.status == BeatStatus::Completed)
        {
            return Err(format!("{id} requires completed {b}"));
        }
        if let Some(beat) = &n.during
            && !world.progress.beats.get(beat).is_some_and(|b| {
                b.status == BeatStatus::Active && n.step.is_none_or(|s| s == b.step)
            })
        {
            return Err(format!("{id} requires its authored beat step"));
        }
        for edge in self
            .edges
            .iter()
            .filter(|e| e.to == id && e.rel == "supports")
        {
            if !self
                .nodes
                .get(&edge.from)
                .is_some_and(|n| matches!(n.status, CanonStatus::Locked | CanonStatus::Open))
            {
                return Err(format!("{id} requires revealed {}", edge.from));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CanonEdge {
    pub from: String,
    pub to: String,
    pub rel: String,
}

impl Default for CanonState {
    fn default() -> Self {
        let data: CanonFile = toml::from_str(include_str!("../../../packs/core/data/canon.toml"))
            .expect("bundled canon");
        Self {
            nodes: data.node.into_iter().map(|n| (n.id.clone(), n)).collect(),
            edges: data.edge,
        }
    }
}
