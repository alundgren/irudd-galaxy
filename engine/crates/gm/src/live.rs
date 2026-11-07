//! `galaxy-gm live ...`: reads a running `galaxy play` and hot-loads modules into it
//! through the live socket (`galaxy_live`). Each command finds the running games, chooses
//! one, sends one request and prints its reply with the game it went to; `live input`
//! ([`input`]) holds keys over one connection for as long as its steps run; `live edit`
//! ([`edits`]) edits the running world.

mod input;

use std::path::{Path, PathBuf};
use std::time::Duration;

use galaxy_live::client::{self, Game};
use galaxy_live::{Op, WireError};
use galaxy_module::cli::{self, Output};
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover};
use galaxy_motion::Opts;
use galaxy_sim::edit::Edit;
use serde_json::{Value, json};

/// Options each command takes besides `--game`.
pub(crate) const STATUS_OPTIONS: &[&str] = &["game"];
pub(crate) const STATE_OPTIONS: &[&str] = &["game", "entity", "component", "all"];
pub(crate) const HISTORY_OPTIONS: &[&str] = &["game", "last", "since", "kind", "inputs"];
pub(crate) const WATCH_OPTIONS: &[&str] =
    &["game", "entity", "component", "field", "ticks", "every"];
pub(crate) const SHOT_OPTIONS: &[&str] = &["game", "out"];
pub(crate) const LOAD_OPTIONS: &[&str] = &["game"];
pub(crate) const INPUT_OPTIONS: &[&str] = &["game", "follow"];
pub(crate) const EDIT_OPTIONS: &[&str] = &["game", "file"];
pub(crate) const DEBUG_OPTIONS: &[&str] = &["game"];

/// Ticks a watch covers when not told: one second.
const WATCH_TICKS: u64 = 30;

/// Runs `live <what>`. `usage` makes a usage failure for this command.
pub(crate) fn run(
    what: &str,
    positional: &[String],
    opts: &Opts,
    usage: impl Fn(String) -> Failure,
) -> Result<Output, Failure> {
    let known = match what {
        "status" => STATUS_OPTIONS,
        "state" => STATE_OPTIONS,
        "history" => HISTORY_OPTIONS,
        "watch" => WATCH_OPTIONS,
        "shot" => SHOT_OPTIONS,
        "load" => LOAD_OPTIONS,
        "input" => INPUT_OPTIONS,
        "edit" => EDIT_OPTIONS,
        "debug" => DEBUG_OPTIONS,
        other => unreachable!("galaxy-gm has help for `live {other}` but does not run it"),
    };
    if let Some(name) = opts.names().find(|n| *n != "game" && !known.contains(n)) {
        let list: Vec<String> = known.iter().map(|k| format!("--{k}")).collect();
        return Err(usage(format!(
            "live {what} has no option --{name}; it takes {}",
            if list.is_empty() {
                "none".to_owned()
            } else {
                list.join(", ")
            }
        )));
    }
    if !matches!(what, "load" | "input" | "edit" | "debug") && !positional.is_empty() {
        return Err(usage(format!(
            "live {what} takes options only, not `{}`",
            positional.join(" ")
        )));
    }
    let cwd = cli::current_dir()?;
    if what == "input" {
        return input::run(positional, opts, &cwd, usage);
    }
    let games = client::games(&galaxy_live::live_dir());
    let text = |name: &str| opts.text(name).map(str::to_owned);
    let count = |name: &str| opts.count(name).map_err(&usage);
    let mut needs: Vec<String> = Vec::new();
    let (op, wait) = match what {
        "status" => (Op::Status {}, 10),
        "state" => (
            Op::State {
                entity: text("entity"),
                component: text("component"),
                all: opts.flag("all"),
            },
            10,
        ),
        "history" => (
            Op::History {
                last: count("last")?,
                since: count("since")?.map(|t| t as u64),
                kind: text("kind"),
                inputs: opts.flag("inputs"),
            },
            10,
        ),
        "watch" => {
            let (Some(entity), Some(component)) = (text("entity"), text("component")) else {
                return Err(usage(
                    "live watch needs --entity <id> and --component <name>".into(),
                ));
            };
            let ticks = count("ticks")?.map_or(WATCH_TICKS, |t| t as u64);
            let every = count("every")?.map_or(1, |t| t as u64);
            (
                Op::Watch {
                    entity,
                    component,
                    field: text("field"),
                    ticks,
                    every,
                },
                // Twice real time, and some: the window may drop frames.
                ticks / 15 + 15,
            )
        }
        "shot" => (
            Op::Shot {
                out: text("out").map(|o| discover::resolve(&cwd, Path::new(&o))),
            },
            30,
        ),
        "edit" => {
            let file = text("file").map(|f| discover::resolve(&cwd, Path::new(&f)));
            (
                Op::Edit {
                    edits: edits(positional, file.as_deref()).map_err(|e| match e {
                        LoadArg::Usage(m) => usage(m),
                        LoadArg::Failure(f) => f,
                    })?,
                },
                10,
            )
        }
        "debug" => {
            let Some((event, pairs)) = positional.split_first() else {
                return Err(usage(
                    "live debug needs a debug event, such as \"live debug <event> <field>=<value>\""
                        .into(),
                ));
            };
            let payload = galaxy_sim::edit::read_pairs(pairs).map_err(&usage)?;
            (
                Op::Debug {
                    event: event.clone(),
                    payload,
                },
                10,
            )
        }
        _ => {
            let dirs = load_dirs(&cwd, positional).map_err(|e| match e {
                LoadArg::Usage(m) => usage(m),
                LoadArg::Failure(f) => f,
            })?;
            for d in &dirs {
                needs.push(Module::read(d)?.id().to_owned());
            }
            (Op::Load { dirs }, 60)
        }
    };
    if let Err(e) = op.check() {
        return Err(usage(e));
    }
    let (game, why) =
        client::choose(games, text("game").as_deref(), &needs, &cwd).map_err(failure)?;
    let result = client::call(&game.discovery, &op, Duration::from_secs(wait)).map_err(|e| {
        let mut f = failure(e);
        f.fields.insert("game".into(), game_json(&game, why));
        f
    })?;
    let mut out = json!({ "ok": true, "game": game_json(&game, why) });
    if let (Value::Object(o), Value::Object(r)) = (&mut out, result) {
        o.extend(r);
    }
    Ok(Output::Json(out))
}

