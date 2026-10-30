use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};

use crate::step::ShipStep;
use crate::trade::TradeDesk;

/// Every ship, every tick: flight, the context action, mining, docking, lanes and the
/// order runner. The player's input steers the player's flagship; every other ship
/// (tenders, NPC craft) runs its orders with the same verbs. Hulls berthed at prestige
/// (`Player::is_berthed`) are skipped.
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

/// What earlier ships took this tick, so later ships (see [`turn_order`]) don't plan
/// with the same units. Lives for one tick only.
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
        // A retiring layer can remove a demonstration craft or close a ring
        // between ticks. Do not fly or trade against those stale records.
        let retiring = ctx.queued().iter().any(|b| {
            b.commands.iter().any(|c| {
                matches!(
                    c,
                    Command::PutShip { value: None, .. } | Command::CloseFoundedStation { .. }
                )
            })
        });
        let projected = retiring.then(|| ctx.projected());
        let world = projected.as_ref().unwrap_or(ctx.world);
        let mut claims = Claims::default();
        for ship in turn_order(world, is_hands_on(world, ctx.input)) {
            // A hull left at prestige is kept, not flown: it never runs orders, even
            // ones a script or a stale save gave it.
            if world.player.is_berthed(&ship.id) {
                continue;
            }
            let Some(hull) = world.hulls.get(&ship.hull) else {
                continue;
            };
            let input = (world.player.ship.as_ref() == Some(&ship.id)).then_some(ctx.input);
            let step = ShipStep::new(world, self.desk.as_ref(), &mut claims, ship, hull);
            for batch in step.run(input) {
                ctx.submit(batch);
            }
        }
    }
}

/// Is the player steering by hand this tick (keys, Space, a click, or input held over
/// from a rejected tick)? Only then does the flagship jump the queue.
pub(crate) fn is_hands_on(world: &WorldState, input: &PlayerInput) -> bool {
    let held = world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
        .is_some_and(|s| s.activity.held_press || !s.activity.held_clicks.is_empty());
    held || input.action || input.click.is_some() || input.thrust.length() > 0.0
}

/// Who claims contested market lines and rock units first this tick. When the player
/// steers by hand (`hands_on`), the flagship goes first: what the player asks for wins
/// over every NPC craft and tender. The others follow in ID order, rotated by one each
/// tick, so each is first of them at least once every `n` ticks and no ship waits for
/// ever. When the flagship is only running queued orders it takes its turn in the
/// rotation like any craft, so the player's own automation can't starve a tender.
/// The rotation reads the tick, so it survives a save.
pub(crate) fn turn_order(world: &WorldState, hands_on: bool) -> impl Iterator<Item = &Ship> {
    let flagship = world.player.ship.as_ref().filter(|_| hands_on);
    let others: Vec<&Ship> = world
        .ships
        .values()
        .filter(|s| Some(&s.id) != flagship)
        .collect();
    let start = match others.len() {
        0 => 0,
        n => (world.meta.tick.0 % n as u64) as usize,
    };
    let (before, after) = others.split_at(start);
    flagship
        .and_then(|id| world.ships.get(id))
        .into_iter()
        .chain(after.to_vec())
        .chain(before.to_vec())
}
