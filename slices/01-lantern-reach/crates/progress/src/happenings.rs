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
//! [`due`] works the batches out from the tick's starting world; each checks the
//! flagship is still docked where it was ([`Command::ExpectDocked`]) and the kernel's
//! calm-rule invariant refuses a second running happening. Nothing here reads a clock.

use galaxy_contract::happening::{ENDED_FACT, Happening, HappeningRun, STARTED_FACT};
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
/// one is missing.
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

/// This tick's batches: every running happening whose last docking has passed ends,
/// and, at the station the player is docked at, the first upcoming happening whose
/// causes are met starts if nothing runs there (nor ends this tick: the next starts
/// a tick later).
pub fn due(world: &WorldState, history: &History) -> Vec<CommandBatch> {
    let mut out: Vec<CommandBatch> = world
        .happenings
        .values()
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
        .find_map(|h| start(world, ship, h, causes_met(history, h)?));
    if let Some(batch) = next {
        out.push(batch);
    }
    out
}
