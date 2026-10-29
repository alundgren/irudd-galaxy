//! The fast-model layer's data: models, limits, per-region budgets and NPC briefs.
//!
//! Lives in the core pack as `data/mind.toml`. Every number here is tunable pack data,
//! not an engine constant. The game master will set briefs and budgets through the
//! `Mind` hook later; until then they come from this file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use galaxy_contract::ids::{AnyId, NpcId, RegionId};
use serde::{Deserialize, Serialize};

use crate::menu::{DOCK_TALK, KINDS, RUMOR_SPREAD, TRADER_ROUTE};

/// The data file name under a pack's `data/`.
pub const MIND_FILE: &str = "mind.toml";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindConfig {
    #[serde(default)]
    pub models: Models,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub budget: Budgets,
    #[serde(default)]
    pub talk: TalkRules,
    /// One brief per NPC or craft the layer may speak or decide for.
    #[serde(default)]
    pub brief: Vec<Brief>,
}

/// OpenRouter model IDs, swappable without code changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Models {
    /// Typed choices through the Decisions API.
    pub decision: String,
    /// Short lines through chat completions with a JSON schema.
    pub line: String,
}

impl Default for Models {
    fn default() -> Self {
        Self {
            decision: "typesafe/jev-1.14".into(),
            line: "openai/gpt-oss-20b".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// Ticks the sim waits for an answer before it takes the utility default. Ticks
    /// don't pass while paused, so nothing is lost to a pause.
    pub deadline_ticks: u64,
    /// Requests queued or running at once; more are answered by the default.
    pub max_in_flight: usize,
    /// Options a menu offers the model (the utility top N, at most 9).
    pub menu_size: usize,
    /// Most history facts in a grounding packet.
    pub packet_facts: usize,
    /// Largest packet sent, in bytes (Jev slows down above about 32 KB).
    pub packet_bytes: usize,
    /// HTTP timeout per attempt, in milliseconds.
    pub timeout_ms: u64,
    /// Token cap for a line call (the line model reasons before it answers).
    pub line_max_tokens: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            deadline_ticks: 150,
            max_in_flight: 8,
            menu_size: 4,
            packet_facts: 12,
            packet_bytes: 24_000,
            timeout_ms: 6_000,
            line_max_tokens: 600,
        }
    }
}

/// Call budgets. Nothing refills on a clock: a region's window is counted in the
/// player's dockings, so the layer spends where the player is active.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Hard cap on the whole save's spend, in US dollars.
    pub spend_cap_usd: f64,
    /// For regions without their own entry.
    pub default: Budget,
    #[serde(default)]
    pub region: BTreeMap<RegionId, Budget>,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            spend_cap_usd: 1.0,
            default: Budget::default(),
            region: BTreeMap::new(),
        }
    }
}

