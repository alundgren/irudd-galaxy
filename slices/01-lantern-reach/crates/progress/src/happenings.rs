//! Happenings at stations (issue #47, 4.1): the runtime that starts, counts and ends
//! them, and the read API the docked view draws from.
//!
//! - **Start.** An upcoming happening starts once every cause fact exists
//!   (`galaxy_core::happenings::find_cause`) and the player is docked at its station
//!   with nothing else running there. Its speaker says its line in dock talk, citing
//!   the cause facts, and a `happening_started` fact is recorded. The calm rule: one
//!   happening at a time per station; the next waits, in ID order.
//! - **Count.** It runs for `lasts.dockings` counted player dockings at the station: the
//!   kernel's guarded dockings counter, so a re-dock before the ship has gone anywhere
//!   doesn't count. The docking it starts in is its first.
//! - **End.** Once the station's count reaches its end, it ends with a
//!   `happening_ended` fact, silently.
//!
//! [`Waiting::due`] runs every tick, whether or not anything happened, so a happening
//! however it was written (authored data, a hook, a backfill) starts the tick after its
//! causes and the player's docking are both there. It remembers what it found in
//! history ([`Waiting`]), so a happening waiting at the docked station costs only the
//! facts added since the last tick, not a scan of the whole log.
//!
//! The batches are worked out from the tick's starting world. Each carries the record
//! it was worked out from ([`Command::ExpectHappening`]), so a change another batch
//! committed earlier in the tick fails it whole instead of being overwritten, and it
//! tries again next tick; a happening a batch queued earlier this tick writes waits a
//! tick without trying. A start also checks the flagship is still docked where it was
//! ([`Command::ExpectDocked`]), and the kernel's calm-rule invariant refuses a second
//! running happening. Nothing here reads a clock.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::happening::{
    ENDED_FACT, Happening, HappeningCause, HappeningRun, STARTED_FACT,
};
use galaxy_contract::prelude::*;
use galaxy_core::History;
use galaxy_core::happenings::{dockings_at, find_cause, is_running, start_line};

/// The cause name of the runtime's batches.
pub const SYSTEM_NAME: &str = "happenings";

/// The happening running at `station` now (at most one): what the docked view shows.
pub fn active<'a>(world: &'a WorldState, station: &StationId) -> Vec<&'a Happening> {
    galaxy_core::happenings::active(world, station)
}

/// The happenings at `station` still waiting to start, in the order they would.
pub fn upcoming<'a>(world: &'a WorldState, station: &StationId) -> Vec<&'a Happening> {
    galaxy_core::happenings::of_station(world, station)
        .filter(|h| h.run.is_upcoming())
        .collect()
}

/// The happenings at `station` that have run, oldest first, and any running one whose
/// last docking has passed but whose end isn't recorded yet.
pub fn ended<'a>(world: &'a WorldState, station: &StationId) -> Vec<&'a Happening> {
    let mut out: Vec<&Happening> = galaxy_core::happenings::of_station(world, station)
        .filter(|h| match h.run {
            HappeningRun::Ended { .. } => true,
            HappeningRun::Active { .. } => !is_running(world, h),
            HappeningRun::Upcoming => false,
        })
        .collect();
    out.sort_by_key(|h| match h.run {
        HappeningRun::Ended { at, .. } => at,
        _ => Tick(u64::MAX),
    });
    out
}

/// The cause facts that would start `h` now, newest match for each, or `None` while
/// one is missing. Scans history; [`Waiting::causes_met`] is the cheap repeat.
pub fn causes_met(history: &History, h: &Happening) -> Option<Vec<FactId>> {
    let mut cites = Vec::new();
    for c in &h.cause.facts {
        let id = find_cause(history, c)?.id;
        if !cites.contains(&id) {
            cites.push(id);
        }
    }
    Some(cites)
}

/// What [`Waiting::due`] remembers between ticks about the upcoming happenings it has
/// looked at: for each, the cause it saw and the newest fact matching each of its
/// patterns among the first `seen` facts. History only grows, so the next look scans
/// just the facts added since. Not saved: a fresh one finds the same answers with one
/// full scan per happening.
#[derive(Debug, Default)]
pub struct Waiting {
    looked: BTreeMap<HappeningId, Looked>,
}

#[derive(Debug)]
struct Looked {
    cause: HappeningCause,
    /// How many facts have been scanned, and the last one's ID (a check that it's
    /// still the same log).
    seen: usize,
    last: Option<FactId>,
    /// The newest match for each pattern in `cause.facts` (`None` for those by ID).
    newest: Vec<Option<FactId>>,
}

