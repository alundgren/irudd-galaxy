//! Which sound cues a tick asks for, and `galaxy audio render`.
//!
//! Cues come only from what a tick committed (the flagship's dock and undock events,
//! ore it moved into its hold) and from dock actions that went through, never from
//! frames or held keys. Each is seeded by the fact or tick that caused it, so its
//! variation is reproducible. `galaxy_audio` decides whether it actually sounds (rate
//! limits, thinning).

use std::path::Path;

use galaxy_audio::Cue;
use galaxy_contract::prelude::*;
use serde_json::{Value, json};

use crate::session::TickOutcome;
use crate::trades::Settled;

/// Works out each tick's cues. It remembers how far the flagship's mining run had got
/// at the last committed tick, so ore ticks follow units entering the hold rather than
/// the run's one `mine` event, which only comes when the run ends.
#[derive(Clone, Debug, Default)]
pub struct Cues {
    /// The flagship's run at the last tick seen: its rock and units moved so far.
    run: Option<(RockId, u32)>,
}

impl Cues {
    /// Starts from `world` as loaded, so a run already under way doesn't tick for the
    /// units it moved before this session.
    pub fn new(world: &WorldState) -> Self {
        Self {
            run: flagship_run(world),
        }
    }

    /// The cues for the tick just committed, with their seeds, in the order to play
    /// them.
    pub fn tick(
        &mut self,
        world: &WorldState,
        events: &[EventEnvelope],
        out: &TickOutcome,
    ) -> Vec<(Cue, u64)> {
        let before = std::mem::replace(&mut self.run, flagship_run(world));
        let Some(flagship) = world.player.ship.as_ref() else {
            return Vec::new();
        };
        let seen = |rock: &RockId| match &before {
            Some((r, mined)) if r == rock => *mined,
            _ => 0,
        };
        let mut cues = Vec::new();
        // Ore entered the hold this tick: the run still going moved more units, or the
        // run that ended (its `mine` event carries the total) moved some since last tick.
        let mut ore = match &self.run {
            Some((rock, mined)) if *mined > seen(rock) => Some(world.meta.tick.0),
            _ => None,
        };
        for e in events {
            match &e.event {
                Event::Dock { ship, .. } if ship == flagship => {
                    cues.push((Cue::Dock, e.fact.seq()))
                }
                Event::Undock { ship, .. } if ship == flagship => {
                    cues.push((Cue::Undock, e.fact.seq()));
                }
                Event::Mine {
                    ship, rock, amount, ..
                } if ship == flagship && *amount > seen(rock) => ore = Some(e.fact.seq()),
                _ => {}
            }
        }
        // One cue per tick for mining: the tick, or the hold filling up instead.
        if let Some(seed) = ore {
            cues.push((
                if hold_full(world, flagship) {
                    Cue::HoldFull
                } else {
                    Cue::OreTick
                },
                seed,
            ));
        }
        if out.trade == Some(Settled::Applied) {
            cues.push((Cue::Confirm, world.meta.tick.0));
        }
        cues
    }
}

fn flagship_run(world: &WorldState) -> Option<(RockId, u32)> {
    let ship = world.ships.get(world.player.ship.as_ref()?)?;
    let run = ship.activity.mining.as_ref()?;
    Some((run.rock.clone(), run.mined))
}

fn hold_full(world: &WorldState, ship: &ShipId) -> bool {
    world.ships.get(ship).is_some_and(|s| {
        world
            .hulls
            .get(&s.hull)
            .is_some_and(|h| s.hold.total() >= u64::from(h.hold_capacity))
    })
}

