//! Faction standing and regard.
//!
//! Standing is a hidden number per faction. Players only ever see its regard tier
//! (how people treat them), so this module hands out tiers for UI and talk and writes
//! every change as a deed: `AdjustStanding` plus a `standing_changed` fact naming the
//! deed, and a `regard_changed` fact when the tier moves.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_contract::progress::{DeedRule, RegardTier};
use galaxy_core::History;

/// The tier a standing falls in (the last whose `min_standing` it reaches).
pub fn tier_of(world: &WorldState, standing: i32) -> Option<usize> {
    world
        .progress
        .rules
        .regard
        .iter()
        .rposition(|t| t.min_standing <= standing)
}

/// The player's regard tier index with a faction.
pub fn tier_index(world: &WorldState, faction: &FactionId) -> Option<usize> {
    world
        .factions
        .get(faction)
        .and_then(|f| tier_of(world, f.standing))
}

/// The player's regard tier with a faction.
pub fn tier<'w>(world: &'w WorldState, faction: &FactionId) -> Option<&'w RegardTier> {
    tier_index(world, faction).map(|i| &world.progress.rules.regard[i])
}

/// The regard tier of a station's owner, for dock talk and prices (`None` when the
/// station has no owner).
pub fn station_tier<'w>(world: &'w WorldState, station: &StationId) -> Option<&'w RegardTier> {
    let owner = world.stations.get(station)?.owner.as_ref()?;
    tier(world, owner)
}

/// Standing changes for one batch. Tracks what earlier deeds in the batch did, so the
/// clamp and the tier change are right when several deeds touch one faction.
pub struct Deeds<'w> {
    world: &'w WorldState,
    pending: BTreeMap<FactionId, i32>,
    pub commands: Vec<Command>,
}

impl<'w> Deeds<'w> {
    pub fn new(world: &'w WorldState) -> Self {
        Self {
            world,
            pending: BTreeMap::new(),
            commands: Vec::new(),
        }
    }

    /// The standing a faction will have once this batch applies.
    pub fn standing(&self, faction: &FactionId) -> Option<i32> {
        let base = self.world.factions.get(faction)?.standing;
        Some(base + self.pending.get(faction).copied().unwrap_or(0))
    }

    /// Records one deed. Gains are scaled by traits that boost `tags` (rounded half
    /// away from zero); the result is clamped to the standing range. Returns the
    /// delta actually applied (`None` for an unknown faction).
    pub fn deed(
        &mut self,
        faction: &FactionId,
        deed: &AnyId,
        delta: i32,
        tags: &[String],
    ) -> Option<i32> {
        let before = self.standing(faction)?;
        let rules = &self.world.progress.rules;
        let scaled = if delta > 0 && !tags.is_empty() {
            (f64::from(delta) * crate::modifiers::deed_bonus(self.world, tags)).round() as i32
        } else {
            delta
        };
        let (lo, hi) = if rules.standing_min <= rules.standing_max {
            (rules.standing_min, rules.standing_max)
        } else {
            (-100, 100)
        };
        let after = before.saturating_add(scaled).clamp(lo.min(before), hi.max(before));
        let applied = after - before;
        if applied != 0 {
            self.commands.push(Command::AdjustStanding {
                faction: faction.clone(),
                delta: applied,
            });
        }
        self.commands.push(Command::Emit {
            event: Event::StandingChanged {
                faction: faction.clone(),
                deed: deed.clone(),
                delta: applied,
            },
        });
        let (from, to) = (tier_of(self.world, before), tier_of(self.world, after));
        if from != to
            && let (Some(f), Some(t)) = (from, to)
        {
            self.commands.push(Command::Emit {
                event: Event::RegardChanged {
                    faction: faction.clone(),
                    from: rules.regard[f].id.clone(),
                    to: rules.regard[t].id.clone(),
                },
            });
        }
        *self.pending.entry(faction.clone()).or_insert(0) += applied;
        Some(applied)
    }
}

fn one_of<T: PartialEq>(list: &[T], have: Option<&T>) -> bool {
    list.is_empty() || have.is_some_and(|h| list.contains(h))
}

/// Does a deed rule count this fact?
pub fn counts(rule: &DeedRule, fact: &Fact, player: Option<&ShipId>) -> bool {
    let s = &fact.subjects;
    rule.on.kinds.contains(&fact.kind)
        && one_of(&rule.on.stations, s.station.as_ref())
        && one_of(&rule.on.wares, s.ware.as_ref())
        && one_of(&rule.on.others, s.other.as_ref())
        && (rule.on.any_ship || (player.is_some() && s.ship.as_ref() == player))
}

/// The facts committed on the last tick: what the deed, trait and rank checks react to.
pub fn last_tick_facts<'h>(world: &WorldState, history: &'h History) -> Vec<&'h Fact> {
    let t = world.meta.tick;
    if t.0 == 0 {
        return Vec::new();
    }
    history.find(&HistoryQuery {
        since: Some(t),
        until: Some(t),
        ..HistoryQuery::default()
    })
}

/// Standing earned by deeds the facts of the last tick completed, as commands for one
/// batch. Stateless: what's due is worked out from history (the deed's matching facts,
/// within the player's latest docking when the rule has a per-docking cap) minus the
/// `standing_changed` facts already written for it.
pub fn due_deeds(world: &WorldState, history: &History) -> Vec<Command> {
    let new = last_tick_facts(world, history);
    if new.is_empty() {
        return Vec::new();
    }
    let player = world.player.ship.as_ref();
    let mut deeds = Deeds::new(world);
    let mut done = BTreeSet::new();
    for rule in &world.progress.rules.deeds {
        if !done.insert(rule.id.clone()) || !new.iter().any(|f| counts(rule, f, player)) {
            continue;
        }
        let paid_q = |after: Option<FactId>| HistoryQuery {
            kind: Some(EventKind::StandingChanged.fact_kind()),
            other: Some(rule.id.clone()),
            after,
            ..HistoryQuery::default()
        };
        let steps = if rule.once {
            u64::from(history.last(&paid_q(None)).is_none())
        } else {
            // The window: from the player's latest docking (inclusive) when capped.
            let after = rule.cap_per_docking.and_then(|_| {
                history
                    .last(&HistoryQuery {
                        kind: Some(EventKind::Dock.fact_kind()),
                        ship: player.cloned(),
                        ..HistoryQuery::default()
                    })
                    .map(|d| FactId(d.id.0.saturating_sub(1)))
            });
            let matching = history
                .find(&HistoryQuery {
                    after,
                    ..HistoryQuery::default()
                })
                .into_iter()
                .filter(|f| counts(rule, f, player));
            let earned = match rule.per {
                Some(per) => {
                    let units: i64 = matching.filter_map(|f| f.amount).sum();
                    u64::try_from(units).unwrap_or(0) / per
                }
                None => matching.count() as u64,
            };
            let earned = rule
                .cap_per_docking
                .map_or(earned, |c| earned.min(u64::from(c)));
            let paid = history.count(&paid_q(after)) as u64;
            earned.saturating_sub(paid)
        };
        for _ in 0..steps {
            deeds.deed(&rule.faction, &rule.id, rule.delta, &rule.tags);
        }
    }
    deeds.commands
}
