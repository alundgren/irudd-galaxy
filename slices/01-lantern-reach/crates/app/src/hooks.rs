//! What the screen reads from progression and the legibility channels, through plain
//! functions over the world (and its history).
//!
//! The screen asks these and draws whatever comes back; it draws nothing for `None` or
//! an empty list ("text is important or absent").
//!
//! - The goal line is the newest active beat's goal ([`beats::hud_goal`]).
//! - The talk panel shows what was said since the flagship last docked: dock talk and
//!   chronicle entries from the world's talk log, each naming a person or a place and
//!   citing a fact.
//! - "What changed?" ([`what_changed`]) is a dockmaster's answer: the lines the core
//!   pack said when the player asked (a `choice.what_changed` reply), digest lines that
//!   say plainly what a fact records, each citing it, and chronicle entries.
//! - Open choices ([`open_choices`]) are the answers people at the dock are waiting
//!   for; the dock shows them as buttons (keys 1 to 9) and the pick goes out as a
//!   `PlayerInput::replies` entry.
//! - Station services ([`station_services`]) are the hullwright's, tender keeper's and
//!   dockmaster's offers, from [`services::offers`].

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, SaidLine, TalkLine};
use galaxy_core::History;
use galaxy_mind::TalkLines;
use galaxy_progress::{beats, prestige, services};

/// The one goal line at the top of the screen: the newest active beat's goal.
pub fn goal_line(world: &WorldState) -> Option<String> {
    beats::hud_goal(world).map(|g| g.goal)
}

/// Where the goal line points, for the edge marker: the station it names (the longest
/// name, so "Lantern Wharf" wins over a shorter name inside it), else the station of a
/// person it names. Nothing when it names neither.
pub fn goal_place(world: &WorldState) -> Option<StationId> {
    let line = goal_line(world)?;
    let named = world
        .stations
        .values()
        .filter(|s| line.contains(&s.name))
        .max_by_key(|s| s.name.len())
        .map(|s| s.id.clone());
    named.or_else(|| {
        world
            .npcs
            .values()
            .filter(|n| {
                n.name
                    .split_whitespace()
                    .next()
                    .is_some_and(|first| line.contains(first))
            })
            .find_map(|n| n.location.clone())
    })
}

/// The tick of the flagship's most recent docking (0 before any).
fn last_docking(world: &WorldState, history: &History) -> Tick {
    history
        .last(&HistoryQuery {
            kind: Some(FactKind::from_static("dock")),
            ship: world.player.ship.clone(),
            ..HistoryQuery::default()
        })
        .map_or(Tick(0), |f| f.tick)
}

/// The latest lines for the talk panel, newest last: dock talk and chronicle entries
/// said since the flagship last docked (so they stay up on the way to the next
/// station), at most `limit`.
pub fn talk_lines(world: &WorldState, history: &History, limit: usize) -> Vec<TalkLine> {
    let since = last_docking(world, history);
    let mut out: Vec<&SaidLine> = world
        .talk
        .lines
        .values()
        .filter(|l| l.tick >= since)
        .filter(|l| matches!(l.line.channel, Channel::DockTalk | Channel::Chronicle))
        .collect();
    let skip = out.len().saturating_sub(limit);
    out.drain(..skip);
    out.into_iter().map(|l| l.line.clone()).collect()
}

/// The latest chronicle entries, newest last.
pub fn chronicle(world: &WorldState, limit: usize) -> Vec<TalkLine> {
    let all: Vec<&SaidLine> = world.talk.on(Channel::Chronicle).collect();
    let skip = all.len().saturating_sub(limit);
    all[skip..].iter().map(|l| l.line.clone()).collect()
}

/// The speaker's name for a line, if it has one.
pub fn speaker(world: &WorldState, line: &TalkLine) -> Option<String> {
    let npc = line.speaker.as_ref()?;
    Some(
        world
            .npcs
            .get(npc)
            .map_or_else(|| npc.to_string(), |n| n.name.clone()),
    )
}

/// The place's name for a line, if it names one.
pub fn place(world: &WorldState, line: &TalkLine) -> Option<String> {
    let s = line.place.as_ref()?;
    Some(
        world
            .stations
            .get(s)
            .map_or_else(|| s.to_string(), |s| s.name.clone()),
    )
}

/// Whether `line` belongs in the talk panel.
pub fn is_talk(line: &TalkLine) -> bool {
    matches!(line.channel, Channel::DockTalk | Channel::Chronicle)
}

/// The NPC at `station` with `role`, if any.
pub fn keeper(world: &WorldState, station: &StationId, role: &str) -> Option<NpcId> {
    world
        .npcs
        .values()
        .find(|n| n.location.as_ref() == Some(station) && n.role == role)
        .map(|n| n.id.clone())
}

