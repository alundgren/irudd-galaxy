//! Saved map drawings and runtime assets. These contain no renderer state.
use crate::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapOverlay {
    pub region: RegionId,
    pub center: Vec2,
    pub radius: f32,
    pub owner: Option<FactionId>,
    #[serde(default)]
    pub contested: bool,
    /// A bearing has a direction, never a destination.
    pub direction: Option<Vec2>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyBuoy {
    #[serde(default)]
    pub reward: Option<SurveyReward>,
    pub region: RegionId,
    pub position: Vec2,
    pub radius: f32,
    pub planted_by: FactionId,
    #[serde(default)]
    pub read: bool,
}

/// A player-claimed opportunity. The native reply pays it, so rollback keeps earnings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurveyReward {
    pub person: NpcId,
    pub choice: AnyId,
    pub credits: u32,
    pub standing: std::collections::BTreeMap<FactionId, i32>,
    #[serde(default)]
    pub claimed: bool,
}
