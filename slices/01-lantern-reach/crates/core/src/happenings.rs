//! The happenings validator and read helpers (issue #47, 4.1). Every happening a
//! station gets, authored or written by a pack, passes [`check`]:
//!
//! - the station exists, and the speaker is a person at it;
//! - the line has a `line.` ID and one short line of text, said the way any dock-talk
//!   line is (`TalkLine::validate`), with at most four answers;
//! - `lasts.dockings` is 1 to [`DOCKINGS_MAX`];
//! - every `show` entry is `<kind>:<value>` with a kind from
//!   [`galaxy_contract::happening::SHOW_KINDS`];
//! - the cause names at least one fact, each by ID or by a pattern with a `kind`, and
//!   the stations, people, factions and wares a pattern names exist;
//! - at most one happening is running at a station (the calm rule).
//!
//! [`check_facts`] adds what only history knows: a cause fact named by ID exists. The
//! runtime that starts, counts and ends them is `galaxy_progress::happenings`.

use galaxy_contract::happening::{CauseFact, Happening, HappeningRun, SHOW_KINDS};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};

use crate::History;

/// The most dockings a happening can last.
pub const DOCKINGS_MAX: u32 = 12;
/// The most `show` entries and cause facts a happening has.
pub const SHOW_MAX: usize = 8;
pub const CAUSES_MAX: usize = 8;
/// The longest `show` entry and line ID, in characters.
const SHOW_LEN: usize = 48;
const LINE_ID_LEN: usize = 64;

fn word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
}

fn check_cause(world: &WorldState, i: usize, c: &CauseFact, errors: &mut Vec<String>) {
    let at = format!("cause fact {i}");
    match (&c.id, &c.kind) {
        (Some(_), Some(_)) => errors.push(format!("{at} names both an `id` and a `kind`")),
        (None, None) => errors.push(format!("{at} needs a fact `id` or a `kind` to match")),
        (Some(_), None) => {
            let pattern = c.station.is_some()
                || c.npc.is_some()
                || c.faction.is_some()
                || c.ware.is_some()
                || c.other.is_some()
                || c.amount.is_some();
            if pattern {
                errors.push(format!("{at} names a fact by `id`, so it takes no pattern"));
            }
        }
        (None, Some(kind)) => {
            if !word(kind) || kind.starts_with('.') || kind.ends_with('.') {
                errors.push(format!("{at}: `{kind}` isn't a fact kind"));
            }
        }
    }
    if let Some(s) = &c.station
        && !world.stations.contains(s)
    {
        errors.push(format!("{at}: unknown station `{s}`"));
    }
    if let Some(n) = &c.npc
        && !world.npcs.contains(n)
    {
        errors.push(format!("{at}: unknown person `{n}`"));
    }
    if let Some(f) = &c.faction
        && !world.factions.contains(f)
    {
        errors.push(format!("{at}: unknown faction `{f}`"));
    }
    if let Some(w) = &c.ware
        && !world.wares.contains(w)
    {
        errors.push(format!("{at}: unknown ware `{w}`"));
    }
}

/// The dock-talk line a happening's speaker says when it starts, citing `cites`.
pub fn start_line(h: &Happening, cites: Vec<FactId>) -> TalkLine {
    TalkLine {
        channel: Channel::DockTalk,
        speaker: Some(h.line.speaker.clone()),
        place: Some(h.station.clone()),
        text: h.line.text.clone(),
        cites,
        about: Vec::new(),
        choices: h.line.choices.clone(),
    }
}