impl Waiting {
    /// [`causes_met`], scanning only the facts added since `h` was last looked at with
    /// the same cause.
    pub fn causes_met(&mut self, history: &History, h: &Happening) -> Option<Vec<FactId>> {
        let facts = history.facts();
        let same = self.looked.get(&h.id).is_some_and(|l| {
            l.cause == h.cause
                && l.seen <= facts.len()
                && l.seen.checked_sub(1).map(|i| facts[i].id) == l.last
        });
        if !same {
            self.looked.insert(
                h.id.clone(),
                Looked {
                    cause: h.cause.clone(),
                    seen: 0,
                    last: None,
                    newest: vec![None; h.cause.facts.len()],
                },
            );
        }
        let looked = self.looked.get_mut(&h.id)?;
        for f in &facts[looked.seen..] {
            for (c, newest) in h.cause.facts.iter().zip(&mut looked.newest) {
                if c.id.is_none() && c.matches(f) {
                    *newest = Some(f.id);
                }
            }
        }
        looked.seen = facts.len();
        looked.last = facts.last().map(|f| f.id);
        let mut cites = Vec::new();
        for (c, newest) in h.cause.facts.iter().zip(&looked.newest) {
            let id = match c.id {
                Some(id) => history.fact(id)?.id,
                None => (*newest)?,
            };
            if !cites.contains(&id) {
                cites.push(id);
            }
        }
        Some(cites)
    }

    /// This tick's batches: every running happening whose last docking has passed
    /// ends, and, at the station the player is docked at, the first upcoming happening
    /// whose causes are met starts if nothing runs there (nor ends this tick: the next
    /// starts a tick later). A happening that a batch in `queued` (those applying
    /// before these) writes is left until next tick, and holds up those after it.
    pub fn due(
        &mut self,
        world: &WorldState,
        history: &History,
        queued: &[CommandBatch],
    ) -> Vec<CommandBatch> {
        self.looked.retain(|id, _| {
            world
                .happenings
                .get(id)
                .is_some_and(|h| h.run.is_upcoming())
        });
        let written: BTreeSet<&HappeningId> = queued
            .iter()
            .flat_map(|b| &b.commands)
            .filter_map(|c| match c {
                Command::PutHappening { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        let mut out: Vec<CommandBatch> = world
            .happenings
            .values()
            .filter(|h| !written.contains(&h.id))
            .filter_map(|h| end(world, h))
            .collect();
        let Some(ship) = world.player.ship.as_ref() else {
            return out;
        };
        let Some(station) = galaxy_core::parts::docked_at(world) else {
            return out;
        };
        let busy = galaxy_core::happenings::of_station(world, &station).any(|h| h.is_active());
        if busy {
            return out;
        }
        let next = galaxy_core::happenings::of_station(world, &station)
            .filter(|h| h.run.is_upcoming())
            .find_map(|h| {
                if written.contains(&h.id) {
                    // Rewritten this tick: the calm order waits for it.
                    return Some(None);
                }
                let cites = self.causes_met(history, h)?;
                Some(Some(start(world, ship, h, cites)?))
            })
            .flatten();
        if let Some(batch) = next {
            out.push(batch);
        }
        out
    }
}

fn fact(kind: &str, h: &Happening, npc: Option<NpcId>, note: String) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: kind.parse().expect("happening fact kinds are valid"),
            subjects: Subjects {
                station: Some(h.station.clone()),
                npc,
                other: h.id.as_str().parse().ok(),
                ..Subjects::default()
            },
            amount: Some(i64::from(h.lasts.dockings)),
            note,
        }),
    }
}

/// The check that `h` is still the record the batch was worked out from.
fn expect(h: &Happening) -> Command {
    Command::ExpectHappening {
        id: h.id.clone(),
        value: Some(Box::new(h.clone())),
    }
}

/// The batch that ends `h`, if its last docking has passed.
fn end(world: &WorldState, h: &Happening) -> Option<CommandBatch> {
    let HappeningRun::Active {
        since,
        first_docking,
        ..
    } = h.run
    else {
        return None;
    };
    if is_running(world, h) {
        return None;
    }
    let mut over = h.clone();
    over.run = HappeningRun::Ended {
        since,
        first_docking,
        at: Tick(world.meta.tick.0 + 1),
    };
    Some(CommandBatch {
        cause: Cause::system(SYSTEM_NAME, format!("{} is over", h.id)),
        commands: vec![
            expect(h),
            Command::PutHappening {
                id: h.id.clone(),
                value: Some(Box::new(over)),
            },
            fact(
                ENDED_FACT,
                h,
                None,
                format!("{} ended after {} dockings", h.id, h.lasts.dockings),
            ),
        ],
    })
}

