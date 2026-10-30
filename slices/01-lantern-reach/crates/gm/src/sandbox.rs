//! The sandbox validation step: fork the save, fast-forward headless with the pack and
//! without it for a fixed amount of activity, and compare.
//!
//! - **Child side** ([`run_child`], `galaxy sim sandbox`): loads a fork, the packs
//!   that run on it and maybe the candidate, and lets the [`ActivityBot`] fly until it
//!   has docked `dockings` times (or `max_ticks` pass: a dead end). It prints a
//!   [`SandboxRun`]: telemetry at the start, at each docking and at the end, what each
//!   pack wrote and said, aborted and quarantined handlers, rejected batches.
//! - **Parent side** ([`sandbox`]): forks the base save twice into a temp folder and
//!   runs both children at once as separate `galaxy` processes, so a runaway pack can
//!   only hurt a copy, then [`compare`]s them into the report's `sandbox` and
//!   `legibility` steps.
//!
//! Wall-clock time appears only in the parent, as a kill timeout for a child; the runs
//! themselves are measured in dockings and ticks.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitStatus, Stdio};
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use galaxy_contract::pack::{Diagnostic, Severity, StepOutcome, StepReport, ValidationStep};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::{Save, Sim, SimState, System, TickCtx};
use galaxy_economy::EconomyTelemetry;
use galaxy_script::{HostEvent, ScriptHost};
use serde::{Deserialize, Serialize};

use crate::bot::ActivityBot;
use crate::{Composed, Hook};

/// The observer's system name.
pub const OBSERVER: &str = "gm_sandbox_observer";
/// The system the child's own batches are caused by.
pub const SYSTEM_NAME: &str = "gm_sandbox";

/// How much activity a sandbox run covers, and the limits it judges by.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxRules {
    /// Flagship dockings each run flies.
    pub dockings: u32,
    /// Ticks one docking may take before the run counts as a dead end.
    pub ticks_per_docking: u64,
    /// A line's production with the pack may be at most this many times its
    /// production without (or 1 unit for a new line).
    pub max_production_factor: f64,
    /// A line's stock may be at most this many times its target.
    pub max_stock_factor: f64,
    /// Seconds a child may run before it's killed.
    pub child_timeout_secs: u64,
}

impl Default for SandboxRules {
    fn default() -> Self {
        Self {
            dockings: 6,
            ticks_per_docking: 3000,
            max_production_factor: 4.0,
            max_stock_factor: 3.0,
            child_timeout_secs: 300,
        }
    }
}

/// What one pack did in a run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PackTally {
    /// Batches its scripts submitted that committed.
    pub batches: u32,
    /// Commands that change the world (not lines, facts or the pack's own bookkeeping).
    pub writes: u32,
    pub talk: u32,
    pub map_notes: u32,
    pub chronicle: u32,
    pub facts: u32,
    /// Batches the kernel rejected (a runtime invariant or a failed command).
    pub rejected: u32,
}

impl PackTally {
    pub fn lines(&self) -> u32 {
        self.talk + self.map_notes + self.chronicle
    }
}

/// A handler abort or quarantine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HandlerTrouble {
    pub pack: PackId,
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler: Option<String>,
    /// `over_budget`, `runtime_error`, `rejected_write`, ... or `quarantined`.
    pub code: String,
    pub message: String,
    pub tick: Tick,
}

/// The extremes one market line reached, sampled at each docking.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinePeak {
    pub station: StationId,
    pub ware: WareId,
    pub target: u32,
    pub max_stock: u32,
    pub max_ratio: f64,
    pub min_ratio: f64,
    pub max_production: f64,
    /// Pinned at its floor or ceiling at the end of the run.
    pub pinned_at_end: bool,
}

