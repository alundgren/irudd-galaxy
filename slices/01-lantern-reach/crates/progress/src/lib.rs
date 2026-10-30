//! `galaxy-progress` (W4): the journey's runtime.
//!
//! - [`beats`]: how registered beats start, advance and complete; goal lines for the
//!   HUD. The `core` pack registers beats in Luau (`G.beat`); the script host drives
//!   them with this module.
//! - [`standing`]: hidden standing, regard tiers, faction deeds.
//! - [`growth`]: pilot traits and hull ranks, earned from deeds done many times.
//! - [`services`]: what hullwrights and dockmasters fit and build; [`prestige`]: the
//!   hullwright's Hearthbound trade-up.
//! - [`survey`]: survey-buoy rewards, paid on a reply with standing deeds.
//! - [`modifiers`]: the native effects of traits, fittings and ranks, added up per
//!   ship, for the ship, economy and app crates.
//! - [`ProgressSystem`]: the per-tick system. Register it after the ship and economy
//!   systems.
//!
//! Every threshold, cost and fee is pack data (`world.progress.rules`, loaded from the
//! core pack). Nothing here reads a clock.

pub mod beats;
pub mod eval;
pub mod growth;
pub mod modifiers;
pub mod prestige;
pub mod services;
pub mod standing;
pub mod survey;
mod system;

pub use eval::{Eval, EvalError};
pub use modifiers::Modifiers;
pub use system::{ProgressSystem, SYSTEM_NAME};