fn npc_name(world: &WorldState, npc: &NpcId) -> String {
    world
        .npcs
        .get(npc)
        .map_or_else(|| npc.to_string(), |n| n.name.clone())
}

/// One service on the dock screen besides the market.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceEntry {
    pub service: Service,
    pub name: String,
    /// Who runs it, if a named NPC does.
    pub keeper: Option<NpcId>,
    pub keeper_name: Option<String>,
    /// What it sells here now: upgrades, tenders and the prestige offer.
    pub offers: Vec<OfferRow>,
    /// The dockmaster can be asked what changed.
    pub can_ask: bool,
}

/// One thing for sale, in plain words.
#[derive(Clone, Debug, PartialEq)]
pub struct OfferRow {
    pub request: ServiceRequest,
    pub name: String,
    /// What it does.
    pub effect: String,
    /// "600 cr", or "a gift".
    pub price: String,
    /// What stands in the way, if anything ("You have 40 cr"). `None`: it can be
    /// bought now.
    pub blocked: Option<String>,
    pub prestige: bool,
}

/// The station's services other than the market, with what each sells now.
pub fn station_services(
    world: &WorldState,
    history: &History,
    station: &StationId,
) -> Vec<ServiceEntry> {
    let Some(s) = world.stations.get(station) else {
        return Vec::new();
    };
    let offers = services::offers(world, history, station);
    let mut out: Vec<ServiceEntry> = Vec::new();
    // Every station's dockmaster can be asked what changed, whether or not its data
    // lists the service: a resident dockmaster is enough (Odo, Rook, Hesper, or one a
    // game-master pack founds).
    let mut listed: Vec<Service> = s.services.iter().copied().collect();
    if !listed.contains(&Service::Dockmaster) && keeper(world, station, "dockmaster").is_some() {
        listed.insert(0, Service::Dockmaster);
    }
    for sv in listed.iter().filter(|sv| **sv != Service::Market) {
        let (name, role) = match sv {
            Service::Dockmaster => ("Dockmaster", "dockmaster"),
            Service::Hullwright => ("Hullwright", "hullwright"),
            Service::Archive => ("Archive", "archivist"),
            Service::Market => ("Market", "market"),
        };
        let keeper = keeper(world, station, role);
        out.push(ServiceEntry {
            service: *sv,
            name: name.into(),
            keeper_name: keeper.as_ref().map(|k| npc_name(world, k)),
            keeper,
            offers: Vec::new(),
            can_ask: *sv == Service::Dockmaster,
        });
    }
    // Each offer goes to its seller's service; a seller with none of the station's
    // services (Tamsin, the tender keeper) gets a row of their own.
    for o in offers {
        let row = offer_row(world, history, &o);
        let slot = out
            .iter_mut()
            .find(|e| e.keeper.is_some() && e.keeper == o.seller);
        match slot {
            Some(e) => e.offers.push(row),
            None => {
                let name = o
                    .seller
                    .as_ref()
                    .and_then(|n| world.npcs.get(n))
                    .map_or("Workshop".to_owned(), |n| title_case(&n.role));
                out.push(ServiceEntry {
                    service: Service::Hullwright,
                    name,
                    keeper_name: o.seller.as_ref().map(|k| npc_name(world, k)),
                    keeper: o.seller.clone(),
                    offers: vec![row],
                    can_ask: false,
                });
            }
        }
    }
    out
}

/// `tender_keeper` -> "Tender keeper".
fn title_case(role: &str) -> String {
    let words = role.replace('_', " ");
    let mut c = words.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn offer_row(world: &WorldState, history: &History, o: &services::Offer) -> OfferRow {
    let price = if o.cost == 0 {
        "a gift".to_owned()
    } else {
        format!("{} cr", show_credits(o.cost))
    };
    let blocked = services::check(world, history, &o.request)
        .err()
        .map(|r| crate::trades::service_refusal(world, &o.request, &r));
    OfferRow {
        prestige: matches!(o.request, ServiceRequest::Prestige { .. }),
        request: o.request.clone(),
        name: o.name.clone(),
        effect: o.effect.clone(),
        price,
        blocked,
    }
}

/// What a prestige keeps and what it resets, for the confirm card.
pub fn prestige_summary(
    world: &WorldState,
    history: &History,
) -> Option<(Vec<String>, Vec<String>)> {
    let offer = prestige::offer(world, history)?;
    Some(prestige::summary(world, &offer))
}

/// A dockmaster's answer to "What changed?".
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub station: StationId,
    pub speaker: Option<NpcId>,
    /// Each names a person or place and cites a fact. Empty: nothing has.
    pub lines: Vec<TalkLine>,
    /// The plain digest lines among `lines`, which the app says itself (the pack said
    /// the rest).
    pub digest: Vec<TalkLine>,
}