/// One child run's result (`galaxy sim sandbox` prints it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SandboxRun {
    pub save: PathBuf,
    /// The child's process ID (the parent's differs).
    pub pid: u32,
    pub packs: Vec<PackId>,
    pub dockings: u32,
    pub ticks: u64,
    /// Why the bot couldn't finish, if it couldn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stuck: Option<String>,
    /// A pack that didn't load (its fast checks failed against the fork).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_failures: Vec<Diagnostic>,
    pub tallies: BTreeMap<PackId, PackTally>,
    pub trouble: Vec<HandlerTrouble>,
    pub peaks: Vec<LinePeak>,
    pub start: EconomyTelemetry,
    pub end: EconomyTelemetry,
    pub hash: String,
}

/// `galaxy sim sandbox` arguments.
#[derive(Clone, Debug)]
pub struct ChildArgs {
    pub save: PathBuf,
    pub packs: Vec<PathBuf>,
    pub dockings: u32,
    pub ticks_per_docking: u64,
}

#[derive(Default)]
struct Seen {
    /// Tallies by pack for the batches queued this tick.
    pending: Vec<(Cause, PackId, PackTally)>,
    /// Batches queued before the first tick (install data), not counted.
    skip: usize,
}

/// Records what script batches the tick queued (it runs last).
struct Observer {
    seen: Rc<RefCell<Seen>>,
}

fn tally_batch(batch: &CommandBatch) -> Option<(PackId, PackTally)> {
    let CauseKind::Script { pack, .. } = &batch.cause.kind else {
        return None;
    };
    let mut t = PackTally {
        batches: 1,
        ..PackTally::default()
    };
    for c in &batch.commands {
        match c {
            Command::Say { line } => match line.channel {
                Channel::DockTalk => t.talk += 1,
                Channel::MapNote => t.map_notes += 1,
                Channel::Chronicle => t.chronicle += 1,
            },
            Command::RecordFact { .. } | Command::Emit { .. } => t.facts += 1,
            Command::SetScriptValue { .. }
            | Command::SetTriggerFired { .. }
            | Command::SetScriptHealth { .. }
            | Command::SetRevisionRetired { .. }
            | Command::SetPackOff { .. }
            | Command::ExpectDocked { .. }
            | Command::PutTalk { .. } => {}
            _ => t.writes += 1,
        }
    }
    Some((pack.clone(), t))
}

impl System for Observer {
    fn name(&self) -> &'static str {
        OBSERVER
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let mut seen = self.seen.borrow_mut();
        let skip = std::mem::take(&mut seen.skip);
        let found: Vec<_> = ctx
            .queued()
            .iter()
            .skip(skip)
            .filter_map(|b| tally_batch(b).map(|(p, t)| (b.cause.clone(), p, t)))
            .collect();
        seen.pending = found;
    }
}

fn sample(peaks: &mut BTreeMap<(StationId, WareId), LinePeak>, world: &WorldState) {
    let t = EconomyTelemetry::build(world);
    for r in &t.regions {
        for s in &r.stations {
            for l in &s.lines {
                let p = peaks
                    .entry((s.station.clone(), l.ware.clone()))
                    .or_insert_with(|| LinePeak {
                        station: s.station.clone(),
                        ware: l.ware.clone(),
                        target: l.target,
                        max_stock: l.stock,
                        max_ratio: l.price_ratio,
                        min_ratio: l.price_ratio,
                        max_production: l.production,
                        pinned_at_end: false,
                    });
                p.target = l.target;
                p.max_stock = p.max_stock.max(l.stock);
                p.max_ratio = p.max_ratio.max(l.price_ratio);
                p.min_ratio = p.min_ratio.min(l.price_ratio);
                p.max_production = p.max_production.max(l.production);
                p.pinned_at_end = l.at_floor || l.at_ceiling;
            }
        }
    }
}

