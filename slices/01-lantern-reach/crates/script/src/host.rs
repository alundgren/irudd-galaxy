//! The script host: loaded packs, the per-tick [`System`], dev hot reload.
//!
//! Each tick, for every loaded pack (in pack ID order) whose revision isn't retired,
//! and every script (in manifest order) that isn't quarantined:
//!
//! 1. each event committed last tick with an `on.<event>` handler runs it;
//! 2. each `G.after` trigger whose activity counters are due runs once.
//!
//! A successful call submits its batches (one per cause, `Cause::Script`); later calls
//! in the same tick read a copy of the world with those writes applied, so two events
//! in one tick don't grow a station twice. An aborted call submits nothing, counts
//! against the script, and after `limits.quarantine_after` aborts the script is
//! quarantined (for that revision) with a `script_quarantined` history fact.
//!
//! The Luau states hold no world state, so the host itself is safe to rebuild at any
//! time: a reload swaps a pack's state between ticks and loses nothing.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use galaxy_contract::history::FactDraft;
use galaxy_contract::pack::{Diagnostic, RollbackLine, ScriptHealth, ValidationReport};
use galaxy_contract::prelude::*;
use galaxy_core::apply::{Staged, apply};
use galaxy_core::{History, Sim, System, TickCtx};
use serde::Serialize;

use crate::check::{CheckOptions, check_dir};
use crate::pack::Pack;
use crate::vm::{PackVm, Target, View};
use crate::watch::PackWatcher;

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

/// Applies a batch all-or-nothing (no invariants, no history): for read-your-writes
/// copies of the world.
pub fn apply_atomic(world: &mut WorldState, batch: &CommandBatch) -> Result<(), String> {
    let mut undo = Vec::new();
    let mut staged = Staged::default();
    for c in &batch.commands {
        match apply(world, c, &mut staged) {
            Ok(u) => undo.push(u),
            Err(e) => {
                for u in undo.iter().rev() {
                    if let UndoRecord::Invert { command } = u {
                        let _ = apply(world, command, &mut Staged::default());
                    }
                }
                return Err(e.to_string());
            }
        }
    }
    Ok(())
}

struct ScriptSystem {
    inner: Rc<RefCell<Inner>>,
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
        // A copy with this tick's script writes applied, made on the first write.
        let mut scratch: Option<WorldState> = None;
        let mut health: BTreeMap<(PackId, String), ScriptHealth> = BTreeMap::new();
        let mut log = Vec::new();
        let mut out = Vec::new();
        let mut quarantine = CommandBatch::new(Cause::system(SYSTEM_NAME, "scripts aborted"));

        for (pack_id, loaded) in &inner.packs {
            let vm = &loaded.vm;
            let revision = &vm.pack.revision;
            let limit = vm.pack.limits().quarantine_after;
            if world
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
                        world
                            .packs
                            .get(pack_id)
                            .and_then(|p| p.health.get(&script.decl.id))
                            .filter(|h| h.revision == *revision)
                            .cloned()
                            .unwrap_or_else(|| ScriptHealth {
                                revision: revision.clone(),
                                ..ScriptHealth::default()
                            })
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
                    match vm.call(i, target, view, ctx) {
                        Ok(mut batches) => {
                            if let Target::Trigger { name, fired } = target {
                                let mark = Command::SetTriggerFired {
                                    pack: pack_id.clone(),
                                    trigger: format!("{}.{name}", script.decl.id),
                                    fired: fired + 1,
                                };
                                match batches.first_mut() {
                                    Some(b) => b.push(mark),
                                    None => batches.push(
                                        CommandBatch::new(Cause::script(
                                            pack_id.clone(),
                                            revision.clone(),
                                            format!("{handler} fired"),
                                        ))
                                        .with(mark),
                                    ),
                                }
                            }
                            if !batches.is_empty() {
                                let copy = scratch.get_or_insert_with(|| world.clone());
                                for b in &batches {
                                    let _ = apply_atomic(copy, b);
                                }
                            }
                            out.extend(batches);
                        }
                        Err(abort) => {
                            h.aborts += 1;
                            let diagnostic = abort.diagnostic();
                            if h.aborts >= limit && !h.quarantined {
                                h.quarantined = true;
                                quarantine.push(Command::RecordFact {
                                    fact: Box::new(FactDraft {
                                        kind: QUARANTINED,
                                        subjects: Default::default(),
                                        amount: Some(i64::from(h.aborts)),
                                        note: format!(
                                            "{pack_id}/{} (revision {revision}) quarantined after {} aborted calls; last: {}",
                                            script.decl.id, h.aborts, abort.message
                                        ),
                                    }),
                                });
                                log.push(HostEvent::Quarantined {
                                    tick,
                                    pack: pack_id.clone(),
                                    script: script.decl.id.clone(),
                                });
                            }
                            log.push(HostEvent::Aborted {
                                tick,
                                pack: pack_id.clone(),
                                script: script.decl.id.clone(),
                                handler,
                                diagnostic,
                            });
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
