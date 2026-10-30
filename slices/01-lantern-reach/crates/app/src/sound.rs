//! Which sound cues a tick asks for, and `galaxy audio render`.
//!
//! Cues come only from what a tick committed (the flagship's dock, undock and mine
//! events) and from dock actions that went through, never from frames or held keys.
//! Each is seeded by the fact that caused it, so its variation is reproducible.
//! `galaxy_audio` decides whether it actually sounds (rate limits, thinning).

use std::path::Path;

use galaxy_audio::Cue;
use galaxy_contract::prelude::*;
use serde_json::{Value, json};

use crate::session::TickOutcome;
use crate::trades::Settled;

/// The cues for one tick, with their seeds, in the order to play them.
pub fn cues(world: &WorldState, events: &[EventEnvelope], out: &TickOutcome) -> Vec<(Cue, u64)> {
    let Some(flagship) = world.player.ship.as_ref() else {
        return Vec::new();
    };
    let mut cues = Vec::new();
    let mut mined = None;
    for e in events {
        match &e.event {
            Event::Dock { ship, .. } if ship == flagship => cues.push((Cue::Dock, e.fact.seq())),
            Event::Undock { ship, .. } if ship == flagship => {
                cues.push((Cue::Undock, e.fact.seq()));
            }
            Event::Mine { ship, .. } if ship == flagship => mined = Some(e.fact.seq()),
            _ => {}
        }
    }
    // One cue per tick for mining: the tick, or the hold filling up instead.
    if let Some(seed) = mined {
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
        let got = cues(&w, &events, &TickOutcome::default());
        // One mining cue per tick, seeded by the last mine; nobody else's events.
        assert_eq!(got, vec![(Cue::Dock, 10), (Cue::OreTick, 12)]);
        let traded = TickOutcome {
            trade: Some(Settled::Applied),
            ..TickOutcome::default()
        };
        assert_eq!(cues(&w, &[], &traded), vec![(Cue::Confirm, w.meta.tick.0)]);
        let retrying = TickOutcome {
            trade: Some(Settled::Retrying),
            ..TickOutcome::default()
        };
        assert!(cues(&w, &[], &retrying).is_empty());
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
            cues(&w, &[mine], &TickOutcome::default()),
            vec![(Cue::HoldFull, 5)]
        );
    }
}