/// The standing question every dockmaster answers, sent as a reply.
pub const WHAT_CHANGED: &str = "choice.what_changed";

/// The fact kind the core pack records when the player asks (b17 waits on it).
pub const ASKED: FactKind = FactKind::from_static("core.asked_what_changed");

/// How many lines an answer has at most (`05-people.md`: up to 3, plus the chronicle).
const ANSWER_LINES: usize = 3;
const ANSWER_CHRONICLE: usize = 2;

/// The dockmaster's answer at `station`, once the core pack has answered the question
/// (`asked`, its `core.asked_what_changed` fact): the lines the pack said with it (its
/// authored lines, or Ilse's homecoming in b17), then digest lines for the rest of what
/// happened since the player's previous docking there, then the chronicle entries
/// written since. The whole answer shares one line per cited place and never repeats
/// a fact. Like all ambient talk, a digest line never tells a fact some said line
/// already cites, or one a `queued` batch is about to say.
pub fn what_changed(
    world: &WorldState,
    history: &History,
    queued: &[CommandBatch],
    station: &StationId,
    asked: &Fact,
) -> Answer {
    let speaker = asked
        .subjects
        .npc
        .clone()
        .or_else(|| keeper(world, station, "dockmaster"));
    let since = previous_docking(world, history, station);
    // What the pack said in the batch that recorded the question.
    let mut out: Vec<TalkLine> = world
        .talk
        .on(Channel::DockTalk)
        .filter(|l| l.tick == asked.tick)
        .filter(|l| l.line.place.as_ref() == Some(station) || l.line.speaker == speaker)
        .map(|l| l.line.clone())
        .collect();
    let mut cited = TalkLines::cited_facts(world, queued);
    let mut answer_cites: BTreeSet<FactId> =
        out.iter().flat_map(|l| l.cites.iter().copied()).collect();
    cited.extend(&answer_cites);
    let mut places: BTreeSet<Option<StationId>> =
        out.iter().flat_map(|l| cited_places(history, l)).collect();
    let mut digest = Vec::new();
    for f in history.facts().iter().rev().take_while(|f| f.id > since) {
        if out.len() + digest.len() >= ANSWER_LINES {
            break;
        }
        if cited.contains(&f.id) {
            continue;
        }
        let Some(text) = self::digest(world, f) else {
            continue;
        };
        let place = f.subjects.station.clone();
        if !places.insert(place.clone()) {
            continue;
        }
        answer_cites.insert(f.id);
        digest.push(TalkLine {
            channel: Channel::DockTalk,
            speaker: speaker.clone(),
            place: place.or_else(|| Some(station.clone())),
            text,
            cites: vec![f.id],
            about: Vec::new(),
            choices: Vec::new(),
        });
    }
    out.extend(digest.iter().cloned());
    let since_tick = history.fact(since).map_or(Tick(0), |f| f.tick);
    // Chronicle pages have already been included in the previous answer at this
    // dock. Keep them in history without replaying them on every question.
    let previous_question = history.facts().iter().rev().find(|f| {
        f.kind == ASKED && f.id < asked.id && f.subjects.station.as_ref() == Some(station)
    });
    let mut chronicle: Vec<TalkLine> = world
        .talk
        .lines
        .values()
        .rev()
        .filter(|l| l.line.channel == Channel::Chronicle)
        .filter(|l| l.tick >= since_tick)
        .filter(|l| previous_question.is_none_or(|q| l.tick > q.tick))
        .filter(|l| {
            let line_places = cited_places(history, &l.line);
            if l.line.cites.iter().any(|f| answer_cites.contains(f))
                || line_places.iter().any(|p| places.contains(p))
            {
                return false;
            }
            answer_cites.extend(&l.line.cites);
            places.extend(line_places);
            true
        })
        .take(ANSWER_CHRONICLE)
        .map(|l| l.line.clone())
        .collect();
    chronicle.reverse();
    out.extend(chronicle);
    Answer {
        station: station.clone(),
        speaker,
        lines: out,
        digest,
    }
}

/// A line is about its cited facts' stations, not necessarily where it was spoken.
/// Facts without a station share one place, matching the pack's selection rule.
fn cited_places(history: &History, line: &TalkLine) -> BTreeSet<Option<StationId>> {
    line.cites
        .iter()
        .map(|f| history.fact(*f).and_then(|f| f.subjects.station.clone()))
        .collect()
}

/// An answer someone at the dock is waiting for: a choice a line offered, not yet
/// picked.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenChoice {
    pub npc: NpcId,
    pub choice: AnyId,
    /// What the player says.
    pub text: String,
}

/// Most open choices the dock shows (keys 1 to 9).
pub const MAX_OPEN_CHOICES: usize = 9;

