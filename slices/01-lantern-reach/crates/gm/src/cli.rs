//! The CLI's game-master commands, on a save file or on the running game (`--live`).
//! `galaxy` (`crates/app/src/main.rs`) parses arguments and calls these; each returns
//! the one JSON value the command prints, or a [`CliError`] with its exit code.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::{ReportStatus, ValidationReport};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use serde_json::{Value, json};

use crate::bridge::{self, Bridge, BridgeOptions};
use crate::live::{self, CallError, Client, WireError};
use crate::propose::{self, ProposeOptions};
use crate::reads::{self, StateQuery};
use crate::store::GmStore;

/// A failed command: exit code (1 runtime, 2 usage, 4 bad input, 5 rejected), kind,
/// message, and for a rejected pack the report to print instead.
#[derive(Debug)]
pub struct CliError {
    pub code: u8,
    pub kind: String,
    pub message: String,
    pub body: Option<Value>,
}

impl CliError {
    pub fn new(code: u8, kind: &str, message: impl Into<String>) -> Self {
        Self {
            code,
            kind: kind.into(),
            message: message.into(),
            body: None,
        }
    }

    fn rejected(body: Value) -> Self {
        Self {
            code: 5,
            kind: "rejected".into(),
            message: String::new(),
            body: Some(body),
        }
    }
}

impl From<(u8, String)> for CliError {
    fn from((code, message): (u8, String)) -> Self {
        Self::new(
            code,
            if code == 4 { "bad_input" } else { "runtime" },
            message,
        )
    }
}

impl From<WireError> for CliError {
    fn from(e: WireError) -> Self {
        Self {
            code: e.code,
            kind: e.kind,
            message: e.message,
            body: e.body,
        }
    }
}

impl From<CallError> for CliError {
    fn from(e: CallError) -> Self {
        match e {
            CallError::Remote(w) => w.into(),
            CallError::Io(e) => Self::new(1, "live", format!("the live game: {e}")),
        }
    }
}

impl From<galaxy_core::SaveError> for CliError {
    fn from(e: galaxy_core::SaveError) -> Self {
        use galaxy_core::SaveError as E;
        let input = matches!(
            e,
            E::Exists(_) | E::Missing(_) | E::Format { .. } | E::NoSnapshot(_)
        );
        Self::new(
            if input { 4 } else { 1 },
            if input { "bad_input" } else { "save" },
            e.to_string(),
        )
    }
}

type Out = Result<Value, CliError>;

/// Where a command runs.
pub enum Target {
    Save(PathBuf),
    Live(Box<Client>),
}

/// Calls time out after this long on the live socket.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Connects to the running game: the one on `save` if given, else the newest one in
/// the per-user `registry`.
pub fn connect(
    save: Option<&Path>,
    registry: Option<&Path>,
    timeout: Option<Duration>,
) -> Result<Client, CliError> {
    let d = live::find(save, registry).map_err(|e| CliError::new(4, "no_live_game", e))?;
    Client::connect(&d, timeout).map_err(|e| {
        CliError::new(
            4,
            "no_live_game",
            format!("the game on {} doesn't answer: {e}", d.save.display()),
        )
    })
}

/// Picks the target from `--save` and `--live [SAVE]`.
pub fn target(
    save: Option<PathBuf>,
    live: Option<Option<PathBuf>>,
    registry: Option<&Path>,
) -> Result<Target, CliError> {
    match (save, live) {
        (Some(_), Some(_)) => Err(CliError::new(
            2,
            "usage",
            "pass --save PATH or --live, not both",
        )),
        (Some(s), None) => Ok(Target::Save(s)),
        (None, Some(l)) => Ok(Target::Live(Box::new(connect(
            l.as_deref(),
            registry,
            Some(CALL_TIMEOUT),
        )?))),
        (None, None) => Err(CliError::new(
            2,
            "usage",
            "pass --save PATH, or --live for the running game",
        )),
    }
}

fn call(c: &mut Client, op: &str, args: Value) -> Out {
    Ok(c.call(op, args)?)
}

fn load(save: &Path) -> Result<SimState, CliError> {
    Ok(Save::open(save)?.load()?)
}

/// `state get`.
pub fn state_get(t: &mut Target, path: Option<&str>) -> Out {
    match t {
        Target::Save(s) => Ok(reads::state_get(&Save::open(s)?.world_json()?, path)?),
        Target::Live(c) => call(c, "state.get", json!({ "path": path })),
    }
}

/// `state query`.
pub fn state_query(t: &mut Target, q: &StateQuery) -> Out {
    match t {
        Target::Save(s) => Ok(reads::state_query(&Save::open(s)?.world_json()?, q)?),
        Target::Live(c) => call(
            c,
            "state.query",
            serde_json::to_value(q).expect("queries serialize"),
        ),
    }
}

