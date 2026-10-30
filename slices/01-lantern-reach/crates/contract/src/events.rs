//! Events and activity counters.
//!
//! An event is emitted by a command batch (`Command::Emit`) and only goes out if the
//! batch commits. On commit the engine also appends a history fact for it (the event
//! is the fact's payload, and the envelope carries the fact ID so scripts and talk
//! lines can cite it) and bumps the activity counters listed by
//! [`Event::counter_bumps`].
//!
//! Counters count the player's activity: ships with `Owner::Player` (the flagship and
//! the player's automated craft). Each bump lands in the player scope, plus the
//! station and region the event happened at. NPC activity is in history, not counters.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::history::{FactKind, Subjects};
use crate::ids::*;
use crate::verbs::VerbKind;

/// Something that happened, with stable IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Dock {
        ship: ShipId,
        station: StationId,
    },
    Undock {
        ship: ShipId,
        station: StationId,
    },
    /// Units moved from a rock into a hold.
    Mine {
        ship: ShipId,
        rock: RockId,
        ware: WareId,
        amount: u32,
    },
    RockDepleted {
        rock: RockId,
        ship: Option<ShipId>,
    },
    Buy {
        ship: ShipId,
        station: StationId,
        ware: WareId,
        quantity: u32,
        /// The average unit price, in centicredits.
        unit_price: u32,
    },
    Sell {
        ship: ShipId,
        station: StationId,
        ware: WareId,
        quantity: u32,
        /// The average unit price, in centicredits.
        unit_price: u32,
    },
    /// Cargo handed over for a goal or a station's growth, not sold.
    Deliver {
        ship: ShipId,
        station: StationId,
        ware: WareId,
        quantity: u32,
        /// The delivery tally it counts toward (also the fact's `other` subject).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ledger: Option<LedgerId>,
    },
    Jump {
        ship: ShipId,
        lane: LaneId,
        from: RegionId,
        to: RegionId,
    },
    /// A price snapshot was taken at a station.
    Scout {
        ship: ShipId,
        station: StationId,
    },
    OrderDone {
        ship: ShipId,
        verb: VerbKind,
    },
    /// A beat's next step started (`quest` is the beat's ID).
    QuestStep {
        quest: QuestId,
        step: String,
    },
    TraitEarned {
        trait_id: TraitId,
        /// How many times the deed had been done.
        #[serde(default)]
        deed_count: u64,
    },
    Prestige {
        from_hull: HullId,
        to_hull: HullId,
        /// Centicredits paid to the hullwright.
        #[serde(default)]
        fee: i64,
    },
    /// A play session began (the app emits it when the game starts).
    SessionStart {},
    /// A beat's preconditions held and it started.
    BeatStarted {
        beat: BeatId,
        act: u32,
    },
    BeatCompleted {
        beat: BeatId,
    },
    UpgradeFitted {
        upgrade: UpgradeId,
        hull: HullId,
    },
    /// A control capability flag was set for the first time.
    CapUnlocked {
        cap: FlagId,
    },
    HullRank {
        hull: HullId,
        rank: u32,
    },
    /// A deed moved the player's standing. The number is never shown.
    StandingChanged {
        faction: FactionId,
        deed: AnyId,
        delta: i32,
    },
    /// The player's regard tier with a faction changed (tier IDs).
    RegardChanged {
        faction: FactionId,
        from: String,
        to: String,
    },
    /// A station built the player an automated craft.
    TenderAcquired {
        craft: ShipId,
        hull: HullId,
        station: StationId,
    },
    /// The PAUSED card opened. The app submits it every time the card opens; the sim
    /// doesn't tick while paused, so it commits on the first tick after the card
    /// closes. `session` is the `sessions` counter of the play session it happened in.
    Paused {
        session: u64,
    },
}

/// Event names, for subscriptions and filters.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Dock,
    Undock,
    Mine,
    RockDepleted,
    Buy,
    Sell,
    Deliver,
    Jump,
    Scout,
    OrderDone,
    QuestStep,
    TraitEarned,
    Prestige,
    SessionStart,
    BeatStarted,
    BeatCompleted,
    UpgradeFitted,
    CapUnlocked,
    HullRank,
    StandingChanged,
    RegardChanged,
    TenderAcquired,
    Paused,
}

