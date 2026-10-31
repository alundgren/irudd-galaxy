//! The per-tick progress system.
//!
//! 1. Each answer the player gave at the dock (`PlayerInput::replies`) becomes a
//!    `reply` event, and each service the player asked for this tick
//!    (`PlayerInput::services`) one atomic batch; one that doesn't apply is dropped
//!    (check it first). A reply committed last tick that claims a read survey buoy's
//!    reward pays it ([`crate::survey`]).
//! 2. After anything happened (facts committed last tick): a part a friend sends
//!    arriving in the hull's locker once the flagship has docked at the friend's
//!    station ([`hullwright::arrivals`]), then standing from faction deeds, pilot
//!    traits, blueprints found, and the next hull rank, each as its own batch.
//!
//! Every batch is worked out from the tick's starting world and checks what it relies
//! on when it applies (`Expect*`), so a purchase asked for twice, a rank earned on the
//! prestige tick, or a deed whose standing another batch moved fails whole instead of
//! overwriting. Deeds are worked out after the standing changes already queued this
//! tick (by systems that run earlier, such as the script host).
//!
//! Beats run in the script host, which owns the packs that register them.

use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};

use crate::{growth, hullwright, services, standing, survey};

/// The name the system runs under.
pub const SYSTEM_NAME: &str = "progress";

/// Stateless: everything it needs is in the world and history.
pub struct ProgressSystem;

impl System for ProgressSystem {
    fn name(&self) -> &'static str {
        SYSTEM_NAME
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let (world, history) = (ctx.world, ctx.history);
        // Standing other systems (the script host's beats and deeds) change this tick.
        let mut earlier: Vec<Command> = ctx
            .queued()
            .iter()
            .flat_map(|b| &b.commands)
            .filter(|c| matches!(c, Command::AdjustStanding { .. }))
            .cloned()
            .collect();
        for reply in &ctx.input.replies {
            if let Some(batch) = services::reply(world, reply) {
                ctx.submit(batch);
            }
        }
        for batch in survey::claims(world, ctx.events, &earlier) {
            earlier.extend(
                batch
                    .commands
                    .iter()
                    .filter(|c| matches!(c, Command::AdjustStanding { .. }))
                    .cloned(),
            );
            ctx.submit(batch);
        }
        let requests = ctx.input.services.clone();
        let mut prestige = false;
        for req in &requests {
            if let Ok(batch) = services::batch(world, history, req) {
                prestige |= matches!(req, ServiceRequest::Prestige { .. });
                ctx.submit(batch);
            }
        }
        if standing::last_tick_facts(world, history).is_empty() {
            return;
        }
        // A docking is a fact, so a part a friend sends arrives the tick after it.
        for batch in hullwright::arrivals(world, history) {
            ctx.submit(batch);
        }
        let mut submit = |summary: &str, commands: Vec<Command>| {
            if !commands.is_empty() {
                ctx.submit(CommandBatch {
                    cause: Cause::system(SYSTEM_NAME, summary),
                    commands,
                });
            }
        };
        submit(
            "factions remember what the player did",
            standing::due_deeds(world, history, &earlier),
        );
        submit(
            "the pilot picked up a habit",
            growth::earned_traits(world, history),
        );
        submit(
            "the pilot found a blueprint",
            hullwright::found_blueprints(world, history),
        );
        // A prestige carries the stripe due this tick itself. If it fails, the next
        // check after anything happens finds the stripe still due.
        if !prestige {
            submit(
                "the hull earned a stripe",
                growth::earned_rank(world, history),
            );
        }
    }
}
