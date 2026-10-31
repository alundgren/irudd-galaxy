//! The station-parts validator (issue #47, 3.1). Every part a station gets, authored or
//! written by a pack, passes [`check`]:
//!
//! - the station exists, and the anchor is one of its kit recipe's
//!   ([`galaxy_kit::anchors`]) and no other part of the station uses it;
//! - the module is a module kind ([`galaxy_kit::MODULE_KINDS`]) and the station stays
//!   within the art bible's budget: its recipe's parts plus its modules, per kind and in
//!   all;
//! - the keeper is a person at the station, and a part with a cause or that isn't open
//!   has the keeper's line; a cause cites at least one fact;
//! - the sign and the lines are one line each, short enough to read at a glance.
//!
//! Changes apply only at a calm moment ([`is_calm`]): the same rule the game-master
//! bridge stages packs by. Scripts run inside ticks, so for them that means docked: the
//! `ctx.parts` hooks refuse otherwise, and their batch expects the flagship still
//! docked where it was, so an undock earlier in the tick makes it contention.

use galaxy_contract::parts::{PartState, StationPart};
use galaxy_contract::prelude::*;
use galaxy_kit::StationRecipe;
use galaxy_kit::art_bible::ArtBible;

/// The longest sign, in characters: one line under a module.
pub const SIGN_MAX: usize = 64;
/// The longest keeper line or rollback line, in characters.
pub const LINE_MAX: usize = 160;
/// The highest level a part can be upgraded to.
pub const LEVEL_MAX: u32 = 5;

/// The kit recipe a station is drawn with: its runtime recipe if a pack gave it one,
/// else the authored one for its build stage.
pub fn recipe_of(world: &WorldState, station: &Station) -> StationRecipe {
    world
        .station_recipes
        .get(&station.id)
        .cloned()
        .unwrap_or_else(|| {
            let stage = u8::try_from(station.build.stage.min(3)).unwrap_or(3);
            StationRecipe::for_station(station.id.as_str(), stage)
        })
}

/// Where the flagship is docked, if it is.
pub fn docked_at(world: &WorldState) -> Option<StationId> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .and_then(|s| s.docked_at.clone())
}

/// Is the player at a calm moment, when station parts may change: paused, or the
/// flagship docked? (`galaxy_gm::bridge::is_calm` is the same rule over a `Sim`.)
pub fn is_calm(world: &WorldState, paused: bool) -> bool {
    paused || docked_at(world).is_some()
}

fn one_line(text: &str, max: usize, what: &str, errors: &mut Vec<String>) {
    if text.trim().is_empty() {
        errors.push(format!("{what} is empty"));
    } else if text.contains(['\n', '\r']) {
        errors.push(format!("{what} must be one line"));
    } else if text.chars().count() > max {
        errors.push(format!(
            "{what} is {} characters; at most {max}",
            text.chars().count()
        ));
    }
}