/// The child: runs one fork with `packs` loaded. `open` builds the game (the app's
/// composition, `core` and `gm-examples` included); `packs` load into its script host
/// after its own, replacing a pack of the same ID, before the host is attached.
pub fn run_child<G: Composed>(
    args: &ChildArgs,
    open: impl FnOnce(SimState, Hook<'_>) -> Result<G, String>,
) -> Result<SandboxRun, String> {
    let state = Save::open(&args.save)
        .and_then(|s| s.load())
        .map_err(|e| e.to_string())?;
    let mut load_failures = Vec::new();
    let mut packs = Vec::new();
    let mut game = open(state, &mut |host: &ScriptHost, sim: &Sim| {
        for dir in &args.packs {
            let report = host.load(dir, sim.world(), sim.history());
            if report.accepted() {
                packs.push(report.pack.clone());
            } else {
                load_failures.extend(report.errors().cloned());
            }
        }
    })?;
    let host = game.scripts().clone();
    // The parent passes only packs that run (or the candidate), so each one is on here
    // as it would be once applied: a candidate for a pack a rollback switched off is
    // the new revision that switches it back on.
    let mut on = CommandBatch::new(Cause::system(SYSTEM_NAME, "the sandbox's packs run"));
    for pack in &packs {
        if galaxy_script::host::pack_off(
            game.sim().world(),
            &game.sim().state().kernel.pending,
            pack,
        ) {
            on.push(Command::SetPackOff {
                pack: pack.clone(),
                off: false,
            });
        }
    }
    if !on.commands.is_empty() {
        game.sim_mut().submit(on);
    }
    // The packs' install batches are queued now; they aren't the packs' activity.
    let seen = Rc::new(RefCell::new(Seen {
        pending: Vec::new(),
        skip: game.sim().state().kernel.pending.len(),
    }));
    let sim = game.sim_mut();
    sim.add_system(Box::new(Observer { seen: seen.clone() }));
    let sub = sim.subscribe("gm-sandbox");

    let start = EconomyTelemetry::build(game.sim().world());
    let mut peaks = BTreeMap::new();
    sample(&mut peaks, game.sim().world());
    let mut tallies: BTreeMap<PackId, PackTally> = packs
        .iter()
        .map(|p| (p.clone(), PackTally::default()))
        .collect();
    let mut trouble = Vec::new();
    let mut stuck = None;
    let mut dockings = 0;
    let from = game.sim().tick();
    match ActivityBot::new(game.sim().world()) {
        None => stuck = Some("no flagship, or no station in its region".to_owned()),
        Some(mut bot) => {
            let mut leg_start = game.sim().tick();
            while bot.dockings < args.dockings {
                if game.sim().tick().0 - leg_start.0 > args.ticks_per_docking {
                    stuck = Some(format!(
                        "no docking at {} within {} ticks (after {} of {} dockings)",
                        bot.target(),
                        args.ticks_per_docking,
                        bot.dockings,
                        args.dockings
                    ));
                    break;
                }
                let input = bot.input(game.sim().world());
                let Some(report) = game.step(&input) else {
                    game.sim_mut().set_paused(false);
                    continue;
                };
                let events = game.sim_mut().drain_events(&sub);
                let docked = bot.dockings;
                bot.observe(game.sim().world(), &events);
                dockings = bot.dockings;
                if bot.dockings != docked {
                    leg_start = game.sim().tick();
                    sample(&mut peaks, game.sim().world());
                }
                let pending = std::mem::take(&mut seen.borrow_mut().pending);
                for (cause, pack, t) in pending {
                    let entry = tallies.entry(pack).or_default();
                    if report.rejected.iter().any(|r| r.cause == cause) {
                        entry.rejected += 1;
                    } else {
                        entry.batches += t.batches;
                        entry.writes += t.writes;
                        entry.talk += t.talk;
                        entry.map_notes += t.map_notes;
                        entry.chronicle += t.chronicle;
                        entry.facts += t.facts;
                    }
                }
            }
        }
    }
    sample(&mut peaks, game.sim().world());
    for e in host.take_events() {
        match e {
            HostEvent::Aborted {
                tick,
                pack,
                script,
                handler,
                diagnostic,
            } => trouble.push(HandlerTrouble {
                pack,
                script,
                handler: Some(handler),
                code: diagnostic.code,
                message: diagnostic.message,
                tick,
            }),
            HostEvent::Quarantined { tick, pack, script } => trouble.push(HandlerTrouble {
                pack,
                script,
                handler: None,
                code: "quarantined".into(),
                message: "quarantined after repeated aborts".into(),
                tick,
            }),
            _ => {}
        }
    }
    Ok(SandboxRun {
        save: args.save.clone(),
        pid: std::process::id(),
        packs,
        dockings,
        ticks: game.sim().tick().0 - from.0,
        stuck,
        load_failures,
        tallies,
        trouble,
        peaks: peaks.into_values().collect(),
        start,
        end: EconomyTelemetry::build(game.sim().world()),
        hash: game.sim().hash(),
    })
}

/// The two runs the sandbox step compares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SandboxPair {
    pub with: SandboxRun,
    pub without: SandboxRun,
    pub parent_pid: u32,
}