impl Budgets {
    pub fn for_region(&self, region: Option<&RegionId>) -> &Budget {
        region
            .and_then(|r| self.region.get(r))
            .unwrap_or(&self.default)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Model calls per window in this region. 0 turns the layer off there.
    pub calls: u32,
    /// The window, in the player's dockings. 0 means it never refills.
    pub window_dockings: u32,
    /// Hard cap on this region's total spend, in US dollars.
    pub spend_cap_usd: f64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            calls: 30,
            window_dockings: 10,
            spend_cap_usd: 0.25,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TalkRules {
    /// Let the decision model pick among eligible dock-talk lines (else the most
    /// specific line wins).
    pub pick: bool,
    /// Let the line model rephrase the picked line in the NPC's voice.
    pub phrase: bool,
    /// Default word cap for a line; a brief may lower it.
    pub max_words: u32,
    pub max_chars: u32,
    /// Terms no fast-model line may contain (canon only the game master touches).
    pub forbidden: Vec<String>,
    /// Words that say something happened. A rephrased line may use one (or its `-s`,
    /// `-ed`, `-ing` form) only if the authored line or a cited fact does, so a model
    /// can't add an event the record doesn't hold.
    pub claims: Vec<String>,
}

/// The claim words used when `talk.claims` isn't set.
pub const DEFAULT_CLAIMS: &[&str] = &[
    "attack", "ambush", "arrest", "bankrupt", "blockade", "bought", "bounty", "built", "burn",
    "buy", "crash", "dead", "debt", "deliver", "destroy", "die", "discover", "explode", "kill",
    "murder", "owe", "paid", "pay", "pirate", "raid", "reward", "sabotage", "sell", "smuggle",
    "sold", "steal", "stole", "stolen", "theft", "thief", "war", "wreck",
];

impl Default for TalkRules {
    fn default() -> Self {
        Self {
            pick: true,
            phrase: true,
            max_words: 20,
            max_chars: 160,
            forbidden: Vec::new(),
            claims: DEFAULT_CLAIMS.iter().map(|c| (*c).to_owned()).collect(),
        }
    }
}

/// What the game master sets for one NPC or craft (research page, "The game master's
/// brief for each NPC").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    /// `npc.*` for a named person, `craft.*` for a ship.
    pub id: AnyId,
    /// Bumped on every change; part of the situation key, so a new brief never reuses
    /// an old cached answer.
    pub revision: u32,
    pub name: String,
    pub role: String,
    /// The named person behind a craft, if any (for `Cause::Mind { npc }`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc: Option<NpcId>,
    #[serde(default)]
    pub goals: Vec<String>,
    /// A few numeric traits, 0 to 1 (greed, caution, warmth ...).
    #[serde(default)]
    pub temperament: BTreeMap<String, f32>,
    /// Menu kinds this brief may decide: `trader_route`, `dock_talk`, `rumor_spread`.
    #[serde(default)]
    pub allowed: Vec<String>,
    pub voice: String,
    /// Word cap for this voice (default: `talk.max_words`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_words: Option<u32>,
    /// Fact kinds this NPC remembers; empty means any fact about them or their place.
    #[serde(default)]
    pub knows: Vec<String>,
    /// Topics only the game master may touch.
    #[serde(default)]
    pub off_limits: Vec<String>,
}

impl Brief {
    pub fn allows(&self, kind: &str) -> bool {
        self.allowed.iter().any(|k| k == kind)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Toml {
        path: String,
        source: Box<toml::de::Error>,
    },
    #[error("{0}")]
    Invalid(String),
}

impl MindConfig {
    /// Loads `data/mind.toml` from a pack's data folder. A missing file gives the
    /// defaults with no briefs, so the layer stays quiet.
    pub fn load_dir(data_dir: &Path) -> Result<Self, ConfigError> {
        let path = data_dir.join(MIND_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path).map_err(|source| ConfigError::Io {
            path: MIND_FILE.into(),
            source,
        })?;
        Self::from_toml(&text).map_err(|e| match e {
            ConfigError::Toml { source, .. } => ConfigError::Toml {
                path: MIND_FILE.into(),
                source,
            },
            e => e,
        })
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|e| ConfigError::Toml {
            path: MIND_FILE.into(),
            source: Box::new(e),
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn brief(&self, id: &str) -> Option<&Brief> {
        self.brief.iter().find(|b| b.id.as_str() == id)
    }

    /// Checks what serde can't: unique briefs, known menu kinds, sane limits.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let bad = |m: String| Err(ConfigError::Invalid(m));
        let mut seen = BTreeSet::new();
        for b in &self.brief {
            if !seen.insert(b.id.as_str()) {
                return bad(format!("brief `{}` is listed twice", b.id));
            }
            if b.revision == 0 {
                return bad(format!("brief `{}`: revision starts at 1", b.id));
            }
            if let Some(k) = b.allowed.iter().find(|k| !KINDS.contains(&k.as_str())) {
                return bad(format!(
                    "brief `{}`: unknown menu kind `{k}` (known: {TRADER_ROUTE}, {DOCK_TALK}, {RUMOR_SPREAD})",
                    b.id
                ));
            }
            if let Some((k, v)) = b
                .temperament
                .iter()
                .find(|(_, v)| !(0.0..=1.0).contains(*v))
            {
                return bad(format!(
                    "brief `{}`: temperament `{k}` = {v} is outside 0..1",
                    b.id
                ));
            }
        }
        let l = &self.limits;
        if !(1..=9).contains(&l.menu_size) {
            return bad("limits.menu_size must be 1 to 9".into());
        }
        if l.max_in_flight == 0 || l.timeout_ms == 0 {
            return bad("limits.max_in_flight and limits.timeout_ms must be above 0".into());
        }
        let caps = std::iter::once(("spend_cap_usd", self.budget.spend_cap_usd))
            .chain(std::iter::once((
                "default.spend_cap_usd",
                self.budget.default.spend_cap_usd,
            )))
            .chain(
                self.budget
                    .region
                    .values()
                    .map(|b| ("region.*.spend_cap_usd", b.spend_cap_usd)),
            );
        for (name, v) in caps {
            if !v.is_finite() || v < 0.0 {
                return bad(format!("budget.{name} must be a finite amount >= 0"));
            }
        }
        if self.talk.max_words == 0 || self.talk.max_chars == 0 {
            return bad("talk.max_words and talk.max_chars must be above 0".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_pack_mind_file_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let config = MindConfig::load_dir(&dir).expect("core mind.toml is valid");
        assert!(config.brief.len() >= 14, "ten NPCs and four traders");
        assert!(config.brief("craft.ninefold").unwrap().allows(TRADER_ROUTE));
        assert!(config.brief("npc.ilse_marrow").unwrap().allows(DOCK_TALK));
    }

    #[test]
    fn rejects_unknown_kind_and_duplicates() {
        let base = r#"
[[brief]]
id = "npc.a"
revision = 1
name = "A"
role = "dockmaster"
voice = "plain"
allowed = ["dock_talk"]
"#;
        assert!(MindConfig::from_toml(base).is_ok());
        let twice = format!("{base}{base}");
        assert!(MindConfig::from_toml(&twice).is_err());
        let unknown = base.replace("dock_talk", "dance");
        assert!(MindConfig::from_toml(&unknown).is_err());
        let typo = base.replace("voice", "vioce");
        assert!(MindConfig::from_toml(&typo).is_err());
    }
}
