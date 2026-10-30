//! The script host: loaded packs, the per-tick [`System`], dev hot reload.
//!
//! Each tick, for every loaded pack (in pack ID order) whose revision isn't retired
//! (or being retired by a rollback queued for this tick), in a pack a rollback hasn't
//! switched off ([`pack_off`]), and every script (in manifest order) that isn't
//! quarantined:
//!
//! 1. each event committed last tick with an `on.<event>` handler runs it;
//! 2. each `G.after` trigger whose activity counters are due runs once;
//! 3. each beat it registers starts, advances or completes if its requirements hold.
//!    `G.test` predicates and the transition's callback each run in a fresh state.
//!
//! A successful call's writes (or a beat transition with its callback's) are one batch
//! (`Cause::Script`). The host first checks that the pack declares every capability
//! the batch's commands need ([`galaxy_contract::needs`]): a command it may not cause
//! is a fault, whichever route wrote it (a hook, a beat's rewards, a trigger). That is
//! defence in depth on top of the per-hook gating of `ctx`, and it covers the routes
//! that aren't one hook. Pack data installs ([`install_batches`]) answer to the same
//! table. A beat transition is checked in two parts: the beat runtime's own commands
//! (`Route::Beat`: the beat's state, its events, its rewards) and its callback's
//! writes (`Route::Hooks`), so a callback can't borrow the runtime's right to write
//! journey events. A game-master layer has no beat route at all.
//!
//! Each pack's layer (core or game master) comes from the path that loads it, never
//! from its ID or from it being new: [`ScriptHost::load`] is the core path (the
//! authored backbone), [`ScriptHost::load_layer`] the game-master one (`gm-examples`,
//! the bridge, the sandbox), and a reload keeps the folder's layer even if its manifest
//! names a new ID. A game-master layer never takes `core`'s namespace or a core pack's
//! ID. The layer is recorded in the world's pack state the first time the pack loads or
//! is applied ([`ScriptHost::record_layers`]), and every later load keeps it: a pack
//! the world or the host knows as a game-master layer is checked as one
//! ([`CheckOptions::game_master`]) and run as one, whichever path loads it (a reload,
//! the dev watcher, `play`, `sim run`, the bridge, the sandbox).
//!
//! Then it checks the batch the way the kernel will
//! ([`TickCtx::preview`]): against the world the kernel will apply it to (the batches
//! queued ahead of it this tick, then the script batches before it) and with every
//! invariant of the sim. If it would be rejected, nothing is submitted, and the same
//! check on the world the call read decides whose that is:
//!
//! - **Fault**: the batch is invalid there too. Like a Luau error or running over a
//!   quota, the call is aborted and counts against the script; after
//!   `limits.quarantine_after` aborts the script is quarantined (for that revision,
//!   for good: loading the same revision again doesn't clear it) with a
//!   `script_quarantined` history fact.
//! - **Contention**: the batch is valid there and only loses to a change made earlier
//!   in the tick (an undock, moved cargo, a compare-and-set or `Expect*` check). It
//!   never counts. A beat or trigger is simply due again next tick. An event handler is
//!   queued in the script's saved health ([`ScriptHealth::retries`]) and runs again
//!   for the same event next tick, before that tick's events, up to
//!   `limits.contention_retries` times; then it is dropped with a
//!   `script_contention_dropped` fact.
//!
//! Later calls in the same tick read the tick-start world plus the script writes
//! before them, so two events in one tick don't grow a station twice.
//!
//! The Luau states hold no world state, so the host itself is safe to rebuild at any
//! time: a reload swaps a pack's state between ticks and loses nothing.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use galaxy_contract::history::FactDraft;
use galaxy_contract::needs::{Caller, Route};
use galaxy_contract::pack::{
    Diagnostic, HandlerRetry, Location, PackLayer, ReportStatus, RollbackLine, ScriptHealth,
    StepOutcome, StepReport, ValidationReport, ValidationStep,
};
use galaxy_contract::prelude::*;
use galaxy_core::apply::{Staged, apply};
use galaxy_core::{History, Sim, System, TickCtx};
use serde::Serialize;

use crate::check::{CheckOptions, check_dir};
use crate::pack::Pack;
use crate::vm::{Abort, BeatCall, LoadedBeat, PackVm, Target, View, clip_text};
use crate::watch::PackWatcher;
use galaxy_progress::{Eval, EvalError, beats};

/// The name the host runs under (and its RNG streams are keyed by).
pub const SYSTEM_NAME: &str = "script_host";
/// Fact kind recorded when a script is quarantined.
pub const QUARANTINED: FactKind = FactKind::from_static("script_quarantined");
/// Fact kind recorded when an event handler that kept losing to contention is dropped.
pub const CONTENTION_DROPPED: FactKind = FactKind::from_static("script_contention_dropped");
const EVENT_CAP: usize = 500;

/// Whether `pack` is switched off (`PackState::off`) on `world` once the `queued`
/// batches apply (`SimState::kernel.pending`, or `TickCtx::queued`): a rollback of one
/// of its changes switches it off, and only the game master applying a new revision
/// switches it back on. Every place that loads or runs a pack checks this first.
pub fn pack_off(world: &WorldState, queued: &[CommandBatch], pack: &PackId) -> bool {
    let mut off = world.packs.get(pack).is_some_and(|p| p.off);
    for c in queued.iter().flat_map(|b| &b.commands) {
        if let Command::SetPackOff { pack: p, off: o } = c
            && p == pack
        {
            off = *o;
        }
    }
    off
}