/// The choices people at `station` offered since the flagship docked there and nobody
/// has answered yet, oldest line first. A line closes once one of its choices is
/// picked; a choice offered again (a re-asked question) shows once, from its newest
/// line.
pub fn open_choices(world: &WorldState, history: &History, station: &StationId) -> Vec<OpenChoice> {
    let docked_here = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .is_some_and(|s| s.docked_at.as_ref() == Some(station));
    if !docked_here {
        return Vec::new();
    }
    let since = last_docking(world, history);
    let replies: Vec<&Fact> = history
        .facts()
        .iter()
        .rev()
        .take_while(|f| f.tick >= since)
        .filter(|f| f.kind.as_str() == "reply")
        .collect();
    let mut out: Vec<OpenChoice> = Vec::new();
    for said in world.talk.on(Channel::DockTalk).filter(|l| l.tick >= since) {
        let line = &said.line;
        let Some(npc) = &line.speaker else { continue };
        if line.choices.is_empty()
            || world.npcs.get(npc).and_then(|n| n.location.as_ref()) != Some(station)
        {
            continue;
        }
        let answered = replies.iter().any(|r| {
            r.tick >= said.tick
                && r.subjects.npc.as_ref() == Some(npc)
                && line
                    .choices
                    .iter()
                    .any(|c| r.subjects.other.as_ref() == Some(&c.id))
        });
        if answered {
            continue;
        }
        for c in &line.choices {
            out.retain(|o| !(&o.npc == npc && o.choice == c.id));
            out.push(OpenChoice {
                npc: npc.clone(),
                choice: c.id.clone(),
                text: c.text.clone(),
            });
        }
    }
    let skip = out.len().saturating_sub(MAX_OPEN_CHOICES);
    out.drain(..skip);
    out
}

/// The player's previous docking at `station` before the current one.
fn previous_docking(world: &WorldState, history: &History, station: &StationId) -> FactId {
    let ship = world.player.ship.as_ref();
    let mut docks = history.facts().iter().rev().filter(|f| match &f.payload {
        FactPayload::Event(Event::Dock {
            ship: s,
            station: at,
        }) => Some(s) == ship && at == station,
        _ => false,
    });
    // Docked here now: the docking before this one.
    let docked_here = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .is_some_and(|s| s.docked_at.as_ref() == Some(station));
    if docked_here {
        docks.next();
    }
    docks.next().map_or(FactId(0), |f| f.id)
}

/// A plain line for a fact worth telling, in words, from its own fields. `None` for
/// routine facts (dockings, trades, mining) and anything not meant for the player.
pub fn digest(world: &WorldState, f: &Fact) -> Option<String> {
    let kind = f.kind.as_str();
    if let FactPayload::Note(note) = &f.payload {
        // Station growth from the core pack or a game-master pack: its note is the
        // authored description ("Kettle Dock stage 1: a fogged glass dome").
        return kind.ends_with("stage_built").then(|| note.clone());
    }
    let FactPayload::Event(event) = &f.payload else {
        return None;
    };
    let rules = &world.progress.rules;
    match event {
        Event::UpgradeFitted { upgrade, .. } => {
            let u = rules.upgrades.get(upgrade)?;
            // By first name, as people at the dock say it.
            let who = u.seller.as_ref().map(|n| {
                let name = npc_name(world, n);
                name.split_whitespace().next().unwrap_or(&name).to_owned()
            });
            Some(match who {
                Some(who) => format!("{who} fitted your {}.", u.name.to_lowercase()),
                None => format!("Your {} is fitted.", u.name.to_lowercase()),
            })
        }
        Event::TraitEarned { trait_id, .. } => {
            let t = rules.traits.get(trait_id)?;
            Some(format!(
                "You've picked up a habit: {}.",
                t.name.to_lowercase()
            ))
        }
        Event::RegardChanged { faction, to, .. } => {
            let f = world.factions.get(faction)?;
            Some(format!(
                "The {} count you {} now.",
                f.name,
                to.to_lowercase()
            ))
        }
        Event::TenderAcquired { craft, station, .. } => {
            let name = world
                .ships
                .get(craft)
                .map_or(craft.as_str(), |s| s.name.as_str());
            let at = world.stations.get(station)?;
            Some(format!("{name} is yours, berthed at {}.", at.name))
        }
        Event::Prestige {
            from_hull, to_hull, ..
        } => {
            let from = world.hulls.get(from_hull)?;
            let to = world.hulls.get(to_hull)?;
            Some(format!(
                "You traded the {} up into a {}.",
                from.name, to.name
            ))
        }
        Event::HullRank { hull, .. } => {
            let h = world.hulls.get(hull)?;
            Some(format!("The {} earned a new stripe.", h.name))
        }
        _ => None,
    }
}
