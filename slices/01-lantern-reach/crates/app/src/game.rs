//! The game's composition: one [`Game`] for `galaxy play`, `galaxy sim run`, `galaxy
//! shot` and the tests, so all of them run the same world.
//!
//! In tick order: the kernel's systems, the ship's (buy and sell orders go through the
//! economy's desk), the economy's (pulses, scouting, re-pricing and the dock screen's
//! trades), the script host running the `core` pack (the journey's beats and its
//! backbone) and, unless switched off, `gm-examples`, then progress (station services,
//! faction deeds, pilot traits and hull ranks), which counts the standing the scripts
//! changed earlier in the tick.
//!
//! Outside the tick, the fast-model layer ([`MindHost`]) is pumped after every step:
//! it reads new facts and its answers come back as ordinary batches; a line it decides
//! on is said through the talk log (`Command::Say`).
//!
//! - **Dock talk** has one catalog: the core pack (`lib/lines.luau`). Its talk
//!   script offers ordered, filled candidates; the mind returns one ID (or the
//!   script's default) through `talk_picked`. The script rechecks the docking and
//!   citation, then says and retires the line through its normal talk path.
//!   "What changed?" and beat conversations stay authored script replies.
//!
//! - **Packs.** `core` must load; it is checked with the strict typecheck when
//!   `luau-analyze` is found and loads without it, with a logged warning, when it isn't.
//!   `gm-examples` (every folder under `packs/gm-examples` but `bad_*`) is installed on
//!   new games ([`new_state`] sets [`GM_FLAG`]) and loads on saves that have it, unless
//!   [`GameOptions::gm`] is off (`--no-gm`, or `GALAXY_NO_GM=1`). A gm pack that fails
//!   its checks is logged and skipped; the journey never needs it. So is one a rollback
//!   switched off in this save (`PackState::off`): it stays off until the game master
//!   applies a new revision of it.
//! - **The mind.** [`MindMode::Live`] (play) runs `OpenRouterMind` on a [`Worker`]
//!   thread when `OPENROUTER_API_KEY` is set, and otherwise no model: people say their
//!   most specific authored line, and the log says so once. [`MindMode::Fake`] (`sim
//!   run`, `shot`, tests) answers with the seeded `FakeMind` inline, never the network.
//! - **One script host.** The game master's packs run in the same host: the bridge
//!   ([`Game::open_gm`]) loads the save's applied packs into it before it is attached,
//!   and stages, applies and rolls back packs there, so a gm pack runs in the same tick
//!   order as `core` and `gm-examples`, and a new revision of a pack replaces the one
//!   running. The sandbox child loads its packs the same way ([`Game::open_with`]).
//! - **Hot reload** ([`GameOptions::hot_reload`]) is for development only.
//! - **Older saves.** A save made before newer authored data gets it on open, applied
//!   with the first step: one `pacing_backfill` system batch with the `pacing.toml`
//!   tunables it lacks (at their authored values; saved ones stay), then one
//!   `craft_backfill` batch per named trader it lacks.
//! - **Quitting.** [`Game::finish`] shuts the mind down (booking every call still out),
//!   then steps once so its batches apply; save after it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use galaxy_contract::pack::{Severity, ValidationReport};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::TalkLine;
use galaxy_core::{Sim, SimState, System, TickReport};
use galaxy_gm::{Bridge, BridgeOptions};
use galaxy_mind::executor::{Done, Executor, Job};
use galaxy_mind::{FakeMind, Inline, MindConfig, MindHost, OpenRouterMind, SpokenLine, Worker};
use galaxy_script::ScriptHost;

/// Set on saves that install `gm-examples`. New games get it unless `--no-gm`.
pub const GM_FLAG: FlagId = FlagId::from_static("flag.gm_examples");

/// The environment variable that switches `gm-examples` off (`1`, `true`, `yes`).
pub const NO_GM_ENV: &str = "GALAXY_NO_GM";

/// Which fast model answers.
#[derive(Clone, Debug, PartialEq)]
pub enum MindMode {
    /// OpenRouter if `OPENROUTER_API_KEY` is set, otherwise no model.
    Live,
    /// The seeded fake, inline: deterministic and offline.
    Fake { seed: u64 },
    /// No model calls: script menus still receive their authored default.
    Off,
}

