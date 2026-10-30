//! Loading the progression tables (W4) into `world.progress`: controls, upgrades and
//! tenders, traits, hull ranks, regard tiers, deeds, prestige and the pacing
//! tunables. Dangling references are load errors like everywhere else.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::data::*;
use galaxy_contract::prelude::*;
use galaxy_contract::progress::*;

use crate::load::cents;

/// The authored files the progression tables come from.
pub(crate) struct Files {
    pub controls: ControlsFile,
    pub upgrades: UpgradesFile,
    pub parts: ShipPartsFile,
    pub hull_fits: Vec<galaxy_contract::fit::HullFit>,
    pub traits: TraitsFile,
    pub pacing: PacingFile,
    pub ranks: Vec<RankDef>,
    pub standing: Option<StandingRange>,
    pub regard: Vec<RegardTier>,
    pub deeds: Vec<DeedRule>,
}

/// Most a price in these tables may be, in centicredits.
const MAX_COST: i64 = i64::MAX / 4;

pub(crate) fn build(world: &mut WorldState, files: Files, errors: &mut Vec<String>) {
    let mut rules = ProgressRules {
        profile: files.pacing.profile,
        standing_min: files.standing.as_ref().map_or(-100, |s| s.min),
        standing_max: files.standing.as_ref().map_or(100, |s| s.max),
        tiers: files.controls.tier,
        regard: files.regard,
        deeds: files.deeds,
        ranks: files.ranks,
        ..ProgressRules::default()
    };
    if rules.standing_min > 0 || rules.standing_max < 0 {
        errors.push("factions.toml: [standing] must include 0".into());
    }
    for c in files.controls.capability {
        if let Some(old) = rules.capabilities.insert(c.id.clone(), c) {
            errors.push(format!("controls.toml: duplicate capability `{}`", old.id));
        }
    }
    for s in files.controls.slot {
        if let Some(old) = rules.slots.insert(s.id.clone(), s) {
            errors.push(format!("controls.toml: duplicate slot `{}`", old.id));
        }
    }
    for u in files.upgrades.upgrade {
        let def = UpgradeDef {
            cost: cents(
                u.cost,
                MAX_COST,
                &format!("upgrades.toml: {} cost", u.id),
                errors,
            ),
            id: u.id,
            name: u.name,
            effect: u.effect,
            seller: u.seller,
            station: u.station,
            unlock: u.unlock,
            grants: u.grants,
            modifiers: u.modifiers,
            socket: u.socket,
            power: u.power,
            mass: u.mass,
            look: u.look,
            source: u.source,
        };
        if let Some(old) = rules.upgrades.insert(def.id.clone(), def) {
            errors.push(format!("upgrades.toml: duplicate upgrade `{}`", old.id));
        }
    }
    for c in files.upgrades.craft {
        let def = CraftDef {
            cost: cents(
                c.cost,
                MAX_COST,
                &format!("upgrades.toml: {} cost", c.id),
                errors,
            ),
            id: c.id,
            name: c.name,
            effect: c.effect,
            hull: c.hull,
            seller: c.seller,
            station: c.station,
            unlock: c.unlock,
            grants: c.grants,
        };
        if let Some(old) = rules.crafts.insert(def.id.clone(), def) {
            errors.push(format!("upgrades.toml: duplicate craft `{}`", old.id));
        }
    }
    let fit = &mut rules.fit;
    fit.mass = files.parts.mass;
    for x in files.parts.socket {
        if let Some(old) = fit.sockets.insert(x.id.clone(), x) {
            errors.push(format!("ship_parts.toml: duplicate socket `{}`", old.id));
        }
    }
    for x in files.parts.wharf {
        if let Some(old) = fit.wharves.insert(x.station.clone(), x) {
            errors.push(format!("ship_parts.toml: two wharves at `{}`", old.station));
        }
    }
    for x in files.parts.blueprint {
        if let Some(old) = fit.blueprints.insert(x.id.clone(), x) {
            errors.push(format!("ship_parts.toml: duplicate blueprint `{}`", old.id));
        }
    }
    for x in files.hull_fits {
        if let Some(old) = fit.hulls.insert(x.hull.clone(), x) {
            errors.push(format!("hulls.toml: two [[fit]] tables for `{}`", old.hull));
        }
    }
    for t in files.traits.traits {
        if let Some(old) = rules.traits.insert(t.id.clone(), t) {
            errors.push(format!("traits.toml: duplicate trait `{}`", old.id));
        }
    }
    rules.prestige = files.pacing.prestige.map(|p| PrestigeRules {
        fee: cents(p.fee, MAX_COST, "pacing.toml: prestige fee", errors),
        station: p.station,
        seller: p.seller,
        branches: p.branch,
        requires: p.requires,
    });
    for (id, value) in files.pacing.tunables {
        world.progress.tunables.insert(
            id,
            Tunable {
                authored: value.clone(),
                value,
                set_by: None,
            },
        );
    }
    check(world, &rules, errors);
    world.progress.rules = rules;
}