/// `history query`.
pub fn history_query(t: &mut Target, q: &HistoryQuery) -> Out {
    match t {
        Target::Save(s) => {
            let facts = Save::open(s)?.query_history(q)?;
            Ok(json!({ "count": facts.len(), "facts": facts }))
        }
        Target::Live(c) => call(
            c,
            "history.query",
            serde_json::to_value(q).expect("queries serialize"),
        ),
    }
}

/// `gm digest`.
pub fn digest(t: &mut Target, since: Option<FactId>) -> Out {
    match t {
        Target::Save(s) => {
            let state = load(s)?;
            Ok(serde_json::to_value(crate::digest(&state, since)).expect("digests serialize"))
        }
        Target::Live(c) => call(c, "gm.digest", json!({ "since": since })),
    }
}

/// `gm status`.
pub fn status(t: &mut Target) -> Out {
    match t {
        Target::Save(s) => {
            let store = GmStore::for_save(s);
            let gm = store.load().map_err(|e| CliError::new(4, "bad_input", e))?;
            let state = load(s)?;
            let mut v = bridge::status_value(&gm, &store, &state.world, None);
            v["tick"] = json!(state.world.meta.tick);
            v["live"] = json!(false);
            Ok(v)
        }
        Target::Live(c) => call(c, "gm.status", json!({})),
    }
}

/// Refuses a save-file write while a game plays that save (it owns `state.json`).
fn not_running(save: &Path) -> Result<(), CliError> {
    if let Ok(d) = live::find(Some(save), None)
        && live::answers(&d, Duration::from_secs(5))
    {
        return Err(CliError::new(
            4,
            "game_running",
            format!("the game is playing {}; use --live", save.display()),
        ));
    }
    Ok(())
}

fn report_json(r: &ValidationReport) -> Value {
    serde_json::to_value(r).expect("reports serialize")
}

/// `gm propose DIR`: full validation, then stage. On a save the pack applies at the
/// next load; live, at the next calm moment (`wait` waits for that and reports the
/// change ID).
pub fn propose(t: &mut Target, dir: &Path, opts: &ProposeOptions, wait: Option<Duration>) -> Out {
    if !dir.join("manifest.toml").is_file() {
        return Err(CliError::new(
            4,
            "bad_input",
            format!("{} is not a pack folder", dir.display()),
        ));
    }
    match t {
        Target::Save(save) => {
            if !save.is_file() {
                return Err(galaxy_core::SaveError::Missing(save.clone()).into());
            }
            not_running(save)?;
            let store = GmStore::for_save(save);
            let mut gm = store.load().map_err(|e| CliError::new(4, "bad_input", e))?;
            let world = load(save)?.world;
            let running = bridge::running(&gm, &store, &world);
            let mut report = propose::validate(dir, save, &running, opts);
            if !report.accepted() {
                propose::skip_stage(&mut report);
                return Err(CliError::rejected(report_json(&report)));
            }
            let rel = import(&store, dir, &report)?;
            let entry = bridge::stage_entry(&store, &mut gm, &rel, report, "cli", |stage| {
                format!(
                    "staged as {stage}; it applies the next time the save is loaded (galaxy play or galaxy sim run)"
                )
            })?;
            store
                .write(&gm)
                .map_err(|e| CliError::new(1, "runtime", e))?;
            Ok(report_json(&entry.report))
        }
        Target::Live(c) => {
            let hello = c.call("hello", json!({}))?;
            let save = PathBuf::from(hello["save"].as_str().unwrap_or_default());
            let tmp = tempfile::Builder::new()
                .prefix("galaxy-propose-")
                .tempdir()
                .map_err(|e| CliError::new(1, "runtime", e.to_string()))?;
            let base = tmp.path().join("base.sqlite");
            let fork = c.call("gm.fork", json!({ "to": base }))?;
            let running: Vec<(PackId, PathBuf)> = fork["running"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    let pack = r["pack"].as_str()?.parse().ok()?;
                    Some((pack, PathBuf::from(r["dir"].as_str()?)))
                })
                .collect();
            let mut report = propose::validate(dir, &base, &running, opts);
            if !report.accepted() {
                propose::skip_stage(&mut report);
                return Err(CliError::rejected(report_json(&report)));
            }
            // Validating can take minutes: stage over a fresh connection rather than
            // one the game may have closed as idle.
            let discovery = c.discovery.clone();
            **c = Client::connect(&discovery, Some(CALL_TIMEOUT)).map_err(|e| {
                CliError::new(
                    4,
                    "no_live_game",
                    format!("the game stopped answering: {e}"),
                )
            })?;
            let store = GmStore::for_save(&save);
            let rel = import(&store, dir, &report)?;
            let staged = c.call("gm.stage", json!({ "dir": rel, "report": report }))?;
            let mut report: ValidationReport = serde_json::from_value(staged["report"].clone())
                .map_err(|e| CliError::new(1, "live", e.to_string()))?;
            let stage = staged["stage"].as_str().unwrap_or_default().to_owned();
            if let Some(wait) = wait {
                let change = wait_applied(c, &stage, wait)?;
                report.status = ReportStatus::Applied { change };
                if let Some(s) = report.steps.last_mut() {
                    s.diagnostics.push(propose::stage_info(
                        "applied",
                        format!("{stage} applied at a calm moment; roll it back with `galaxy gm rollback {change} --live`"),
                    ));
                }
            }
            Ok(report_json(&report))
        }
    }
}

