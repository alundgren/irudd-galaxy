//! The native modifiers a ship has from the pilot's traits and fittings and its hull's
//! ranks, added up. Traits and fittings belong to the pilot, so they apply to the
//! ship the player flies; tenders get only `tender_hold`. The ship, economy and app
//! crates read these totals instead of knowing about traits or upgrades.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;

/// One ship's modifiers, combined: factors multiply, units add, radii take the largest.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifiers {
    pub mining_rate: f64,
    /// Extra yield when mining the richest face (0 = none).
    pub rich_face: f64,
    pub hold: u32,
    pub turn_rate: f64,
    pub speed: f64,
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
    if let Some(ship) = crate::eval::player_ship(world) {
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
        .is_some_and(|s| s.owner == Owner::Player && !crate::prestige::is_berthed(world, ship))
    {
        for x in pilot(world) {
            if let Modifier::TenderHold { units } = x {
                m.hold = m.hold.saturating_add(*units);
            }
        }
    }
    m
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
