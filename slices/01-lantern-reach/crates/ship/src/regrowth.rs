//! Rocks regrow with activity, never with time.
//!
//! A region's activity is its weighted activity counters (`ship_rules.regrowth.pulses`,
//! dockings and lane arrivals by default): each unit is one region pulse. Each pulse
//! adds the field's `regrow` units, in thousandths, to the rock with the lowest
//! remaining share (spent rocks included, ties to the lowest ID), capped at its full
//! yield; whatever a full rock can't take goes to the next. A rock being mined doesn't
//! regrow. A spent rock keeps its regrowth aside and stays spent until that reaches
//! `reform_share` of its yield; then it reforms with exactly those units. So a field
//! holds its yield, minus what was mined, plus what regrew: nothing more.

use std::cmp::Ordering;
use std::collections::BTreeMap;

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

/// One rock as the field's regrowth sees it, in thousandths of a unit.
struct Share<'w> {
    rock: &'w Rock,
    /// `remaining` plus regrowth not yet in it.
    milli: u64,
    full: u64,
    mined_now: bool,
}

impl Share<'_> {
    fn room(&self) -> u64 {
        self.full.saturating_sub(self.milli)
    }

    /// Lower remaining share first, then the lower ID (the order they come in).
    fn lower_than(&self, other: &Share<'_>) -> bool {
        let a = u128::from(self.milli) * u128::from(other.full);
        let b = u128::from(other.milli) * u128::from(self.full);
        a.cmp(&b) == Ordering::Less
    }
}

/// Gives `pulses` pulses of `per_pulse` thousandths to the lowest shares.
fn allocate(shares: &mut [Share<'_>], pulses: u64, per_pulse: u64) {
    for _ in 0..pulses {
        let mut give = per_pulse;
        while give > 0 {
            let mut best: Option<usize> = None;
            for (i, s) in shares.iter().enumerate() {
                if s.mined_now || s.room() == 0 {
                    continue;
                }
                if best.is_none_or(|b| s.lower_than(&shares[b])) {
                    best = Some(i);
                }
            }
            // Every rock is full or being mined: the rest of the regrowth is lost.
            let Some(i) = best else {
                return;
            };
            let g = give.min(shares[i].room());
            shares[i].milli += g;
            give -= g;
        }
    }
}

impl System for RegrowthSystem {
    fn name(&self) -> &'static str {
        "rock_regrowth"
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world = ctx.world;
        let reform_share = f64::from(world.ship_rules.regrowth.reform_share.clamp(0.0, 1.0));
        let mut by_field: BTreeMap<&FieldId, Vec<&Rock>> = BTreeMap::new();
        for rock in world.rocks.values() {
            by_field.entry(&rock.field).or_default().push(rock);
        }
        for (field_id, rocks) in by_field {
            let Some(field) = world.fields.get(field_id) else {
                continue;
            };
            let now = region_pulses(world, &field.region);
            // The field's mark: the newest any of its rocks has (a rock added later
            // starts from the field's, not from zero).
            let mark = rocks.iter().map(|r| r.growth.mark).max().unwrap_or(now);
            if mark == now && rocks.iter().all(|r| r.growth.mark == now) {
                continue;
            }
            let per_pulse = (f64::from(field.regrow) * 1000.0).round().max(0.0) as u64;
            let mut shares: Vec<Share> = rocks
                .iter()
                .map(|&rock| Share {
                    rock,
                    milli: u64::from(rock.remaining) * 1000 + rock.growth.milli,
                    full: u64::from(rock.initial) * 1000,
                    mined_now: world.ships.values().any(|s| {
                        s.activity
                            .mining
                            .as_ref()
                            .is_some_and(|m| m.rock == rock.id)
                    }),
                })
                .collect();
            if per_pulse > 0 {
                allocate(&mut shares, now.saturating_sub(mark), per_pulse);
            }

            let mut batch = CommandBatch::new(Cause::system(
                "rock_regrowth",
                format!("{} regrows", field.name),
            ));
            for s in &shares {
                let rock = s.rock;
                let spent = rock.remaining == 0;
                // A reformed rock has at least one whole unit.
                let reform_at = ((rock.initial as f64 * reform_share * 1000.0).ceil() as u64)
                    .max(1000)
                    .min(s.full);
                let whole = if spent && s.milli < reform_at {
                    0
                } else {
                    s.milli / 1000
                };
                let units = whole.saturating_sub(u64::from(rock.remaining));
                let growth = RockGrowth {
                    mark: now,
                    milli: s.milli - whole.max(u64::from(rock.remaining)) * 1000,
                };
                if units > 0 {
                    batch.push(Command::AdjustRock {
                        rock: rock.id.clone(),
                        delta: units as i64,
                    });
                }
                if growth != rock.growth {
                    batch.push(Command::SetRockGrowth {
                        rock: rock.id.clone(),
                        growth,
                    });
                }
                if spent && units > 0 {
                    batch.push(Command::RecordFact {
                        fact: Box::new(FactDraft {
                            kind: FactKind::from_static("rock_reformed"),
                            subjects: Subjects {
                                rock: Some(rock.id.clone()),
                                region: Some(field.region.clone()),
                                ..Subjects::default()
                            },
                            amount: Some(units as i64),
                            note: format!("{} reformed in {}", rock.id, field.name),
                        }),
                    });
                }
            }
            if !batch.commands.is_empty() {
                ctx.submit(batch);
            }
        }
    }
}