/// Checks `h` against `world` as it would be with `h` in place (replacing the
/// happening with the same ID, if any). `Err` lists every problem.
pub fn check(world: &WorldState, h: &Happening) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if !world.stations.contains(&h.station) {
        return Err(vec![format!("unknown station `{}`", h.station)]);
    }
    let speaker = &h.line.speaker;
    match world.npcs.get(speaker) {
        None => errors.push(format!("unknown speaker `{speaker}`")),
        Some(n) if n.location.as_ref() != Some(&h.station) => {
            errors.push(format!("speaker `{speaker}` isn't at {}", h.station))
        }
        Some(_) => {}
    }
    let id = &h.line.id;
    if !id.starts_with("line.") || !word(id) || id.chars().count() > LINE_ID_LEN {
        errors.push(format!(
            "line id `{id}` should be `line.<speaker>.<name>` in [a-z0-9_.], at most {LINE_ID_LEN} characters"
        ));
    }
    // Validated as the line it will be, citing a placeholder fact.
    if let Err(e) = start_line(h, vec![FactId(1)]).validate() {
        errors.push(format!("the line: {e}"));
    } else if h.line.text.contains(['\n', '\r']) {
        errors.push("the line must be one line".into());
    } else if h.line.text.chars().count() > crate::parts::LINE_MAX {
        errors.push(format!(
            "the line is {} characters; at most {}",
            h.line.text.chars().count(),
            crate::parts::LINE_MAX
        ));
    }
    if !(1..=DOCKINGS_MAX).contains(&h.lasts.dockings) {
        errors.push(format!(
            "lasts {} dockings; a happening lasts 1 to {DOCKINGS_MAX}",
            h.lasts.dockings
        ));
    }
    if h.show.len() > SHOW_MAX {
        errors.push(format!("{} show entries; at most {SHOW_MAX}", h.show.len()));
    }
    for s in &h.show {
        match s.split_once(':') {
            Some((kind, value))
                if SHOW_KINDS.contains(&kind)
                    && !value.is_empty()
                    && s.chars().count() <= SHOW_LEN
                    && !s.chars().any(char::is_whitespace) => {}
            _ => errors.push(format!(
                "show `{s}` should be `<kind>:<value>` with a kind from {SHOW_KINDS:?}, no spaces, at most {SHOW_LEN} characters"
            )),
        }
    }
    if h.cause.facts.is_empty() {
        errors.push("the cause names no fact".into());
    } else if h.cause.facts.len() > CAUSES_MAX {
        errors.push(format!(
            "the cause names {} facts; at most {CAUSES_MAX}",
            h.cause.facts.len()
        ));
    }
    for (i, c) in h.cause.facts.iter().enumerate() {
        check_cause(world, i, c, &mut errors);
    }
    if h.is_active()
        && let Some(other) = world
            .happenings
            .values()
            .find(|o| o.id != h.id && o.station == h.station && o.is_active())
    {
        errors.push(format!(
            "`{}` is already running at {}: one happening at a time per station",
            other.id, h.station
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// What only history can check: every cause fact named by ID exists.
pub fn check_facts(history: &History, h: &Happening) -> Result<(), Vec<String>> {
    let missing: Vec<String> = h
        .cause
        .facts
        .iter()
        .filter_map(|c| c.id)
        .filter(|id| history.fact(*id).is_none())
        .map(|id| format!("the cause cites `{id}`, which isn't a history fact"))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// The station's counted player dockings (the kernel's guarded counter).
pub fn dockings_at(world: &WorldState, station: &StationId) -> u64 {
    world.counters.get(
        CounterKind::Dockings,
        &CounterScope::Station {
            station: station.clone(),
        },
    )
}

/// Whether `h` is running now: started, and the station's counted dockings haven't
/// reached its end yet (so it reads as over from the docking that ends it, before the
/// runtime records the end).
pub fn is_running(world: &WorldState, h: &Happening) -> bool {
    match &h.run {
        HappeningRun::Active { first_docking, .. } => {
            dockings_at(world, &h.station) < first_docking + u64::from(h.lasts.dockings)
        }
        _ => false,
    }
}

/// The happenings at `station`, in ID order.
pub fn of_station<'a>(
    world: &'a WorldState,
    station: &StationId,
) -> impl Iterator<Item = &'a Happening> + 'a {
    let station = station.clone();
    world
        .happenings
        .values()
        .filter(move |h| h.station == station)
}

/// The happening running at `station` now, if any.
pub fn active<'a>(world: &'a WorldState, station: &StationId) -> Vec<&'a Happening> {
    world
        .happenings
        .values()
        .filter(|h| &h.station == station && is_running(world, h))
        .collect()
}

/// The newest fact in `history` that `c` names, if any.
pub fn find_cause<'a>(
    history: &'a History,
    c: &CauseFact,
) -> Option<&'a galaxy_contract::history::Fact> {
    match c.id {
        Some(id) => history.fact(id),
        None => history.facts().iter().rev().find(|f| c.matches(f)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_contract::happening::{HappeningCause, HappeningLine, Lasts};

    fn world() -> WorldState {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        crate::load_world(&dir, 1).unwrap()
    }

    fn lamp_night() -> Happening {
        Happening {
            id: "happening.wharf.test_night".parse().unwrap(),
            station: "station.lantern_wharf".parse().unwrap(),
            show: vec!["banner_string:lamps".into(), "residents:+6".into()],
            lasts: Lasts { dockings: 3 },
            cause: HappeningCause {
                facts: vec![CauseFact {
                    kind: Some("stage_built".into()),
                    station: Some("station.kettle_dock".parse().unwrap()),
                    amount: Some(3),
                    ..CauseFact::default()
                }],
            },
            line: HappeningLine {
                id: "line.ilse_marrow.test_night".into(),
                speaker: "npc.ilse_marrow".parse().unwrap(),
                text: "We've strung lamps along the pier tonight.".into(),
                choices: Vec::new(),
            },
            by: None,
            run: HappeningRun::Upcoming,
        }
    }

    #[test]
    fn the_authored_happenings_pass() {
        let w = world();
        assert!(
            w.happenings.len() >= 2,
            "the core pack authors the greenhouse happenings"
        );
        for h in w.happenings.values() {
            check(&w, h).unwrap_or_else(|e| panic!("{}: {e:?}", h.id));
            assert!(h.run.is_upcoming(), "{} starts upcoming", h.id);
        }
    }

    #[test]
    fn a_good_happening_passes_and_each_rule_refuses_its_own_mistake() {
        let w = world();
        check(&w, &lamp_night()).unwrap();
        let refused = |edit: &dyn Fn(&mut Happening), says: &str| {
            let mut h = lamp_night();
            edit(&mut h);
            let errors = check(&w, &h).unwrap_err();
            assert!(
                errors.iter().any(|e| e.contains(says)),
                "{says}: {errors:?}"
            );
        };
        refused(
            &|h| h.station = "station.nowhere".parse().unwrap(),
            "unknown station",
        );
        refused(
            &|h| h.line.speaker = "npc.odo_fenn".parse().unwrap(),
            "isn't at",
        );
        refused(
            &|h| h.line.speaker = "npc.nobody".parse().unwrap(),
            "unknown speaker",
        );
        refused(&|h| h.line.id = "ilse.night".into(), "line id");
        refused(&|h| h.line.text = "Two\nlines".into(), "the line");
        refused(&|h| h.line.text.clear(), "the line");
        refused(&|h| h.lasts.dockings = 0, "lasts 0 dockings");
        refused(&|h| h.lasts.dockings = 99, "lasts 99 dockings");
        refused(
            &|h| h.show.push("fireworks:big".into()),
            "show `fireworks:big`",
        );
        refused(&|h| h.show.push("crowd".into()), "show `crowd`");
        refused(&|h| h.cause.facts.clear(), "names no fact");
        refused(
            &|h| h.cause.facts[0].kind = None,
            "needs a fact `id` or a `kind`",
        );
        refused(
            &|h| h.cause.facts[0].station = Some("station.nowhere".parse().unwrap()),
            "unknown station `station.nowhere`",
        );
        refused(
            &|h| {
                h.cause.facts.push(CauseFact {
                    id: Some(FactId(1)),
                    kind: Some("dock".into()),
                    ..CauseFact::default()
                })
            },
            "both",
        );
    }

    #[test]
    fn one_happening_runs_at_a_time_per_station() {
        let mut w = world();
        let mut first = lamp_night();
        first.run = HappeningRun::Active {
            since: Tick(1),
            first_docking: 0,
            cites: vec![FactId(1)],
        };
        check(&w, &first).unwrap();
        w.happenings.insert(first.id.clone(), first.clone());
        let mut second = first.clone();
        second.id = "happening.wharf.second".parse().unwrap();
        let errors = check(&w, &second).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("one happening at a time")));
        // Waiting is fine.
        second.run = HappeningRun::Upcoming;
        check(&w, &second).unwrap();
        // Rewriting the running one is fine too.
        check(&w, &first).unwrap();
    }

    #[test]
    fn running_is_counted_in_dockings_at_the_station() {
        let mut w = world();
        let wharf: StationId = "station.lantern_wharf".parse().unwrap();
        let mut h = lamp_night();
        let now = dockings_at(&w, &wharf);
        h.run = HappeningRun::Active {
            since: Tick(1),
            first_docking: now,
            cites: vec![FactId(1)],
        };
        w.happenings.insert(h.id.clone(), h.clone());
        assert_eq!(active(&w, &wharf).len(), 1);
        let scope = CounterScope::Station {
            station: wharf.clone(),
        };
        w.counters.bump(CounterKind::Dockings, &scope, 2);
        assert_eq!(
            active(&w, &wharf).len(),
            1,
            "the third docking still shows it"
        );
        w.counters.bump(CounterKind::Dockings, &scope, 1);
        assert!(active(&w, &wharf).is_empty(), "over from the fourth");
    }

    #[test]
    fn saves_from_before_happenings_gain_the_authored_ones() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let mut w = world();
        let all = w.happenings.len();
        assert!(crate::happenings_backfill(&dir, &w).unwrap().is_none());
        w.happenings = Default::default();
        let batch = crate::happenings_backfill(&dir, &w).unwrap().unwrap();
        assert_eq!(batch.commands.len(), all);
        let mut sim = crate::game(crate::SimState::new(w));
        assert_eq!(crate::queue_happenings_backfill(&mut sim, &dir).unwrap(), 1);
        assert_eq!(crate::queue_happenings_backfill(&mut sim, &dir).unwrap(), 0);
        sim.step(&PlayerInput::default()).unwrap();
        assert_eq!(sim.world().happenings.len(), all);
    }
}
