//! Menus: the allowed, complete options for one moment, best first.
//!
//! A [`MenuProvider`] looks at each new history fact and may open menus. Option 0 is
//! the utility top choice and always a complete answer: the host applies it whenever
//! the model is out of budget, slow, wrong or absent.
//!
//! Built in:
//! - [`TraderMenus`]: an NPC craft docks; its route and cargo plans come from a
//!   [`PlanSource`] (the economy's `top_choices` at integration).
//! - [`DockTalk`]: the player docks; eligible authored lines come from a
//!   [`LineSource`] (the talk store's matcher at integration), each filled from the
//!   fact it cites. The model picks one; the game says it as filled. No line, no talk.
//! - [`RumorSpread`]: an NPC craft docks and carries a rumor from its last station,
//!   with the rumor's truth value unchanged.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_core::History;
use serde_json::json;

use crate::config::MindConfig;

pub const TRADER_ROUTE: &str = "trader_route";
pub const DOCK_TALK: &str = "dock_talk";
pub const RUMOR_SPREAD: &str = "rumor_spread";
/// Every menu kind a brief may allow.
pub const KINDS: [&str; 3] = [TRADER_ROUTE, DOCK_TALK, RUMOR_SPREAD];

/// What providers read.
pub struct MenuCtx<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    pub config: &'a MindConfig,
}

/// One moment's choices.
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub kind: &'static str,
    /// Whose brief decides: `npc.*` or `craft.*`.
    pub actor: AnyId,
    /// The named person, for `Cause::Mind { npc }`.
    pub npc: Option<NpcId>,
    pub ship: Option<ShipId>,
    pub station: Option<StationId>,
    /// The fact that opened the menu.
    pub trigger: FactId,
    /// What the model is asked, in one or two sentences.
    pub instructions: String,
    /// Best first; `options[0]` is the default.
    pub options: Vec<MenuOption>,
    /// Facts the menu is about; always in the packet.
    pub cites: Vec<FactId>,
    /// Must still hold when the answer is applied.
    pub preconditions: Vec<Precondition>,
}

