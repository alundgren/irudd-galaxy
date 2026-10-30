//! The script host: loaded packs, the per-tick [`System`], dev hot reload.
//!
//! Each tick, for every loaded pack (in pack ID order) whose revision isn't retired
//! (or being retired by a rollback queued for this tick), and every script (in manifest
//! order) that isn't quarantined:
//!
//! 1. each event committed last tick with an `on.<event>` handler runs it;
//! 2. each `G.after` trigger whose activity counters are due runs once;
//! 3. each beat it registers starts, advances or completes if its requirements hold.
//!    `G.test` predicates and the transition's callback each run in a fresh state.
//!
//! A successful call's writes (or a beat transition with its callback's) are one batch
//! (`Cause::Script`). The host first checks it the way the kernel will
//! ([`TickCtx::preview`]): against the world the kernel will apply it to (the batches
//! queued ahead of it this tick, then the script batches before it) and with every
//! invariant of the sim. If it would be rejected, the call
//! counts as aborted and nothing is submitted. Later calls in the same tick read the
//! tick-start world plus the script writes before them, so two events in one tick
//! don't grow a station twice. An aborted call
//! submits nothing, counts against the script, and after `limits.quarantine_after`
//! aborts the script is quarantined (for that revision, for good: loading the same
//! revision again doesn't clear it) with a `script_quarantined` history fact.
//!
//! The Luau states hold no world state, so the host itself is safe to rebuild at any
//! time: a reload swaps a pack's state between ticks and loses nothing.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use galaxy_contract::history::FactDraft;
use galaxy_contract::pack::{Diagnostic, Location, RollbackLine, ScriptHealth, ValidationReport};
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
const EVENT_CAP: usize = 500;

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
    /// A pack (re)loaded after passing the fast checks.
    Loaded { report: ValidationReport },
    /// A reload failed the checks; the last good revision keeps running.
    ReloadRejected { report: ValidationReport },
}

struct Loaded {
    root: PathBuf,
    vm: PackVm,
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

