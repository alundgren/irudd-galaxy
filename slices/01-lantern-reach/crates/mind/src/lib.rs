//! `galaxy-mind`: the fast-model layer (system 17), on OpenRouter, behind a trait.
//!
//! Utility AI narrows each moment to a short menu of allowed, complete options; a fast
//! model only picks among them or rephrases an authored line. Nothing waits on it:
//!
//! ```text
//! history fact (a docking ...)        MindHost::pump, once per frame or tick
//!   -> MenuProvider::menus            trader routes, dock talk, rumor spread
//!   -> brief + grounding packet       packet::build, keyed by situation::key
//!   -> cache / budget / queue         answer from history, or the utility default
//!   -> Executor (worker thread)       Mind::decide (Jev) or Mind::line (gpt-oss)
//!   -> validate                       shape, references, rules, limits, text
//!   -> CommandBatch                   Cause::Mind { npc, decision }, applied by the sim
//!      + a `mind.decision` / `mind.line` fact with the request key
//! ```
//!
//! - [`Mind`]: the trait. [`OpenRouterMind`] (real HTTP), [`FakeMind`] (seeded,
//!   deterministic) and [`RecordedMind`] (replays decisions from history).
//! - [`Executor`]: [`Worker`] runs a mind on its own thread with a bounded queue;
//!   [`Inline`] answers at once, for tests and the journey bot.
//! - [`MindHost`]: reads new history facts, opens menus, applies answers as ordinary
//!   command batches. Results never touch the world directly.
//! - [`MindConfig`]: models, limits, per-region budgets and NPC briefs, from the core
//!   pack's `data/mind.toml`.
//!
//! The API key comes only from `OPENROUTER_API_KEY`. `cargo test` never touches the
//! network; the live tests are `#[ignore]`.

pub mod config;
pub mod executor;
pub mod fake;
pub mod host;
pub mod menu;
pub mod mind;
pub mod openrouter;
pub mod packet;
pub mod situation;
pub mod validate;

pub use config::{Brief, Budget, MindConfig};
pub use executor::{Done, Executor, Inline, Job, JobResult, Worker};
pub use fake::{FakeMind, RecordedMind, Recording};
pub use host::{MindHost, PumpReport, SpokenLine};
pub use menu::{
    DockTalk, LineCandidate, LinePool, LineSource, Menu, MenuCtx, MenuOption, MenuProvider,
    PlanOption, PlanSource, PoolLine, Precondition, RumorSpread, TraderMenus,
};
pub use mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Question, Reply,
};
pub use openrouter::{ApiKey, HttpReply, OpenRouterMind, Transport, UreqTransport};
pub use packet::Packet;

/// Fact kind of a decision outcome (one per menu that was opened).
pub const DECISION_FACT: &str = "mind.decision";
/// Fact kind of a spoken line (dock talk).
pub const LINE_FACT: &str = "mind.line";
/// Fact kind of a rumor carried to a new station.
pub const RUMOR_SPREAD_FACT: &str = "mind.rumor_spread";
