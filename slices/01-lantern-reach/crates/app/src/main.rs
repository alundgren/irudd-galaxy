//! `galaxy`: the game and its JSON CLI.
//!
//! Every command prints one JSON value on stdout. Exit codes:
//! 0 ok, 1 runtime failure (I/O, SQLite), 2 usage error, 3 not implemented yet,
//! 4 bad input (missing or existing save, invalid data, unknown path or ID).
//!
//! Keep this crate thin: parse arguments, call library code, print JSON.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, SaveError};
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
    /// Play in a window (not implemented yet).
    Play(Rest),
    /// Jump to a journey beat (not implemented yet).
    Beat {
        #[command(subcommand)]
        command: BeatCmd,
    },
    /// Validate a pack (not implemented yet).
    Pack {
        #[command(subcommand)]
        command: PackCmd,
    },
    /// Game-master bridge (not implemented yet).
    Gm {
        #[command(subcommand)]
        command: GmCmd,
    },
    /// Render a screenshot headless (not implemented yet).
    Shot(Rest),
    /// The fast-model layer on OpenRouter.
    Mind {
        #[command(subcommand)]
        command: MindCmd,
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
    },
}

#[derive(Subcommand)]
enum StateCmd {
    /// Print the world state, or part of it.
    Get {
        #[arg(long)]
        save: PathBuf,
        /// Slash-separated path into the world, e.g. `stations` or
        /// `stations/station.kettle_dock/build`.
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
enum HistoryCmd {
    /// Facts matching filters, oldest first.
    Query {
        #[arg(long)]
        save: PathBuf,
        #[command(flatten)]
        filter: HistoryFilter,
    },
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
    /// Facts at or after this tick.
    #[arg(long)]
    since: Option<u64>,
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
    Check(Rest),
}

#[derive(Subcommand)]
enum GmCmd {
    Propose(Rest),
    Rollback(Rest),
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
}

impl Fail {
    fn input(message: impl Into<String>) -> Self {
        Fail {
            code: 4,
            kind: "bad_input",
            message: message.into(),
            command: None,
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
        }
    }
}

type Out = Result<Value, Fail>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::New { save, seed, packs } => new(&save, seed, packs.packs),
        Cmd::Sim {
            command:
                SimCmd::Run {
                    save,
                    ticks,
                    dry_run,
                },
        } => sim_run(&save, ticks, dry_run),
        Cmd::State {
            command: StateCmd::Get { save, path },
        } => state_get(&save, path.as_deref()),
        Cmd::History {
            command: HistoryCmd::Query { save, filter },
        } => history_query(&save, filter),
        Cmd::Api {
            command: ApiCmd::Dump { name },
        } => api_dump(name.as_deref()),
        Cmd::Play(_) => not_implemented("play"),
        Cmd::Beat {
            command: BeatCmd::Jump(_),
        } => not_implemented("beat jump"),
        Cmd::Pack {
            command: PackCmd::Check(_),
        } => not_implemented("pack check"),
        Cmd::Gm {
            command: GmCmd::Propose(_),
        } => not_implemented("gm propose"),
        Cmd::Gm {
            command: GmCmd::Rollback(_),
        } => not_implemented("gm rollback"),
        Cmd::Shot(_) => not_implemented("shot"),
        Cmd::Mind {
            command: MindCmd::Try { packs },
        } => mind_try(packs.packs),
    };
    match result {
        Ok(value) => {
            print_json(&value);
            ExitCode::SUCCESS
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

fn not_implemented(command: &str) -> Out {
    Err(Fail {
        code: 3,
        kind: "not_implemented",
        message: "not implemented yet".into(),
        command: Some(command.to_owned()),
    })
}

/// Finds the packs folder: flag, `GALAXY_PACKS`, or the nearest `packs/core` above
/// the working directory or the executable.
fn packs_dir(flag: Option<PathBuf>) -> Result<PathBuf, Fail> {
    if let Some(p) = flag.or_else(|| std::env::var_os("GALAXY_PACKS").map(PathBuf::from)) {
        return Ok(p);
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
                return Ok(packs);
            }
        }
    }
    Err(Fail::input(
        "can't find the packs folder; pass --packs or set GALAXY_PACKS",
    ))
}

fn new(save: &Path, seed: u64, packs: Option<PathBuf>) -> Out {
    if save.exists() {
        return Err(SaveError::Exists(save.to_path_buf()).into());
    }
    let data = packs_dir(packs)?.join("core").join("data");
    let state = galaxy_core::new_game(&data, seed).map_err(|e| Fail::input(e.to_string()))?;
    let hash = galaxy_core::state_hash(&state);
    Save::create(save, &state)?;
    Ok(json!({
        "save": save,
        "seed": seed,
        "tick": state.world.meta.tick,
        "hash": hash,
        "stations": state.world.stations.len(),
        "rocks": state.world.rocks.len(),
    }))
}

/// The game's systems, in tick order: the kernel's, the ship's (buy and sell orders go
/// through the economy's desk), then the economy's (pulses, scouting, re-pricing and
/// the player's dock-screen trades), which reads the dockings the ships made.
fn game(state: galaxy_core::SimState) -> galaxy_core::Sim {
    galaxy_core::game(state)
        .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
        .with_systems([Box::new(galaxy_economy::EconomySystem) as Box<dyn galaxy_core::System>])
}

fn sim_run(path: &Path, ticks: u64, dry_run: bool) -> Out {
    let mut save = Save::open(path)?;
    let mut sim = game(save.load()?);
    let start = sim.tick();
    let input = PlayerInput::default();
    let (mut applied, mut events) = (0usize, 0usize);
    let mut rejected = Vec::new();
    for _ in 0..ticks {
        let Some(report) = sim.step(&input) else {
            break;
        };
        applied += report.applied;
        events += report.events;
        rejected.extend(report.rejected);
    }
    if !dry_run {
        save.write(sim.state())?;
    }
    Ok(json!({
        "from_tick": start,
        "tick": sim.tick(),
        "ticks_run": sim.tick().0 - start.0,
        "hash": sim.hash(),
        "batches_applied": applied,
        "events": events,
        "rejected_count": rejected.len(),
        "rejected": rejected.iter().take(20).collect::<Vec<_>>(),
        "saved": !dry_run,
    }))
}

fn state_get(path: &Path, sub: Option<&str>) -> Out {
    let world = Save::open(path)?.world_json()?;
    let Some(sub) = sub.filter(|s| !s.is_empty() && *s != "/") else {
        return Ok(world);
    };
    let pointer: String = if sub.starts_with('/') {
        sub.to_owned()
    } else {
        sub.split('/')
            .map(|seg| format!("/{}", seg.replace('~', "~0")))
            .collect()
    };
    world
        .pointer(&pointer)
        .cloned()
        .ok_or_else(|| Fail::input(format!("no `{sub}` in the world state")))
}

fn parse<T: std::str::FromStr>(v: Option<String>) -> Result<Option<T>, Fail>
where
    T::Err: std::fmt::Display,
{
    v.map(|s| s.parse::<T>().map_err(|e| Fail::input(e.to_string())))
        .transpose()
}

fn history_query(path: &Path, f: HistoryFilter) -> Out {
    let q = HistoryQuery {
        kind: parse(f.kind)?,
        ship: parse(f.ship)?,
        station: parse(f.station)?,
        ware: parse(f.ware)?,
        region: parse(f.region)?,
        since: f.since.map(Tick),
        until: f.until.map(Tick),
        limit: f.limit,
        ..HistoryQuery::default()
    };
    let facts = Save::open(path)?.query_history(&q)?;
    Ok(json!({ "count": facts.len(), "facts": facts }))
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
        },
    })
}