impl EventKind {
    pub const ALL: [EventKind; 23] = [
        EventKind::Dock,
        EventKind::Undock,
        EventKind::Mine,
        EventKind::RockDepleted,
        EventKind::Buy,
        EventKind::Sell,
        EventKind::Deliver,
        EventKind::Jump,
        EventKind::Scout,
        EventKind::OrderDone,
        EventKind::QuestStep,
        EventKind::TraitEarned,
        EventKind::Prestige,
        EventKind::SessionStart,
        EventKind::BeatStarted,
        EventKind::BeatCompleted,
        EventKind::UpgradeFitted,
        EventKind::CapUnlocked,
        EventKind::HullRank,
        EventKind::StandingChanged,
        EventKind::RegardChanged,
        EventKind::TenderAcquired,
        EventKind::Paused,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::Dock => "dock",
            EventKind::Undock => "undock",
            EventKind::Mine => "mine",
            EventKind::RockDepleted => "rock_depleted",
            EventKind::Buy => "buy",
            EventKind::Sell => "sell",
            EventKind::Deliver => "deliver",
            EventKind::Jump => "jump",
            EventKind::Scout => "scout",
            EventKind::OrderDone => "order_done",
            EventKind::QuestStep => "quest_step",
            EventKind::TraitEarned => "trait_earned",
            EventKind::Prestige => "prestige",
            EventKind::SessionStart => "session_start",
            EventKind::BeatStarted => "beat_started",
            EventKind::BeatCompleted => "beat_completed",
            EventKind::UpgradeFitted => "upgrade_fitted",
            EventKind::CapUnlocked => "cap_unlocked",
            EventKind::HullRank => "hull_rank",
            EventKind::StandingChanged => "standing_changed",
            EventKind::RegardChanged => "regard_changed",
            EventKind::TenderAcquired => "tender_acquired",
            EventKind::Paused => "paused",
        }
    }

    /// The history fact kind for this event: the same name.
    pub const fn fact_kind(self) -> FactKind {
        FactKind::from_static(self.as_str())
    }
}

impl Event {
    pub fn kind(&self) -> EventKind {
        match self {
            Event::Dock { .. } => EventKind::Dock,
            Event::Undock { .. } => EventKind::Undock,
            Event::Mine { .. } => EventKind::Mine,
            Event::RockDepleted { .. } => EventKind::RockDepleted,
            Event::Buy { .. } => EventKind::Buy,
            Event::Sell { .. } => EventKind::Sell,
            Event::Deliver { .. } => EventKind::Deliver,
            Event::Jump { .. } => EventKind::Jump,
            Event::Scout { .. } => EventKind::Scout,
            Event::OrderDone { .. } => EventKind::OrderDone,
            Event::QuestStep { .. } => EventKind::QuestStep,
            Event::TraitEarned { .. } => EventKind::TraitEarned,
            Event::Prestige { .. } => EventKind::Prestige,
            Event::SessionStart {} => EventKind::SessionStart,
            Event::BeatStarted { .. } => EventKind::BeatStarted,
            Event::BeatCompleted { .. } => EventKind::BeatCompleted,
            Event::UpgradeFitted { .. } => EventKind::UpgradeFitted,
            Event::CapUnlocked { .. } => EventKind::CapUnlocked,
            Event::HullRank { .. } => EventKind::HullRank,
            Event::StandingChanged { .. } => EventKind::StandingChanged,
            Event::RegardChanged { .. } => EventKind::RegardChanged,
            Event::TenderAcquired { .. } => EventKind::TenderAcquired,
            Event::Paused { .. } => EventKind::Paused,
        }
    }

