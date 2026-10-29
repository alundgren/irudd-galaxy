use galaxy_core::{System, TickCtx};

use crate::step::ShipStep;
use crate::trade::TradeDesk;

/// Every ship, every tick: flight, the context action, mining, docking, lanes and the
/// order runner. The player's input steers the player's flagship; every other ship
/// (tenders, NPC craft) runs its orders with the same verbs.
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

impl System for ShipSystem {
    fn name(&self) -> &'static str {
        "ship"
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let world = ctx.world;
        for ship in world.ships.values() {
            let Some(hull) = world.hulls.get(&ship.hull) else {
                continue;
            };
            let input = (world.player.ship.as_ref() == Some(&ship.id)).then_some(ctx.input);
            for batch in ShipStep::new(world, self.desk.as_ref(), ship, hull).run(input) {
                ctx.submit(batch);
            }
        }
    }
}
