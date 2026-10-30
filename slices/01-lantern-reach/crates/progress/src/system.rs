//! The per-tick progress system.
//!
//! 1. Each service the player asked for this tick (`PlayerInput::services`) becomes
//!    one atomic batch; one that doesn't apply is dropped (check it first).
//! 2. After anything happened (facts committed last tick): standing from faction
//!    deeds, pilot traits, and the next hull rank, each as its own batch.
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

use crate::{growth, services, standing};

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
        let earlier: Vec<Command> = ctx
            .queued()
            .iter()
            .flat_map(|b| &b.commands)
            .filter(|c| matches!(c, Command::AdjustStanding { .. }))
            .cloned()
            .collect();
        let requests = ctx.input.services.clone();
        for req in &requests {
            if let Ok(batch) = services::batch(world, history, req) {
                ctx.submit(batch);
            }
        }
        if standing::last_tick_facts(world, history).is_empty() {
            return;
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
            "the hull earned a stripe",
            growth::earned_rank(world, history),
        );
    }
}
