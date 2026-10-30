//! The native modifiers a ship has from the pilot's traits and fittings and its hull's
//! ranks, added up. Traits and fittings belong to the pilot, so they apply to the
//! ship the player flies; tenders get only `tender_hold`.
//!
//! This lives in the contract, not in `galaxy-progress`, because the kernel's own
//! invariants (a hold within its capacity), the ship crate (flight, mining, docking,
//! lanes) and the economy (free hold for a buy) all need the totals, and
//! `galaxy-progress` depends on `galaxy-core`. Every crate reads the same pure function
//! of the world; `galaxy_progress::modifiers` re-exports it. The rules it adds up are
//! the progress data in `world.progress.rules`.

use std::collections::BTreeSet;

use crate::ids::ShipId;
use crate::progress::Modifier;
use crate::world::{Owner, Ship, WorldState};

/// One ship's modifiers, combined: factors multiply, units add, radii take the largest.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifiers {
    pub mining_rate: f64,
    /// Extra yield when mining the richest face (0 = none).
    pub rich_face: f64,
    pub hold: u32,
    pub turn_rate: f64,
    pub speed: f64,
    /// Camera zoom factor (`flight.zoom` times this).
    pub zoom: f64,
    pub dock_reach: f64,
    /// Lane mouth catch radius, if a modifier sets one.
    pub mouth_radius: Option<f32>,
    /// Live-price range, if a modifier gives one.
    pub price_glass: Option<f32>,
    /// Presentation features (`sell_preview`, `lane_radio` ...).
    pub features: BTreeSet<String>,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            mining_rate: 1.0,
            rich_face: 0.0,
            hold: 0,
            turn_rate: 1.0,
            speed: 1.0,
            zoom: 1.0,
            dock_reach: 1.0,
            mouth_radius: None,
            price_glass: None,
            features: BTreeSet::new(),
        }
    }
}

impl Modifiers {
    fn add(&mut self, m: &Modifier) {
        match m {
            Modifier::MiningRate { factor } => self.mining_rate *= factor,
            Modifier::RichFace { bonus } => self.rich_face += bonus,
            Modifier::Hold { units } => self.hold = self.hold.saturating_add(*units),
            Modifier::TurnRate { factor } => self.turn_rate *= factor,
            Modifier::Speed { factor } => self.speed *= factor,
            Modifier::Zoom { factor } => self.zoom *= factor,
            Modifier::DockReach { factor } => self.dock_reach *= factor,
            Modifier::MouthRadius { radius } => {
                self.mouth_radius = Some(self.mouth_radius.map_or(*radius, |r| r.max(*radius)));
            }
            Modifier::PriceGlass { range } => {
                self.price_glass = Some(self.price_glass.map_or(*range, |r| r.max(*range)));
            }
            Modifier::Feature { feature } => {
                self.features.insert(feature.clone());
            }
            // Pilot-wide, read where standing is paid (`deed_bonus`), or tender-only.
            Modifier::DeedBonus { .. } | Modifier::TenderHold { .. } => {}
        }
    }

    pub fn has(&self, feature: &str) -> bool {
        self.features.contains(feature)
    }
}

fn player_ship(world: &WorldState) -> Option<&Ship> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
}

/// Every modifier the pilot carries: traits, fittings, and the current hull's ranks.
pub fn pilot(world: &WorldState) -> Vec<&Modifier> {
    let rules = &world.progress.rules;
    let mut out: Vec<&Modifier> = Vec::new();
    for t in &world.player.traits {
        if let Some(def) = rules.traits.get(t) {
            out.extend(&def.modifiers);
        }
    }
    for u in &world.player.fittings {
        if let Some(def) = rules.upgrades.get(u) {
            out.extend(&def.modifiers);
        }
    }
    if let Some(ship) = player_ship(world) {
        let rank = world
            .player
            .hull_records
            .get(&ship.hull)
            .map_or(0, |r| r.rank);
        for r in rules
            .ranks
            .iter()
            .filter(|r| r.hull == ship.hull && r.rank <= rank)
        {
            out.extend(&r.modifiers);
        }
    }
    out
}

/// The modifiers for one ship: the flagship gets the pilot's, a player tender gets
/// the `tender_hold` bonuses, and anyone else's ship gets none.
pub fn for_ship(world: &WorldState, ship: &ShipId) -> Modifiers {
    let mut m = Modifiers::default();
    if world.player.ship.as_ref() == Some(ship) {
        for x in pilot(world) {
            m.add(x);
        }
    } else if world
        .ships
        .get(ship)
        .is_some_and(|s| s.owner == Owner::Player && !world.player.is_berthed(ship))
    {
        for x in pilot(world) {
            if let Modifier::TenderHold { units } = x {
                m.hold = m.hold.saturating_add(*units);
            }
        }
    }
    m
}

/// A ship's hold capacity: its hull's plus its hold modifiers. Every check of free
/// hold (the kernel's invariant, mining, buying, the HUD) reads this.
pub fn hold_capacity(world: &WorldState, ship: &Ship) -> u64 {
    let base = world
        .hulls
        .get(&ship.hull)
        .map_or(0, |h| u64::from(h.hold_capacity));
    base + u64::from(for_ship(world, &ship.id).hold)
}

/// The flagship's hold capacity on `hull` straight after a prestige: that hull's
/// capacity plus the pilot's trait and fitting hold modifiers (they carry across). The
/// new hull starts unstriped, so no rank adds anything yet.
pub fn hold_capacity_after_prestige(world: &WorldState, hull: &crate::ids::HullId) -> u64 {
    let rules = &world.progress.rules;
    let base = world
        .hulls
        .get(hull)
        .map_or(0, |h| u64::from(h.hold_capacity));
    let traits = world
        .player
        .traits
        .iter()
        .filter_map(|t| rules.traits.get(t))
        .flat_map(|d| &d.modifiers);
    let fittings = world
        .player
        .fittings
        .iter()
        .filter_map(|u| rules.upgrades.get(u))
        .flat_map(|d| &d.modifiers);
    let extra: u64 = traits
        .chain(fittings)
        .map(|m| match m {
            Modifier::Hold { units } => u64::from(*units),
            _ => 0,
        })
        .sum();
    base + extra
}

/// The multiplier traits give standing gains from deeds with any of these tags.
pub fn deed_bonus(world: &WorldState, tags: &[String]) -> f64 {
    pilot(world)
        .into_iter()
        .filter_map(|m| match m {
            Modifier::DeedBonus { tag, factor } if tags.contains(tag) => Some(*factor),
            _ => None,
        })
        .product()
}
