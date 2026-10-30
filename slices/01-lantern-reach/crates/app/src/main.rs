//! `galaxy`: the game and its JSON CLI.
//!
//! Every command prints one JSON value on stdout. Exit codes:
//! 0 ok, 1 runtime failure (I/O, SQLite), 2 usage error, 3 not implemented yet,
//! 4 bad input (missing or existing save, invalid data, unknown path or ID),
//! 5 rejected (`pack check` rejected the pack, or `pack test` had failures; the report
//! is on stdout).
//!
//! Keep this crate thin: parse arguments, call library code, print JSON.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, SaveError, SimState};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    name = "galaxy",
    version,
    about = "The Lantern Reach: game and JSON CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new save from the core pack.
    New {
        #[arg(long)]
        save: PathBuf,
        /// New-game seed (default 1, so runs are reproducible).
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[command(flatten)]
        packs: PacksArg,
        /// Don't install `gm-examples`: the save runs the core pack only.
        #[arg(long)]
        no_gm: bool,
    },
    /// Headless simulation.
    Sim {
        #[command(subcommand)]
        command: SimCmd,
    },
    /// Read world state.
    State {
        #[command(subcommand)]
        command: StateCmd,
    },
    /// Read the history log.
    History {
        #[command(subcommand)]
        command: HistoryCmd,
    },
    /// The contract's JSON Schemas.
    Api {
        #[command(subcommand)]
        command: ApiCmd,
    },
    /// Play in a window. Continues the latest save in the per-user save folder unless
    /// `--new` or `--save` says otherwise.
    Play {
        /// The save to play (default: the latest in the per-user save folder).
        #[arg(long)]
        save: Option<PathBuf>,
        /// Start a new game.
        #[arg(long)]
        new: bool,
        /// New-game seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Jump to a journey beat (not implemented yet).
        #[arg(long)]
        beat: Option<u32>,
        /// Close after this many frames: a smoke test of the window.
        #[arg(long, hide = true)]
        quit_after_frames: Option<u32>,
        #[command(flatten)]
        packs: PacksArg,
        /// Run the core pack only, without `gm-examples` (also `GALAXY_NO_GM=1`).
        #[arg(long)]
        no_gm: bool,
        /// Reload packs when their files change (development).
        #[arg(long)]
        hot_reload: bool,
    },
    /// Jump to a journey beat (not implemented yet).
    Beat {
        #[command(subcommand)]
        command: BeatCmd,
    },
    /// Validate a pack or run its tests.
    Pack {
        #[command(subcommand)]
        command: PackCmd,
    },
    /// The game-master bridge: digest, propose, status, rollback.
    Gm {
        #[command(subcommand)]
        command: GmCmd,
    },
    /// Stream the running game's events as JSON lines (live only).
    Watch {
        #[command(flatten)]
        live: LiveArg,
        /// Replay facts after this one first (`fact.N`).
        #[arg(long)]
        since: Option<String>,
        /// Stop after this many items.
        #[arg(long)]
        count: Option<u64>,
    },
    /// Render a save headless to a PNG, with the same scene and panels as play; or, with
    /// `--live`, what the running game shows right now.
    Shot {
        #[arg(long)]
        save: Option<PathBuf>,
        #[command(flatten)]
        live: LiveArg,
        #[arg(long)]
        out: PathBuf,
        /// Show the map instead of the world.
        #[arg(long)]
        map: bool,
        /// Image size, `WxH`.
        #[arg(long, default_value = "1280x720")]
        size: String,
        /// Visible world height (the mouse-wheel zoom).
        #[arg(long)]
        zoom: Option<f32>,
        /// Scripted input to run first, e.g. `d*120,space*150` (see `script.rs`).
        #[arg(long)]
        keys: Option<String>,
        /// Write the scripted run back to the save.
        #[arg(long)]
        write: bool,
    },
    /// The fast-model layer on OpenRouter.
    Mind {
        #[command(subcommand)]
        command: MindCmd,
    },
    /// The sound cues.
    Audio {
        #[command(subcommand)]
        command: AudioCmd,
    },
}