pub(crate) fn game_json(game: &Game, why: &str) -> Value {
    json!({
        "pid": game.discovery.pid,
        "session": game.discovery.session,
        "build": game.discovery.build,
        "chosen": why,
    })
}

/// The edits `live edit` makes: its arguments, each one edit or several joined with `;`,
/// or the `[[edit]]` entries of `--file`, which take no tick: a live edit is made now.
fn edits(positional: &[String], file: Option<&Path>) -> Result<Vec<Edit>, LoadArg> {
    match (positional.is_empty(), file) {
        (false, Some(_)) => Err(LoadArg::Usage(
            "live edit takes edits as arguments or --file <edits.toml>, not both".into(),
        )),
        (true, None) => Err(LoadArg::Usage(
            "live edit needs an edit, such as \"set <entity> <component> <field>=<value>\", \
             or --file <edits.toml>"
                .into(),
        )),
        (false, None) => positional
            .iter()
            .flat_map(|a| a.split(';'))
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| Edit::parse_text(l).map_err(LoadArg::Usage))
            .collect(),
        (true, Some(path)) => {
            let bad = |e: String| {
                LoadArg::Failure(
                    Failure::bad_input(format!("{}: {e}", path.display()))
                        .with("path", path.display().to_string()),
                )
            };
            let text = std::fs::read_to_string(path).map_err(|e| bad(e.to_string()))?;
            let table: toml::Table = toml::from_str(&text).map_err(|e| bad(e.to_string()))?;
            if let Some(other) = table.keys().find(|k| *k != "edit") {
                return Err(bad(format!(
                    "`{other}`: an edits file holds [[edit]] entries only"
                )));
            }
            let list = table
                .get("edit")
                .and_then(toml::Value::as_array)
                .ok_or_else(|| bad("no [[edit]] entries".into()))?;
            list.iter()
                .map(|v| {
                    let t = v
                        .as_table()
                        .ok_or_else(|| bad("an [[edit]] is a table".into()))?;
                    if t.contains_key("tick") {
                        return Err(bad(
                            "a live edit is made now, between the next two ticks; leave out tick"
                                .into(),
                        ));
                    }
                    Edit::from_toml(t).map(|(_, e)| e).map_err(bad)
                })
                .collect()
        }
    }
}

enum LoadArg {
    Usage(String),
    Failure(Failure),
}

/// The module folders `live load [<module>]` names: none (the game's own), a module, or
/// every module in a folder of modules.
fn load_dirs(cwd: &Path, positional: &[String]) -> Result<Vec<PathBuf>, LoadArg> {
    match positional {
        [] => Ok(Vec::new()),
        [one] => {
            let dir = discover::resolve(cwd, Path::new(one));
            if !dir.is_dir() {
                return Err(LoadArg::Failure(
                    Failure::bad_input(format!("{} is not a folder", dir.display()))
                        .with("path", dir.display().to_string()),
                ));
            }
            let modules = discover::modules_in(&dir);
            if modules.is_empty() {
                return Err(LoadArg::Failure(
                    Failure::bad_input(format!(
                        "no modules in {}: a module is a folder with a manifest.toml",
                        dir.display()
                    ))
                    .with("path", dir.display().to_string()),
                ));
            }
            Ok(modules)
        }
        _ => Err(LoadArg::Usage(
            "live load takes at most one module folder (or a folder of modules)".into(),
        )),
    }
}

/// A wire error as this command's failure.
pub(crate) fn failure(e: WireError) -> Failure {
    let kind = match e.kind.as_str() {
        "usage" => "usage",
        "bad_input" => "bad_input",
        "engine_version" => "engine_version",
        "no_game" => "no_game",
        "several_games" => "several_games",
        "live_protocol" => "live_protocol",
        "hot_load_refused" => "hot_load_refused",
        _ => "runtime",
    };
    let mut f = Failure::new(e.code, kind, e.message);
    f.fields = e.fields;
    f
}
