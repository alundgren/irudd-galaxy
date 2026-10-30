//! The economy's per-tick system.
//!
//! Each tick it reads the events committed last tick and:
//!
//! 1. Tracks dockings for the counted-docking rule, and turns counted dockings and
//!    lane arrivals into pulses.
//! 2. Applies pulses to the markets (stock flows, saturation recovers). Pulses are
//!    first added to the tallies as *pending*, then applied by a separate batch that
//!    clears them; if that batch loses a race with a trade at the same station, the
//!    pulses stay pending and apply next tick. Activity is never lost.
//! 3. Re-prices any line whose cached price or band is stale (new games, levers,
//!    regard changes).
//! 4. Scouts: a player-owned ship's docking writes that station's price snapshot; a
//!    lane arrival in a hull with `lane_scout` writes the whole region's; undocking
//!    refreshes the snapshot of the station it leaves, so it reads "as of your last
//!    visit". Snapshots read the committed world (the end of last tick), never this
//!    tick's proposed batches, which may yet be rejected.
//! 5. Executes the player's trade requests from the dock screen, in order.
//! 6. Writes the facts progression counts: `far_docking` when the flagship docks
//!    where its snapshot is more than `scouting.far_docking` of its dockings old, and
//!    `split_sale` when it sells a ware at one station after selling part of that
//!    stack at another and keeping the rest (`economy.split_stacks`).
//!
//! The system is stateless: everything it remembers lives in `world.economy`.
//! Batches are worked out against a scratch copy of the world that each earlier batch
//! has been applied to, so they chain within a tick.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_core::apply::{Staged, apply};
use galaxy_core::{System, TickCtx};

use crate::pricing::{self, spread_at};
use crate::pulse::{self, Pulses};
use crate::scout;
use crate::trade;

/// The living market. Register it after the systems that dock and fly ships.
pub struct EconomySystem;

/// The system's name (also its cause name and RNG key).
pub const NAME: &str = "economy";

/// The world as this tick's batches so far leave it.
struct Work<'a> {
    base: &'a WorldState,
    scratch: Option<WorldState>,
}

impl<'a> Work<'a> {
    fn world(&self) -> &WorldState {
        self.scratch.as_ref().unwrap_or(self.base)
    }

    /// Applies `batch` to the scratch world and submits it. A batch that doesn't
    /// apply cleanly is dropped.
    fn commit(&mut self, ctx: &mut TickCtx<'_>, batch: CommandBatch) {
        if batch.commands.is_empty() {
            return;
        }
        let world = self.scratch.get_or_insert_with(|| self.base.clone());
        let mut undo = Vec::new();
        for cmd in &batch.commands {
            match apply(world, cmd, &mut Staged::default()) {
                Ok(u) => undo.push(u),
                Err(_) => {
                    for u in undo.iter().rev() {
                        if let UndoRecord::Invert { command } = u {
                            let _ = apply(world, command, &mut Staged::default());
                        }
                    }
                    return;
                }
            }
        }
        ctx.submit(batch);
    }
}

fn player_owned(world: &WorldState, ship: &ShipId) -> bool {
    world
        .ships
        .get(ship)
        .is_some_and(|s| s.owner == Owner::Player)
}

fn cause(summary: &str) -> Cause {
    Cause::system(NAME, summary)
}

/// A fact for one of the player's trading deeds.
fn deed(
    kind: &'static str,
    ship: &ShipId,
    station: &StationId,
    ware: Option<&WareId>,
    amount: Option<i64>,
    note: String,
) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: FactKind::from_static(kind),
            subjects: Subjects {
                ship: Some(ship.clone()),
                station: Some(station.clone()),
                ware: ware.cloned(),
                ..Subjects::default()
            },
            amount,
            note,
        }),
    }
}

impl System for EconomySystem {
    fn name(&self) -> &'static str {
        NAME
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world: &WorldState = ctx.world;
        let events: &[EventEnvelope] = ctx.events;
        let input: &PlayerInput = ctx.input;
        let rules = &world.economy.rules;
        let mut work = Work {
            base: world,
            scratch: None,
        };