/// Runs `galaxy sim sandbox` on two forks of `base`: one with `running` plus the
/// candidate (replacing a running revision of the same pack), one with `running`
/// only. Both children run at once, over the packs folder `packs_root` (`--packs`; without
/// one each child looks for it the way every command does).
pub fn sandbox(
    exe: &Path,
    base: &Path,
    running: &[(PackId, PathBuf)],
    candidate: (&PackId, &Path),
    rules: &SandboxRules,
    packs_root: Option<&Path>,
) -> Result<SandboxPair, String> {
    let tmp = tempfile::Builder::new()
        .prefix("galaxy-sandbox-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let with_save = tmp.path().join("with.sqlite");
    let without_save = tmp.path().join("without.sqlite");
    galaxy_core::fork(base, &with_save).map_err(|e| e.to_string())?;
    galaxy_core::fork(base, &without_save).map_err(|e| e.to_string())?;
    let without_packs: Vec<&Path> = running.iter().map(|(_, d)| d.as_path()).collect();
    let mut with_packs: Vec<&Path> = running
        .iter()
        .filter(|(id, _)| id != candidate.0)
        .map(|(_, d)| d.as_path())
        .collect();
    with_packs.push(candidate.1);

    let timeout = Duration::from_secs(rules.child_timeout_secs);
    let spawn = |save: &Path, packs: &[&Path]| {
        let mut cmd = Process::new(exe);
        cmd.args(["sim", "sandbox", "--save"])
            .arg(save)
            .args(["--dockings", &rules.dockings.to_string()])
            .args(["--ticks-per-docking", &rules.ticks_per_docking.to_string()]);
        if let Some(root) = packs_root {
            cmd.arg("--packs").arg(root);
        }
        for p in packs {
            cmd.arg("--pack").arg(p);
        }
        Child::start(cmd, timeout)
            .map_err(|e| format!("can't start the sandbox ({}): {e}", exe.display()))
    };
    // Declared after `tmp`, so on every early return both children are killed and
    // reaped before their folder is removed.
    let mut with = spawn(&with_save, &with_packs)?;
    let mut without = spawn(&without_save, &without_packs)?;
    while !(with.poll()? & without.poll()?) {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(SandboxPair {
        with: with.finish()?,
        without: without.finish()?,
        parent_pid: std::process::id(),
    })
}

/// A sandbox child process. Its output is drained from the moment it starts (so a
/// chatty child never blocks on a full pipe), its deadline counts from its own spawn,
/// and dropping it before it has exited kills and reaps it.
struct Child {
    child: std::process::Child,
    timeout: Duration,
    deadline: Instant,
    stdout: Option<JoinHandle<String>>,
    stderr: Option<JoinHandle<String>>,
    status: Option<ExitStatus>,
}

fn drain(mut from: impl std::io::Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = from.read_to_string(&mut s);
        s
    })
}

impl Child {
    fn start(mut cmd: Process, timeout: Duration) -> std::io::Result<Child> {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let deadline = Instant::now() + timeout;
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        Ok(Child {
            child,
            timeout,
            deadline,
            stdout,
            stderr,
            status: None,
        })
    }