fn check(world: &WorldState, rules: &ProgressRules, errors: &mut Vec<String>) {
    let npc = |id: &Option<NpcId>, what: &str, errors: &mut Vec<String>| {
        if let Some(n) = id
            && !world.npcs.contains(n)
        {
            errors.push(format!("{what}: unknown npc `{n}`"));
        }
    };
    let station = |id: &StationId, what: &str, errors: &mut Vec<String>| {
        if !world.stations.contains(id) {
            errors.push(format!("{what}: unknown station `{id}`"));
        }
    };
    let hull = |id: &HullId, what: &str, errors: &mut Vec<String>| {
        if !world.hulls.contains(id) {
            errors.push(format!("{what}: unknown hull `{id}`"));
        }
    };
    let grants = |flags: &[FlagId], what: &str, errors: &mut Vec<String>| {
        for f in flags {
            if f.as_str().starts_with("cap.") && !rules.capabilities.contains_key(f) {
                errors.push(format!(
                    "{what}: grants `{f}`, which isn't a capability in controls.toml"
                ));
            }
        }
    };

    // The ship crate unlocks input modes by the flags ship_rules.toml names; each must
    // be a capability here, so upgrades and beats set the flags the ship reads.
    if !rules.capabilities.is_empty() {
        let c = &world.ship_rules.controls;
        for flag in [
            &c.click_fly,
            &c.click_mine,
            &c.click_dock,
            &c.map_travel,
            &c.queues,
            &c.loops,
            &c.automation,
        ] {
            if !rules.capabilities.contains_key(flag) {
                errors.push(format!(
                    "ship_rules.toml: control flag `{flag}` isn't a capability in controls.toml"
                ));
            }
        }
    }
    let tiers: BTreeSet<u32> = rules.tiers.iter().map(|t| t.tier).collect();
    for c in rules.capabilities.values() {
        if !tiers.contains(&c.tier) {
            errors.push(format!(
                "controls.toml: {} is in tier {}, which isn't listed",
                c.id, c.tier
            ));
        }
    }
    for u in rules.upgrades.values() {
        let what = format!("upgrades.toml: {}", u.id);
        npc(&u.seller, &what, errors);
        if let Some(s) = &u.station {
            station(s, &what, errors);
        }
        grants(&u.grants, &what, errors);
        requirement(&u.unlock, world, rules, &what, errors);
    }
    check_fit(world, rules, errors);
    for c in rules.crafts.values() {
        let what = format!("upgrades.toml: {}", c.id);
        npc(&c.seller, &what, errors);
        station(&c.station, &what, errors);
        hull(&c.hull, &what, errors);
        grants(&c.grants, &what, errors);
        if world.ships.contains(&c.id) {
            errors.push(format!("{what}: a ship with that ID already exists"));
        }
        requirement(&c.unlock, world, rules, &what, errors);
    }
    for t in rules.traits.values() {
        let what = format!("traits.toml: {}", t.id);
        if t.times == 0 {
            errors.push(format!("{what}: `times` must be at least 1"));
        }
        for n in &t.noticed_by {
            npc(&Some(n.clone()), &what, errors);
        }
    }
    let mut ranks: BTreeMap<&HullId, Vec<u32>> = BTreeMap::new();
    for r in &rules.ranks {
        hull(&r.hull, "hulls.toml: [[rank]]", errors);
        ranks.entry(&r.hull).or_default().push(r.rank);
    }
    for (h, list) in ranks {
        let want: Vec<u32> = (1..=list.len() as u32).collect();
        if list != want {
            errors.push(format!(
                "hulls.toml: {h}'s ranks must be 1, 2, 3 ... in order (got {list:?})"
            ));
        }
    }
    let mut last = None;
    let mut ids = BTreeSet::new();
    for t in &rules.regard {
        if last.is_some_and(|l| t.min_standing <= l) {
            errors.push("factions.toml: [[regard]] tiers must rise by min_standing".into());
        }
        last = Some(t.min_standing);
        if !ids.insert(&t.id) {
            errors.push(format!("factions.toml: duplicate regard tier `{}`", t.id));
        }
    }
    if let Some(first) = rules.regard.first()
        && first.min_standing > rules.standing_min
    {
        errors.push("factions.toml: the lowest regard tier must cover the lowest standing".into());
    }
    if !rules.regard.is_empty() {
        let thresholds: BTreeSet<i32> = rules.regard.iter().map(|t| t.min_standing).collect();
        for t in &world.economy.rules.pricing.regard {
            if t.min_standing > rules.standing_min && !thresholds.contains(&t.min_standing) {
                errors.push(format!(
                    "economy.toml: a spread starts at standing {}, which isn't a regard tier in factions.toml",
                    t.min_standing
                ));
            }
        }
    }
    let mut deed_ids = BTreeSet::new();
    for d in &rules.deeds {
        let what = format!("factions.toml: deed {}", d.id);
        if !deed_ids.insert(&d.id) {
            errors.push(format!("{what}: duplicate"));
        }
        if !world.factions.contains(&d.faction) {
            errors.push(format!("{what}: unknown faction `{}`", d.faction));
        }
        if d.on.kinds.is_empty() {
            errors.push(format!("{what}: `on.kinds` names no fact kind"));
        }
        for s in &d.on.stations {
            station(s, &what, errors);
        }
        for w in &d.on.wares {
            if !world.wares.contains(w) {
                errors.push(format!("{what}: unknown ware `{w}`"));
            }
        }
        if d.per == Some(0) {
            errors.push(format!("{what}: `per` must be at least 1"));
        }
    }
    if let Some(p) = &rules.prestige {
        let what = "pacing.toml: [prestige]";
        station(&p.station, what, errors);
        npc(&p.seller, what, errors);
        for b in &p.branches {
            hull(&b.from, what, errors);
            for t in &b.to {
                hull(t, what, errors);
            }
            if world.ships.contains(&b.berth_as) {
                errors.push(format!(
                    "{what}: berth_as `{}` is already a ship",
                    b.berth_as
                ));
            }
        }
        requirement(&p.requires, world, rules, what, errors);
    }
}