impl Menu {
    /// Whether `cause` is this menu's outcome: a fast-model decision on the same kind
    /// of menu, for the same person, citing the same trigger. Another menu the same
    /// docking opened (a rumor the craft carried in) doesn't answer this one.
    pub fn answered_by(&self, cause: &Cause) -> bool {
        matches!(&cause.kind, CauseKind::Mind { npc, decision }
            if *npc == self.npc
                && decision.split_once(':').is_some_and(|(kind, _)| kind == self.kind))
            && cause.cites.contains(&self.trigger)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuOption {
    /// Stable within the menu (`plan.1`, a rumor ID, a line ID).
    pub id: String,
    /// What it means, for the model and the cause summary.
    pub text: String,
    /// Utility score; options are sorted by it.
    pub score: f64,
    /// IDs it touches. Each must exist when it is applied.
    pub refs: Vec<AnyId>,
    /// What it does. A provider's `resolve` may recompute them.
    pub commands: Vec<Command>,
    /// For dock talk: the line it says.
    pub line: Option<LineCandidate>,
}

/// A world condition an answer depends on.
#[derive(Clone, Debug, PartialEq)]
pub enum Precondition {
    Docked { ship: ShipId, station: StationId },
}

impl Precondition {
    pub fn holds(&self, world: &WorldState) -> Result<(), String> {
        match self {
            Precondition::Docked { ship, station } => {
                match world.ships.get(ship).map(|s| s.docked_at.as_ref()) {
                    Some(Some(at)) if at == station => Ok(()),
                    Some(_) => Err(format!("{ship} is no longer docked at {station}")),
                    None => Err(format!("{ship} no longer exists")),
                }
            }
        }
    }
}

/// Opens menus from history facts and turns a chosen option into commands.
pub trait MenuProvider {
    fn name(&self) -> &'static str;
    /// Recover saved requests that have no committed outcome when a host reopens.
    fn resume(&self, _cx: &MenuCtx<'_>, _facts: &[Fact]) -> Vec<Menu> {
        Vec::new()
    }
    /// Menus this fact opens (usually none).
    fn menus(&self, cx: &MenuCtx<'_>, fact: &Fact) -> Vec<Menu>;
    /// The commands for `option` against the world as it is now. The default uses the
    /// commands built with the menu; override to re-price or re-plan.
    fn resolve(
        &self,
        _world: &WorldState,
        _menu: &Menu,
        option: &MenuOption,
    ) -> Result<Vec<Command>, String> {
        Ok(option.commands.clone())
    }
}

fn docked(fact: &Fact) -> Option<(&ShipId, &StationId)> {
    match &fact.payload {
        FactPayload::Event(Event::Dock { ship, station }) => Some((ship, station)),
        _ => None,
    }
}

fn any(id: &str) -> Option<AnyId> {
    AnyId::new(id).ok()
}

fn top(mut options: Vec<MenuOption>, n: usize) -> Vec<MenuOption> {
    options.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    options.truncate(n);
    options
}

// --- Trader routes ---

/// One complete trade plan, ready to apply.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanOption {
    /// One line a person (or a model) can read.
    pub summary: String,
    pub score: f64,
    pub refs: Vec<AnyId>,
    pub commands: Vec<Command>,
}

/// Where trade plans come from. The economy crate's `rank_trades` / `top_choices`
/// plugs in here at integration.
pub trait PlanSource {
    /// Every plan this craft may make, docked where it is. Order doesn't matter.
    fn plans(&self, world: &WorldState, ship: &ShipId) -> Vec<PlanOption>;
    /// The plan's commands against the world as it is now (default: as built).
    fn replan(&self, _world: &WorldState, _ship: &ShipId, plan: &MenuOption) -> Vec<Command> {
        plan.commands.clone()
    }
}

/// Opens a `trader_route` menu when a craft with a trader brief docks.
pub struct TraderMenus<P: PlanSource> {
    pub plans: P,
}

impl<P: PlanSource> MenuProvider for TraderMenus<P> {
    fn name(&self) -> &'static str {
        TRADER_ROUTE
    }

    /// A craft whose route choice was still out when the save was written sits docked
    /// with no orders, and only a new docking would ask again. On reopening, each such
    /// craft is asked again for its last docking, unless a committed route decision for
    /// that craft already answered it (the host also skips one whose answer is queued in
    /// the save). Other decisions citing the docking, such as the rumor it carried in,
    /// don't count. The outcome is the model's pick or the utility default, so it never
    /// stays parked.
    fn resume(&self, cx: &MenuCtx<'_>, facts: &[Fact]) -> Vec<Menu> {
        let decided: Vec<(&Cause, Option<&ShipId>)> = cx
            .history
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == crate::DECISION_FACT)
            .filter_map(|f| Some((&cx.history.cause(f.cause)?.cause, f.subjects.ship.as_ref())))
            .collect();
        let mut out = Vec::new();
        for (id, ship) in cx.world.ships.iter() {
            if !cx.world.traders.contains_key(id)
                || ship.docked_at.is_none()
                || !ship.orders.orders.is_empty()
            {
                continue;
            }
            // The craft's latest docking or undocking, if it was saved: a newer one
            // opens its menu the usual way.
            let last = cx.history.facts().iter().rev().find(|f| {
                f.subjects.ship.as_ref() == Some(id)
                    && matches!(
                        f.payload,
                        FactPayload::Event(Event::Dock { .. } | Event::Undock { .. })
                    )
            });
            if let Some(f) = last
                && facts.last().is_some_and(|saved| f.id <= saved.id)
                && docked(f).is_some_and(|(_, at)| ship.docked_at.as_ref() == Some(at))
            {
                out.extend(self.menus(cx, f).into_iter().filter(|m| {
                    !decided
                        .iter()
                        .any(|(cause, ship)| *ship == m.ship.as_ref() && m.answered_by(cause))
                }));
            }
        }
        out
    }

