//! Rocks regrow with activity, never with time.
//!
//! A region's activity is its weighted activity counters (`ship_rules.regrowth.pulses`,
//! dockings and lane arrivals by default): each unit is one region pulse. A field
//! regrows `regrow` units per pulse, shared evenly by its rocks; a rock being mined
//! doesn't regrow. A spent rock stays spent until `reform` pulses after it was spent,
//! then comes back at `reform_share` of its full yield. Depletion persists in saves.

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};

pub struct RegrowthSystem;

/// The region's activity in pulses.
pub fn region_pulses(world: &WorldState, region: &RegionId) -> u64 {
    let scope = CounterScope::Region {
        region: region.clone(),
    };
    world
        .ship_rules
        .regrowth
        .pulses
        .iter()
        .map(|p| {
            world
                .counters
                .get(p.counter, &scope)
                .saturating_mul(p.weight)
        })
        .fold(0, u64::saturating_add)
}

impl System for RegrowthSystem {
    fn name(&self) -> &'static str {
        "rock_regrowth"
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world = ctx.world;
        let reform_share = world.ship_rules.regrowth.reform_share;
        let mut pulses = std::collections::BTreeMap::new();
        for rock in world.rocks.values() {
            let Some(field) = world.fields.get(&rock.field) else {
                continue;
            };
            let now = *pulses
                .entry(field.region.clone())
                .or_insert_with(|| region_pulses(world, &field.region));
            let g = &rock.growth;
            let cause = || Cause::system("rock_regrowth", format!("{} regrows", rock.id));
            let set = |growth: RockGrowth| Command::SetRockGrowth {
                rock: rock.id.clone(),
                growth,
            };

            if rock.remaining == 0 {
                match (g.spent_at, field.reform) {
                    (None, _) => ctx.submit(CommandBatch::new(cause()).with(set(RockGrowth {
                        mark: now,
                        spent_at: Some(now),
                    }))),
                    (Some(at), Some(reform)) if now.saturating_sub(at) >= reform => {
                        let units = ((rock.initial as f32 * reform_share).ceil() as u32)
                            .clamp(1, rock.initial.max(1))
                            .min(rock.initial);
                        let fact = FactDraft {
                            kind: FactKind::from_static("rock_reformed"),
                            subjects: Subjects {
                                rock: Some(rock.id.clone()),
                                region: Some(field.region.clone()),
                                ..Subjects::default()
                            },
                            amount: Some(i64::from(units)),
                            note: format!("{} reformed in {}", rock.id, field.name),
                        };
                        ctx.submit(
                            CommandBatch::new(cause())
                                .with(Command::AdjustRock {
                                    rock: rock.id.clone(),
                                    delta: i64::from(units),
                                })
                                .with(set(RockGrowth {
                                    mark: now,
                                    spent_at: None,
                                }))
                                .with(Command::RecordFact {
                                    fact: Box::new(fact),
                                }),
                        );
                    }
                    _ => {}
                }
                continue;
            }
            if g.mark == now && g.spent_at.is_none() {
                continue;
            }
            let mined_now = world.ships.values().any(|s| {
                s.activity
                    .mining
                    .as_ref()
                    .is_some_and(|m| m.rock == rock.id)
            });
            let per_pulse = field.regrow / field.count.max(1) as f32;
            let missing = rock.initial.saturating_sub(rock.remaining);
            let mut batch = CommandBatch::new(cause());
            let mut mark = now;
            if !mined_now && missing > 0 && per_pulse > 0.0 {
                let due = (now.saturating_sub(g.mark) as f32 * per_pulse).floor() as u32;
                let units = due.min(missing);
                if units == 0 {
                    // Not a whole unit yet: keep counting from the old mark.
                    mark = g.mark;
                } else {
                    batch.push(Command::AdjustRock {
                        rock: rock.id.clone(),
                        delta: i64::from(units),
                    });
                    if units == due {
                        // Carry the leftover pulses into the next unit.
                        let used = (units as f32 / per_pulse).ceil() as u64;
                        mark = g.mark.saturating_add(used).min(now);
                    }
                }
            }
            let growth = RockGrowth {
                mark,
                spent_at: None,
            };
            if &growth != g {
                batch.push(set(growth));
            }
            ctx.submit(batch);
        }
    }
}