#[derive(Clone, Debug)]
pub struct GameOptions {
    /// The packs folder (holding `core/` and `gm-examples/`).
    pub packs: PathBuf,
    /// Load `gm-examples` on saves that install it.
    pub gm: bool,
    pub mind: MindMode,
    /// Watch the packs and reload them between ticks (development only).
    pub hot_reload: bool,
}

impl GameOptions {
    /// Headless defaults: gm-examples as the save says (and the environment allows),
    /// the fake mind seeded by the world, no hot reload.
    pub fn headless(packs: PathBuf, seed: u64) -> Self {
        Self {
            packs,
            gm: gm_allowed(),
            mind: MindMode::Fake { seed },
            hot_reload: false,
        }
    }
}

/// Whether the environment allows `gm-examples` (`GALAXY_NO_GM` unset or false).
pub fn gm_allowed() -> bool {
    !std::env::var(NO_GM_ENV)
        .is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

/// The kernel's, the ship's and the economy's systems, in tick order: everything native
/// that runs before the script host.
fn native(state: SimState) -> Sim {
    galaxy_core::game(state)
        .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
        .with_systems([Box::new(galaxy_economy::EconomySystem) as Box<dyn System>])
}

/// A new game from the core pack, installing `gm-examples` if `gm`.
pub fn new_state(packs: &Path, seed: u64, gm: bool) -> Result<SimState, String> {
    let mut state = galaxy_core::new_game(&core_data(packs), seed).map_err(|e| e.to_string())?;
    if gm {
        // Part of building the new world, before any tick: the save remembers it.
        state.world.player.flags.insert(GM_FLAG);
    }
    Ok(state)
}

/// The `gm-examples` pack folders, in name order (`bad_*` folders are the packs that
/// must be rejected, never installed).
pub fn gm_packs(packs: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(packs.join("gm-examples")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("manifest.toml").is_file())
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("bad_"))
        })
        .collect();
    dirs.sort();
    dirs
}

/// No model: people say their authored lines and trader menus take the utility
/// default at once (no calls budgeted, so no refused request is booked).
fn no_model(config: &mut MindConfig) {
    config.talk.pick = false;
    config.talk.free_phrasing = false;
    config.budget.default.calls = 0;
    config.budget.region.clear();
}

/// The executor for "no model": it takes no jobs, so every moment takes its default.
struct NoModel;

impl Executor for NoModel {
    fn submit(&mut self, _ticket: u64, _job: Job) -> bool {
        false
    }
    fn poll(&mut self) -> Vec<Done> {
        Vec::new()
    }
    fn in_flight(&self) -> usize {
        0
    }
}

/// The running game: the sim with every system attached, its script host and mind.
pub struct Game {
    sim: Sim,
    scripts: ScriptHost,
    mind: Option<MindHost>,
    hot_reload: bool,
    /// What a player or developer should know (warnings, skipped packs, the mind).
    notes: Vec<String>,
    reports: Vec<ValidationReport>,
}

/// The cause of a line the mind decided on.
fn said_cause(line: &SpokenLine) -> Cause {
    Cause {
        kind: CauseKind::Mind {
            npc: line.talk.speaker.clone(),
            decision: format!("dock_talk:{}", line.line),
        },
        summary: format!("{} says a line", line.line),
        cites: line.talk.cites.clone(),
    }
}

impl Game {
    pub fn mind_debug(&self) -> String {
        self.mind.as_ref().map_or_else(
            || "Mind off".into(),
            |mind| {
                format!(
                    "Mind ${:.4} / ${:.4}  calls {}  in flight {}",
                    mind.spent_usd(),
                    mind.config().budget.spend_cap_usd,
                    mind.stats().requests,
                    mind.in_flight()
                )
            },
        )
    }
    /// Builds the game over `state`. Fails only if the core pack can't load.
    pub fn open(state: SimState, opts: &GameOptions) -> Result<Game, String> {
        Self::open_with(state, opts, &mut |_, _| {})
    }