    /// Has it exited? An error once it has run past its deadline (dropping it then
    /// stops it).
    fn poll(&mut self) -> Result<bool, String> {
        if self.status.is_some() {
            return Ok(true);
        }
        match self.child.try_wait() {
            Ok(Some(s)) => {
                self.status = Some(s);
                Ok(true)
            }
            Ok(None) if Instant::now() > self.deadline => Err(format!(
                "the sandbox run took longer than {}s and was stopped",
                self.timeout.as_secs()
            )),
            Ok(None) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }

    /// The run it printed. Call once [`Child::poll`] says it has exited.
    fn finish(mut self) -> Result<SandboxRun, String> {
        let status = self.status.expect("polled to the end");
        let stdout = self
            .stdout
            .take()
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        let stderr = self
            .stderr
            .take()
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        if !status.success() {
            let tail: String = stderr
                .chars()
                .rev()
                .take(2000)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            return Err(format!(
                "the sandbox run failed ({status}): {}{}",
                stdout.trim(),
                if tail.trim().is_empty() {
                    String::new()
                } else {
                    format!(" / {}", tail.trim())
                }
            ));
        }
        serde_json::from_str(&stdout)
            .map_err(|e| format!("the sandbox printed something unexpected: {e}"))
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The report code for a handler trouble code.
fn trouble_code(code: &str) -> &'static str {
    match code {
        "quarantined" => "quarantined",
        "over_budget" => "over_budget",
        _ => "handler_aborted",
    }
}

fn diag(severity: Severity, code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        severity,
        code: code.into(),
        message: message.into(),
        location: None,
    }
}

fn step(step: ValidationStep, diagnostics: Vec<Diagnostic>) -> StepReport {
    let failed = diagnostics.iter().any(|d| d.severity == Severity::Error);
    StepReport {
        step,
        outcome: if failed {
            StepOutcome::Failed
        } else {
            StepOutcome::Passed
        },
        diagnostics,
    }
}

/// The `sandbox` and `legibility` steps from a pair of runs of `candidate`.
pub fn compare(
    pair: &SandboxPair,
    candidate: &PackId,
    rules: &SandboxRules,
) -> (StepReport, StepReport) {
    use Severity::{Error, Info, Warning};
    let (with, without) = (&pair.with, &pair.without);
    let mut d = Vec::new();
    d.push(diag(
        Info,
        "sandbox_run",
        format!(
            "forked the save and flew the activity bot for {} dockings with the pack ({} ticks) and without it ({} ticks), each in its own process",
            rules.dockings, with.ticks, without.ticks
        ),
    ));
    for f in &with.load_failures {
        d.push(diag(
            Error,
            "sandbox_load_failed",
            format!("a pack didn't load on the fork: {}", f.message),
        ));
    }
    if !with.packs.contains(candidate) && with.load_failures.is_empty() {
        d.push(diag(
            Error,
            "sandbox_load_failed",
            "the pack didn't load on the fork",
        ));
    }
    match (&with.stuck, &without.stuck) {
        (Some(why), None) => d.push(diag(
            Error,
            "dead_end",
            format!("with the pack the activity loop can't go on: {why}"),
        )),
        (Some(why), Some(_)) => d.push(diag(
            Warning,
            "bot_stuck",
            format!("the activity bot got stuck with and without the pack: {why}"),
        )),
        _ => {}
    }
    // The candidate's own trouble always counts. Another running pack's counts where it
    // is worse with the candidate than without: the candidate broke it.
    for t in with.trouble.iter().filter(|t| &t.pack == candidate) {
        d.push(diag(
            Error,
            trouble_code(&t.code),
            format!(
                "{}{} at {}: {} ({})",
                t.script,
                t.handler
                    .as_ref()
                    .map(|h| format!(" {h}"))
                    .unwrap_or_default(),
                t.tick,
                t.message,
                t.code
            ),
        ));
    }
    let others = |run: &SandboxRun| {
        let mut n: BTreeMap<(PackId, String, String), (u32, String)> = BTreeMap::new();
        for t in run.trouble.iter().filter(|t| &t.pack != candidate) {
            let e = n
                .entry((t.pack.clone(), t.script.clone(), t.code.clone()))
                .or_insert((0, t.message.clone()));
            e.0 += 1;
        }
        n
    };
    let baseline = others(without);
    for ((pack, script, code), (n, message)) in others(with) {
        let before = baseline
            .get(&(pack.clone(), script.clone(), code.clone()))
            .map_or(0, |b| b.0);
        if n > before {
            d.push(diag(
                Error,
                trouble_code(&code),
                format!(
                    "the pack breaks {pack}, which already runs: {script} had {n} `{code}` with the pack and {before} without (first: {message})"
                ),
            ));
        }
    }
    let tally = with.tallies.get(candidate).cloned().unwrap_or_default();
    if tally.rejected > 0 {
        d.push(diag(
            Error,
            "invariant_hit",
            format!(
                "{} of the pack's batches were rejected by the kernel (a runtime invariant or a failed command)",
                tally.rejected
            ),
        ));
    }
    for (pack, t) in with.tallies.iter().filter(|(p, _)| *p != candidate) {
        let before = without.tallies.get(pack).map_or(0, |b| b.rejected);
        if t.rejected > before {
            d.push(diag(
                Error,
                "invariant_hit",
                format!(
                    "the pack breaks {pack}, which already runs: the kernel rejected {} of its batches with the pack and {before} without",
                    t.rejected
                ),
            ));
        }
    }
    let base: BTreeMap<(&StationId, &WareId), &LinePeak> = without
        .peaks
        .iter()
        .map(|p| ((&p.station, &p.ware), p))
        .collect();
    for p in &with.peaks {
        let b = base.get(&(&p.station, &p.ware));
        let where_ = format!("{} at {}", p.ware, p.station);
        if p.pinned_at_end && !b.is_some_and(|b| b.pinned_at_end) {
            d.push(diag(
                Error,
                "price_out_of_band",
                format!(
                    "{where_} ends pinned at the edge of its price band (ratio {:.2} to {:.2})",
                    p.min_ratio, p.max_ratio
                ),
            ));
        }
        let allowed = b.map_or(1.0, |b| b.max_production.max(1.0)) * rules.max_production_factor;
        if p.max_production > allowed {
            d.push(diag(
                Error,
                "runaway_producer",
                format!(
                    "{where_} produces {:.1} per pulse with the pack, {} without",
                    p.max_production,
                    b.map_or("nothing".to_owned(), |b| format!("{:.1}", b.max_production))
                ),
            ));
        }
        let stock_cap = f64::from(p.target.max(1)) * rules.max_stock_factor;
        if f64::from(p.max_stock) > stock_cap
            && !b.is_some_and(|b| f64::from(b.max_stock) > stock_cap)
        {
            d.push(diag(
                Error,
                "runaway_stock",
                format!(
                    "{where_} reached {} units against a target of {}",
                    p.max_stock, p.target
                ),
            ));
        }
        if let Some(b) = b {
            let shift =
                (p.max_ratio / b.max_ratio.max(0.01)).max(b.min_ratio / p.min_ratio.max(0.01));
            if shift > 1.5 {
                d.push(diag(
                    Warning,
                    "price_shift",
                    format!(
                        "{where_} prices moved from {:.2}..{:.2} to {:.2}..{:.2} of base",
                        b.min_ratio, b.max_ratio, p.min_ratio, p.max_ratio
                    ),
                ));
            }
        }
    }
    let sandbox = step(ValidationStep::Sandbox, d);

    let mut l = Vec::new();
    if tally.writes > 0 && tally.lines() == 0 {
        l.push(diag(
            Error,
            "silent_change",
            format!(
                "the pack changed the world {} times in the sandbox and never said so: no dock talk, map note or chronicle line fired",
                tally.writes
            ),
        ));
    } else if tally.batches == 0 {
        l.push(diag(
            Warning,
            "nothing_fired",
            format!(
                "the pack did nothing in {} dockings of the activity loop, so its lines couldn't be seen to fire",
                rules.dockings
            ),
        ));
    } else {
        l.push(diag(
            Info,
            "lines_fired",
            format!(
                "{} writes, {} dock talk, {} map notes, {} chronicle lines in {} dockings",
                tally.writes, tally.talk, tally.map_notes, tally.chronicle, rules.dockings
            ),
        ));
    }
    (sandbox, step(ValidationStep::Legibility, l))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANDIDATE: PackId = PackId::from_static("gm.candidate");
    const DOCK: PackId = PackId::from_static("gm.dock");

    fn run(trouble: Vec<HandlerTrouble>, rejected: u32) -> SandboxRun {
        let telemetry = EconomyTelemetry {
            tick: Tick(0),
            regions: Vec::new(),
        };
        SandboxRun {
            save: PathBuf::new(),
            pid: 0,
            packs: vec![DOCK, CANDIDATE],
            dockings: 6,
            ticks: 600,
            stuck: None,
            load_failures: Vec::new(),
            tallies: [
                (
                    DOCK,
                    PackTally {
                        rejected,
                        ..PackTally::default()
                    },
                ),
                (CANDIDATE, PackTally::default()),
            ]
            .into_iter()
            .collect(),
            trouble,
            peaks: Vec::new(),
            start: telemetry.clone(),
            end: telemetry,
            hash: String::new(),
        }
    }

    fn spins(n: usize) -> Vec<HandlerTrouble> {
        (0..n)
            .map(|i| HandlerTrouble {
                pack: DOCK,
                script: "dock".into(),
                handler: Some("on.dock".into()),
                code: "over_budget".into(),
                message: "ran over its step budget".into(),
                tick: Tick(i as u64),
            })
            .collect()
    }

    fn errors(pair: &SandboxPair) -> Vec<String> {
        let (s, _) = compare(pair, &CANDIDATE, &SandboxRules::default());
        s.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| format!("{}: {}", d.code, d.message))
            .collect()
    }