/// The layer the world records `pack` as (`PackState::layer`), counting the batches
/// still `queued`.
pub fn recorded_layer(
    world: &WorldState,
    queued: &[CommandBatch],
    pack: &PackId,
) -> Option<PackLayer> {
    let mut layer = world.packs.get(pack).and_then(|p| p.layer);
    for c in queued.iter().flat_map(|b| &b.commands) {
        if let Command::SetPackLayer { pack: p, layer: l } = c
            && p == pack
            && layer.is_none()
        {
            layer = *l;
        }
    }
    layer
}

/// Something the host did that a developer (or the overlay) wants to see.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostEvent {
    /// A handler call was aborted; its writes were discarded.
    Aborted {
        tick: Tick,
        pack: PackId,
        script: String,
        handler: String,
        diagnostic: Diagnostic,
    },
    Quarantined {
        tick: Tick,
        pack: PackId,
        script: String,
    },
    /// A call's writes lost to contention (a change queued or written earlier in the
    /// tick): nothing applied and nothing counted against the script. A beat or
    /// trigger tries again next tick; an event handler is queued to run again.
    Contended {
        tick: Tick,
        pack: PackId,
        script: String,
        handler: String,
        reason: String,
    },
    /// An event handler lost to contention more than `limits.contention_retries` times
    /// in a row and was dropped (`script_contention_dropped`).
    ContentionDropped {
        tick: Tick,
        pack: PackId,
        script: String,
        handler: String,
        lost: u32,
    },
    /// A pack (re)loaded after passing the fast checks.
    Loaded { report: ValidationReport },
    /// A reload failed the checks; the last good revision keeps running.
    ReloadRejected { report: ValidationReport },
    /// A pack's data would write something its `uses` don't allow, so none of it was
    /// installed.
    InstallRefused { pack: PackId, reason: String },
}

struct Loaded {
    root: PathBuf,
    vm: PackVm,
    /// The layer it was loaded as. A game-master layer stays one in this host.
    layer: PackLayer,
}

#[derive(Default)]
struct Inner {
    packs: BTreeMap<PackId, Loaded>,
    analyzer: Option<PathBuf>,
    watcher: Option<PackWatcher>,
    events: Vec<HostEvent>,
}

impl Inner {
    fn log(&mut self, e: HostEvent) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }
}

/// The script host. Cheap to clone: clones share the loaded packs, so the app keeps
/// one handle for reloads while [`ScriptHost::system`] runs inside the sim.
#[derive(Clone, Default)]
pub struct ScriptHost {
    inner: Rc<RefCell<Inner>>,
}

impl ScriptHost {
    /// A host that typechecks reloads with `analyzer` (if given).
    pub fn new(analyzer: Option<PathBuf>) -> Self {
        let host = Self::default();
        host.inner.borrow_mut().analyzer = analyzer;
        host
    }

    /// Checks and loads a pack against this world as a core-layer pack: the path for
    /// the authored backbone (`Game` loads the packs root's `core/` folder here) and
    /// for tests. A pack with the same ID is replaced only if the new one passes. A
    /// pack the host or the world already knows as a game-master layer still loads as
    /// one: this path never promotes a pack.
    pub fn load(&self, dir: &Path, world: &WorldState, history: &History) -> ValidationReport {
        self.load_as(dir, world, history, PackLayer::Core, None)
    }

    /// [`ScriptHost::load`] for a game-master layer (`gm-examples`, and `galaxy-gm`'s
    /// applied, staged and sandboxed packs): checked and run as one whatever its ID
    /// ([`CheckOptions::game_master`]). It may not take `core`'s namespace or the ID
    /// of a pack the host runs or the world records as core (`gm_core_id`), so it
    /// never replaces a core pack's VM. [`ScriptHost::record_layers`] writes the layer
    /// into the world, so every later load of the pack, by any path, keeps it.
    pub fn load_layer(
        &self,
        dir: &Path,
        world: &WorldState,
        history: &History,
    ) -> ValidationReport {
        self.load_as(dir, world, history, PackLayer::GameMaster, None)
    }