    fn menus(&self, cx: &MenuCtx<'_>, fact: &Fact) -> Vec<Menu> {
        let Some((ship, station)) = docked(fact) else {
            return Vec::new();
        };
        let Some(brief) = cx.config.brief(ship.as_str()) else {
            return Vec::new();
        };
        if !brief.allows(TRADER_ROUTE) {
            return Vec::new();
        }
        let options: Vec<MenuOption> = self
            .plans
            .plans(cx.world, ship)
            .into_iter()
            .map(|p| MenuOption {
                id: String::new(),
                text: p.summary,
                score: p.score,
                refs: p.refs,
                commands: p.commands,
                line: None,
            })
            .collect();
        let mut options = top(options, cx.config.limits.menu_size);
        for (i, o) in options.iter_mut().enumerate() {
            o.id = format!("plan.{}", i + 1);
        }
        if options.is_empty() {
            return Vec::new();
        }
        vec![Menu {
            kind: TRADER_ROUTE,
            actor: brief.id.clone(),
            npc: brief.npc.clone(),
            ship: Some(ship.clone()),
            station: Some(station.clone()),
            trigger: fact.id,
            instructions: format!(
                "{} has docked. Pick the trade plan this captain makes, given their goals and temperament.",
                brief.name
            ),
            options,
            cites: vec![fact.id],
            preconditions: vec![Precondition::Docked {
                ship: ship.clone(),
                station: station.clone(),
            }],
        }]
    }

    fn resolve(
        &self,
        world: &WorldState,
        menu: &Menu,
        option: &MenuOption,
    ) -> Result<Vec<Command>, String> {
        let ship = menu.ship.as_ref().ok_or("trader menu without a ship")?;
        Ok(self.plans.replan(world, ship, option))
    }
}

// --- Dock talk ---

/// An authored line that qualifies right now, already filled from its fact.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCandidate {
    /// `line.<npc>.<nn>`.
    pub line: String,
    pub npc: NpcId,
    /// The fact it cites.
    pub cites: FactId,
    /// The authored text, slots filled. Said word for word on any failure.
    pub text: String,
    /// The authored text with its slots (`{qty}`, `{ware}`, `{station}`) left in.
    /// Equal to `text` for a line without slots.
    pub template: String,
    /// Slot name (no braces) -> the value filled in.
    pub slots: BTreeMap<String, String>,
    /// How specific it is (more matching conditions first).
    pub specificity: u32,
}

/// Eligible lines for a docking, most specific first. The talk store's line matcher
/// (ambient retirement, newest-fact ties) plugs in here at integration.
pub trait LineSource {
    fn candidates(&self, cx: &MenuCtx<'_>, station: &StationId, dock: &Fact) -> Vec<LineCandidate>;
}

/// Opens a `dock_talk` menu when the player's ship docks and some line qualifies.
pub struct DockTalk<L: LineSource> {
    pub lines: L,
}

impl<L: LineSource> MenuProvider for DockTalk<L> {
    fn name(&self) -> &'static str {
        DOCK_TALK
    }

    fn menus(&self, cx: &MenuCtx<'_>, fact: &Fact) -> Vec<Menu> {
        let Some((ship, station)) = docked(fact) else {
            return Vec::new();
        };
        if cx.world.player.ship.as_ref() != Some(ship) {
            return Vec::new();
        }
        let mut lines = self.lines.candidates(cx, station, fact);
        // Most specific first, then the newest fact (05-people.md, "Dock talk").
        lines.sort_by(|a, b| {
            b.specificity
                .cmp(&a.specificity)
                .then(b.cites.cmp(&a.cites))
                .then_with(|| a.line.cmp(&b.line))
        });
        lines.truncate(cx.config.limits.menu_size);
        let Some(first) = lines.first() else {
            return Vec::new();
        };
        let n = lines.len();
        let options = lines
            .iter()
            .enumerate()
            .map(|(i, l)| MenuOption {
                id: l.line.clone(),
                text: format!("{} says: \"{}\"", l.npc, l.text),
                score: (n - i) as f64,
                refs: vec![any(l.npc.as_str())].into_iter().flatten().collect(),
                commands: Vec::new(),
                line: Some(l.clone()),
            })
            .collect();
        vec![Menu {
            kind: DOCK_TALK,
            actor: any(station.as_str()).expect("station IDs are entity IDs"),
            npc: Some(first.npc.clone()),
            ship: Some(ship.clone()),
            station: Some(station.clone()),
            trigger: fact.id,
            instructions:
                "The player has just docked. Pick the one line the player most needs to hear."
                    .into(),
            options,
            cites: lines.iter().map(|l| l.cites).collect(),
            preconditions: Vec::new(),
        }]
    }
}