/// The batch that starts `h` at the station the flagship `ship` is docked at, if `h`
/// still passes its checks there (a speaker who has moved away makes it wait).
fn start(
    world: &WorldState,
    ship: &ShipId,
    h: &Happening,
    cites: Vec<FactId>,
) -> Option<CommandBatch> {
    let mut running = h.clone();
    running.run = HappeningRun::Active {
        since: Tick(world.meta.tick.0 + 1),
        first_docking: dockings_at(world, &h.station),
        cites: cites.clone(),
    };
    galaxy_core::happenings::check(world, &running).ok()?;
    let mut cause = Cause::system(SYSTEM_NAME, format!("{} begins", h.id));
    cause.cites = cites.clone();
    Some(CommandBatch {
        cause,
        commands: vec![
            Command::ExpectDocked {
                ship: ship.clone(),
                station: h.station.clone(),
            },
            expect(h),
            Command::PutHappening {
                id: h.id.clone(),
                value: Some(Box::new(running)),
            },
            fact(
                STARTED_FACT,
                h,
                Some(h.line.speaker.clone()),
                format!("{} began: {}", h.id, h.line.text),
            ),
            Command::Say {
                line: Box::new(start_line(h, cites)),
            },
        ],
    })
}

/// This tick's batches, worked out afresh: [`Waiting::due`] with nothing remembered
/// and nothing queued ahead.
pub fn due(world: &WorldState, history: &History) -> Vec<CommandBatch> {
    Waiting::default().due(world, history, &[])
}

#[cfg(test)]
mod tests {
    use galaxy_contract::happening::{CauseFact, HappeningCause, HappeningLine, Lasts};
    use galaxy_contract::history::FactPayload;

    use super::*;

    const WHARF: StationId = StationId::from_static("station.lantern_wharf");
    const KETTLE: StationId = StationId::from_static("station.kettle_dock");

    fn fact(id: u64, kind: &str, station: &StationId) -> Fact {
        Fact {
            id: FactId(id),
            tick: Tick(id),
            kind: kind.parse().unwrap(),
            cause: CauseId(id),
            subjects: Subjects {
                station: Some(station.clone()),
                ..Subjects::default()
            },
            amount: None,
            payload: FactPayload::Note(String::new()),
        }
    }

    fn waiting(cause: Vec<CauseFact>) -> Happening {
        Happening {
            id: "gm.test.crowd".parse().unwrap(),
            station: WHARF,
            show: vec![],
            lasts: Lasts { dockings: 1 },
            cause: HappeningCause { facts: cause },
            line: HappeningLine {
                id: "line.ilse_marrow.crowd".into(),
                speaker: "npc.ilse_marrow".parse().unwrap(),
                text: "A crowd.".into(),
                choices: vec![],
            },
            by: None,
            run: HappeningRun::Upcoming,
        }
    }

    fn pattern(kind: &str, station: StationId) -> CauseFact {
        CauseFact {
            kind: Some(kind.into()),
            station: Some(station),
            ..CauseFact::default()
        }
    }

    /// The remembered answer is the full scan's at every length of a growing log, and
    /// follows a cause that changes.
    #[test]
    fn remembered_causes_match_a_full_scan_as_history_grows() {
        let log = [
            fact(1, "dock", &KETTLE),
            fact(2, "dock", &WHARF),
            fact(3, "gm.test.stage_built", &WHARF),
            fact(4, "dock", &WHARF),
            fact(5, "core.stage_built", &WHARF),
            fact(6, "dock", &KETTLE),
        ];
        let mut h = waiting(vec![
            pattern("dock", WHARF),
            pattern("stage_built", WHARF),
            CauseFact {
                id: Some(FactId(1)),
                ..CauseFact::default()
            },
        ]);
        let mut memo = Waiting::default();
        for n in 0..=log.len() {
            let history = History::from_parts(log[..n].to_vec(), Vec::new());
            if n == 4 {
                h.cause.facts.pop();
            }
            assert_eq!(
                memo.causes_met(&history, &h),
                causes_met(&history, &h),
                "after {n} facts"
            );
        }
        let all = History::from_parts(log.to_vec(), Vec::new());
        assert_eq!(memo.causes_met(&all, &h), Some(vec![FactId(4), FactId(5)]));
        // Another log entirely (a different sim's) isn't mistaken for this one.
        let other = History::from_parts(vec![fact(9, "dock", &KETTLE)], Vec::new());
        assert_eq!(memo.causes_met(&other, &h), None);
    }
}