    /// Checks and loads `dir` as `path`, the layer its load path gives it. The layer
    /// never comes from the pack's ID or from it being new: only a game-master layer
    /// the host or the world already knows for that ID overrides it. `reloads` is the
    /// loaded pack whose folder this is ([`ScriptHost::reload`]): if the folder now
    /// names another ID, that is a new pack of the same layer, which replaces the old
    /// entry, and never one that is already loaded.
    fn load_as(
        &self,
        dir: &Path,
        world: &WorldState,
        history: &History,
        path: PackLayer,
        reloads: Option<&PackId>,
    ) -> ValidationReport {
        let (analyzer, core_ids) = {
            let inner = self.inner.borrow();
            let core_ids: Vec<PackId> = inner
                .packs
                .iter()
                .filter(|(_, l)| l.layer == PackLayer::Core)
                .map(|(id, _)| id.clone())
                .collect();
            (inner.analyzer.clone(), core_ids)
        };
        let run = |game_master| {
            check_dir(
                dir,
                CheckOptions {
                    world: Some(world),
                    history: Some(history),
                    analyzer: analyzer.as_deref(),
                    require_typecheck: false,
                    game_master,
                    core_ids: &core_ids,
                },
            )
        };
        let mut checked = run(path == PackLayer::GameMaster);
        // A pack this host runs as a game-master layer stays one, whichever path loads
        // it again. The check itself also reads the layer the world records.
        let id = checked.pack.as_ref().map(|p| p.manifest.id.clone());
        let known = id.as_ref().is_some_and(|id| {
            self.inner
                .borrow()
                .packs
                .get(id)
                .is_some_and(|l| l.layer == PackLayer::GameMaster)
        });
        if known && path == PackLayer::Core {
            checked = run(true);
        }
        let recorded = id
            .as_ref()
            .and_then(|id| world.packs.get(id).and_then(|p| p.layer));
        let layer =
            if path == PackLayer::GameMaster || known || recorded == Some(PackLayer::GameMaster) {
                PackLayer::GameMaster
            } else {
                PackLayer::Core
            };
        let mut report = checked.report.clone();
        let mut inner = self.inner.borrow_mut();
        let renamed = reloads.filter(|old| id.as_ref().is_some_and(|id| id != *old));
        if let (Some(old), Some(id)) = (renamed, &id)
            && inner.packs.contains_key(id)
        {
            refuse(
                &mut report,
                "reload_id_taken",
                format!(
                    "the folder of `{old}` now names `{id}`, which another loaded pack holds: \
                     a reload never replaces a different pack"
                ),
            );
        }
        match checked.vm {
            Some(vm) if report.accepted() => {
                if let Some(old) = renamed {
                    // The folder is a new pack now, in the same layer: the old ID stops
                    // running here, and the watcher follows the folder.
                    inner.packs.remove(old);
                    if let Some(w) = &mut inner.watcher {
                        w.rename(old, &report.pack);
                    }
                }
                inner.packs.insert(
                    report.pack.clone(),
                    Loaded {
                        root: dir.to_path_buf(),
                        vm,
                        layer,
                    },
                );
                inner.log(HostEvent::Loaded {
                    report: report.clone(),
                });
            }
            _ => inner.log(HostEvent::ReloadRejected {
                report: report.clone(),
            }),
        }
        report
    }

    /// Loads a pack without the validation pipeline, only failing if its scripts
    /// don't load. For harnesses and tests that need a bad script running (the
    /// budget, discard and quarantine paths); never the game-master path. It runs as a
    /// core-layer pack unless the host or the world knows it as a game-master layer.
    pub fn load_unchecked(&self, dir: &Path) -> Result<(), Vec<Diagnostic>> {
        let pack = Rc::new(crate::pack::read_pack(dir)?);
        let vm = PackVm::new(pack.clone())?;
        let mut inner = self.inner.borrow_mut();
        let layer = inner
            .packs
            .get(&pack.manifest.id)
            .map_or(PackLayer::Core, |l| l.layer);
        inner.packs.insert(
            pack.manifest.id.clone(),
            Loaded {
                root: dir.to_path_buf(),
                vm,
                layer,
            },
        );
        Ok(())
    }

    /// The layer the host runs `pack` as, if it's loaded.
    pub fn layer(&self, pack: &PackId) -> Option<PackLayer> {
        self.inner.borrow().packs.get(pack).map(|l| l.layer)
    }

    /// Records each loaded pack's layer in the world (`PackState::layer`) where the
    /// world (with the batches queued) doesn't have one yet: one system batch, so it
    /// is never journaled and no rollback forgets it. Call between ticks; `install`
    /// (and so `attach`, `load_into` and `reload`) calls it, and the game-master
    /// bridge calls it when it applies a pack.
    pub fn record_layers(&self, sim: &mut Sim) {
        let inner = self.inner.borrow();
        let mut batch = CommandBatch::new(Cause::system(SYSTEM_NAME, "records the packs' layers"));
        for (id, l) in &inner.packs {
            if recorded_layer(sim.world(), &sim.state().kernel.pending, id).is_none() {
                batch.push(Command::SetPackLayer {
                    pack: id.clone(),
                    layer: Some(l.layer),
                });
            }
        }
        drop(inner);
        if !batch.commands.is_empty() {
            sim.submit(batch);
        }
    }

    /// Loads a pack into a running sim as a core-layer pack ([`ScriptHost::load`]) and
    /// installs its data.
    pub fn load_into(&self, dir: &Path, sim: &mut Sim) -> ValidationReport {
        let report = self.load(dir, sim.world(), sim.history());
        if report.accepted() {
            self.install(sim);
        }
        report
    }

    /// Registers the host as a system and installs every loaded pack's data.
    pub fn attach(&self, sim: &mut Sim) {
        sim.add_system(self.system());
        self.install(sim);
    }

    /// The system to add to a sim. Every clone runs the same packs.
    pub fn system(&self) -> Box<dyn System> {
        Box::new(ScriptSystem {
            inner: self.inner.clone(),
        })
    }