/// A plain line source over authored lines, for tests and until the talk store's
/// matcher lands: a line qualifies when a fact of its kind at this station is newer
/// than the player's previous docking here, and every placeholder can be filled.
#[derive(Clone, Debug, Default)]
pub struct LinePool {
    pub lines: Vec<PoolLine>,
}

#[derive(Clone, Debug)]
pub struct PoolLine {
    pub id: String,
    pub npc: NpcId,
    /// The fact kind cited; `<pack>.<kind>` and `<kind>` match each other.
    pub cites: String,
    pub text: String,
}

fn same_kind(want: &str, have: &str) -> bool {
    want == have
        || have.rsplit_once('.').is_some_and(|(_, k)| k == want)
        || want.rsplit_once('.').is_some_and(|(_, k)| k == have)
}

impl LinePool {
    /// The line filled from its fact, and the slots it used.
    pub(crate) fn fill(
        world: &WorldState,
        text: &str,
        fact: &Fact,
    ) -> Option<(String, BTreeMap<String, String>)> {
        let s = &fact.subjects;
        let name = |id: Option<&str>, get: &dyn Fn(&str) -> Option<String>| {
            id.map(|i| get(i).unwrap_or_else(|| i.to_owned()))
        };
        let station = name(s.station.as_ref().map(|x| x.as_str()), &|i| {
            world.stations.get(i).map(|x| x.name.clone())
        });
        let ware = name(s.ware.as_ref().map(|x| x.as_str()), &|i| {
            world.wares.get(i).map(|x| x.name.to_lowercase())
        });
        let mut out = text.to_owned();
        let mut slots = BTreeMap::new();
        for (key, value) in [
            ("station", station),
            ("ware", ware),
            ("qty", fact.amount.map(|a| a.to_string())),
        ] {
            let slot = format!("{{{key}}}");
            if out.contains(&slot) {
                let value = value?;
                out = out.replace(&slot, &value);
                slots.insert(key.to_owned(), value);
            }
        }
        (!out.contains('{')).then_some((out, slots))
    }
}

impl LineSource for LinePool {
    fn candidates(&self, cx: &MenuCtx<'_>, station: &StationId, dock: &Fact) -> Vec<LineCandidate> {
        let facts = cx.history.facts();
        let ship = cx.world.player.ship.as_ref();
        // The player's previous docking here bounds "new".
        let since = facts
            .iter()
            .rev()
            .filter(|f| f.id < dock.id)
            .find(|f| docked(f).is_some_and(|(s, at)| Some(s) == ship && at == station))
            .map_or(FactId(0), |f| f.id);
        let mut out = Vec::new();
        for line in &self.lines {
            let here = cx
                .world
                .npcs
                .get(&line.npc)
                .is_none_or(|n| n.location.as_ref() == Some(station));
            if !here {
                continue;
            }
            let fact = facts.iter().rev().take_while(|f| f.id > since).find(|f| {
                same_kind(&line.cites, f.kind.as_str())
                    && f.subjects.station.as_ref().is_none_or(|s| s == station)
            });
            if let Some(f) = fact
                && let Some((text, slots)) = Self::fill(cx.world, &line.text, f)
            {
                out.push(LineCandidate {
                    line: line.id.clone(),
                    npc: line.npc.clone(),
                    cites: f.id,
                    text,
                    template: line.text.clone(),
                    slots,
                    specificity: u32::from(f.subjects.station.is_some()),
                });
            }
        }
        out
    }
}

// --- Rumor spread ---

/// Opens a `rumor_spread` menu when an NPC craft docks somewhere a rumor it heard at
/// its last station hasn't reached yet. Each docking carries one rumor (the freshest
/// first); its truth value travels with it.
pub struct RumorSpread;