#[derive(Subcommand)]
enum AudioCmd {
    /// Render every take of every cue to WAV files in DIR and measure them against the
    /// design limits. Exit 5 if one is outside them.
    Render {
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum MindCmd {
    /// One live Jev choice and one live gpt-oss line, with latency and cost. Needs
    /// `OPENROUTER_API_KEY`.
    Try {
        #[command(flatten)]
        packs: PacksArg,
    },
}

#[derive(Subcommand)]
enum SimCmd {
    /// Fast-forward a save headless with hands-off input, then write it back.
    Run {
        #[arg(long)]
        save: PathBuf,
        #[arg(long)]
        ticks: u64,
        /// Don't write the result back to the save.
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        packs: PacksArg,
        /// Run the core pack only, without `gm-examples`.
        #[arg(long)]
        no_gm: bool,
    },
    /// One sandbox run (the child process `gm propose` starts): fly the activity bot on
    /// a fork with these packs and print what happened. Never writes the save.
    Sandbox {
        #[arg(long)]
        save: PathBuf,
        /// A pack folder to run (repeat).
        #[arg(long)]
        pack: Vec<PathBuf>,
        #[arg(long, default_value_t = 6)]
        dockings: u32,
        #[arg(long, default_value_t = 3000)]
        ticks_per_docking: u64,
        #[command(flatten)]
        packs_dir: PacksArg,
    },
}

#[derive(Subcommand)]
enum StateCmd {
    /// Print the world state, or part of it.
    Get {
        #[command(flatten)]
        target: TargetArgs,
        /// A path into the world: dotted (`stations.station.kettle_dock.build`; IDs
        /// keep their dots) or slash-separated (`stations/station.kettle_dock/build`).
        #[arg(long)]
        path: Option<String>,
    },
    /// Records of one store matching filters.
    Query {
        /// The store: `stations`, `ships`, `npcs`, `markets`, `rocks`, `rumors` ...
        store: String,
        #[command(flatten)]
        target: TargetArgs,
        /// `field=value` (or `field~text`, a substring); repeat for more.
        #[arg(long = "where")]
        filters: Vec<String>,
        /// Only these fields, comma-separated.
        #[arg(long, value_delimiter = ',')]
        fields: Vec<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum HistoryCmd {
    /// Facts matching filters, oldest first.
    Query {
        #[command(flatten)]
        target: TargetArgs,
        #[command(flatten)]
        filter: HistoryFilter,
    },
}

/// `--save PATH` or `--live [SAVE]`.
#[derive(Args)]
struct TargetArgs {
    /// Read or change this save file.
    #[arg(long)]
    save: Option<PathBuf>,
    #[command(flatten)]
    live: LiveArg,
}

#[derive(Args)]
struct LiveArg {
    /// Talk to the running game: the newest one, or the one playing SAVE.
    #[arg(long, num_args = 0..=1, value_name = "SAVE")]
    live: Option<Option<PathBuf>>,
}

#[derive(Args)]
struct HistoryFilter {
    /// Fact kind, e.g. `dock` or `gm.kettle.stage_up`.
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    ship: Option<String>,
    #[arg(long)]
    station: Option<String>,
    #[arg(long)]
    ware: Option<String>,
    #[arg(long)]
    region: Option<String>,
    /// `fact.N`: facts after that fact. A bare number: facts at or after that tick.
    #[arg(long)]
    since: Option<String>,
    /// Facts at or before this tick.
    #[arg(long)]
    until: Option<u64>,
    #[arg(long)]
    limit: Option<u32>,
}

#[derive(Subcommand)]
enum ApiCmd {
    /// Print every schema as `{name: schema}`, or one with `--name`.
    Dump {
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum BeatCmd {
    Jump(Rest),
}

#[derive(Subcommand)]
enum PackCmd {
    /// Validate a pack: manifest, strict typecheck, hooks, references, invariants and
    /// a smoke run. Prints the validation report; exit 0 if accepted, 5 if rejected.
    Check {
        /// The pack folder.
        dir: PathBuf,
        /// Resolve IDs and run the smoke step against this save (default: a new game
        /// from the core pack).
        #[arg(long)]
        save: Option<PathBuf>,
        #[command(flatten)]
        packs: PacksArg,
        /// The `luau-analyze` binary (default: $GALAXY_LUAU_ANALYZE, else `tools/luau/`
        /// from `cargo xtask fetch-luau`).
        #[arg(long)]
        analyzer: Option<PathBuf>,
        /// Skip the strict typecheck with a warning when `luau-analyze` is missing,
        /// instead of rejecting.
        #[arg(long)]
        no_typecheck: bool,
    },
    /// Run the pack's `tests/*.luau` unit tests. Exit 0 if all pass, 5 if not.
    Test {
        /// The pack folder.
        dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum GmCmd {
    /// What changed since a fact, by place and person, citing fact IDs.
    Digest {
        #[command(flatten)]
        target: TargetArgs,
        /// `fact.N` (the last digest's `until`); everything without.
        #[arg(long)]
        since: Option<String>,
    },
    /// Validate a pack in full (fast checks, sandbox run, legibility) and stage it for
    /// the next calm moment (live) or the next load (save). Exit 5 if rejected.
    Propose {
        /// The pack folder.
        dir: PathBuf,
        #[command(flatten)]
        target: TargetArgs,
        /// Live: wait until it applies and report the change ID.
        #[arg(long)]
        wait: bool,
        /// Seconds `--wait` waits.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        /// Dockings each sandbox run flies.
        #[arg(long, default_value_t = 6)]
        dockings: u32,
        #[arg(long)]
        analyzer: Option<PathBuf>,
        /// Skip the strict typecheck with a warning when `luau-analyze` is missing.
        #[arg(long)]
        no_typecheck: bool,
        /// The packs folder the sandbox runs use. Live, the default is the running
        /// game's own.
        #[command(flatten)]
        packs: PacksArg,
    },
    /// Applied, staged and quarantined packs.
    Status {
        #[command(flatten)]
        target: TargetArgs,
    },
    /// In-world rollback of a change (`cause.N` or `stage.N`).
    Rollback {
        change: String,
        #[command(flatten)]
        target: TargetArgs,
        /// The packs folder the save is opened with (save only).
        #[command(flatten)]
        packs: PacksArg,
    },
}

/// Accepts anything, so stubs can be filled in without breaking callers.
#[derive(Args)]
struct Rest {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    args: Vec<String>,
}

#[derive(Args)]
struct PacksArg {
    /// The packs folder (default: $GALAXY_PACKS, else the nearest `packs/` above the
    /// working directory or the executable).
    #[arg(long)]
    packs: Option<PathBuf>,
}

/// A failed command: exit code plus message.
struct Fail {
    code: u8,
    kind: &'static str,
    message: String,
    command: Option<String>,
    /// Printed instead of the error object (a rejected pack's report).
    body: Option<Value>,
}

impl Fail {
    fn input(message: impl Into<String>) -> Self {
        Fail {
            code: 4,
            kind: "bad_input",
            message: message.into(),
            command: None,
            body: None,
        }
    }
}

impl Fail {
    fn usage(message: impl Into<String>) -> Self {
        Fail {
            code: 2,
            kind: "usage",
            message: message.into(),
            command: None,
            body: None,
        }
    }
}

impl From<gm::CliError> for Fail {
    fn from(e: gm::CliError) -> Self {
        let kind = match e.code {
            1 => "runtime",
            2 => "usage",
            4 => "bad_input",
            5 => "rejected",
            _ => "error",
        };
        Fail {
            code: e.code,
            kind,
            message: if e.kind.is_empty() || e.kind == kind {
                e.message
            } else {
                format!("{} ({})", e.message, e.kind)
            },
            command: None,
            body: e.body,
        }
    }
}

impl From<SaveError> for Fail {
    fn from(e: SaveError) -> Self {
        let input = matches!(
            e,
            SaveError::Exists(_)
                | SaveError::Missing(_)
                | SaveError::Format { .. }
                | SaveError::NoSnapshot(_)
        );
        Fail {
            code: if input { 4 } else { 1 },
            kind: if input { "bad_input" } else { "save" },
            message: e.to_string(),
            command: None,
            body: None,
        }
    }
}

type Out = Result<Value, Fail>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::New {
            save,
            seed,
            packs,
            no_gm,
        } => new(&save, seed, packs.packs, no_gm),
        Cmd::Sim {
            command:
                SimCmd::Run {
                    save,
                    ticks,
                    dry_run,
                    packs,
                    no_gm,
                },
        } => sim_run(&save, ticks, dry_run, packs.packs, no_gm),
        Cmd::Sim {
            command:
                SimCmd::Sandbox {
                    save,
                    pack,
                    dockings,
                    ticks_per_docking,
                    packs_dir,
                },
        } => sim_sandbox(save, pack, dockings, ticks_per_docking, packs_dir.packs),
        Cmd::State {
            command: StateCmd::Get { target, path },
        } => with_target(target, |t| gm::state_get(t, path.as_deref())),
        Cmd::State {
            command:
                StateCmd::Query {
                    store,
                    target,
                    filters,
                    fields,
                    limit,
                },
        } => with_target(target, |t| {
            gm::state_query(
                t,
                &galaxy_gm::reads::StateQuery {
                    store,
                    filters,
                    fields,
                    limit,
                },
            )
        }),
        Cmd::History {
            command: HistoryCmd::Query { target, filter },
        } => history_query(target, filter),
        Cmd::Api {
            command: ApiCmd::Dump { name },
        } => api_dump(name.as_deref()),
        Cmd::Play { beat: Some(_), .. } => not_implemented("play --beat"),
        Cmd::Play {
            save,
            new,
            seed,
            packs,
            beat: None,
            quit_after_frames,
            no_gm,
            hot_reload,
        } => galaxy_app::play::play(galaxy_app::play::PlayArgs {
            save,
            new,
            seed,
            packs: packs.packs,
            no_gm,
            hot_reload,
            quit_after_frames,
        })
        .map_err(fail),
        Cmd::Beat {
            command: BeatCmd::Jump(_),
        } => not_implemented("beat jump"),
        Cmd::Pack {
            command:
                PackCmd::Check {
                    dir,
                    save,
                    packs,
                    analyzer,
                    no_typecheck,
                },
        } => pack_check(&dir, save.as_deref(), packs.packs, analyzer, no_typecheck),
        Cmd::Pack {
            command: PackCmd::Test { dir },
        } => pack_test(&dir),
        Cmd::Gm {
            command: GmCmd::Digest { target, since },
        } => parse_fact(since).and_then(|since| with_target(target, |t| gm::digest(t, since))),
        Cmd::Gm {
            command:
                GmCmd::Propose {
                    dir,
                    target,
                    wait,
                    timeout,
                    dockings,
                    analyzer,
                    no_typecheck,
                    packs,
                },
        } => {
            // Live, the sandbox runs with the running game's packs folder unless one
            // is given; on a save, with the one this command would load.
            let packs = match (&packs.packs, target.live.live.is_some()) {
                (None, true) => None,
                _ => galaxy_app::game::packs_dir(packs.packs)
                    .and_then(|p| std::path::absolute(p).ok()),
            };
            let opts = galaxy_gm::ProposeOptions {
                exe: std::env::current_exe().unwrap_or_else(|_| "galaxy".into()),
                analyzer: galaxy_script::analyzer::find(analyzer.as_deref()),
                require_typecheck: !no_typecheck,
                packs,
                rules: galaxy_gm::SandboxRules {
                    dockings,
                    ..galaxy_gm::SandboxRules::default()
                },
            };
            let wait = wait.then(|| std::time::Duration::from_secs(timeout));
            with_target(target, |t| gm::propose(t, &dir, &opts, wait))
        }
        Cmd::Gm {
            command: GmCmd::Status { target },
        } => with_target(target, gm::status),
        Cmd::Gm {
            command:
                GmCmd::Rollback {
                    change,
                    target,
                    packs,
                },
        } => with_target(target, |t| gm::rollback(t, &change, headless(packs.packs))),
        Cmd::Watch { live, since, count } => watch(live, since, count),
        Cmd::Shot {
            live: LiveArg { live: Some(live) },
            out,
            ..
        } => gm::connect(
            live.as_deref(),
            registry().as_deref(),
            Some(gm::CALL_TIMEOUT),
        )
        .and_then(|mut c| gm::shot_live(&mut c, &out))
        .map_err(Fail::from),
        Cmd::Shot {
            save,
            live: LiveArg { live: None },
            out,
            map,
            size,
            zoom,
            keys,
            write,
        } => match save {
            Some(save) => shot(save, out, map, &size, zoom, keys, write),
            None => Err(Fail::usage(
                "pass --save PATH, or --live for the running game",
            )),
        },
        Cmd::Mind {
            command: MindCmd::Try { packs },
        } => mind_try(packs.packs),
        Cmd::Audio {
            command: AudioCmd::Render { out },
        } => galaxy_app::sound::render(&out).map_err(|(code, body)| Fail {
            code,
            kind: if code == 5 { "rejected" } else { "runtime" },
            message: String::new(),
            command: None,
            body: Some(body),
        }),
    };
    match result {
        Ok(value) => {
            print_json(&value);
            ExitCode::SUCCESS
        }
        Err(Fail {
            code,
            body: Some(body),
            ..
        }) => {
            print_json(&body);
            ExitCode::from(code)
        }
        Err(f) => {
            let mut out = json!({ "error": f.message, "kind": f.kind });
            if let Some(command) = f.command {
                out["command"] = command.into();
            }
            print_json(&out);
            ExitCode::from(f.code)
        }
    }
}

fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("JSON values serialize")
    );
}

/// A library error as a CLI failure: 4 is bad input, anything else a runtime failure.
fn fail((code, message): (u8, String)) -> Fail {
    Fail {
        code,
        kind: if code == 4 { "bad_input" } else { "runtime" },
        message,
        command: None,
        body: None,
    }
}

fn shot(
    save: PathBuf,
    out: PathBuf,
    map: bool,
    size: &str,
    zoom: Option<f32>,
    keys: Option<String>,
    write: bool,
) -> Out {
    let (width, height) = galaxy_app::shot::parse_size(size).map_err(Fail::input)?;
    galaxy_app::shot::shot(&galaxy_app::shot::ShotArgs {
        save,
        out,
        map,
        width,
        height,
        zoom,
        keys,
        write,
    })
    .map_err(fail)
}

fn not_implemented(command: &str) -> Out {
    Err(Fail {
        code: 3,
        kind: "not_implemented",
        message: "not implemented yet".into(),
        command: Some(command.to_owned()),
        body: None,
    })
}

/// Exit 5 with `body` on stdout.
fn rejected(body: Value) -> Fail {
    Fail {
        code: 5,
        kind: "rejected",
        message: String::new(),
        command: None,
        body: Some(body),
    }
}

fn pack_check(
    dir: &Path,
    save: Option<&Path>,
    packs: Option<PathBuf>,
    analyzer: Option<PathBuf>,
    no_typecheck: bool,
) -> Out {
    if !dir.is_dir() {
        return Err(Fail::input(format!("{} is not a folder", dir.display())));
    }
    let state = match save {
        Some(path) => Save::open(path)?.load()?,
        None => {
            let data = packs_dir(packs)?.join("core").join("data");
            galaxy_core::new_game(&data, 1).map_err(|e| Fail::input(e.to_string()))?
        }
    };
    let analyzer = galaxy_script::analyzer::find(analyzer.as_deref());
    let checked = galaxy_script::check_dir(
        dir,
        galaxy_script::CheckOptions {
            world: Some(&state.world),
            history: Some(&state.history),
            analyzer: analyzer.as_deref(),
            require_typecheck: !no_typecheck,
            ..Default::default()
        },
    );
    let report = serde_json::to_value(&checked.report).expect("reports serialize");
    if checked.accepted() {
        Ok(report)
    } else {
        Err(rejected(report))
    }
}

fn pack_test(dir: &Path) -> Out {
    if !dir.is_dir() {
        return Err(Fail::input(format!("{} is not a folder", dir.display())));
    }
    match galaxy_script::run_pack_tests(dir) {
        Ok(r) => {
            let v = serde_json::to_value(&r).expect("reports serialize");
            if r.passed { Ok(v) } else { Err(rejected(v)) }
        }
        Err(report) => Err(rejected(
            serde_json::to_value(&*report).expect("reports serialize"),
        )),
    }
}

/// Finds the packs folder: flag, `GALAXY_PACKS`, or the nearest `packs/core` above
/// the working directory or the executable.
fn packs_dir(flag: Option<PathBuf>) -> Result<PathBuf, Fail> {
    galaxy_app::game::packs_dir(flag)
        .ok_or_else(|| Fail::input("can't find the packs folder; pass --packs or set GALAXY_PACKS"))
}

fn new(save: &Path, seed: u64, packs: Option<PathBuf>, no_gm: bool) -> Out {
    if save.exists() {
        return Err(SaveError::Exists(save.to_path_buf()).into());
    }
    let gm = !no_gm && galaxy_app::game::gm_allowed();
    let state = galaxy_app::game::new_state(&packs_dir(packs)?, seed, gm).map_err(Fail::input)?;
    let hash = galaxy_core::state_hash(&state);
    Save::create(save, &state)?;
    Ok(json!({
        "save": save,
        "seed": seed,
        "tick": state.world.meta.tick,
        "hash": hash,
        "stations": state.world.stations.len(),
        "rocks": state.world.rocks.len(),
        "gm_examples": gm,
    }))
}

/// Fast-forwards the whole game (the same composition as play, with the seeded fake
/// mind instead of the network).
fn sim_run(path: &Path, ticks: u64, dry_run: bool, packs: Option<PathBuf>, no_gm: bool) -> Out {
    use galaxy_app::game::{Game, GameOptions};
    let mut save = Save::open(path)?;
    let state = save.load()?;
    let seed = state.world.meta.seed;
    let mut opts = GameOptions::headless(packs_dir(packs)?, seed);
    opts.gm &= !no_gm;
    // The packs the game master applied run in the game's script host, and staged
    // ones apply on this load (a calm moment), unless it's a dry run.
    let bridge_opts = galaxy_gm::BridgeOptions {
        packs: std::path::absolute(&opts.packs).ok(),
        ..galaxy_gm::BridgeOptions::default()
    };
    let (mut game, bridge) = Game::open_gm(state, &opts, path, bridge_opts).map_err(Fail::input)?;
    let mut bridge = bridge.map_err(Fail::input)?;
    let gm_applied = if dry_run {
        Vec::new()
    } else {
        bridge.apply_staged(game.sim_mut(), &mut save)
    };
    let start = game.sim().tick();
    let input = PlayerInput::default();
    let (mut applied, mut events) = (0usize, 0usize);
    let mut rejected = Vec::new();
    for _ in 0..ticks {
        let Some(report) = game.step(&input) else {
            break;
        };
        applied += report.applied;
        events += report.events;
        rejected.extend(report.rejected);
    }
    game.finish();
    let notes = game.take_notes();
    let packs: Vec<String> = game.reports().iter().map(|r| r.pack.to_string()).collect();
    let sim = game.sim();
    if !dry_run {
        // The save first: state.json never runs ahead of it (and if it did, the next
        // load would reconcile it).
        save.write(sim.state())?;
        bridge.after_tick(sim);
    }
    Ok(json!({
        "packs": packs,
        "notes": notes,
        "from_tick": start,
        "tick": sim.tick(),
        "ticks_run": sim.tick().0 - start.0,
        "hash": sim.hash(),
        "batches_applied": applied,
        "events": events,
        "rejected_count": rejected.len(),
        "rejected": rejected.iter().take(20).collect::<Vec<_>>(),
        "saved": !dry_run,
        "gm_applied": gm_applied,
        "gm_notes": bridge.notes,
    }))
}

use galaxy_gm::cli as gm;

/// The per-user registry of running games.
fn registry() -> Option<PathBuf> {
    galaxy_app::saves::live_registry()
}

fn with_target(
    t: TargetArgs,
    f: impl FnOnce(&mut gm::Target) -> Result<Value, gm::CliError>,
) -> Out {
    let mut target = gm::target(t.save, t.live.live, registry().as_deref())?;
    Ok(f(&mut target)?)
}

fn parse<T: std::str::FromStr>(v: Option<String>) -> Result<Option<T>, Fail>
where
    T::Err: std::fmt::Display,
{
    v.map(|s| s.parse::<T>().map_err(|e| Fail::input(e.to_string())))
        .transpose()
}

fn parse_fact(v: Option<String>) -> Result<Option<FactId>, Fail> {
    parse(v)
}

fn history_query(target: TargetArgs, f: HistoryFilter) -> Out {
    // `--since fact.N` pages by fact; a bare number is a tick.
    let (after, since) = match f.since {
        Some(s) if s.starts_with("fact.") => (parse_fact(Some(s))?, None),
        Some(s) => {
            let tick = s
                .parse::<u64>()
                .map_err(|_| Fail::input(format!("--since `{s}` should be fact.N or a tick")))?;
            (None, Some(Tick(tick)))
        }
        None => (None, None),
    };
    let q = HistoryQuery {
        kind: parse(f.kind)?,
        ship: parse(f.ship)?,
        station: parse(f.station)?,
        ware: parse(f.ware)?,
        region: parse(f.region)?,
        since,
        after,
        until: f.until.map(Tick),
        limit: f.limit,
        ..HistoryQuery::default()
    };
    with_target(target, |t| gm::history_query(t, &q))
}

/// Prints each stream item as a JSON line (the one command whose output is a stream).
fn watch(live: LiveArg, since: Option<String>, count: Option<u64>) -> Out {
    let Some(live) = live.live else {
        return Err(Fail::usage("watch streams the running game: pass --live"));
    };
    let since = parse_fact(since)?;
    let mut client = gm::connect(live.as_deref(), registry().as_deref(), None)?;
    gm::watch(&mut client, since, count, |item| println!("{item}"))?;
    std::process::exit(0);
}

/// The game's one composition, headless (gm-examples as the save says, the seeded
/// fake mind), opened with a hook on its script host: for the save-file rollback and
/// the sandbox child.
fn headless(
    packs: Option<PathBuf>,
) -> impl FnOnce(SimState, galaxy_gm::Hook<'_>) -> Result<galaxy_app::game::Game, String> {
    use galaxy_app::game::{Game, GameOptions};
    move |state: SimState, hook: galaxy_gm::Hook<'_>| {
        let packs = galaxy_app::game::packs_dir(packs)
            .ok_or("can't find the packs folder; pass --packs or set GALAXY_PACKS")?;
        let opts = GameOptions::headless(packs, state.world.meta.seed);
        Game::open_with(state, &opts, hook)
    }
}

fn sim_sandbox(
    save: PathBuf,
    packs: Vec<PathBuf>,
    dockings: u32,
    ticks_per_docking: u64,
    packs_dir: Option<PathBuf>,
) -> Out {
    let args = galaxy_gm::sandbox::ChildArgs {
        save,
        packs,
        dockings,
        ticks_per_docking,
    };
    let run = galaxy_gm::sandbox::run_child(&args, headless(packs_dir)).map_err(|e| Fail {
        code: 1,
        kind: "sandbox",
        message: e,
        command: None,
        body: None,
    })?;
    Ok(serde_json::to_value(run).expect("runs serialize"))
}

fn api_dump(name: Option<&str>) -> Out {
    let all = galaxy_contract::schema::all();
    match name {
        None => Ok(Value::Object(
            all.into_iter()
                .map(|(n, s)| (n.to_owned(), s.to_value()))
                .collect(),
        )),
        Some(name) => all
            .into_iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| s.to_value())
            .ok_or_else(|| Fail::input(format!("no schema named `{name}`"))),
    }
}

fn mind_try(packs: Option<PathBuf>) -> Out {
    use galaxy_mind::{MindConfig, MindError};
    let data = packs_dir(packs)?.join("core").join("data");
    let config = MindConfig::load_dir(&data).map_err(|e| Fail::input(e.to_string()))?;
    galaxy_mind::probe::live(&config).map_err(|e| match e {
        MindError::NoKey => Fail::input(e.to_string()),
        e => Fail {
            code: 1,
            kind: "mind",
            message: e.to_string(),
            command: None,
            body: None,
        },
    })
}