    /// Submits the records each pack's data defines that the world doesn't have yet.
    /// They apply at the end of the next tick, caused by the pack revision.
    /// A pack whose data needs a capability it doesn't declare installs nothing
    /// ([`HostEvent::InstallRefused`]).
    pub fn install(&self, sim: &mut Sim) -> usize {
        self.record_layers(sim);
        let mut inner = self.inner.borrow_mut();
        let mut batches: Vec<CommandBatch> = Vec::new();
        let mut refused = Vec::new();
        for (id, l) in &inner.packs {
            match install_batches(&l.vm.pack, sim.world()) {
                Ok(b) => batches.extend(b),
                Err(reason) => refused.push(HostEvent::InstallRefused {
                    pack: id.clone(),
                    reason,
                }),
            }
        }
        for e in refused {
            inner.log(e);
        }
        drop(inner);
        let n = batches.len();
        for b in batches {
            sim.submit(b);
        }
        n
    }

    /// Re-reads one pack from disk and swaps it in if it passes the fast checks.
    /// Call between ticks. If it fails, the last good revision keeps running. The
    /// folder keeps the layer it was loaded as: a game-master layer whose manifest now
    /// names another ID is a new game-master layer (never `core`, a core pack's ID or
    /// another loaded pack's), not a core pack.
    pub fn reload(&self, pack: &PackId, sim: &mut Sim) -> Option<ValidationReport> {
        let (root, layer) = {
            let inner = self.inner.borrow();
            let l = inner.packs.get(pack)?;
            (l.root.clone(), l.layer)
        };
        let report = self.load_as(&root, sim.world(), sim.history(), layer, Some(pack));
        if report.accepted() {
            self.install(sim);
        }
        Some(report)
    }

    /// Starts watching every loaded pack's folder for changes.
    pub fn watch(&self) -> notify::Result<()> {
        let roots: Vec<(PackId, PathBuf)> = self
            .inner
            .borrow()
            .packs
            .iter()
            .map(|(id, l)| (id.clone(), l.root.clone()))
            .collect();
        let watcher = PackWatcher::new(roots)?;
        self.inner.borrow_mut().watcher = Some(watcher);
        Ok(())
    }

    /// Reloads packs whose files changed since the last poll. Call between ticks.
    pub fn poll_reload(&self, sim: &mut Sim) -> Vec<ValidationReport> {
        let changed = match &self.inner.borrow().watcher {
            Some(w) => w.changed(),
            None => return Vec::new(),
        };
        changed.iter().filter_map(|p| self.reload(p, sim)).collect()
    }

    /// The running revision of a pack.
    pub fn revision(&self, pack: &PackId) -> Option<String> {
        self.inner
            .borrow()
            .packs
            .get(pack)
            .map(|l| l.vm.pack.revision.clone())
    }

    /// Takes a pack out of the host (its installed data stays in the world). Returns
    /// whether it was loaded.
    pub fn unload(&self, pack: &PackId) -> bool {
        self.inner.borrow_mut().packs.remove(pack).is_some()
    }

    /// The loaded pack, as read from disk.
    pub fn pack(&self, pack: &PackId) -> Option<Rc<Pack>> {
        self.inner
            .borrow()
            .packs
            .get(pack)
            .map(|l| l.vm.pack.clone())
    }

    /// The in-world line a pack declares for its rollbacks.
    pub fn rollback_line(&self, pack: &PackId) -> Option<RollbackLine> {
        self.pack(pack).and_then(|p| p.manifest.rollback.clone())
    }

    /// Takes the host's event log.
    pub fn take_events(&self) -> Vec<HostEvent> {
        std::mem::take(&mut self.inner.borrow_mut().events)
    }

    /// `print` output from a pack's scripts.
    pub fn script_log(&self, pack: &PackId) -> Vec<String> {
        self.inner
            .borrow()
            .packs
            .get(pack)
            .map(|l| l.vm.log.borrow().clone())
            .unwrap_or_default()
    }
}

/// Whether `pack`, running as `layer`, may cause `commands` along `route`: its
/// manifest declares every capability they need and their provenance holds
/// ([`galaxy_contract::needs::COMMAND_NEEDS`]). `Err` names the first command that
/// needs more and what it lacks.
pub fn declared(
    pack: &Pack,
    layer: PackLayer,
    route: Route,
    commands: &[Command],
) -> Result<(), String> {
    let caller = Caller {
        pack: &pack.manifest.id,
        uses: &pack.manifest.uses,
        layer,
    };
    galaxy_contract::needs::check(commands, &caller, route)
}

/// Batches that create a pack's data records missing from `world`. `Err` if the pack
/// doesn't declare what they need (`npcs.toml` needs `npcs.write`): nothing installs.
pub fn install_batches(pack: &Pack, world: &WorldState) -> Result<Vec<CommandBatch>, String> {
    let mut batch = CommandBatch::new(Cause::script(
        pack.manifest.id.clone(),
        pack.revision.clone(),
        format!("{} arrives", pack.manifest.name),
    ));
    for npc in &pack.data.npcs {
        if !world.npcs.contains(&npc.id) {
            batch.push(Command::PutNpc {
                id: npc.id.clone(),
                value: Some(Box::new(npc.clone())),
            });
        }
    }
    if batch.commands.is_empty() {
        return Ok(Vec::new());
    }
    let layer = world
        .packs
        .get(&pack.manifest.id)
        .and_then(|p| p.layer)
        .unwrap_or(PackLayer::GameMaster);
    declared(pack, layer, Route::Data, &batch.commands)
        .map_err(|e| format!("its data can't install: {e}"))?;
    Ok(vec![batch])
}