/// Checks `part` against `world` as it would be with the part in place (replacing the
/// part with the same ID, if any). `Err` lists every problem.
pub fn check(world: &WorldState, part: &StationPart) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let Some(station) = world.stations.get(&part.station) else {
        return Err(vec![format!("unknown station `{}`", part.station)]);
    };
    match world.npcs.get(&part.keeper) {
        None => errors.push(format!("unknown keeper `{}`", part.keeper)),
        Some(n) if n.location.as_ref() != Some(&part.station) => errors.push(format!(
            "keeper `{}` isn't at {}",
            part.keeper, part.station
        )),
        Some(_) => {}
    }
    let recipe = recipe_of(world, station);
    let others: Vec<&StationPart> = world
        .station_parts
        .values()
        .filter(|p| p.station == part.station && p.id != part.id)
        .collect();
    let anchors = galaxy_kit::anchors(part.station.as_str(), &recipe);
    if !anchors.iter().any(|a| a.name == part.anchor) {
        let names: Vec<_> = anchors.iter().map(|a| a.name).collect();
        errors.push(format!(
            "anchor `{}` isn't on {}'s recipe (it has {names:?})",
            part.anchor, part.station
        ));
    } else if let Some(taken) = others.iter().find(|p| p.anchor == part.anchor) {
        errors.push(format!(
            "anchor `{}` is taken by `{}`",
            part.anchor, taken.id
        ));
    }
    match galaxy_kit::module_part(&part.module, part.level.max(1)) {
        None => errors.push(format!(
            "module `{}` isn't a module kind ({:?})",
            part.module,
            galaxy_kit::MODULE_KINDS
        )),
        Some(kind) => {
            let bible = ArtBible::bundled();
            let name = galaxy_kit::kind_name(&kind);
            let same = recipe
                .parts
                .iter()
                .filter(|p| galaxy_kit::kind_name(&p.kind) == name)
                .count()
                + others.iter().filter(|p| p.module == part.module).count()
                + 1;
            let cap = bible.kind_cap(name) as usize;
            if same > cap {
                errors.push(format!(
                    "{same} {name} parts on {} with this module; the art bible allows {cap}",
                    part.station
                ));
            }
            let total = recipe.parts.len() + others.len() + 1;
            if total > bible.budget.max_parts {
                errors.push(format!(
                    "{total} parts on {}; the art bible allows {}",
                    part.station, bible.budget.max_parts
                ));
            }
        }
    }
    if !(1..=LEVEL_MAX).contains(&part.level) {
        errors.push(format!("level {} isn't 1 to {LEVEL_MAX}", part.level));
    }
    one_line(&part.sign, SIGN_MAX, "the sign", &mut errors);
    one_line(
        &part.rollback_line,
        LINE_MAX,
        "the rollback line",
        &mut errors,
    );
    if part.cause.is_some() || part.state != PartState::Open || !part.line.is_empty() {
        one_line(&part.line, LINE_MAX, "the keeper's line", &mut errors);
    }
    if let Some(cause) = &part.cause
        && cause.facts.is_empty()
    {
        errors.push("the cause cites no fact for the keeper's line".into());
    }
    if let Some(n) = &part.name {
        one_line(n, 24, "the name", &mut errors);
    }
    if let Some(q) = &part.quarter {
        one_line(q, 32, "the quarter", &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The parts at `station`, in a stable order (by ID).
pub fn of_station<'a>(
    world: &'a WorldState,
    station: &'a StationId,
) -> impl Iterator<Item = &'a StationPart> + 'a {
    world
        .station_parts
        .values()
        .filter(move |p| &p.station == station)
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_contract::parts::{PartCause, RoomKind};

    fn world() -> WorldState {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        crate::load_world(&dir, 1).unwrap()
    }

    fn canteen() -> StationPart {
        StationPart {
            id: "part.wharf.canteen".parse().unwrap(),
            station: "station.lantern_wharf".parse().unwrap(),
            room: RoomKind::Canteen,
            name: None,
            module: "dome".into(),
            anchor: "quay.northwest".into(),
            quarter: None,
            keeper: "npc.ilse_marrow".parse().unwrap(),
            state: PartState::Building,
            level: 1,
            sign: "Canteen · opens when Kettle ships rations".into(),
            line: "We're raising a canteen frame by the lamps.".into(),
            cause: Some(PartCause {
                script: Some("gm.test".parse().unwrap()),
                facts: vec![FactId(3)],
            }),
            rollback_line: "We've taken the canteen frame down to re-seal it.".into(),
            news: None,
        }
    }

    #[test]
    fn the_authored_parts_pass() {
        let w = world();
        assert!(w.station_parts.len() >= 10, "every station lists its parts");
        for p in w.station_parts.values() {
            check(&w, p).unwrap_or_else(|e| panic!("{}: {e:?}", p.id));
        }
    }

    #[test]
    fn a_good_part_passes_and_each_rule_refuses_its_own_mistake() {
        let w = world();
        check(&w, &canteen()).unwrap();
        let refused = |edit: &dyn Fn(&mut StationPart), says: &str| {
            let mut p = canteen();
            edit(&mut p);
            let errors = check(&w, &p).unwrap_err();
            assert!(
                errors.iter().any(|e| e.contains(says)),
                "{says}: {errors:?}"
            );
        };
        // The anchor exists on the recipe and is free.
        refused(&|p| p.anchor = "pier.nowhere".into(), "isn't on");
        refused(&|p| p.anchor = "quay.west".into(), "is taken by");
        // The module kind is a module kind and within the art bible's budget.
        refused(&|p| p.module = "spire".into(), "isn't a module kind");
        // A keeper at the station, with a line citing the cause.
        refused(&|p| p.keeper = "npc.odo_fenn".parse().unwrap(), "isn't at");
        refused(
            &|p| p.keeper = "npc.nobody".parse().unwrap(),
            "unknown keeper",
        );
        refused(&|p| p.line.clear(), "the keeper's line is empty");
        refused(
            &|p| p.cause.as_mut().unwrap().facts.clear(),
            "cites no fact",
        );
        refused(&|p| p.sign = "Two\nlines".into(), "one line");
        refused(&|p| p.sign = "x".repeat(SIGN_MAX + 1), "at most");
        refused(&|p| p.level = 9, "level 9");
        refused(
            &|p| p.station = "station.nowhere".parse().unwrap(),
            "unknown station",
        );
    }

    #[test]
    fn the_budget_counts_the_recipe_and_the_other_modules() {
        let mut w = world();
        // Cranes: the art bible allows two per station and the Wharf's recipe has
        // none. Two crane modules fit; turning the market into a third doesn't.
        let mut first = canteen();
        first.module = "crane".into();
        check(&w, &first).unwrap();
        w.station_parts.insert(first.id.clone(), first.clone());
        let mut second = canteen();
        second.id = "part.wharf.second_crane".parse().unwrap();
        second.anchor = "pier.southwest".into();
        second.module = "crane".into();
        check(&w, &second).unwrap();
        w.station_parts.insert(second.id.clone(), second);
        let mut third = w.station_parts.get("part.wharf.market").unwrap().clone();
        third.module = "crane".into();
        let errors = check(&w, &third).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("allows 2")), "{errors:?}");
    }

    #[test]
    fn changes_wait_for_a_calm_moment() {
        let mut w = world();
        assert!(docked_at(&w).is_some(), "a new game starts docked");
        assert!(is_calm(&w, false));
        let ship = w.player.ship.clone().unwrap();
        w.ships.get_mut(&ship).unwrap().docked_at = None;
        assert!(!is_calm(&w, false), "flying is not calm");
        assert!(is_calm(&w, true), "paused is");
    }
}
