//! Pilot traits and hull ranks: earned from deeds done many times, checked after
//! anything happens. No XP bars: the counts stay inside, only the fact and the effect
//! show.

use galaxy_contract::prelude::*;
use galaxy_core::History;

use crate::eval::{measure, player_ship};

/// Traits whose deed count has been reached, as commands: the trait and a
/// `trait_earned` fact with the count. Traits are never removed.
pub fn earned_traits(world: &WorldState, history: &History) -> Vec<Command> {
    let mut out = Vec::new();
    for (id, def) in &world.progress.rules.traits {
        if world.player.traits.contains(id) {
            continue;
        }
        let n = measure(world, history, &def.deed, None, None);
        if n >= def.times {
            out.push(Command::SetTrait {
                trait_id: id.clone(),
                on: true,
            });
            out.push(Command::Emit {
                event: Event::TraitEarned {
                    trait_id: id.clone(),
                    deed_count: n,
                },
            });
        }
    }
    out
}

/// The next rank of the hull the player flies, if its deed is done: the new record
/// (behind an `ExpectHullRecord` on the one it was worked out from) and a `hull_rank`
/// fact. One rank per check.
pub fn earned_rank(world: &WorldState, history: &History) -> Vec<Command> {
    let Some(ship) = player_ship(world) else {
        return Vec::new();
    };
    let stored = world.player.hull_records.get(&ship.hull).cloned();
    let record = stored.clone().unwrap_or_default();
    if record.until.is_some() {
        return Vec::new();
    }
    let Some(next) = world
        .progress
        .rules
        .ranks
        .iter()
        .find(|r| r.hull == ship.hull && r.rank == record.rank + 1)
    else {
        return Vec::new();
    };
    if measure(world, history, &next.deed, record.since, None) < next.at_least {
        return Vec::new();
    }
    // Only the rank changes, and only if the record is still the one read here: a
    // prestige committed earlier in the tick froze it, and must keep its berth.
    vec![
        Command::ExpectHullRecord {
            hull: ship.hull.clone(),
            record: stored,
        },
        Command::SetHullRecord {
            hull: ship.hull.clone(),
            record: Some(HullRecord {
                rank: next.rank,
                ..record
            }),
        },
        Command::Emit {
            event: Event::HullRank {
                hull: ship.hull.clone(),
                rank: next.rank,
            },
        },
    ]
}

/// A hull's rank as the player sees it: its painted stripes and their names.
pub fn stripes<'w>(world: &'w WorldState, hull: &HullId) -> Vec<&'w str> {
    let rank = world.player.hull_records.get(hull).map_or(0, |r| r.rank);
    world
        .progress
        .rules
        .ranks
        .iter()
        .filter(|r| &r.hull == hull && r.rank <= rank)
        .map(|r| r.name.as_str())
        .collect()
}