/// Rejects `report` with an error in its invariants step.
fn refuse(report: &mut ValidationReport, code: &str, message: String) {
    let d = crate::pack::error(code, message, Some("manifest.toml"), None);
    match report
        .steps
        .iter_mut()
        .find(|s| s.step == ValidationStep::Invariants)
    {
        Some(s) => {
            s.diagnostics.push(d);
            s.outcome = StepOutcome::Failed;
        }
        None => report.steps.push(StepReport {
            step: ValidationStep::Invariants,
            outcome: StepOutcome::Failed,
            diagnostics: vec![d],
        }),
    }
    report.status = ReportStatus::Rejected;
}

/// The fault for a batch that needs a capability its pack doesn't declare.
fn undeclared(what: &str, reason: String, location: Option<Location>) -> Abort {
    Abort {
        code: "undeclared_capability".into(),
        message: format!("{what} a capability the pack doesn't declare, so none apply: {reason}"),
        location,
    }
}

/// The fault for a batch that changes the player's stuff without their say-so
/// ([`crate::confirm`]).
fn unconfirmed(reason: String, location: Option<Location>) -> Abort {
    Abort {
        code: "unconfirmed_change".into(),
        message: format!("none of its writes apply: {reason}"),
        location,
    }
}

/// The world after `batch`, applied the way the kernel applies it (finite numbers,
/// every command, then the built-in invariants), without history or events. `Err` is
/// the reason the kernel would reject the batch. For read-your-writes copies of the
/// world and for checking a batch before it is submitted.
pub fn preview(world: &WorldState, batch: &CommandBatch) -> Result<WorldState, String> {
    for (i, c) in batch.commands.iter().enumerate() {
        galaxy_core::finite::check(c)
            .map_err(|e| format!("command {i}: numbers must be finite: {e}"))?;
    }
    let mut next = world.clone();
    let mut staged = Staged::default();
    for (i, c) in batch.commands.iter().enumerate() {
        apply(&mut next, c, &mut staged).map_err(|e| format!("command {i}: {e}"))?;
    }
    for inv in galaxy_core::invariants::builtin() {
        inv.check(&next)
            .map_err(|e| format!("invariant `{}`: {e}", inv.name()))?;
    }
    Ok(next)
}

struct ScriptSystem {
    inner: Rc<RefCell<Inner>>,
}

/// Where an aborted call happened, for its health bookkeeping.
struct Aborted<'a> {
    tick: Tick,
    pack: &'a PackId,
    script: &'a str,
    revision: &'a str,
    limit: u32,
}

impl Aborted<'_> {
    /// Counts an abort against the script, quarantining it at the limit.
    fn record(
        &self,
        h: &mut ScriptHealth,
        abort: &Abort,
        handler: String,
        quarantine: &mut CommandBatch,
        log: &mut Vec<HostEvent>,
    ) {
        let (pack_id, revision) = (self.pack, self.revision);
        h.aborts = h.aborts.saturating_add(1);
        let diagnostic = abort.diagnostic();
        if h.aborts >= self.limit && !h.quarantined {
            h.quarantined = true;
            quarantine.push(Command::RecordFact {
                fact: Box::new(FactDraft {
                    kind: QUARANTINED,
                    subjects: Default::default(),
                    amount: Some(i64::from(h.aborts)),
                    note: format!(
                        "{pack_id}/{} (revision {revision}) quarantined after {} aborted calls; last: {}",
                        self.script, h.aborts, clip_text(&abort.message)
                    ),
                }),
            });
            log.push(HostEvent::Quarantined {
                tick: self.tick,
                pack: pack_id.clone(),
                script: self.script.to_owned(),
            });
        }
        log.push(HostEvent::Aborted {
            tick: self.tick,
            pack: pack_id.clone(),
            script: self.script.to_owned(),
            handler,
            diagnostic,
        });
    }
}

/// Why a call's writes aren't submitted.
enum Failed {
    /// The script's fault: an error, a quota, or a write invalid on the world it read.
    /// Counts toward quarantine.
    Fault(Abort),
    /// Lost to contention. Never counts toward quarantine.
    Contended(String),
}

/// How the projection refused a batch.
enum Refused {
    /// Invalid on the world the call read, too.
    Invalid(String),
    /// Valid on the world the call read; only a change queued or written earlier this
    /// tick (an undock, moved cargo, a failed compare-and-set or `Expect*` check, a
    /// record gone) makes it fail.
    Contended(String),
}

impl Refused {
    fn into_failed(self, fault: impl FnOnce(String) -> Abort) -> Failed {
        match self {
            Refused::Invalid(reason) => Failed::Fault(fault(reason)),
            Refused::Contended(reason) => Failed::Contended(reason),
        }
    }
}