        // 1. Dock tracking and pulses from last tick's events.
        let mut tracks: BTreeMap<ShipId, Option<DockTrack>> = BTreeMap::new();
        let track =
            |tracks: &BTreeMap<ShipId, Option<DockTrack>>, ship: &ShipId| match tracks.get(ship) {
                Some(t) => t.clone(),
                None => world.economy.docks.get(ship).cloned(),
            };
        let mut pulses = Pulses::default();
        let mut scouts: Vec<(ShipId, StationId)> = Vec::new();
        let mut left: Vec<StationId> = Vec::new();
        let mut deeds = CommandBatch::new(cause("trading deeds"));
        let mut stacks = world.economy.split_stacks.clone();
        let flagship = world.player.ship.as_ref();
        for env in events {
            match &env.event {
                Event::Dock { ship, station } if Some(ship) == flagship => {
                    if let Some(snap) = world.player.price_snapshots.get(station) {
                        let ago = scout::dockings_ago(world, snap);
                        if ago > rules.scouting.far_docking {
                            deeds.push(deed(
                                "far_docking",
                                ship,
                                station,
                                None,
                                Some(ago as i64),
                                format!("{ship} docked at {station}, last seen {ago} dockings ago"),
                            ));
                        }
                    }
                }
                Event::Sell {
                    ship,
                    station,
                    ware,
                    quantity,
                    ..
                } if Some(ship) == flagship => {
                    if stacks.get(ware).is_some_and(|s| s != station) {
                        deeds.push(deed(
                            "split_sale",
                            ship,
                            station,
                            Some(ware),
                            Some(i64::from(*quantity)),
                            format!("{ship} sold the rest of a {ware} stack at {station}"),
                        ));
                        stacks.remove(ware);
                    }
                    let kept = world
                        .ships
                        .get(ship)
                        .is_some_and(|s| s.hold.count(ware) > 0);
                    if kept {
                        stacks.insert(ware.clone(), station.clone());
                    } else {
                        stacks.remove(ware);
                    }
                }
                _ => {}
            }
        }
        let wares: BTreeSet<&WareId> = stacks
            .keys()
            .chain(world.economy.split_stacks.keys())
            .collect();
        for ware in wares {
            let now = stacks.get(ware);
            if world.economy.split_stacks.get(ware) != now {
                deeds.push(Command::SetSplitStack {
                    ware: ware.clone(),
                    station: now.cloned(),
                });
            }
        }
        work.commit(ctx, deeds);
        for env in events {
            match &env.event {
                Event::Dock { ship, station } => {
                    if pulse::is_counted(track(&tracks, ship).as_ref(), station) {
                        pulses.docking(world, station, &rules.pulses);
                    }
                    tracks.insert(
                        ship.clone(),
                        Some(DockTrack {
                            station: station.clone(),
                            away: false,
                        }),
                    );
                    if player_owned(world, ship) {
                        scouts.push((ship.clone(), station.clone()));
                    }
                }
                Event::Undock { ship, station } => {
                    if player_owned(world, ship) {
                        left.push(station.clone());
                    }
                }
                Event::Jump { ship, to, .. } => {
                    pulses.arrival(world, to, &rules.pulses);
                    let lane_scout = world.ships.get(ship).is_some_and(|s| {
                        s.owner == Owner::Player && scout::reach_of(world, s).lane_scout
                    });
                    if lane_scout {
                        for station in pulse::stations_in(world, to) {
                            scouts.push((ship.clone(), station.clone()));
                        }
                    }
                }
                Event::Mine { ship, .. } => {
                    if let Some(t) = track(&tracks, ship).filter(|t| !t.away) {
                        tracks.insert(ship.clone(), Some(DockTrack { away: true, ..t }));
                    }
                }
                _ => {}
            }
        }
        // Gone far enough from where it last docked; ships that no longer exist.
        for (ship_id, t) in &world.economy.docks {
            if tracks.contains_key(ship_id) {
                continue;
            }
            let Some(ship) = world.ships.get(ship_id) else {
                tracks.insert(ship_id.clone(), None);
                continue;
            };
            let far = ship.docked_at.is_none()
                && world.stations.get(&t.station).is_none_or(|s| {
                    s.position.distance(ship.position) >= rules.pulses.recount_distance
                });
            if !t.away && far {
                tracks.insert(
                    ship_id.clone(),
                    Some(DockTrack {
                        away: true,
                        ..t.clone()
                    }),
                );
            }
        }
        let track_cmds: Vec<Command> = tracks
            .into_iter()
            .filter(|(ship, t)| world.economy.docks.get(ship) != t.as_ref())
            .map(|(ship, track)| Command::SetDockTrack { ship, track })
            .collect();
        work.commit(
            ctx,
            CommandBatch {
                cause: cause("docking tracks"),
                commands: track_cmds,
            },
        );