/// Copies the validated pack into the store, checking it is still what was validated.
fn import(store: &GmStore, dir: &Path, report: &ValidationReport) -> Result<PathBuf, CliError> {
    let pack = galaxy_script::read_pack(dir).map_err(|d| {
        CliError::new(
            4,
            "bad_input",
            d.first()
                .map_or("unreadable pack".into(), |d| d.message.clone()),
        )
    })?;
    if pack.revision != report.revision {
        return Err(CliError::new(
            4,
            "changed",
            "the pack changed while it was being validated; propose it again",
        ));
    }
    store
        .import(&pack)
        .map_err(|e| CliError::new(1, "runtime", e))
}

/// Polls `gm.status` until `stage` has a change ID (or was quarantined).
fn wait_applied(c: &mut Client, stage: &str, wait: Duration) -> Result<CauseId, CliError> {
    let started = Instant::now();
    loop {
        let status = c.call("gm.status", json!({}))?;
        let applied = status["applied"].as_array().into_iter().flatten();
        for a in applied {
            if a["stage"] == stage
                && let Some(change) = a["change"].as_str().and_then(|c| c.parse().ok())
            {
                return Ok(change);
            }
        }
        if let Some(q) = status["quarantined"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|q| q["stage"] == stage)
        {
            return Err(CliError::new(
                5,
                "quarantined",
                format!(
                    "{stage} was quarantined at the calm moment: {}",
                    q["reason"]
                ),
            ));
        }
        if started.elapsed() > wait {
            return Err(CliError::new(
                1,
                "timeout",
                format!(
                    "{stage} is staged but hasn't applied yet (the player isn't docked or paused); see `galaxy gm status --live`"
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `gm rollback CHANGE`. On a save it queues the rollback, runs one hands-off tick so it
/// commits, and writes the save.
pub fn rollback(t: &mut Target, change: &str, build: impl FnOnce(SimState) -> Sim) -> Out {
    match t {
        Target::Save(save) => {
            not_running(save)?;
            let mut file = Save::open(save.as_path())?;
            let mut sim = build(file.load()?);
            let mut b = Bridge::open(save, &mut sim, BridgeOptions::default())
                .map_err(|e| CliError::new(4, "bad_input", e))?;
            let mut out = bridge::rollback_change(&mut sim, b.store(), b.state(), change)?;
            let report = sim.step(&PlayerInput::default());
            file.write(sim.state())?;
            b.after_tick(&sim);
            out["applies"] = json!("applied: one hands-off tick ran and the save was written");
            out["tick"] = json!(sim.tick());
            out["rejected"] = json!(report.map(|r| r.rejected).unwrap_or_default());
            Ok(out)
        }
        Target::Live(c) => call(c, "gm.rollback", json!({ "change": change })),
    }
}

/// `galaxy watch`: prints stream items as JSON lines until the game closes, or after
/// `count` items.
pub fn watch(
    c: &mut Client,
    since: Option<FactId>,
    count: Option<u64>,
    mut out: impl FnMut(&Value),
) -> Out {
    let ack = c.stream("watch", json!({ "since": since }))?;
    out(&json!({ "type": "watching", "ack": ack }));
    let mut n = 0;
    loop {
        if count.is_some_and(|c| n >= c) {
            return Ok(json!({ "items": n }));
        }
        match c.next_item() {
            Ok(item) => {
                out(&item);
                n += 1;
            }
            Err(CallError::Io(_)) => return Ok(json!({ "items": n, "ended": "the game closed" })),
            Err(e) => return Err(e.into()),
        }
    }
}

/// `galaxy shot --live --out P.png`.
pub fn shot_live(c: &mut Client, out: &Path) -> Out {
    let out = std::path::absolute(out).map_err(|e| CliError::new(4, "bad_input", e.to_string()))?;
    call(c, "shot", json!({ "out": out }))
}