/// `batch` checked the way the kernel will check it, on `base` (the projection). If it
/// would be rejected, the same check on `read` (the world the call read, at this tick)
/// says whose it is: a batch that would apply there lost to contention; one that
/// wouldn't is the script's fault. Both checks are deterministic, so a fault is never
/// retried and contention never quarantines.
fn check(
    ctx: &TickCtx<'_>,
    base: &WorldState,
    read: &WorldState,
    batch: &CommandBatch,
) -> Result<WorldState, Refused> {
    let reason = match ctx.preview(base, batch) {
        Ok(after) => return Ok(after),
        Err(reason) => reason,
    };
    let mut seen = read.clone();
    seen.meta.tick = ctx.tick;
    Err(match ctx.preview(&seen, batch) {
        Ok(_) => Refused::Contended(reason),
        Err(_) => Refused::Invalid(reason),
    })
}

fn contended(tick: Tick, pack: &PackId, script: &str, handler: String, reason: &str) -> HostEvent {
    HostEvent::Contended {
        tick,
        pack: pack.clone(),
        script: script.to_owned(),
        handler,
        reason: clip_text(reason),
    }
}

/// A beat's transition as one batch: the beat runtime's commands under its cause, plus
/// whatever the beat's callback for it writes with `beat.why`.
#[allow(clippy::too_many_arguments)]
fn run_transition(
    vm: &PackVm,
    script: usize,
    index: usize,
    beat: &LoadedBeat,
    t: beats::Transition,
    view: View<'_>,
    pack: &PackId,
    revision: &str,
    tick: Tick,
    rng: &mut dyn crate::vm::RngSource,
) -> Result<Option<(CommandBatch, usize)>, Abort> {
    use beats::Transition as T;
    let def = &beat.def;
    let commands = beats::commands(def, view.world, view.history, pack, tick, t);
    let (hook, word, step) = match t {
        T::Start => ("on_start", "start", 0),
        T::Advance { to } => ("on_step", "step", to),
        T::Complete => ("on_complete", "complete", def.steps.len() as u32),
        T::Refresh => ("", "refresh", 0),
    };
    let summary = match t {
        T::Start => format!("{} starts", def.id),
        T::Advance { to } => format!("{} moves to step {}", def.id, to + 1),
        T::Complete => format!("{} completes", def.id),
        T::Refresh => format!("{} has a new goal line", def.id),
    };
    let seed = CommandBatch {
        cause: Cause::script(pack.clone(), revision, summary.clone()),
        commands,
    };
    // The runtime's own commands come first; the callback's writes follow them.
    let runtime = seed.commands.len();
    if hook.is_empty() || !beat.has_hook(hook) {
        return Ok(Some((seed, runtime)));
    }
    let info = BeatCall {
        beat: &def.id,
        transition: word,
        step,
        step_id: def.steps.get(step as usize).map(|s| s.id.as_str()),
        summary: &summary,
    };
    Ok(vm
        .beat_hook(script, index, hook, &info, view, rng, seed)?
        .map(|b| (b, runtime)))
}

impl System for ScriptSystem {
    fn name(&self) -> &'static str {
        SYSTEM_NAME
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let mut inner = self.inner.borrow_mut();
        let world = ctx.world;
        let history = ctx.history;
        let events = ctx.events;
        let tick = ctx.tick;
        // What later calls read: the tick-start world plus this tick's script writes,
        // made on the first write.
        let mut scratch: Option<WorldState> = None;
        // What the kernel will apply the next script batch to: the tick start, the
        // batches queued ahead of the host, and the script batches so far.
        let mut projected: Option<WorldState> = None;
        let mut health: BTreeMap<(PackId, String), ScriptHealth> = BTreeMap::new();
        let mut log = Vec::new();
        let mut out = Vec::new();
        let mut quarantine = CommandBatch::new(Cause::system(SYSTEM_NAME, "scripts aborted"));
        // Revisions a rollback queued for retirement stop now, before the rollback's
        // inverse batches apply at the end of this tick, so they can't redo the change
        // being undone.
        let retiring: BTreeSet<(PackId, String)> = ctx
            .queued()
            .iter()
            .flat_map(|b| &b.commands)
            .filter_map(|c| match c {
                Command::SetRevisionRetired {
                    pack,
                    revision,
                    retired: true,
                } => Some((pack.clone(), revision.clone())),
                _ => None,
            })
            .collect();