    /// The ship involved, if any (decides whether player counters move).
    pub fn ship(&self) -> Option<&ShipId> {
        match self {
            Event::Dock { ship, .. }
            | Event::Undock { ship, .. }
            | Event::Mine { ship, .. }
            | Event::Buy { ship, .. }
            | Event::Sell { ship, .. }
            | Event::Deliver { ship, .. }
            | Event::Jump { ship, .. }
            | Event::Scout { ship, .. }
            | Event::OrderDone { ship, .. } => Some(ship),
            Event::RockDepleted { ship, .. } => ship.as_ref(),
            Event::TenderAcquired { craft, .. } => Some(craft),
            Event::QuestStep { .. }
            | Event::TraitEarned { .. }
            | Event::Prestige { .. }
            | Event::SessionStart {}
            | Event::BeatStarted { .. }
            | Event::BeatCompleted { .. }
            | Event::UpgradeFitted { .. }
            | Event::CapUnlocked { .. }
            | Event::HullRank { .. }
            | Event::StandingChanged { .. }
            | Event::RegardChanged { .. }
            | Event::Paused { .. } => None,
        }
    }

    /// True for events that are about the player even without a ship.
    pub fn is_player_event(&self) -> bool {
        matches!(
            self,
            Event::QuestStep { .. }
                | Event::TraitEarned { .. }
                | Event::Prestige { .. }
                | Event::SessionStart {}
                | Event::BeatStarted { .. }
                | Event::BeatCompleted { .. }
                | Event::UpgradeFitted { .. }
                | Event::CapUnlocked { .. }
                | Event::HullRank { .. }
                | Event::StandingChanged { .. }
                | Event::RegardChanged { .. }
                | Event::Paused { .. }
        )
    }

    /// Indexed subjects for the history fact.
    pub fn subjects(&self) -> Subjects {
        let mut s = Subjects {
            ship: self.ship().cloned(),
            ..Subjects::default()
        };
        match self {
            Event::Dock { station, .. }
            | Event::Undock { station, .. }
            | Event::Scout { station, .. } => s.station = Some(station.clone()),
            Event::Mine { rock, ware, .. } => {
                s.rock = Some(rock.clone());
                s.ware = Some(ware.clone());
            }
            Event::RockDepleted { rock, .. } => s.rock = Some(rock.clone()),
            Event::Buy { station, ware, .. } | Event::Sell { station, ware, .. } => {
                s.station = Some(station.clone());
                s.ware = Some(ware.clone());
            }
            Event::Deliver {
                station,
                ware,
                ledger,
                ..
            } => {
                s.station = Some(station.clone());
                s.ware = Some(ware.clone());
                s.other = ledger.as_ref().and_then(|l| AnyId::new(l.as_str()).ok());
            }
            Event::Jump { to, lane, .. } => {
                s.region = Some(to.clone());
                s.other = AnyId::new(lane.as_str()).ok();
            }
            Event::QuestStep { quest, .. } => s.other = AnyId::new(quest.as_str()).ok(),
            Event::TraitEarned { trait_id, .. } => s.other = AnyId::new(trait_id.as_str()).ok(),
            Event::Prestige { to_hull, .. } => s.other = AnyId::new(to_hull.as_str()).ok(),
            Event::BeatStarted { beat, .. } | Event::BeatCompleted { beat } => {
                s.other = AnyId::new(beat.as_str()).ok();
            }
            Event::UpgradeFitted { upgrade, .. } => s.other = AnyId::new(upgrade.as_str()).ok(),
            Event::CapUnlocked { cap } => s.other = AnyId::new(cap.as_str()).ok(),
            Event::HullRank { hull, .. } => s.other = AnyId::new(hull.as_str()).ok(),
            Event::StandingChanged { faction, deed, .. } => {
                s.faction = Some(faction.clone());
                s.other = Some(deed.clone());
            }
            Event::RegardChanged { faction, .. } => s.faction = Some(faction.clone()),
            Event::TenderAcquired { station, .. } => s.station = Some(station.clone()),
            Event::OrderDone { .. } | Event::SessionStart {} | Event::Paused { .. } => {}
        }
        s
    }

