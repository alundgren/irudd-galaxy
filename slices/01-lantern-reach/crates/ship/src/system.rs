use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};

use crate::step::ShipStep;
use crate::trade::TradeDesk;

/// Every ship, every tick: flight, the context action, mining, docking, lanes and the
/// order runner. The player's input steers the player's flagship; every other ship
/// (tenders, NPC craft) runs its orders with the same verbs.
///
/// Each ship's tick is one batch: its motion, activity, order progress, the effects of
/// its verbs and their facts commit together or not at all. A batch the kernel
/// rejects (the last unit of a rock taken first, a market line changed by another
/// trade) leaves the ship as it was, to try again next tick.
///
/// Stateless: the desk is configuration, and everything that changes lives in the
/// world (`Ship::activity`, `Ship::orders`).
pub struct ShipSystem {
    desk: Box<dyn TradeDesk>,
}

impl ShipSystem {
    pub fn new(desk: Box<dyn TradeDesk>) -> Self {
        Self { desk }
    }
}

/// What earlier ships took this tick, so later ships (in ID order) don't plan with the
/// same units. Lives for one tick only.
#[derive(Default)]
pub(crate) struct Claims {
    /// Market lines a ship traded on this tick: one trade per line per tick, so two
    /// ships never quote the same stock. The other ship trades next tick.
    pub lines: BTreeSet<(StationId, WareId)>,
    /// Rock units mined this tick.
    pub mined: BTreeMap<RockId, u32>,
}

impl Claims {
    /// Units of `rock` left for this ship to mine this tick.
    pub fn left(&self, rock: &Rock) -> u32 {
        rock.remaining
            .saturating_sub(self.mined.get(&rock.id).copied().unwrap_or(0))
    }
}

impl System for ShipSystem {
    fn name(&self) -> &'static str {
        "ship"
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world = ctx.world;
        let mut claims = Claims::default();
        for ship in world.ships.values() {
            let Some(hull) = world.hulls.get(&ship.hull) else {
                continue;
            };
            let input = (world.player.ship.as_ref() == Some(&ship.id)).then_some(ctx.input);
            let step = ShipStep::new(world, self.desk.as_ref(), &mut claims, ship, hull);
            if let Some(batch) = step.run(input) {
                ctx.submit(batch);
            }
        }
    }
}