impl RumorSpread {
    /// The station this craft docked at before `dock`.
    fn previous_station(history: &History, ship: &ShipId, dock: FactId) -> Option<StationId> {
        history
            .facts()
            .iter()
            .rev()
            .filter(|f| f.id < dock)
            .find_map(|f| match docked(f) {
                Some((s, at)) if s == ship => Some(at.clone()),
                _ => None,
            })
    }
}

impl MenuProvider for RumorSpread {
    fn name(&self) -> &'static str {
        RUMOR_SPREAD
    }

    fn menus(&self, cx: &MenuCtx<'_>, fact: &Fact) -> Vec<Menu> {
        let Some((ship, station)) = docked(fact) else {
            return Vec::new();
        };
        let Some(craft) = cx.world.ships.get(ship) else {
            return Vec::new();
        };
        if matches!(craft.owner, Owner::Player) {
            return Vec::new();
        }
        let Some(from) = Self::previous_station(cx.history, ship, fact.id) else {
            return Vec::new();
        };
        let candidates: Vec<&Rumor> = cx
            .world
            .rumors
            .values()
            .filter(|r| r.heard_at.contains(&from) && !r.heard_at.contains(station))
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }
        let brief = cx.config.brief(ship.as_str());
        let options = candidates
            .into_iter()
            .map(|r| {
                let spread = FactDraft {
                    kind: FactKind::new(crate::RUMOR_SPREAD_FACT).expect("valid kind"),
                    subjects: Subjects {
                        ship: Some(ship.clone()),
                        station: Some(station.clone()),
                        npc: brief.and_then(|b| b.npc.clone()),
                        other: any(r.id.as_str()),
                        ..Subjects::default()
                    },
                    amount: None,
                    note: json!({
                        "rumor": r.id,
                        "truth": r.is_true,
                        "from": from,
                        "claim": r.claim,
                    })
                    .to_string(),
                };
                MenuOption {
                    id: r.id.to_string(),
                    text: format!("Tell {station} the rumor: {}", r.claim),
                    // Fresher rumors (newer source facts) first.
                    score: r.source.map_or(0.0, |f| f.0 as f64),
                    refs: [any(r.id.as_str()), any(station.as_str())]
                        .into_iter()
                        .flatten()
                        .collect(),
                    // Adds this one station: spreads to other stations in the same tick
                    // compose, and a second spread to this one is rejected.
                    commands: vec![
                        Command::SetRumorHeard {
                            rumor: r.id.clone(),
                            station: station.clone(),
                            heard: true,
                        },
                        Command::RecordFact {
                            fact: Box::new(spread),
                        },
                    ],
                    line: None,
                }
            })
            .collect();
        let options = top(options, cx.config.limits.menu_size);
        let actor = brief
            .map(|b| b.id.clone())
            .or_else(|| any(ship.as_str()))
            .expect("ship IDs are entity IDs");
        vec![Menu {
            kind: RUMOR_SPREAD,
            actor,
            npc: brief.and_then(|b| b.npc.clone()),
            ship: Some(ship.clone()),
            station: Some(station.clone()),
            trigger: fact.id,
            instructions: format!(
                "{} has docked at {station} carrying news from {from}. Pick the rumor the captain tells first.",
                craft.name
            ),
            options,
            cites: vec![fact.id],
            preconditions: Vec::new(),
        }]
    }

    fn resolve(
        &self,
        world: &WorldState,
        menu: &Menu,
        option: &MenuOption,
    ) -> Result<Vec<Command>, String> {
        // The commands add one station and never replace the set; this only falls back
        // early if the rumor is gone or already here.
        let station = menu
            .station
            .as_ref()
            .ok_or("rumor menu without a station")?;
        let id = RumorId::new(&option.id).map_err(|e| e.to_string())?;
        let rumor = world
            .rumors
            .get(&id)
            .ok_or_else(|| format!("{id} no longer exists"))?;
        if rumor.heard_at.contains(station) {
            return Err(format!("{id} has already reached {station}"));
        }
        Ok(option.commands.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_match_with_or_without_pack() {
        assert!(same_kind("rumor_spread", "mind.rumor_spread"));
        assert!(same_kind("core.deliver", "deliver"));
        assert!(same_kind("dock", "dock"));
        assert!(!same_kind("dock", "undock"));
    }
}