    /// The fact's `amount`, for summing in queries.
    pub fn amount(&self) -> Option<i64> {
        match self {
            Event::Mine { amount, .. } => Some(i64::from(*amount)),
            Event::Buy { quantity, .. }
            | Event::Sell { quantity, .. }
            | Event::Deliver { quantity, .. } => Some(i64::from(*quantity)),
            Event::TraitEarned { deed_count, .. } => i64::try_from(*deed_count).ok(),
            Event::Prestige { fee, .. } => Some(*fee),
            Event::HullRank { rank, .. } => Some(i64::from(*rank)),
            Event::StandingChanged { delta, .. } => Some(i64::from(*delta)),
            _ => None,
        }
    }

    /// Which counters this event moves, and by how much.
    pub fn counter_bumps(&self) -> Vec<(CounterKind, u64)> {
        use CounterKind as C;
        match self {
            Event::Dock { .. } => vec![(C::Dockings, 1)],
            Event::Undock { .. } => vec![],
            Event::Mine { amount, .. } => vec![(C::UnitsMined, u64::from(*amount))],
            Event::RockDepleted { .. } => vec![(C::RocksDepleted, 1)],
            Event::Buy { .. } => vec![(C::Purchases, 1)],
            Event::Sell { .. } => vec![(C::Sales, 1), (C::Deliveries, 1)],
            Event::Deliver { ledger: None, .. } => vec![(C::Deliveries, 1)],
            Event::Deliver {
                ledger: Some(_), ..
            } => vec![(C::Deliveries, 1), (C::LedgerDeliveries, 1)],
            Event::Jump { .. } => vec![(C::Jumps, 1)],
            Event::Scout { .. } => vec![(C::Scouts, 1)],
            Event::OrderDone { .. } => vec![(C::OrdersDone, 1)],
            Event::SessionStart {} => vec![(C::Sessions, 1)],
            Event::QuestStep { .. }
            | Event::TraitEarned { .. }
            | Event::Prestige { .. }
            | Event::BeatStarted { .. }
            | Event::BeatCompleted { .. }
            | Event::UpgradeFitted { .. }
            | Event::CapUnlocked { .. }
            | Event::HullRank { .. }
            | Event::StandingChanged { .. }
            | Event::RegardChanged { .. }
            | Event::TenderAcquired { .. }
            | Event::Paused { .. } => vec![],
        }
    }
}

/// An emitted event as subscribers see it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventEnvelope {
    pub tick: Tick,
    /// The history fact recorded for this event; cite it.
    pub fact: FactId,
    pub cause: CauseId,
    pub event: Event,
}

/// Activity counters. Pacing reads these, never a clock.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CounterKind {
    Dockings,
    /// Sales and deliveries, one per transaction.
    Deliveries,
    Jumps,
    Sessions,
    UnitsMined,
    RocksDepleted,
    Sales,
    Purchases,
    Scouts,
    OrdersDone,
    /// Handovers to a ledger (goal deliveries, station growth, dispute tallies).
    LedgerDeliveries,
}

/// Where a counter is read.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum CounterScope {
    Player,
    Station { station: StationId },
    Region { region: RegionId },
}

pub type CounterSet = BTreeMap<CounterKind, u64>;

/// Read-only activity counters per player, station and region.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityCounters {
    pub player: CounterSet,
    pub stations: BTreeMap<StationId, CounterSet>,
    pub regions: BTreeMap<RegionId, CounterSet>,
}

impl ActivityCounters {
    pub fn get(&self, kind: CounterKind, scope: &CounterScope) -> u64 {
        let set = match scope {
            CounterScope::Player => Some(&self.player),
            CounterScope::Station { station } => self.stations.get(station),
            CounterScope::Region { region } => self.regions.get(region),
        };
        set.and_then(|s| s.get(&kind)).copied().unwrap_or(0)
    }

    pub fn bump(&mut self, kind: CounterKind, scope: &CounterScope, by: u64) {
        let set = match scope {
            CounterScope::Player => &mut self.player,
            CounterScope::Station { station } => self.stations.entry(station.clone()).or_default(),
            CounterScope::Region { region } => self.regions.entry(region.clone()).or_default(),
        };
        *set.entry(kind).or_insert(0) += by;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_names_match_serde() {
        let e = Event::RockDepleted {
            rock: RockId::new("rock.a.b").unwrap(),
            ship: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], e.kind().as_str());
        for k in EventKind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.as_str());
        }
    }
}