/// The fit model's data: sockets, hull fits, wharves, blueprints and each part's
/// socket and source refer to things that exist, and hold together.
fn check_fit(world: &WorldState, rules: &ProgressRules, errors: &mut Vec<String>) {
    use galaxy_contract::fit::PartSource;
    let fit = &rules.fit;
    let socket = |id: &SocketId, what: &str, errors: &mut Vec<String>| {
        if !fit.sockets.contains_key(id) {
            errors.push(format!("{what}: unknown socket `{id}`"));
        }
    };
    let m = &fit.mass;
    if !(0.0..=1.0).contains(&m.floor) || m.speed_per_unit < 0.0 || m.turn_per_unit < 0.0 {
        errors.push("ship_parts.toml: [mass] floor must be 0..=1 and the shares 0 or more".into());
    }
    for h in fit.hulls.values() {
        let what = format!("hulls.toml: [[fit]] {}", h.hull);
        if !world.hulls.contains(&h.hull) {
            errors.push(format!("{what}: unknown hull"));
        }
        let mut seen = BTreeSet::new();
        for s in &h.sockets {
            socket(s, &what, errors);
            if !seen.insert(s) {
                errors.push(format!("{what}: `{s}` listed twice"));
            }
        }
    }
    for w in fit.wharves.values() {
        let what = format!("ship_parts.toml: wharf at {}", w.station);
        if !world.stations.contains(&w.station) {
            errors.push(format!("{what}: unknown station"));
        }
        if !world.npcs.contains(&w.keeper) {
            errors.push(format!("{what}: unknown keeper `{}`", w.keeper));
        }
        for s in &w.sockets {
            socket(s, &what, errors);
            if fit.sockets.get(s).is_some_and(|d| d.rack) {
                errors.push(format!("{what}: the control rack needs no wharf"));
            }
        }
    }
    for b in fit.blueprints.values() {
        let what = format!("ship_parts.toml: blueprint {}", b.id);
        match rules.upgrades.get(&b.part) {
            Some(u) if matches!(&u.source, PartSource::Make { blueprint, .. } if blueprint == &b.id) =>
                {}
            _ => errors.push(format!(
                "{what}: `{}` isn't a part made with this blueprint",
                b.part
            )),
        }
        requirement(&b.found, world, rules, &what, errors);
    }
    for u in rules.upgrades.values() {
        let what = format!("upgrades.toml: {}", u.id);
        let Some(s) = &u.socket else {
            if !fit.is_empty() {
                errors.push(format!(
                    "{what}: needs a `socket` (`socket.rack` for a control module)"
                ));
            }
            continue;
        };
        socket(s, &what, errors);
        let rack = fit.sockets.get(s).is_some_and(|d| d.rack);
        if rack && !u.source.is_ready() {
            errors.push(format!(
                "{what}: a control module comes ready from its seller"
            ));
        }
        if !rack && u.look.is_none() {
            errors.push(format!("{what}: a part needs a `look` for the renderer"));
        }
        if !u.source.is_ready() || (!rack && u.station.is_some()) {
            let wharf = u.station.as_ref().and_then(|st| fit.wharves.get(st));
            match wharf {
                None => errors.push(format!("{what}: its `station` has no wharf")),
                Some(w) if !w.sockets.contains(s) => errors.push(format!(
                    "{what}: the wharf at {} doesn't work on `{s}`",
                    w.station
                )),
                Some(_) => {}
            }
        }
        match &u.source {
            PartSource::Ready => {}
            PartSource::Make {
                blueprint,
                materials,
            } => {
                if !fit.blueprints.contains_key(blueprint) {
                    errors.push(format!(
                        "{what}: no blueprint `{blueprint}` in ship_parts.toml"
                    ));
                }
                if materials.is_empty() || materials.iter().any(|m| m.units == 0) {
                    errors.push(format!("{what}: making it needs materials"));
                }
                for m in materials {
                    if !world.wares.contains(&m.ware) {
                        errors.push(format!("{what}: unknown ware `{}`", m.ware));
                    }
                }
            }
            PartSource::Friend { npc, station } => {
                if !world.npcs.contains(npc) {
                    errors.push(format!("{what}: unknown npc `{npc}`"));
                }
                if !world.stations.contains(station) {
                    errors.push(format!("{what}: unknown station `{station}`"));
                }
            }
        }
    }
}