    #[test]
    fn failures_the_candidate_causes_in_other_packs_reject_it() {
        let pair = |with, without| SandboxPair {
            with,
            without,
            parent_pid: 0,
        };
        // Healthy without the candidate; the existing dock pack spins with it.
        let broke = errors(&pair(run(spins(3), 0), run(Vec::new(), 0)));
        assert_eq!(broke.len(), 1, "{broke:?}");
        assert!(broke[0].starts_with("over_budget: the pack breaks gm.dock"));
        // Rejected batches of another pack, likewise.
        let rejected = errors(&pair(run(Vec::new(), 2), run(Vec::new(), 0)));
        assert_eq!(rejected.len(), 1, "{rejected:?}");
        assert!(rejected[0].starts_with("invariant_hit: the pack breaks gm.dock"));
        // Trouble the baseline has too isn't the candidate's doing.
        assert!(errors(&pair(run(spins(2), 1), run(spins(2), 1))).is_empty());
        // Worse than the baseline is.
        assert_eq!(errors(&pair(run(spins(3), 1), run(spins(2), 1))).len(), 1);
    }

    fn long_running() -> Process {
        if cfg!(windows) {
            let mut c = Process::new("ping");
            c.args(["-n", "60", "127.0.0.1"]);
            c
        } else {
            let mut c = Process::new("sleep");
            c.arg("60");
            c
        }
    }

    #[test]
    fn a_child_is_killed_when_dropped_or_late() {
        // Dropped while running (a sibling failed to start, say): killed and reaped, so
        // its output pipe closes at once instead of in a minute.
        let mut child = Child::start(long_running(), Duration::from_secs(60)).unwrap();
        assert!(!child.poll().unwrap());
        let out = child.stdout.take().unwrap();
        let started = Instant::now();
        drop(child);
        out.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));

        // Past its deadline: an error, and dropping it stops it.
        let mut late = Child::start(long_running(), Duration::ZERO).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let err = late.poll().unwrap_err();
        assert!(err.contains("took longer"), "{err}");
        let out = late.stdout.take().unwrap();
        drop(late);
        out.join().unwrap();
    }
}