        for (pack_id, loaded) in &inner.packs {
            let vm = &loaded.vm;
            // A game-master layer as the host loaded it or as the world records it: a
            // reload by any path never lifts its restrictions.
            let layer = if loaded.layer == PackLayer::GameMaster
                || recorded_layer(world, ctx.queued(), pack_id) == Some(PackLayer::GameMaster)
            {
                PackLayer::GameMaster
            } else {
                PackLayer::Core
            };
            let revision = &vm.pack.revision;
            let limits = vm.pack.limits();
            let (limit, retry_limit) = (limits.quarantine_after, limits.contention_retries);
            // A pack a rollback switched off runs no revision at all, whichever source
            // loaded it, from the tick its switch-off is queued.
            if retiring.contains(&(pack_id.clone(), revision.clone()))
                || world
                    .packs
                    .get(pack_id)
                    .is_some_and(|p| p.retired.contains(revision))
                || pack_off(world, ctx.queued(), pack_id)
            {
                continue;
            }
            for (i, script) in vm.scripts.iter().enumerate() {
                let key = (pack_id.clone(), script.decl.id.clone());
                let current = |health: &BTreeMap<(PackId, String), ScriptHealth>| {
                    health.get(&key).cloned().unwrap_or_else(|| {
                        ScriptHealth::for_revision(
                            world
                                .packs
                                .get(pack_id)
                                .and_then(|p| p.health.get(&script.decl.id)),
                            revision,
                        )
                    })
                };
                // Event calls that lost to contention earlier run again first, oldest
                // first (while this revision still handles the event).
                let pending: Vec<HandlerRetry> = current(&health)
                    .retries
                    .into_iter()
                    .filter(|r| script.events.contains(&r.event.event.kind()))
                    .collect();
                let mut retries: Vec<HandlerRetry> = Vec::new();
                let on = |e: &EventEnvelope| format!("on.{}", e.event.kind().as_str());
                let mut calls: Vec<(String, Target<'_>, u32)> = pending
                    .iter()
                    .map(|r| (on(&r.event), Target::Event(&r.event), r.lost))
                    .collect();
                calls.extend(
                    events
                        .iter()
                        .filter(|e| script.events.contains(&e.event.kind()))
                        .map(|e| (on(e), Target::Event(e), 0)),
                );
                for t in &script.triggers {
                    let view = scratch.as_ref().unwrap_or(world);
                    let tkey = format!("{}.{}", script.decl.id, t.name);
                    let fired = view
                        .packs
                        .get(pack_id)
                        .and_then(|p| p.triggers.get(&tkey))
                        .copied()
                        .unwrap_or(0);
                    if t.spec.due(view, fired) {
                        calls.push((
                            format!("after.{}", t.name),
                            Target::Trigger {
                                name: &t.name,
                                fired,
                            },
                            0,
                        ));
                    }
                }
                let sid = script.decl.id.as_str();
                for (handler, target, lost) in calls {
                    let mut h = current(&health);
                    if h.quarantined {
                        // Never runs under this revision again; what waits keeps waiting.
                        if let (Target::Event(e), 1..) = (target, lost) {
                            retries.push(HandlerRetry {
                                event: e.clone(),
                                lost,
                            });
                        }
                        continue;
                    }
                    let view = View {
                        world: scratch.as_ref().unwrap_or(world),
                        history,
                    };
                    let result = vm
                        .call(i, target, view, ctx)
                        .map_err(Failed::Fault)
                        .and_then(|batch| {
                            let batch = match target {
                                Target::Trigger { name, fired } => {
                                    let mark = Command::SetTriggerFired {
                                        pack: pack_id.clone(),
                                        trigger: format!("{sid}.{name}"),
                                        fired: fired.saturating_add(1),
                                    };
                                    Some(match batch {
                                        Some(b) => b.with(mark),
                                        None => CommandBatch::new(Cause::script(
                                            pack_id.clone(),
                                            revision.clone(),
                                            format!("{handler} fired"),
                                        ))
                                        .with(mark),
                                    })
                                }
                                Target::Event(_) => batch,
                            };
                            let Some(batch) = batch else { return Ok(None) };
                            let location = || {
                                Some(Location {
                                    file: script.decl.file.clone(),
                                    line: vm.handler_line(i, target),
                                    column: None,
                                })
                            };
                            declared(&vm.pack, layer, Route::Hooks, &batch.commands).map_err(
                                |reason| {
                                    Failed::Fault(undeclared("its writes need", reason, location()))
                                },
                            )?;
                            crate::confirm::confirmed(view.world, history, &batch).map_err(
                                |reason| Failed::Fault(unconfirmed(reason, location())),
                            )?;
                            // The whole call's writes, checked as the kernel will check them.
                            let base = projected.get_or_insert_with(|| ctx.projected());
                            check(ctx, base, view.world, &batch)
                                .map(|after| Some((batch, after)))
                                .map_err(|r| {
                                    r.into_failed(|reason| Abort {
                                        code: "rejected_write".into(),
                                        message: format!(
                                            "its writes would be rejected, so none apply: {reason}"
                                        ),
                                        location: location(),
                                    })
                                })
                        });
                    match result {
                        Ok(Some((batch, after))) => {
                            projected = Some(after);
                            // Reads see the write. (If only the queued batches make it
                            // apply, the reads go without it.)
                            if let Ok(read) = preview(view.world, &batch) {
                                scratch = Some(read);
                            }
                            out.push(batch);
                        }
                        Ok(None) => {}
                        Err(Failed::Fault(abort)) => {
                            let at = Aborted {
                                tick,
                                pack: pack_id,
                                script: sid,
                                revision,
                                limit,
                            };
                            at.record(&mut h, &abort, handler, &mut quarantine, &mut log);
                            health.insert(key.clone(), h);
                        }
                        // A trigger isn't marked fired, so it is due again next tick.
                        Err(Failed::Contended(reason))
                            if matches!(target, Target::Trigger { .. }) =>
                        {
                            log.push(contended(tick, pack_id, sid, handler, &reason));
                        }
                        Err(Failed::Contended(reason)) => {
                            let Target::Event(e) = target else { continue };
                            let lost = lost.saturating_add(1);
                            if lost <= retry_limit {
                                log.push(contended(tick, pack_id, sid, handler, &reason));
                                retries.push(HandlerRetry {
                                    event: e.clone(),
                                    lost,
                                });
                                continue;
                            }
                            quarantine.push(Command::RecordFact {
                                fact: Box::new(FactDraft {
                                    kind: CONTENTION_DROPPED,
                                    subjects: e.event.subjects(),
                                    amount: Some(i64::from(lost)),
                                    note: clip_text(&format!(
                                        "{pack_id}/{sid} (revision {revision}) dropped {handler} for {} after losing to contention {lost} times; last: {reason}",
                                        e.fact
                                    )),
                                }),
                            });
                            log.push(HostEvent::ContentionDropped {
                                tick,
                                pack: pack_id.clone(),
                                script: sid.to_owned(),
                                handler,
                                lost,
                            });
                        }
                    }
                }
                let mut h = current(&health);
                if h.retries != retries {
                    h.retries = retries;
                    health.insert(key.clone(), h);
                }
                // Beats this script registers: start, advance or complete. Every
                // predicate and callback runs in its own fresh state, like a handler.
                for (bi, beat) in script.beats.iter().enumerate() {
                    let mut h = current(&health);
                    if h.quarantined {
                        break;
                    }
                    let view = View {
                        world: scratch.as_ref().unwrap_or(world),
                        history,
                    };
                    let mut pred_abort: Option<Abort> = None;
                    let next = {
                        let mut tests = |idx: u32| -> Result<bool, EvalError> {
                            vm.predicate(i, idx, view, &mut *ctx).map_err(|a| {
                                let msg = a.message.clone();
                                pred_abort = Some(a);
                                EvalError::Predicate(msg)
                            })
                        };
                        beats::next(&beat.def, Eval::new(view.world, history), &mut tests)
                    };
                    let handler = format!("beat {}", beat.def.id);
                    let result = match next {
                        Ok(None) => continue,
                        Err(e) => Err(Failed::Fault(pred_abort.take().unwrap_or_else(|| {
                            Abort {
                                code: "bad_beat".into(),
                                message: format!("{}: {e}", beat.def.id),
                                location: None,
                            }
                        }))),
                        Ok(Some(t)) => run_transition(
                            vm, i, bi, beat, t, view, pack_id, revision, tick, &mut *ctx,
                        )
                        .map_err(Failed::Fault)
                        .and_then(|batch| {
                            let Some((batch, runtime)) = batch else { return Ok(None) };
                            // Its rewards and callback writes answer to the pack's
                            // `uses` like a handler's (a standing reward needs
                            // `standing.write`). The runtime's part is the beat route
                            // (none for a game-master layer); the callback's is hooks.
                            let (own, callback) = batch.commands.split_at(runtime);
                            declared(&vm.pack, layer, Route::Beat, own)
                                .and_then(|()| {
                                    declared(&vm.pack, layer, Route::Hooks, callback)
                                        .map_err(|e| format!("its callback: {e}"))
                                })
                                .map_err(|reason| {
                                    Failed::Fault(undeclared(
                                        &format!("{}'s transition needs", beat.def.id),
                                        reason,
                                        Some(Location {
                                            file: script.decl.file.clone(),
                                            line: None,
                                            column: None,
                                        }),
                                    ))
                                })?;
                            crate::confirm::confirmed(view.world, history, &batch).map_err(
                                |reason| {
                                    Failed::Fault(unconfirmed(
                                        format!("{}'s transition: {reason}", beat.def.id),
                                        Some(Location {
                                            file: script.decl.file.clone(),
                                            line: None,
                                            column: None,
                                        }),
                                    ))
                                },
                            )?;
                            // Checked as the kernel will check it, like a handler's
                            // batch, so a transition the queued batches would reject
                            // never reaches the reads of later beats.
                            let base = projected.get_or_insert_with(|| ctx.projected());
                            check(ctx, base, view.world, &batch)
                                .map(|after| Some((batch, after)))
                                .map_err(|r| {
                                    r.into_failed(|reason| Abort {
                                        code: "rejected_write".into(),
                                        message: format!(
                                            "{}'s transition would be rejected, so none of it applies: {reason}",
                                            beat.def.id
                                        ),
                                        location: Some(Location {
                                            file: script.decl.file.clone(),
                                            line: None,
                                            column: None,
                                        }),
                                    })
                                })
                        }),
                    };
                    match result {
                        Ok(Some((batch, after))) => {
                            projected = Some(after);
                            // Only a transition that will commit is read: a successor
                            // sees its predecessor done once the completion applies.
                            if let Ok(read) = preview(view.world, &batch) {
                                scratch = Some(read);
                            }
                            out.push(batch);
                        }
                        Ok(None) => {}
                        // Nothing changed: the beat is checked again next tick.
                        Err(Failed::Contended(reason)) => {
                            log.push(contended(tick, pack_id, sid, handler, &reason));
                        }
                        Err(Failed::Fault(abort)) => {
                            let at = Aborted {
                                tick,
                                pack: pack_id,
                                script: sid,
                                revision,
                                limit,
                            };
                            at.record(&mut h, &abort, handler, &mut quarantine, &mut log);
                            health.insert(key.clone(), h);
                        }
                    }
                }
            }
        }
        for ((pack, script), h) in health {
            quarantine.push(Command::SetScriptHealth {
                pack,
                script,
                health: h,
            });
        }
        for b in out {
            ctx.submit(b);
        }
        ctx.submit(quarantine);
        for e in log {
            inner.log(e);
        }
    }
}
