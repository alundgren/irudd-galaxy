//! Hints taught once, then out of the way.
//!
//! Each context hint at the bottom of the screen teaches one thing. An action hint
//! ("Space · dock at ...") shows until the player has done that action a few times; a
//! notice ("Your hold is full", the mining-speed readout) shows until it has been seen a
//! few times. How many is data: `retire_after` in the core pack's `data/hints.toml`.
//!
//! The counts belong to the save, not the world: they live in the app's own table in
//! the save file ([`crate::saves::read_hints`]), so learning a hint changes nothing the
//! sim or a replay sees. The session counts from the flagship's state between ticks
//! ([`HintBook::observe`]), so play, `galaxy shot` and the tests count the same way.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_contract::prelude::*;
use serde::Deserialize;

/// What a context hint teaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HintKind {
    /// "Space · dock at ...": retires after a few dockings.
    Dock,
    /// "Space · ride the lantern current ...": after a few rides.
    Ride,
    /// "Hold Space · mine ...": after a few mining runs.
    Mine,
    /// "Mining ice · 1 a second": after a few runs at that rate, so a fitted lens shows
    /// its new rate a few times too.
    MiningRate,
    /// "Your hold is full": after it has been seen a few times.
    HoldFull,
}

impl HintKind {
    pub const ALL: [HintKind; 5] = [
        HintKind::Dock,
        HintKind::Ride,
        HintKind::Mine,
        HintKind::MiningRate,
        HintKind::HoldFull,
    ];

    /// Its name in `hints.toml` and in the save's record.
    pub fn name(self) -> &'static str {
        match self {
            HintKind::Dock => "dock",
            HintKind::Ride => "ride_current",
            HintKind::Mine => "mine",
            HintKind::MiningRate => "mining_rate",
            HintKind::HoldFull => "hold_full",
        }
    }

    /// A notice is learned by seeing it; the others by doing what they say.
    fn is_notice(self) -> bool {
        matches!(self, HintKind::MiningRate | HintKind::HoldFull)
    }
}

/// How many times a hint teaches when `hints.toml` doesn't say.
const DEFAULT_RETIRE_AFTER: u32 = 3;

/// How many times each hint teaches before it retires, from `data/hints.toml`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HintRules {
    #[serde(default)]
    pub retire_after: BTreeMap<String, u32>,
}

impl HintRules {
    pub const FILE: &'static str = "hints.toml";

    /// Reads `hints.toml` from a pack's data folder. No file: every hint teaches
    /// [`DEFAULT_RETIRE_AFTER`] times.
    pub fn load(data: &Path) -> Result<Self, String> {
        let path = data.join(Self::FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let rules: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for name in rules.retire_after.keys() {
            if !HintKind::ALL.iter().any(|k| k.name() == name) {
                let known: Vec<_> = HintKind::ALL.iter().map(|k| k.name()).collect();
                return Err(format!(
                    "`{name}` isn't a hint (they are {})",
                    known.join(", ")
                ));
            }
        }
        Ok(rules)
    }

    pub fn retire_after(&self, kind: HintKind) -> u32 {
        self.retire_after
            .get(kind.name())
            .copied()
            .unwrap_or(DEFAULT_RETIRE_AFTER)
    }
}

/// What the flagship was doing, to spot a hint's action between two ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Acts {
    docked: bool,
    riding: bool,
    mining: bool,
}

impl Acts {
    pub fn of(world: &WorldState) -> Self {
        let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
            return Self::default();
        };
        Self {
            docked: ship.docked_at.is_some(),
            riding: ship.activity.transit.is_some(),
            mining: ship.activity.mining.is_some(),
        }
    }
}

/// How far each hint has been taught in this save, and which notice is up now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HintBook {
    rules: HintRules,
    /// Times taught, by record key: the hint's name, or for the mining readout its
    /// name and rate (`mining_rate:1.5`).
    counts: BTreeMap<String, u32>,
    /// The notice in sight now, by key.
    sighting: Option<String>,
    /// The notice in sight now, if this sighting counted: it stays up until it ends,
    /// even if it was the last one.
    teaching: Option<String>,
    /// Counts changed since [`HintBook::take_changed`].
    changed: bool,
}

impl HintBook {
    pub fn new(rules: HintRules, counts: BTreeMap<String, u32>) -> Self {
        Self {
            rules,
            counts,
            ..Self::default()
        }
    }

    /// The record for the save.
    pub fn counts(&self) -> &BTreeMap<String, u32> {
        &self.counts
    }

    /// Whether the counts changed since the last call (time to write them).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn count(&self, key: &str) -> u32 {
        self.counts.get(key).copied().unwrap_or(0)
    }

    fn bump(&mut self, key: &str) {
        *self.counts.entry(key.to_owned()).or_default() += 1;
        self.changed = true;
    }

    /// Whether `kind` has taught enough and stays hidden now.
    pub fn retired(&self, kind: HintKind, world: &WorldState) -> bool {
        let key = record_key(kind, world);
        self.teaching.as_deref() != Some(key.as_str())
            && self.count(&key) >= self.rules.retire_after(kind)
    }

    /// After a tick: counts the hint actions the flagship did (`before` is how it was
    /// before the tick) and a notice that just came into sight.
    pub fn observe(&mut self, before: Acts, world: &WorldState) {
        let now = Acts::of(world);
        if now.docked && !before.docked {
            self.bump(HintKind::Dock.name());
        }
        if now.riding && !before.riding {
            self.bump(HintKind::Ride.name());
        }
        if now.mining && !before.mining {
            self.bump(HintKind::Mine.name());
        }
        let notice = super::hint_kind(world)
            .filter(|k| k.is_notice())
            .map(|k| (k, record_key(k, world)));
        let key = notice.as_ref().map(|(_, key)| key.clone());
        if key != self.sighting {
            self.teaching = None;
            if let Some((kind, key)) = notice
                && self.count(&key) < self.rules.retire_after(kind)
            {
                self.bump(&key);
                self.teaching = Some(key);
            }
            self.sighting = key;
        }
    }
}

/// The key a hint's count is kept under.
fn record_key(kind: HintKind, world: &WorldState) -> String {
    match (kind, super::mining_rate_text(world)) {
        (HintKind::MiningRate, Some(rate)) => format!("{}:{rate}", kind.name()),
        _ => kind.name().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_read_the_data_and_refuse_unknown_hints() {
        let r = HintRules::parse("[retire_after]\ndock = 5\n").unwrap();
        assert_eq!(r.retire_after(HintKind::Dock), 5);
        assert_eq!(r.retire_after(HintKind::Mine), DEFAULT_RETIRE_AFTER);
        assert!(HintRules::parse("[retire_after]\nwarp = 2\n").is_err());
        assert!(HintRules::parse("retire = 2\n").is_err());
    }

    #[test]
    fn the_core_pack_lists_every_hint() {
        let data =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let r = HintRules::load(&data).unwrap();
        for k in HintKind::ALL {
            assert!(
                r.retire_after.contains_key(k.name()),
                "{} is missing",
                k.name()
            );
        }
    }
}