/// `galaxy audio render --out DIR`: every take of every cue as a WAV in `dir`, with its
/// measurements and anything outside the design limits. `Err` carries the report when
/// a take fails a limit.
pub fn render(dir: &Path) -> Result<Value, (u8, Value)> {
    let reports = galaxy_audio::render_all(Some(dir)).map_err(|e| {
        (
            1,
            json!({ "error": format!("{}: {e}", dir.display()), "kind": "runtime" }),
        )
    })?;
    let takes: Vec<Value> = reports
        .iter()
        .map(|r| {
            let m = r.metrics;
            json!({
                "cue": r.cue.name(),
                "take": r.take,
                "file": r.file,
                "seconds": m.duration,
                "peak_dbfs": m.peak_db,
                "rms_dbfs": m.rms_db,
                "centroid_hz": m.centroid_hz,
                "harsh_2k_5k_share": m.harsh_share,
                "rise_ms": m.rise * 1000.0,
                "dc": m.dc,
                "problems": r.problems,
            })
        })
        .collect();
    let passed = reports.iter().all(|r| r.problems.is_empty());
    let report = json!({ "out": dir, "passed": passed, "takes": takes });
    if passed { Ok(report) } else { Err((5, report)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_contract::events::EventEnvelope;

    fn env(fact: u64, event: Event) -> EventEnvelope {
        EventEnvelope {
            tick: Tick(1),
            fact: FactId(fact),
            cause: CauseId(fact),
            event,
        }
    }

    fn world() -> WorldState {
        let packs = crate::game::packs_dir(None).expect("packs");
        galaxy_core::new_game(&crate::game::core_data(&packs), 1)
            .expect("new game")
            .world
    }

    #[test]
    fn cues_follow_the_flagships_committed_events_only() {
        let w = world();
        let me = w.player.ship.clone().unwrap();
        let other = w.ships.keys().find(|s| **s != me).cloned();
        let station = w.stations.keys().next().cloned().unwrap();
        let rock = RockId::from_static("rock.test");
        let ware = WareId::from_static("ware.ice");
        let mut events = vec![
            env(
                10,
                Event::Dock {
                    ship: me.clone(),
                    station: station.clone(),
                },
            ),
            env(
                11,
                Event::Mine {
                    ship: me.clone(),
                    rock: rock.clone(),
                    ware: ware.clone(),
                    amount: 1,
                },
            ),
            env(
                12,
                Event::Mine {
                    ship: me.clone(),
                    rock: rock.clone(),
                    ware: ware.clone(),
                    amount: 1,
                },
            ),
        ];
        if let Some(o) = other {
            events.push(env(
                13,
                Event::Undock {
                    ship: o.clone(),
                    station: station.clone(),
                },
            ));
            events.push(env(
                14,
                Event::Mine {
                    ship: o,
                    rock,
                    ware,
                    amount: 1,
                },
            ));
        }
        let got = Cues::new(&w).tick(&w, &events, &TickOutcome::default());
        // One mining cue per tick, seeded by the last mine; nobody else's events.
        assert_eq!(got, vec![(Cue::Dock, 10), (Cue::OreTick, 12)]);
        let traded = TickOutcome {
            trade: Some(Settled::Applied),
            ..TickOutcome::default()
        };
        assert_eq!(
            Cues::new(&w).tick(&w, &[], &traded),
            vec![(Cue::Confirm, w.meta.tick.0)]
        );
        let retrying = TickOutcome {
            trade: Some(Settled::Retrying),
            ..TickOutcome::default()
        };
        assert!(Cues::new(&w).tick(&w, &[], &retrying).is_empty());
    }

    #[test]
    fn filling_the_hold_plays_hold_full_instead_of_a_tick() {
        let mut w = world();
        let me = w.player.ship.clone().unwrap();
        let ware = WareId::from_static("ware.ice");
        let hull = w.ships.get(&me).unwrap().hull.clone();
        let cap = w.hulls.get(&hull).unwrap().hold_capacity;
        let ship = w.ships.get_mut(&me).unwrap();
        ship.hold.cargo.insert(ware.clone(), cap);
        let mine = env(
            5,
            Event::Mine {
                ship: me,
                rock: RockId::from_static("rock.test"),
                ware,
                amount: 1,
            },
        );
        assert_eq!(
            Cues::default().tick(&w, &[mine], &TickOutcome::default()),
            vec![(Cue::HoldFull, 5)]
        );
    }

    #[test]
    fn ore_ticks_follow_units_entering_the_hold_not_the_runs_end() {
        let mut w = world();
        let me = w.player.ship.clone().unwrap();
        let rock = RockId::from_static("rock.test");
        let quiet = TickOutcome::default();
        let mut cues = Cues::new(&w);
        let run = |w: &mut WorldState, mined: Option<u32>| {
            w.meta.tick.0 += 1;
            w.ships.get_mut(&me).unwrap().activity.mining = mined.map(|mined| MiningRun {
                rock: rock.clone(),
                progress: 0.0,
                mined,
            });
        };
        // Cutting, nothing in the hold yet: quiet.
        run(&mut w, Some(0));
        assert!(cues.tick(&w, &[], &quiet).is_empty());
        // A unit in: a tick, seeded by the tick.
        run(&mut w, Some(1));
        let t = w.meta.tick.0;
        assert_eq!(cues.tick(&w, &[], &quiet), vec![(Cue::OreTick, t)]);
        // No new unit (or a batch that didn't commit): quiet.
        run(&mut w, Some(1));
        assert!(cues.tick(&w, &[], &quiet).is_empty());
        run(&mut w, Some(2));
        assert_eq!(cues.tick(&w, &[], &quiet).len(), 1);
        // Letting go: the run's `mine` event, but nothing new entered the hold.
        run(&mut w, None);
        let end = env(
            20,
            Event::Mine {
                ship: me.clone(),
                rock: rock.clone(),
                ware: WareId::from_static("ware.ice"),
                amount: 2,
            },
        );
        assert!(cues.tick(&w, &[end], &quiet).is_empty());
        // A run that moved a last unit as it ended still ticks, from its event.
        let mut cues = Cues::new(&w);
        run(&mut w, Some(4));
        cues.tick(&w, &[], &quiet);
        run(&mut w, None);
        let end = env(
            21,
            Event::Mine {
                ship: me.clone(),
                rock: rock.clone(),
                ware: WareId::from_static("ware.ice"),
                amount: 5,
            },
        );
        assert_eq!(cues.tick(&w, &[end], &quiet), vec![(Cue::OreTick, 21)]);
    }

    #[test]
    fn a_run_under_way_at_load_doesnt_tick_for_its_earlier_units() {
        let mut w = world();
        let me = w.player.ship.clone().unwrap();
        w.ships.get_mut(&me).unwrap().activity.mining = Some(MiningRun {
            rock: RockId::from_static("rock.test"),
            progress: 0.0,
            mined: 7,
        });
        assert!(
            Cues::new(&w)
                .tick(&w, &[], &TickOutcome::default())
                .is_empty()
        );
    }
}