    /// Checks and loads a pack against this world. A pack with the same ID is
    /// replaced only if the new one passes.
    pub fn load(&self, dir: &Path, world: &WorldState, history: &History) -> ValidationReport {
        let analyzer = self.inner.borrow().analyzer.clone();
        let checked = check_dir(
            dir,
            CheckOptions {
                world: Some(world),
                history: Some(history),
                analyzer: analyzer.as_deref(),
                require_typecheck: false,
            },
        );
        let report = checked.report.clone();
        let mut inner = self.inner.borrow_mut();
        match checked.vm {
            Some(vm) if report.accepted() => {
                inner.packs.insert(
                    report.pack.clone(),
                    Loaded {
                        root: dir.to_path_buf(),
                        vm,
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
    /// budget, discard and quarantine paths); never the game-master path.
    pub fn load_unchecked(&self, dir: &Path) -> Result<(), Vec<Diagnostic>> {
        let pack = Rc::new(crate::pack::read_pack(dir)?);
        let vm = PackVm::new(pack.clone())?;
        self.inner.borrow_mut().packs.insert(
            pack.manifest.id.clone(),
            Loaded {
                root: dir.to_path_buf(),
                vm,
            },
        );
        Ok(())
    }

    /// Loads a pack into a running sim and installs its data.
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
    pub fn install(&self, sim: &mut Sim) -> usize {
        let batches: Vec<CommandBatch> = self
            .inner
            .borrow()
            .packs
            .values()
            .flat_map(|l| install_batches(&l.vm.pack, sim.world()))
            .collect();
        let n = batches.len();
        for b in batches {
            sim.submit(b);
        }
        n
    }

    /// Re-reads one pack from disk and swaps it in if it passes the fast checks.
    /// Call between ticks. If it fails, the last good revision keeps running.
    pub fn reload(&self, pack: &PackId, sim: &mut Sim) -> Option<ValidationReport> {
        let root = self.inner.borrow().packs.get(pack)?.root.clone();
        Some(self.load_into(&root, sim))
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

/// Batches that create a pack's data records missing from `world`.
pub fn install_batches(pack: &Pack, world: &WorldState) -> Vec<CommandBatch> {
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
        Vec::new()
    } else {
        vec![batch]
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
) -> Result<Option<CommandBatch>, Abort> {
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
    if hook.is_empty() || !beat.has_hook(hook) {
        return Ok(Some(seed));
    }
    let info = BeatCall {
        beat: &def.id,
        transition: word,
        step,
        step_id: def.steps.get(step as usize).map(|s| s.id.as_str()),
        summary: &summary,
    };
    vm.beat_hook(script, index, hook, &info, view, rng, seed)
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
            let revision = &vm.pack.revision;
            let limit = vm.pack.limits().quarantine_after;
            if retiring.contains(&(pack_id.clone(), revision.clone()))
                || world
                    .packs
                    .get(pack_id)
                    .is_some_and(|p| p.retired.contains(revision))
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
                let mut calls: Vec<(String, Target<'_>)> = events
                    .iter()
                    .filter(|e| script.events.contains(&e.event.kind()))
                    .map(|e| (format!("on.{}", e.event.kind().as_str()), Target::Event(e)))
                    .collect();
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
                        ));
                    }
                }
                for (handler, target) in calls {
                    let mut h = current(&health);
                    if h.quarantined {
                        break;
                    }
                    let view = View {
                        world: scratch.as_ref().unwrap_or(world),
                        history,
                    };
                    let result = vm.call(i, target, view, ctx).and_then(|batch| {
                        let batch = match target {
                            Target::Trigger { name, fired } => {
                                let mark = Command::SetTriggerFired {
                                    pack: pack_id.clone(),
                                    trigger: format!("{}.{name}", script.decl.id),
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
                        // The whole call's writes, checked as the kernel will check them.
                        let base = projected.get_or_insert_with(|| ctx.projected());
                        match ctx.preview(base, &batch) {
                            Ok(after) => Ok(Some((batch, after))),
                            Err(reason) => Err(Abort {
                                code: "rejected_write".into(),
                                message: format!(
                                    "its writes would be rejected, so none apply: {reason}"
                                ),
                                location: Some(Location {
                                    file: script.decl.file.clone(),
                                    line: vm.handler_line(i, target),
                                    column: None,
                                }),
                            }),
                        }
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
                        Err(abort) => {
                            let at = Aborted {
                                tick,
                                pack: pack_id,
                                script: &script.decl.id,
                                revision,
                                limit,
                            };
                            at.record(&mut h, &abort, handler, &mut quarantine, &mut log);
                            health.insert(key.clone(), h);
                        }
                    }
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
                    let result = match next {
                        Ok(None) => continue,
                        Err(e) => Err(pred_abort.take().unwrap_or_else(|| Abort {
                            code: "bad_beat".into(),
                            message: format!("{}: {e}", beat.def.id),
                            location: None,
                        })),
                        Ok(Some(t)) => run_transition(
                            vm, i, bi, beat, t, view, pack_id, revision, tick, &mut *ctx,
                        )
                        .and_then(|batch| match batch {
                            None => Ok(None),
                            // Checked as the kernel will check it, like a handler's
                            // batch, so a transition the queued batches would reject
                            // never reaches the reads of later beats.
                            Some(batch) => match ctx.preview(
                                projected.get_or_insert_with(|| ctx.projected()),
                                &batch,
                            ) {
                                Ok(after) => Ok(Some((batch, after))),
                                Err(reason) => Err(Abort {
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
                                }),
                            },
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
                        Err(abort) => {
                            let at = Aborted {
                                tick,
                                pack: pack_id,
                                script: &script.decl.id,
                                revision,
                                limit,
                            };
                            let handler = format!("beat {}", beat.def.id);
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