    /// Builds the game over `state`, running `hook` on its script host after `core`
    /// and `gm-examples` are loaded and before the host is attached: the game-master
    /// bridge loads the save's applied packs there ([`Game::open_gm`]), the sandbox
    /// child its packs. A pack loaded there replaces one with the same ID, runs at the
    /// host's place in the tick order (before progress), and installs its data with
    /// the rest.
    pub fn open_with(
        state: SimState,
        opts: &GameOptions,
        hook: galaxy_gm::Hook<'_>,
    ) -> Result<Game, String> {
        let mut sim = native(state);
        let mut notes = Vec::new();
        // A save made before newer tunables or named traders gets them (no-op otherwise).
        galaxy_core::queue_backfill(&mut sim, &core_data(&opts.packs))
            .map_err(|e| e.to_string())?;
        let analyzer = galaxy_script::analyzer::find(None);
        if analyzer.is_none() {
            notes.push(
                "luau-analyze not found: the packs load without the strict typecheck \
                 (run `cargo xtask fetch-luau`)"
                    .to_owned(),
            );
        }
        let scripts = ScriptHost::new(analyzer);
        let mut reports = Vec::new();
        let core = opts.packs.join("core");
        let report = scripts.load(&core, sim.world(), sim.history());
        if !report.accepted() {
            let why = report
                .errors()
                .next()
                .map_or_else(|| "its checks failed".to_owned(), |d| d.message.clone());
            return Err(format!("the core pack didn't load: {why}"));
        }
        reports.push(report);
        if opts.gm && sim.world().player.flags.contains(&GM_FLAG) {
            for dir in gm_packs(&opts.packs) {
                // A pack a rollback switched off stays off in this save, the bundled
                // copy included, until the game master applies a new revision.
                if let Ok(pack) = galaxy_script::read_pack(&dir)
                    && galaxy_script::host::pack_off(
                        sim.world(),
                        &sim.state().kernel.pending,
                        &pack.manifest.id,
                    )
                {
                    notes.push(format!(
                        "{} was rolled back in this save and stays off",
                        pack.manifest.id
                    ));
                    continue;
                }
                let report = scripts.load(&dir, sim.world(), sim.history());
                if !report.accepted() {
                    let why = report
                        .errors()
                        .next()
                        .map_or_else(|| "its checks failed".to_owned(), |d| d.message.clone());
                    notes.push(format!("{} didn't load and is skipped: {why}", report.pack));
                }
                reports.push(report);
            }
        }
        for r in &reports {
            let warnings = r.steps.iter().flat_map(|s| &s.diagnostics);
            for d in warnings.filter(|d| d.severity == Severity::Warning) {
                if d.code != "no_analyzer" {
                    notes.push(format!("{}: {}", r.pack, d.message));
                }
            }
        }
        hook(&scripts, &sim);
        scripts.attach(&mut sim);
        sim.add_system(Box::new(galaxy_progress::ProgressSystem));
        if opts.hot_reload
            && let Err(e) = scripts.watch()
        {
            notes.push(format!("hot reload is off: {e}"));
        }

        let data = core_data(&opts.packs);
        let mind = match &opts.mind {
            MindMode::Off => {
                let mut config = MindConfig::load_dir(&data).map_err(|e| e.to_string())?;
                no_model(&mut config);
                Some(MindHost::new(config, NoModel, sim.history()))
            }
            mode => {
                let mut config = MindConfig::load_dir(&data).map_err(|e| e.to_string())?;
                let exec: Box<dyn Executor> = match mode {
                    MindMode::Fake { seed } => Box::new(Inline::new(FakeMind::new(*seed))),
                    _ => match OpenRouterMind::from_env(config.models.clone(), &config.limits) {
                        Ok(m) => {
                            notes.push("the fast-model layer is on (OpenRouter)".to_owned());
                            Box::new(Worker::spawn(m, config.limits.max_in_flight.max(1)))
                        }
                        Err(_) => {
                            notes.push(
                                "OPENROUTER_API_KEY isn't set: no fast model, people say \
                                 their authored lines as written"
                                    .to_owned(),
                            );
                            no_model(&mut config);
                            Box::new(NoModel)
                        }
                    },
                };
                Some(MindHost::new(config, BoxedExec(exec), sim.history()))
            }
        };
        Ok(Game {
            sim,
            scripts,
            mind,
            hot_reload: opts.hot_reload,
            notes,
            reports,
        })
    }