/// Checks the world IDs a data requirement names. Beats are registered by scripts, so
/// they can't be checked here (`galaxy pack check` does).
fn requirement(
    req: &Requirement,
    world: &WorldState,
    rules: &ProgressRules,
    what: &str,
    errors: &mut Vec<String>,
) {
    use Requirement as R;
    use galaxy_contract::progress::Num;
    let num = |n: &Num, errors: &mut Vec<String>| {
        if let Num::Tunable { tunable } = n
            && !world
                .progress
                .tunables
                .get(tunable)
                .is_some_and(|t| t.value.as_number().is_some())
        {
            errors.push(format!(
                "{what}: requirement reads tunable `{tunable}`, which isn't a number in pacing.toml"
            ));
        }
    };
    match req {
        R::Counter { at_least, .. }
        | R::Facts { at_least, .. }
        | R::Sum { at_least, .. }
        | R::Credits { at_least }
        | R::Cargo { at_least, .. }
        | R::HoldFree { at_least } => num(at_least, errors),
        _ => {}
    }
    let mut bad = |kind: &str, id: &dyn std::fmt::Display| {
        errors.push(format!("{what}: requirement names unknown {kind} `{id}`"));
    };
    match req {
        R::All(list) | R::Any(list) => {
            for r in list {
                requirement(r, world, rules, what, errors);
            }
        }
        R::Not(r) => requirement(r, world, rules, what, errors),
        R::Counter {
            station, region, ..
        } => {
            if let Some(s) = station.as_ref().filter(|s| !world.stations.contains(*s)) {
                bad("station", s);
            }
            if let Some(r) = region.as_ref().filter(|r| !world.regions.contains(*r)) {
                bad("region", r);
            }
        }
        R::Trait(t) if !rules.traits.contains_key(t) => bad("trait", t),
        R::Fitted(u) if !rules.upgrades.contains_key(u) => bad("upgrade", u),
        R::Hull(h) if !world.hulls.contains(h) => bad("hull", h),
        R::Cargo { ware, .. } if !world.wares.contains(ware) => bad("ware", ware),
        R::DockedAt(s) if !world.stations.contains(s) => bad("station", s),
        R::InRegion(r) | R::Near { region: r, .. } if !world.regions.contains(r) => {
            bad("region", r)
        }
        R::Regard { faction, at_least } => {
            if !world.factions.contains(faction) {
                bad("faction", faction);
            }
            if !rules.regard.iter().any(|t| &t.id == at_least) {
                bad("regard tier", at_least);
            }
        }
        R::Test(_) => errors.push(format!(
            "{what}: a `test` requirement is a Luau predicate; data files can't hold one"
        )),
        _ => {}
    }
}
