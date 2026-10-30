//! What other workstreams fill in, read through plain functions over the world.
//!
//! The screen asks these and draws whatever comes back; it draws nothing for `None` or
//! an empty list ("text is important or absent"). Until progression (beats) and the
//! legibility channels (talk, chronicle) land, the goal line is a placeholder derived
//! from world state and the rest are empty. Swap the bodies, keep the signatures.

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const IRON: WareId = WareId::from_static("ware.iron_ore");

/// The one goal line at the top of the screen.
///
/// Placeholder for the beat runtime's current goal: Act 1 from world state only.
/// Bring ice to Kettle Dock until the player has sold there; then sell iron ore at the
/// Wharf until they have; then nothing.
pub fn goal_line(world: &WorldState) -> Option<String> {
    let sales_at = |station: &StationId| {
        world.counters.get(
            CounterKind::Sales,
            &CounterScope::Station {
                station: station.clone(),
            },
        )
    };
    if world.stations.contains(&KETTLE) && world.wares.contains(&ICE) && sales_at(&KETTLE) == 0 {
        return Some("Bring ice to Kettle Dock".into());
    }
    if world.stations.contains(&WHARF) && world.wares.contains(&IRON) && sales_at(&WHARF) == 0 {
        return Some("Sell iron ore at Lantern Wharf".into());
    }
    None
}

/// Where the goal line points, for the edge marker. Same placeholder rules.
pub fn goal_place(world: &WorldState) -> Option<StationId> {
    let line = goal_line(world)?;
    if line.contains("Kettle Dock") {
        Some(KETTLE)
    } else if line.contains("Lantern Wharf") {
        Some(WHARF)
    } else {
        None
    }
}

/// The latest talk lines, newest last: dock talk where the player is, each citing a
/// fact. The world has no talk store yet, so nobody speaks.
pub fn talk_lines(_world: &WorldState, _limit: usize) -> Vec<TalkLine> {
    Vec::new()
}

/// The latest chronicle entries, newest last. Empty until the chronicle lands.
pub fn chronicle(_world: &WorldState, _limit: usize) -> Vec<TalkLine> {
    Vec::new()
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

/// Whether `line` belongs in the talk panel.
pub fn is_talk(line: &TalkLine) -> bool {
    line.channel == Channel::DockTalk
}

/// One service row on the dock screen besides the market.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceEntry {
    pub service: Service,
    pub name: String,
    /// Who runs it, if a named NPC does.
    pub keeper: Option<String>,
    /// Whether it does anything yet. Progression fills these in (the hullwright's
    /// upgrades, the dockmaster's "what changed?").
    pub open: bool,
}

/// The station's services other than the market, for the dock screen. Placeholder:
/// lists them with their keeper and nothing to do yet.
pub fn station_services(world: &WorldState, station: &StationId) -> Vec<ServiceEntry> {
    let Some(s) = world.stations.get(station) else {
        return Vec::new();
    };
    s.services
        .iter()
        .filter(|sv| **sv != Service::Market)
        .map(|sv| {
            let (name, role) = match sv {
                Service::Dockmaster => ("Dockmaster", "dockmaster"),
                Service::Hullwright => ("Hullwright", "hullwright"),
                Service::Archive => ("Archive", "scholar"),
                Service::Market => ("Market", "market"),
            };
            let keeper = world
                .npcs
                .values()
                .find(|n| n.location.as_ref() == Some(station) && n.role == role)
                .map(|n| n.name.clone());
            ServiceEntry {
                service: *sv,
                name: name.into(),
                keeper,
                open: false,
            }
        })
        .collect()
}