        // 2a. Count the pulses (never conflicts: integer deltas).
        let mut counted = CommandBatch::new(cause("market activity"));
        for (station, &m) in &pulses.stations {
            counted.push(Command::AdjustActivity {
                scope: ActivityScope::Station {
                    station: station.clone(),
                },
                delta: ActivityTally {
                    pulses_milli: m,
                    pending_milli: m,
                    ..ActivityTally::default()
                },
            });
        }
        for (region, &m) in &pulses.regions {
            counted.push(Command::AdjustActivity {
                scope: ActivityScope::Region {
                    region: region.clone(),
                },
                delta: ActivityTally {
                    pulses_milli: m,
                    ..ActivityTally::default()
                },
            });
        }
        work.commit(ctx, counted);

        // 2b. Apply pending pulses to each market.
        let pending: Vec<(StationId, i64)> = work
            .world()
            .economy
            .stations
            .iter()
            .filter(|(_, t)| t.pending_milli != 0)
            .map(|(s, t)| (s.clone(), t.pending_milli))
            .collect();
        for (station, m) in pending {
            let w = work.world();
            let mut batch = CommandBatch::new(cause("market pulse"));
            if let Some(market) = w.markets.get(&station) {
                let pricing_rules = &w.economy.rules.pricing;
                let spread = spread_at(w, &station);
                let next = pulse::pulse_market(market, m as f64 / 1000.0, pricing_rules);
                for (ware, line) in next {
                    let line = pricing::priced(&line, pricing_rules, spread);
                    let old = &market.lines[&ware];
                    if line != *old {
                        batch.push(Command::UpdateMarketLine {
                            station: station.clone(),
                            ware,
                            from: Box::new(old.clone()),
                            to: Box::new(line),
                        });
                    }
                }
            }
            batch.push(Command::AdjustActivity {
                scope: ActivityScope::Station { station },
                delta: ActivityTally {
                    pending_milli: -m,
                    ..ActivityTally::default()
                },
            });
            work.commit(ctx, batch);
        }

        // 3. Re-price stale lines.
        let stale: Vec<(StationId, Vec<Command>)> = {
            let w = work.world();
            let pricing_rules = &w.economy.rules.pricing;
            w.markets
                .values()
                .map(|m| {
                    let spread = spread_at(w, &m.station);
                    let cmds = m
                        .lines
                        .iter()
                        .filter_map(|(ware, l)| {
                            let p = pricing::priced(l, pricing_rules, spread);
                            (p != *l).then(|| Command::UpdateMarketLine {
                                station: m.station.clone(),
                                ware: ware.clone(),
                                from: Box::new(l.clone()),
                                to: Box::new(p),
                            })
                        })
                        .collect();
                    (m.station.clone(), cmds)
                })
                .filter(|(_, c): &(StationId, Vec<Command>)| !c.is_empty())
                .collect()
        };
        for (_, commands) in stale {
            work.commit(
                ctx,
                CommandBatch {
                    cause: cause("re-price"),
                    commands,
                },
            );
        }

        // 4. Scouting. A new game starts docked without a docking event, so the
        //    station the flagship sits at counts as visited too.
        if let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s))
            && let Some(station) = &ship.docked_at
            && !world.player.price_snapshots.contains_key(station)
        {
            scouts.push((ship.id.clone(), station.clone()));
        }
        //    Prices come from `world`, which is committed: this tick's pulses and trades
        //    may still be rejected, and a snapshot must show prices that really were.
        let mut scouted = BTreeSet::new();
        for (ship, station) in scouts {
            if !scouted.insert(station.clone()) {
                continue;
            }
            let commands = scout::scout_commands(world, &ship, &station);
            work.commit(
                ctx,
                CommandBatch {
                    cause: cause("prices seen on docking"),
                    commands,
                },
            );
        }
        for station in left {
            if !scouted.insert(station.clone()) {
                continue;
            }
            let Some(snapshot) = scout::snapshot(world, &station) else {
                continue;
            };
            work.commit(
                ctx,
                CommandBatch::new(cause("prices seen on leaving")).with(
                    Command::SetPriceSnapshot {
                        station,
                        snapshot: Some(snapshot),
                    },
                ),
            );
        }

        // 5. The player's trades.
        let Some(flagship) = world.player.ship.as_ref() else {
            return;
        };
        for req in input.trades.iter().filter(|r| r.ship == *flagship) {
            if let Ok(batch) = trade::trade_batch(work.world(), req) {
                work.commit(ctx, batch);
            }
        }
    }
}