    /// Builds the game with the game-master bridge for the save at `save`: the bridge
    /// loads the packs the game master applied into the game's own script host, so
    /// they run in the same tick order as `core` and `gm-examples`, and it stages,
    /// applies and rolls back packs in that host. The game still opens if the bridge
    /// can't (an unreadable game-master folder): that is the second result.
    pub fn open_gm(
        state: SimState,
        opts: &GameOptions,
        save: &Path,
        bridge: BridgeOptions,
    ) -> Result<(Game, Result<Bridge, String>), String> {
        let mut opened = None;
        let mut bridge = Some(bridge);
        let game = Self::open_with(state, opts, &mut |host, sim| {
            if let Some(b) = bridge.take() {
                opened = Some(Bridge::open(save, sim, host, b));
            }
        })?;
        let bridge = opened.unwrap_or_else(|| Err("the bridge wasn't opened".to_owned()));
        Ok((game, bridge))
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// The sim, for code that works between ticks.
    pub fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
    }

    pub fn scripts(&self) -> &ScriptHost {
        &self.scripts
    }

    /// The packs' validation reports, as loaded.
    pub fn reports(&self) -> &[ValidationReport] {
        &self.reports
    }

    /// Takes what happened worth logging since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// One tick, then the mind's pump. Between ticks, a dev hot reload swaps in packs
    /// that changed and still pass their checks. `None` while paused.
    pub fn step(&mut self, input: &PlayerInput) -> Option<TickReport> {
        if self.hot_reload {
            for r in self.scripts.poll_reload(&mut self.sim) {
                self.notes.push(if r.accepted() {
                    format!("reloaded {} ({})", r.pack, r.revision)
                } else {
                    format!(
                        "{} changed but failed its checks; the last good one runs",
                        r.pack
                    )
                });
            }
        }
        let report = self.sim.step(input)?;
        self.pump();
        Some(report)
    }

    /// Lets the mind read new facts and apply its answers. Never blocks.
    pub fn pump(&mut self) {
        let Some(mind) = &mut self.mind else { return };
        let report = mind.pump(&mut self.sim);
        self.say(&report.lines);
    }

    fn say(&mut self, lines: &[SpokenLine]) {
        for line in lines {
            say(&mut self.sim, said_cause(line), line.talk.clone());
        }
    }

    /// Ends the session before the final save: the mind books every call still out
    /// (blocking at most its drain bound), then, if that left batches, one tick applies
    /// them, even while paused. Save afterwards.
    pub fn finish(&mut self) {
        let Some(mind) = &mut self.mind else { return };
        let wait = mind.drain_bound().min(Duration::from_secs(30));
        let report = mind.shutdown(&mut self.sim, wait);
        self.say(&report.lines);
        if report.batches == 0 && report.lines.is_empty() {
            return;
        }
        let paused = self.sim.is_paused();
        self.sim.set_paused(false);
        self.sim.step(&PlayerInput::default());
        self.sim.set_paused(paused);
    }
}

impl galaxy_gm::Composed for Game {
    fn sim(&self) -> &Sim {
        &self.sim
    }
    fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
    }
    fn scripts(&self) -> &ScriptHost {
        &self.scripts
    }
    fn step(&mut self, input: &PlayerInput) -> Option<TickReport> {
        Game::step(self, input)
    }
}

/// Submits a line for the talk log.
pub fn say(sim: &mut Sim, cause: Cause, line: TalkLine) {
    sim.submit(CommandBatch {
        cause,
        commands: vec![Command::Say {
            line: Box::new(line),
        }],
    });
}

/// A boxed executor is an executor (the mind host takes one by value).
struct BoxedExec(Box<dyn Executor>);

impl Executor for BoxedExec {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        self.0.submit(ticket, job)
    }
    fn poll(&mut self) -> Vec<Done> {
        self.0.poll()
    }
    fn in_flight(&self) -> usize {
        self.0.in_flight()
    }
    fn close(&mut self, wait: Duration) -> Vec<Done> {
        self.0.close(wait)
    }
}

/// Finds the packs folder: the flag, `GALAXY_PACKS`, or the nearest `packs/core` above
/// the working directory or the executable.
pub fn packs_dir(flag: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = flag.or_else(|| std::env::var_os("GALAXY_PACKS").map(PathBuf::from)) {
        return Some(p);
    }
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf)),
    ];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            let packs = dir.join("packs");
            if packs.join("core").is_dir() {
                return Some(packs);
            }
        }
    }
    None
}

/// The core pack's data folder inside `packs`.
pub fn core_data(packs: &Path) -> PathBuf {
    packs.join("core").join("data")
}
