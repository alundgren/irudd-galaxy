//! `galaxy-mind`: the fast-model layer (system 17), on OpenRouter, behind a trait.
//!
//! Utility AI narrows each moment to a short menu of allowed, complete options; a fast
//! model only picks among them. Text is authored or absent: dock talk is pick and
//! fill, where the model picks one of the authored lines that fit and the game fills
//! its slots from the fact it cites, so every word shown is authored text or a fact
//! value. Free phrasing by a line model is an opt-in mode (`talk.free_phrasing`), best
//! effort, with the authored line as fallback. Nothing waits on a model:
//!
//! ```text
//! history fact (a docking ...)        MindHost::pump, once per frame or tick
//!   -> MenuProvider::menus            trader routes, dock talk, rumor spread
//!   -> brief + grounding packet       packet::build, keyed by situation::key
//!   -> cache / budget / queue         answer from history, or the utility default
//!   -> Executor (worker thread)       Mind::decide (Jev); Mind::line (gpt-oss) opt-in
//!   -> validate                       shape, references, rules, limits (text: opt-in)
//!   -> CommandBatch                   Cause::Mind { npc, decision }, applied by the sim
//!      + a `mind.decision` / `mind.line` fact with the request key
//! ```
//!
//! - [`Mind`]: the trait. [`OpenRouterMind`] (real HTTP), [`FakeMind`] (seeded,
//!   deterministic) and [`RecordedMind`] (replays decisions from history).
//! - [`Executor`]: [`Worker`] runs a mind on its own thread with a bounded queue;
//!   [`Inline`] answers at once, for tests and the journey bot.
//! - [`MindHost`]: reads new history facts, opens menus, applies answers as ordinary
//!   command batches. Results never touch the world directly. Call
//!   [`MindHost::shutdown`] before the final save so every billed call is booked.
//! - [`MindConfig`]: models, limits, per-region budgets and NPC briefs, from the core
//!   pack's `data/mind.toml`.
//! - [`MusicLibrary`]: the radio's music, Lyria 3 tracks generated in the background,
//!   cached beside the save, with placeholders when there are none ([`music`]).
//!
//! The API key comes only from `OPENROUTER_API_KEY` and never appears in an error,
//! `Debug` output or log line. `cargo test` never touches the network; the live tests
//! are `#[ignore]`.

pub mod calllog;
pub mod config;
pub mod executor;
pub mod fake;
pub mod host;
pub mod lines;
pub mod live;
pub mod menu;
pub mod mind;
pub mod music;
pub mod openrouter;
pub mod packet;
pub mod probe;
pub mod radio;
mod script_talk;
pub mod situation;
pub mod traders;
pub mod validate;

pub use config::{Brief, Budget, MindConfig};
pub use executor::{Done, Executor, Inline, Job, JobResult, Worker};
pub use fake::{FakeMind, RecordedMind, Recording};
pub use host::{MindHost, PumpReport, SpokenLine};
pub use lines::{LINES_FILE, LineDef, TalkLines};
pub use menu::{
    DockTalk, LineCandidate, LinePool, LineSource, Menu, MenuCtx, MenuOption, MenuProvider,
    PlanOption, PlanSource, PoolLine, Precondition, RumorSpread, TraderMenus,
};
pub use mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Question, Reply,
};
pub use music::{MusicLibrary, MusicRules, TrackSource};
pub use openrouter::{ApiKey, HttpReply, OpenRouterMind, Transport, UreqTransport};
pub use packet::Packet;

/// Fact kind of a decision outcome (one per menu that was opened).
pub const DECISION_FACT: &str = "mind.decision";
/// Fact kind of a spoken line (dock talk).
pub const LINE_FACT: &str = "mind.line";
/// Fact kind of a rumor carried to a new station.
pub const RUMOR_SPREAD_FACT: &str = "mind.rumor_spread";
