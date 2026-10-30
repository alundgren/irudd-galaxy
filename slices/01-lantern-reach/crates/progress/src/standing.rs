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
///
/// Each deed is written as `ExpectStanding` then `AdjustStanding`. The expectation is
/// the band of standings the deed has the same effect from ([`same_effect`]), so a
/// batch worked out before another batch moved that faction's standing in the same
/// tick fails whole if it would now overshoot the range or name the wrong regard tier,
/// and still applies if the other change doesn't matter to it.
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

    /// Deeds for a batch that already holds `earlier` commands (a script call's
    /// buffer, a beat transition's rewards): their standing changes count first.
    pub fn after(world: &'w WorldState, earlier: &[Command]) -> Self {
        let mut deeds = Self::new(world);
        for c in earlier {
            if let Command::AdjustStanding { faction, delta } = c {
                let p = deeds.pending.entry(faction.clone()).or_insert(0);
                *p = p.saturating_add(*delta);
            }
        }
        deeds
    }

    /// The standing a faction will have once this batch applies.
    pub fn standing(&self, faction: &FactionId) -> Option<i32> {
        let base = self.world.factions.get(faction)?.standing;
        Some(base.saturating_add(self.pending.get(faction).copied().unwrap_or(0)))
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
        let after = before
            .saturating_add(scaled)
            .clamp(lo.min(before), hi.max(before));
        let applied = after - before;
        let (from, to) = (tier_of(self.world, before), tier_of(self.world, after));
        let (min, max) = same_effect(self.world, before, scaled, applied, (lo, hi), (from, to));
        self.commands.push(Command::ExpectStanding {
            faction: faction.clone(),
            min,
            max,
        });
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

/// The standings a tier spans (`None`: below the first tier).
fn tier_band(world: &WorldState, tier: Option<usize>) -> (i32, i32) {
    let regard = &world.progress.rules.regard;
    let start = |i: usize| regard.get(i).map(|t| t.min_standing);
    let lo = tier.and_then(start).unwrap_or(i32::MIN);
    let next = tier.map_or(0, |i| i + 1);
    let hi = start(next).map_or(i32::MAX, |m| m.saturating_sub(1));
    (lo, hi)
}

/// The standings `b` a deed worked out from `before` has the same effect from: `b`
/// in the tier `from`, `b + applied` in the tier `to`, and (unless the clamp cut the
/// deed, which only `before` itself gives) `b + scaled` inside the range.
fn same_effect(
    world: &WorldState,
    before: i32,
    scaled: i32,
    applied: i32,
    (lo, hi): (i32, i32),
    (from, to): (Option<usize>, Option<usize>),
) -> (i32, i32) {
    let unclamped = applied == scaled && (lo..=hi).contains(&before);
    if !unclamped {
        return (before, before);
    }
    let (f_lo, f_hi) = tier_band(world, from);
    let (t_lo, t_hi) = tier_band(world, to);
    let min = lo
        .saturating_sub(scaled)
        .max(lo)
        .max(f_lo)
        .max(t_lo.saturating_sub(applied));
    let max = hi
        .saturating_sub(scaled)
        .min(hi)
        .min(f_hi)
        .min(t_hi.saturating_sub(applied));
    // `before` is always in its own band; anything odd falls back to it alone.
    if (min..=max).contains(&before) {
        (min, max)
    } else {
        (before, before)
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

/// How many steps a deed rule has earned over the whole history: units carry over
/// between dockings until they make a step, and steps past `cap_per_docking` within
/// one docking (from one player `dock` fact to the next) are dropped, not saved up.
pub fn earned_steps(rule: &DeedRule, history: &History, player: Option<&ShipId>) -> u64 {
    let dock = EventKind::Dock.fact_kind();
    let (mut steps, mut units, mut this_docking) = (0u64, 0u64, 0u32);
    let mut step = |this_docking: &mut u32| {
        if rule.cap_per_docking.is_none_or(|c| *this_docking < c) {
            steps += 1;
            *this_docking += 1;
        }
    };
    for f in history.facts() {
        if f.kind == dock && player.is_some() && f.subjects.ship.as_ref() == player {
            this_docking = 0;
        }
        if !counts(rule, f, player) {
            continue;
        }
        match rule.per {
            Some(per) => {
                units += u64::try_from(f.amount.unwrap_or(0)).unwrap_or(0);
                while units >= per {
                    units -= per;
                    step(&mut this_docking);
                }
            }
            None => step(&mut this_docking),
        }
    }
    steps
}

/// Standing earned by deeds the facts of the last tick completed, as commands for one
/// batch. Stateless: what's due is the steps the rule has earned over history
/// ([`earned_steps`]) minus the `standing_changed` facts already written for it.
/// `earlier` is what applies ahead of the batch this tick (other systems' queued
/// batches), so the deeds are worked out from the standing they will meet.
pub fn due_deeds(world: &WorldState, history: &History, earlier: &[Command]) -> Vec<Command> {
    let new = last_tick_facts(world, history);
    if new.is_empty() {
        return Vec::new();
    }
    let player = world.player.ship.as_ref();
    let mut deeds = Deeds::after(world, earlier);
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
        let paid = history.count(&paid_q(None)) as u64;
        let steps = if rule.once {
            u64::from(paid == 0)
        } else {
            earned_steps(rule, history, player).saturating_sub(paid)
        };
        for _ in 0..steps {
            deeds.deed(&rule.faction, &rule.id, rule.delta, &rule.tags);
        }
    }
    deeds.commands
}
