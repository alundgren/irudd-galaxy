//! The game's side of the bridge: [`Bridge`] lives next to the running `Sim` (in
//! `galaxy play`, `galaxy sim run` and the save-file commands) and is only ever called
//! between ticks.
//!
//! - **Open.** Loads the save's game-master state, loads every applied pack revision
//!   that wasn't rolled back into a `ScriptHost`, and attaches the host to the sim.
//! - **Calm moments** (docked or paused, and loading a save): staged packs apply.
//!   Each one gets a snapshot first (`Save::snapshot`, through [`Snapshots`]) and
//!   applies only once that snapshot is written and the world is still the one it
//!   holds; a failed snapshot leaves the pack staged. Then its fast checks run again
//!   against the world as it is now, and one batch caused by the pack revision installs
//!   its data and records a `gm_change_applied` fact. That batch's cause is the change
//!   ID; the bridge reads it back when the batch commits.
//! - **Crash safety.** `state.json` records intent, never proof: the save does. On
//!   open, every entry is checked against the save's history and its queued batches
//!   ([`reconcile`]). An applied entry whose apply batch the save doesn't hold goes back
//!   to staged (and applies again at the next calm moment), a staged one the save
//!   already holds counts as applied, and change IDs are the save's own.
//! - **Requests** from the live socket are answered in [`Bridge::serve`], between
//!   frames, at most [`MAX_REQUESTS_PER_SERVE`] at a time. Reads (`state.*`,
//!   `history.query`, `gm.digest`) run on a read thread from a copy of the state, and
//!   other slow work (writing a fork, validating an inbox entry) on a thread of its own.
//! - **Watchers** get every new fact, talk line, script trouble, game-master change
//!   and calm-moment change as a stream item.
//! - **Inbox**: at a calm moment the game claims the first pack folder in `gm-inbox/`
//!   next to the save, validates it in full on a thread (sandbox children included),
//!   writes `<name>.result.json` beside it, and stages it if it was accepted.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use galaxy_contract::history::{FactDraft, FactPayload};
use galaxy_contract::pack::{ReportStatus, ValidationReport};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use galaxy_script::rollback::causes_of;
use galaxy_script::{HostEvent, ScriptHost};
use serde_json::{Value, json};

use crate::digest::digest;
use crate::live::{Incoming, Op, Server, ServerOptions, StreamSink, WireError};
use crate::propose::{self, ProposeOptions};
use crate::reads;
use crate::sandbox::SandboxRules;
use crate::store::{AppliedPack, GmState, GmStore, QuarantinedPack, READY, StagedPack};

/// The system name the bridge's own batches are caused by.
pub const SYSTEM_NAME: &str = "gm_bridge";
/// Recorded by the batch that applies a staged pack. Its cause is the change ID.
pub const APPLIED: FactKind = FactKind::from_static("gm_change_applied");
/// Requests answered per call to [`Bridge::serve`].
pub const MAX_REQUESTS_PER_SERVE: usize = 32;
/// Facts a `watch --since` replays before it goes live.
const MAX_REPLAY: usize = 500;
/// Watchers at once.
pub const MAX_WATCHERS: usize = 8;
/// Reads waiting for the read thread; more are refused as `busy`.
const READ_QUEUE: usize = 16;
/// Script troubles kept for `gm status`.
const TROUBLE_LOG: usize = 50;

/// A snapshot being written.
pub enum SnapshotTicket {
    Done(Result<i64, String>),
    Pending(Receiver<Result<i64, String>>),
}

/// Takes snapshots of the save (the game's background saver, or a `Save` directly).
pub trait Snapshots {
    fn snapshot(&mut self, state: &SimState, label: &str) -> SnapshotTicket;
}

impl Snapshots for Save {
    fn snapshot(&mut self, state: &SimState, label: &str) -> SnapshotTicket {
        SnapshotTicket::Done(Save::snapshot(self, state, label).map_err(|e| e.to_string()))
    }
}

/// Refuses every snapshot, so nothing applies (tests).
pub struct NoSnapshots;

impl Snapshots for NoSnapshots {
    fn snapshot(&mut self, _: &SimState, _: &str) -> SnapshotTicket {
        SnapshotTicket::Done(Err("no snapshots here".into()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct BridgeOptions {
    /// The `galaxy` binary, for sandbox runs of inbox entries. Without one the inbox is
    /// off.
    pub exe: Option<PathBuf>,
    pub analyzer: Option<PathBuf>,
    pub rules: SandboxRules,
}

/// A snapshot asked for before applying `stage`, taken at `at`.
struct PendingSnapshot {
    stage: String,
    at: Moment,
    rx: Receiver<Result<i64, String>>,
}

/// Between ticks the state only changes by a tick or a submitted batch, so the tick
/// and the number of queued batches name it.
type Moment = (Tick, usize);

fn moment(sim: &Sim) -> Moment {
    (sim.tick(), sim.state().kernel.pending.len())
}

/// The read thread, and the state copy it answers from (shared while the state
/// doesn't change).
struct Reader {
    tx: SyncSender<(Arc<SimState>, Incoming)>,
    copy: Option<(Moment, Arc<SimState>)>,
}

impl Reader {
    fn start() -> Option<Reader> {
        let (tx, rx) = mpsc::sync_channel::<(Arc<SimState>, Incoming)>(READ_QUEUE);
        std::thread::Builder::new()
            .name("gm-read".into())
            .spawn(move || {
                while let Ok((state, req)) = rx.recv() {
                    let result = read(&state, &req.op);
                    req.respond(result);
                }
            })
            .ok()?;
        Some(Reader { tx, copy: None })
    }
}

/// A read op, answered from a copy of the state.
fn read(state: &SimState, op: &Op) -> Result<Value, WireError> {
    let world = || serde_json::to_value(&state.world).expect("worlds serialize");
    match op {
        Op::StateGet { path } => reads::state_get(&world(), path.as_deref()).map_err(Into::into),
        Op::StateQuery(q) => reads::state_query(&world(), q).map_err(Into::into),
        Op::HistoryQuery(q) => Ok(reads::history_query(&state.history, q)),
        Op::Digest { since } => {
            Ok(serde_json::to_value(digest(state, *since)).expect("digests serialize"))
        }
        _ => Err(WireError::runtime("not a read")),
    }
}

struct InboxJob {
    name: String,
    claimed: PathBuf,
    rx: Receiver<ValidationReport>,
}

pub struct Bridge {
    store: GmStore,
    state: GmState,
    host: ScriptHost,
    server: Option<Server>,
    watchers: Vec<StreamSink>,
    opts: BridgeOptions,
    seen_fact: Option<FactId>,
    seen_talk: u64,
    calm: Option<bool>,
    /// Staged packs waiting for their snapshot to be written.
    snapshotting: Vec<PendingSnapshot>,
    /// Staged packs whose snapshot failed during this calm moment, with the error. They
    /// are tried again at the next one.
    snapshot_failed: BTreeMap<String, String>,
    reader: Option<Reader>,
    inbox: Option<InboxJob>,
    trouble: VecDeque<Value>,
    dirty: bool,
    /// Problems worth printing (a pack that no longer loads, a failed write).
    pub notes: Vec<String>,
}

/// Is the player at a calm moment: paused, or the flagship docked?
pub fn is_calm(sim: &Sim) -> bool {
    sim.is_paused() || docked_at(sim.world()).is_some()
}

fn docked_at(world: &WorldState) -> Option<StationId> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .and_then(|s| s.docked_at.clone())
}

fn retired(world: &WorldState, pack: &PackId, revision: &str) -> bool {
    world
        .packs
        .get(pack)
        .is_some_and(|p| p.retired.contains(revision))
}

impl Bridge {
    /// Opens the bridge for `save` over `sim`: loads the packs that run on it and
    /// attaches the script host. Doesn't apply staged packs (see
    /// [`Bridge::apply_staged`]).
    pub fn open(save: &Path, sim: &mut Sim, opts: BridgeOptions) -> Result<Bridge, String> {
        let store = GmStore::for_save(save);
        let mut state = store.load()?;
        let (dirty, mut notes) = reconcile(&mut state, sim);
        let host = ScriptHost::new(None);
        for a in state.current() {
            if retired(sim.world(), &a.pack, &a.revision) {
                continue;
            }
            let report = host.load(&store.abs(&a.dir), sim.world(), sim.history());
            if !report.accepted() {
                let why = report
                    .errors()
                    .next()
                    .map_or_else(|| "its checks failed".to_owned(), |d| d.message.clone());
                notes.push(format!("{} ({}) didn't load: {why}", a.pack, a.stage));
            }
        }
        host.attach(sim);
        let mut bridge = Bridge {
            store,
            state,
            host,
            server: None,
            watchers: Vec::new(),
            opts,
            seen_fact: None,
            seen_talk: sim.world().talk.last,
            calm: None,
            snapshotting: Vec::new(),
            snapshot_failed: BTreeMap::new(),
            reader: None,
            inbox: None,
            trouble: VecDeque::new(),
            dirty,
            notes,
        };
        bridge.seen_fact = sim.history().last_fact_id();
        bridge.flush();
        Ok(bridge)
    }

    /// Opens the live socket.
    pub fn start_live(&mut self, registry: Option<PathBuf>) -> Result<(), String> {
        let server = Server::start(ServerOptions {
            save: self.store.save().to_path_buf(),
            gm_dir: self.store.dir().to_path_buf(),
            registry,
            endpoint: None,
            limits: crate::live::Limits::default(),
        })
        .map_err(|e| format!("can't open the live socket: {e}"))?;
        self.server = Some(server);
        Ok(())
    }

    pub fn server(&self) -> Option<&Server> {
        self.server.as_ref()
    }

    /// Watchers streaming now (one that hung up counts until the next serve).
    pub fn watchers(&self) -> usize {
        self.watchers.len()
    }

    pub fn host(&self) -> &ScriptHost {
        &self.host
    }

    pub fn state(&self) -> &GmState {
        &self.state
    }

    pub fn store(&self) -> &GmStore {
        &self.store
    }

    /// Writes `state.json` if anything changed.
    pub fn flush(&mut self) {
        if self.dirty {
            match self.store.write(&self.state) {
                Ok(()) => self.dirty = false,
                Err(e) => self.notes.push(e),
            }
        }
    }

    /// Stages a pack whose copy is already in the store. `report` must be accepted.
    pub fn stage(
        &mut self,
        rel: &Path,
        report: ValidationReport,
        source: &str,
        calm: bool,
    ) -> Result<StagedPack, (u8, String)> {
        let entry = stage_entry(&self.store, &mut self.state, rel, report, source, |stage| {
            if calm {
                format!("staged as {stage}; the player is at a calm moment, so it applies now")
            } else {
                format!("staged as {stage}; it applies at the next calm moment (docked or paused)")
            }
        })?;
        self.dirty = true;
        self.flush();
        self.broadcast(&json!({
            "type": "gm", "what": "staged", "stage": entry.stage,
            "pack": entry.pack, "revision": entry.revision,
        }));
        Ok(entry)
    }

    /// Applies staged packs. Call at a calm moment (or on load), between ticks. Each
    /// one gets a snapshot first and applies only once the snapshot is written and the
    /// world is still the one it holds: for a [`SnapshotTicket::Pending`] snapshot that
    /// is a later call. A failed snapshot leaves the pack staged until the next calm
    /// moment. Returns what happened.
    pub fn apply_staged(&mut self, sim: &mut Sim, snaps: &mut dyn Snapshots) -> Vec<Value> {
        let mut out = Vec::new();
        self.poll_snapshots(sim, &mut out);
        let waiting: Vec<StagedPack> = self
            .state
            .staged
            .iter()
            .filter(|s| {
                !self.snapshotting.iter().any(|p| p.stage == s.stage)
                    && !self.snapshot_failed.contains_key(&s.stage)
            })
            .cloned()
            .collect();
        for s in waiting {
            // Cheap checks first: a pack that can't apply gets no snapshot.
            if let Err(reason) = self.quick_check(sim, &s) {
                self.quarantine(sim, &s.stage, reason, None, &mut out);
                continue;
            }
            let label = format!("before {} {} ({})", s.pack, short(&s.revision), s.stage);
            let at = moment(sim);
            match snaps.snapshot(sim.state(), &label) {
                SnapshotTicket::Done(Ok(id)) => self.apply_one(sim, &s.stage, id, &mut out),
                SnapshotTicket::Done(Err(e)) => self.snapshot_failed(&s.stage, e, &mut out),
                SnapshotTicket::Pending(rx) => self.snapshotting.push(PendingSnapshot {
                    stage: s.stage,
                    at,
                    rx,
                }),
            }
        }
        self.flush();
        out
    }

    /// Snapshots that finished: their pack applies if the player is still calm and the
    /// world hasn't moved on since (otherwise it is snapshotted again at the next calm
    /// moment).
    fn poll_snapshots(&mut self, sim: &mut Sim, out: &mut Vec<Value>) {
        let mut done = Vec::new();
        self.snapshotting.retain(|p| match p.rx.try_recv() {
            Ok(r) => {
                done.push((p.stage.clone(), p.at, r));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => {
                done.push((p.stage.clone(), p.at, Err("the save thread stopped".into())));
                false
            }
        });
        for (stage, at, r) in done {
            match r {
                Ok(id) if is_calm(sim) && moment(sim) == at => {
                    self.apply_one(sim, &stage, id, out);
                }
                Ok(_) => {}
                Err(e) => self.snapshot_failed(&stage, e, out),
            }
        }
        self.flush();
    }

    fn snapshot_failed(&mut self, stage: &str, e: String, out: &mut Vec<Value>) {
        self.notes.push(format!(
            "snapshot before {stage}: {e}; it stays staged until the next calm moment"
        ));
        let item = json!({ "type": "gm", "what": "snapshot_failed", "stage": stage, "error": e });
        self.broadcast(&item);
        out.push(item);
        self.snapshot_failed.insert(stage.to_owned(), e);
    }

    /// The checks that need no snapshot: not rolled back, and the copy is intact.
    fn quick_check(&self, sim: &Sim, s: &StagedPack) -> Result<(), String> {
        if retired(sim.world(), &s.pack, &s.revision) {
            return Err("this revision was rolled back, and a rolled-back revision never runs again; change the pack and propose the new revision".to_owned());
        }
        self.store.read_copy(&s.dir, &s.revision).map(|_| ())
    }

    fn quarantine(
        &mut self,
        sim: &Sim,
        stage: &str,
        reason: String,
        report: Option<ValidationReport>,
        out: &mut Vec<Value>,
    ) {
        let Some(i) = self.state.staged.iter().position(|s| s.stage == stage) else {
            return;
        };
        let s = self.state.staged.remove(i);
        self.dirty = true;
        let q = QuarantinedPack {
            stage: s.stage,
            pack: s.pack,
            revision: s.revision,
            tick: sim.tick(),
            reason: format!("failed its checks at the calm moment: {reason}"),
            report,
        };
        let item = json!({ "type": "gm", "what": "quarantined", "stage": q.stage,
            "pack": q.pack, "revision": q.revision, "reason": q.reason });
        self.broadcast(&item);
        out.push(item);
        self.state.quarantined.push(q);
    }

    /// Applies a staged pack whose snapshot `snapshot` holds the world as it is now.
    fn apply_one(&mut self, sim: &mut Sim, stage: &str, snapshot: i64, out: &mut Vec<Value>) {
        let Some(s) = self.state.staged.iter().find(|s| s.stage == stage).cloned() else {
            return;
        };
        if let Err(reason) = self.quick_check(sim, &s) {
            self.quarantine(sim, stage, reason, None, out);
            return;
        }
        let report = self
            .host
            .load(&self.store.abs(&s.dir), sim.world(), sim.history());
        if !report.accepted() {
            let why = report
                .errors()
                .next()
                .map_or("its checks failed".to_owned(), |d| d.message.clone());
            self.quarantine(sim, stage, why, Some(report), out);
            return;
        }
        self.state.staged.retain(|x| x.stage != stage);
        self.dirty = true;
        let mut batch = CommandBatch::new(Cause::script(
            s.pack.clone(),
            s.revision.clone(),
            format!("the game master applied {} ({})", s.name, s.stage),
        ));
        if let Some(pack) = self.host.pack(&s.pack) {
            for b in galaxy_script::host::install_batches(&pack, sim.world()) {
                batch.commands.extend(b.commands);
            }
        }
        batch.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: APPLIED,
                subjects: Subjects {
                    other: AnyId::new(s.pack.as_str()).ok(),
                    ..Subjects::default()
                },
                amount: None,
                note: applied_note(&s.pack, &s.revision, &s.stage),
            }),
        });
        sim.submit(batch);
        let item = json!({ "type": "gm", "what": "applied", "stage": s.stage,
            "pack": s.pack, "revision": s.revision, "snapshot": snapshot,
            "tick": sim.tick(), "change": "pending: known when the batch commits next tick" });
        self.broadcast(&item);
        out.push(item);
        self.state.applied.push(AppliedPack {
            stage: s.stage,
            pack: s.pack,
            revision: s.revision,
            name: s.name,
            dir: s.dir,
            source: s.source,
            tick: sim.tick(),
            snapshot: Some(snapshot),
            change: None,
            report: s.report,
        });
    }

    /// Fills in change IDs from `gm_change_applied` facts after `after`.
    fn scan_facts(&mut self, sim: &Sim, after: Option<FactId>) -> Vec<Value> {
        let facts = sim.history().facts();
        let from = facts.partition_point(|f| Some(f.id) <= after);
        let mut items = Vec::new();
        for f in &facts[from..] {
            if f.kind == APPLIED
                && let FactPayload::Note(note) = &f.payload
                && let Some(a) = self.state.applied.iter_mut().find(|a| {
                    a.change.is_none() && applied_note(&a.pack, &a.revision, &a.stage) == *note
                })
            {
                a.change = Some(f.cause);
                self.dirty = true;
                items.push(json!({ "type": "gm", "what": "change", "stage": a.stage,
                    "pack": a.pack, "revision": a.revision, "change": f.cause, "fact": f.id }));
            }
        }
        items
    }

    /// After ticks: change IDs and stream items for watchers.
    pub fn after_tick(&mut self, sim: &Sim) {
        let seen = self.seen_fact;
        let items = self.scan_facts(sim, seen);
        let facts = sim.history().facts();
        let from = facts.partition_point(|f| Some(f.id) <= seen);
        if !self.watchers.is_empty() {
            for f in &facts[from..] {
                self.broadcast(&json!({ "type": "fact", "fact": f }));
            }
            let talk: Vec<Value> = sim
                .world()
                .talk
                .lines
                .range(TalkId(self.seen_talk + 1)..)
                .map(|(_, l)| json!({ "type": "talk", "line": l }))
                .collect();
            for t in talk {
                self.broadcast(&t);
            }
        }
        self.seen_fact = sim.history().last_fact_id().or(seen);
        self.seen_talk = sim.world().talk.last;
        for i in items {
            self.broadcast(&i);
        }
        for e in self.host.take_events() {
            let v = match &e {
                HostEvent::Aborted { .. } | HostEvent::Quarantined { .. } => {
                    json!({ "type": "script", "event": e })
                }
                _ => continue,
            };
            self.trouble.push_back(v.clone());
            if self.trouble.len() > TROUBLE_LOG {
                self.trouble.pop_front();
            }
            self.broadcast(&v);
        }
        let calm = is_calm(sim);
        if self.calm != Some(calm) {
            self.calm = Some(calm);
            if !calm {
                // Failed snapshots are tried again at the next calm moment.
                self.snapshot_failed.clear();
            }
            self.broadcast(
                &json!({ "type": "calm", "calm": calm, "paused": sim.is_paused(),
                "docked_at": docked_at(sim.world()), "tick": sim.tick() }),
            );
        }
        self.flush();
    }

    fn broadcast(&mut self, item: &Value) {
        self.watchers.retain_mut(|w| w.send(item));
    }

    /// Between frames: answers waiting requests, and at a calm moment applies staged
    /// packs and works the inbox. Returns requests the app answers itself (`shot`).
    pub fn serve(&mut self, sim: &mut Sim, snaps: &mut dyn Snapshots) -> Vec<Incoming> {
        self.after_tick(sim);
        // Watchers that hung up free their places even when nothing is broadcast.
        self.watchers.retain(StreamSink::is_open);
        let mut left = Vec::new();
        for _ in 0..MAX_REQUESTS_PER_SERVE {
            let Some(req) = self.server.as_ref().and_then(Server::poll) else {
                break;
            };
            if let Some(req) = self.handle(req, sim) {
                left.push(req);
            }
        }
        if is_calm(sim) {
            if !self.state.staged.is_empty() {
                self.apply_staged(sim, snaps);
            }
            self.work_inbox(sim);
        } else {
            // Snapshots that finish now are dropped: the moment has passed.
            self.poll_snapshots(sim, &mut Vec::new());
            self.poll_inbox(sim);
        }
        left
    }

    fn handle(&mut self, req: Incoming, sim: &mut Sim) -> Option<Incoming> {
        let op = req.op.clone();
        if matches!(op, Op::Watch { .. }) {
            // The cap counts only watchers still connected.
            self.watchers.retain(StreamSink::is_open);
        }
        let result: Result<Value, WireError> = match &op {
            Op::Hello {} => Ok(json!({
                "protocol": crate::live::PROTOCOL,
                "game": "lantern-reach",
                "save": self.store.save(),
                "gm_dir": self.store.dir(),
                "pid": std::process::id(),
                "tick": sim.tick(),
                "paused": sim.is_paused(),
                "docked_at": docked_at(sim.world()),
                "calm": is_calm(sim),
                "staged": self.state.staged.len(),
                "applied": self.state.applied.len(),
            })),
            Op::StateGet { .. } | Op::StateQuery(_) | Op::HistoryQuery(_) | Op::Digest { .. } => {
                self.read(req, sim);
                return None;
            }
            Op::Status {} => Ok(self.status(sim)),
            Op::Fork { to } => {
                if to.exists() {
                    Err(WireError::input(format!("{} already exists", to.display())))
                } else {
                    let state = sim.state().clone();
                    let running = self.running_dirs(sim.world());
                    let to = to.clone();
                    let _ = std::thread::Builder::new()
                        .name("gm-fork".into())
                        .spawn(move || {
                            let r = Save::create(&to, &state)
                                .map(|_| {
                                    json!({
                                        "path": to,
                                        "tick": state.world.meta.tick,
                                        "hash": galaxy_core::state_hash(&state),
                                        "running": running.iter().map(|(p, d)| json!({ "pack": p, "dir": d })).collect::<Vec<_>>(),
                                    })
                                })
                                .map_err(|e| WireError::runtime(e.to_string()));
                            req.respond(r);
                        });
                    return None;
                }
            }
            Op::Stage { dir, report } => {
                let calm = is_calm(sim);
                self.stage(dir, (**report).clone(), "cli", calm)
                    .map(|s| {
                        json!({ "stage": s.stage, "pack": s.pack, "revision": s.revision,
                            "calm": calm, "report": s.report })
                    })
                    .map_err(WireError::from)
            }
            Op::Rollback { change } => {
                let r = rollback_change(sim, &self.store, &self.state, change);
                if let Ok(v) = &r {
                    self.broadcast(&json!({ "type": "gm", "what": "rolled_back", "change": change, "rollback": v }));
                }
                r.map_err(WireError::from)
            }
            Op::Watch { since } if self.watchers.len() < MAX_WATCHERS => {
                let ack = json!({ "watching": true, "tick": sim.tick(), "calm": is_calm(sim) });
                let mut sink = req.into_stream(ack);
                if let Some(since) = since {
                    let facts = sim.history().facts();
                    let from = facts.partition_point(|f| f.id <= *since);
                    let replay = &facts[from..];
                    let skip = replay.len().saturating_sub(MAX_REPLAY);
                    for f in &replay[skip..] {
                        sink.send(&json!({ "type": "fact", "fact": f }));
                    }
                }
                self.watchers.push(sink);
                return None;
            }
            Op::Watch { .. } => Err(WireError::new(
                1,
                "busy",
                format!("{MAX_WATCHERS} watchers are already connected"),
            )),
            Op::Shot { .. } => return Some(req),
        };
        req.respond(result);
        None
    }

    /// Hands a read to the read thread, with a copy of the state as it is now.
    fn read(&mut self, req: Incoming, sim: &Sim) {
        if self.reader.is_none() {
            self.reader = Reader::start();
        }
        let Some(reader) = &mut self.reader else {
            req.respond(Err(WireError::runtime("can't start the read thread")));
            return;
        };
        let at = moment(sim);
        let state = match &reader.copy {
            Some((when, s)) if *when == at => s.clone(),
            _ => {
                let s = Arc::new(sim.state().clone());
                reader.copy = Some((at, s.clone()));
                s
            }
        };
        match reader.tx.try_send((state, req)) {
            Ok(()) => {}
            Err(TrySendError::Full((_, req))) => req.respond(Err(WireError::new(
                1,
                "busy",
                "the game is still answering earlier reads; try again",
            ))),
            Err(TrySendError::Disconnected((_, req))) => {
                self.reader = None;
                req.respond(Err(WireError::runtime("the read thread stopped")));
            }
        }
    }

    /// The packs that run now: `(pack, absolute copy folder)`.
    pub fn running_dirs(&self, world: &WorldState) -> Vec<(PackId, PathBuf)> {
        running(&self.state, &self.store, world)
    }

    /// `gm status`.
    pub fn status(&self, sim: &Sim) -> Value {
        let mut v = status_value(&self.state, &self.store, sim.world(), Some(&self.host));
        v["tick"] = json!(sim.tick());
        v["calm"] = json!(is_calm(sim));
        v["live"] = json!(self.server.is_some());
        v["inbox"]["validating"] = json!(self.inbox.as_ref().map(|j| j.name.clone()));
        if let Some(staged) = v["staged"].as_array_mut() {
            for s in staged {
                let stage = s["stage"].as_str().unwrap_or_default().to_owned();
                if self.snapshotting.iter().any(|p| p.stage == stage) {
                    s["snapshot"] = json!("writing");
                } else if let Some(e) = self.snapshot_failed.get(&stage) {
                    s["snapshot"] = json!(format!("failed: {e}"));
                }
            }
        }
        v["script_trouble"] = json!(self.trouble.iter().collect::<Vec<_>>());
        v
    }

    fn work_inbox(&mut self, sim: &Sim) {
        self.poll_inbox(sim);
        if self.inbox.is_some() {
            return;
        }
        let Some(exe) = self.opts.exe.clone() else {
            return;
        };
        let Some((name, src)) = inbox_entries(&self.store.inbox()).into_iter().next() else {
            return;
        };
        let claimed_root = self.store.claimed();
        if let Err(e) = std::fs::create_dir_all(&claimed_root) {
            self.notes.push(format!("{}: {e}", claimed_root.display()));
            return;
        }
        let mut claimed = claimed_root.join(&name);
        let mut n = 2;
        while claimed.exists() {
            claimed = claimed_root.join(format!("{name}-{n}"));
            n += 1;
        }
        // Claim it: a rename, so two games never take the same entry. The marker goes:
        // it isn't part of the pack.
        if std::fs::rename(&src, &claimed).is_err() {
            return;
        }
        if let Err(e) = std::fs::remove_file(claimed.join(READY)) {
            self.notes
                .push(format!("{}: {e}", claimed.join(READY).display()));
        }
        let state = sim.state().clone();
        let running = self.running_dirs(sim.world());
        let opts = ProposeOptions {
            exe,
            analyzer: self.opts.analyzer.clone(),
            require_typecheck: self.opts.analyzer.is_some(),
            rules: self.opts.rules.clone(),
        };
        let (tx, rx) = mpsc::channel();
        let dir = claimed.clone();
        let spawned = std::thread::Builder::new()
            .name("gm-inbox".into())
            .spawn(move || {
                let report = (|| {
                    let tmp = tempfile::Builder::new()
                        .prefix("galaxy-inbox-")
                        .tempdir()
                        .map_err(|e| e.to_string())?;
                    let base = tmp.path().join("base.sqlite");
                    Save::create(&base, &state).map_err(|e| e.to_string())?;
                    Ok::<_, String>(propose::validate(&dir, &base, &running, &opts))
                })();
                let report = report.unwrap_or_else(|e| failed_report(&e));
                let _ = tx.send(report);
            });
        if spawned.is_ok() {
            self.broadcast(&json!({ "type": "gm", "what": "inbox_claimed", "entry": name }));
            self.inbox = Some(InboxJob { name, claimed, rx });
        }
    }

    fn poll_inbox(&mut self, sim: &Sim) {
        let Some(job) = &self.inbox else { return };
        let report = match job.rx.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => failed_report("the inbox validation stopped"),
        };
        let job = self.inbox.take().expect("checked above");
        let mut result = json!({ "entry": job.name, "claimed": job.claimed });
        let report = if report.accepted() {
            let staged = galaxy_script::read_pack(&job.claimed)
                .map_err(|d| {
                    d.first()
                        .map_or("unreadable pack".to_owned(), |d| d.message.clone())
                })
                .and_then(|pack| {
                    if pack.revision != report.revision {
                        return Err("the entry changed while it was being validated".to_owned());
                    }
                    self.store.import(&pack)
                })
                .map_err(|e| (1, e))
                .and_then(|rel| {
                    self.stage(
                        &rel,
                        report.clone(),
                        &format!("inbox:{}", job.name),
                        is_calm(sim),
                    )
                });
            match staged {
                Ok(s) => {
                    result["stage"] = json!(s.stage);
                    s.report
                }
                Err((_, e)) => {
                    let mut r = report;
                    r.steps.push(propose::stage_step(vec![propose::stage_error(
                        "stage_failed",
                        e,
                    )]));
                    propose::settle(&mut r);
                    r
                }
            }
        } else {
            let mut r = report;
            propose::skip_stage(&mut r);
            r
        };
        result["report"] = serde_json::to_value(&report).expect("reports serialize");
        let out = self.store.inbox().join(format!("{}.result.json", job.name));
        if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&result).expect("json")) {
            self.notes.push(format!("{}: {e}", out.display()));
        }
        self.broadcast(
            &json!({ "type": "gm", "what": "inbox_result", "entry": job.name,
            "status": report.status }),
        );
    }
}

/// The note of the `gm_change_applied` fact that applies `stage`: what ties a save's
/// history to an entry in `state.json`.
pub fn applied_note(pack: &PackId, revision: &str, stage: &str) -> String {
    format!("{pack} revision {revision} applied ({stage})")
}

/// Matches `state.json` to what the save holds (see the module docs): applied entries
/// whose apply batch is neither committed nor queued in the save go back to staged,
/// staged ones the save already applied move to applied, and change IDs are the
/// save's. Returns whether anything changed, and notes.
pub fn reconcile(state: &mut GmState, sim: &Sim) -> (bool, Vec<String>) {
    // Apply facts the save committed, and apply batches it holds queued, by note.
    let mut committed: BTreeMap<String, (CauseId, Tick)> = BTreeMap::new();
    for f in sim.history().facts().iter().filter(|f| f.kind == APPLIED) {
        if let FactPayload::Note(note) = &f.payload
            && sim
                .history()
                .cause(f.cause)
                .is_some_and(|c| matches!(c.cause.kind, CauseKind::Script { .. }))
        {
            committed.entry(note.clone()).or_insert((f.cause, f.tick));
        }
    }
    let queued: BTreeSet<String> = sim
        .state()
        .kernel
        .pending
        .iter()
        .filter(|b| matches!(b.cause.kind, CauseKind::Script { .. }))
        .flat_map(|b| &b.commands)
        .filter_map(|c| match c {
            Command::RecordFact { fact } if fact.kind == APPLIED => Some(fact.note.clone()),
            _ => None,
        })
        .collect();
    let mut changed = false;
    let mut notes = Vec::new();
    let mut applied = Vec::new();
    let mut staged = Vec::new();
    for mut a in std::mem::take(&mut state.applied) {
        let note = applied_note(&a.pack, &a.revision, &a.stage);
        if let Some((c, _)) = committed.get(&note) {
            changed |= a.change != Some(*c);
            a.change = Some(*c);
            applied.push(a);
        } else if queued.contains(&note) {
            changed |= a.change.take().is_some();
            applied.push(a);
        } else {
            changed = true;
            notes.push(format!(
                "{} ({}) was applied after the save was last written; it applies again at the next calm moment",
                a.pack, a.stage
            ));
            staged.push(StagedPack {
                stage: a.stage,
                pack: a.pack,
                revision: a.revision,
                name: a.name,
                dir: a.dir,
                source: a.source,
                report: a.report,
            });
        }
    }
    for s in std::mem::take(&mut state.staged) {
        let note = applied_note(&s.pack, &s.revision, &s.stage);
        let done = committed.get(&note).copied();
        if done.is_none() && !queued.contains(&note) {
            staged.push(s);
            continue;
        }
        changed = true;
        applied.push(AppliedPack {
            stage: s.stage,
            pack: s.pack,
            revision: s.revision,
            name: s.name,
            dir: s.dir,
            source: s.source,
            tick: done.map_or(sim.tick(), |(_, t)| t),
            snapshot: None,
            change: done.map(|(c, _)| c),
            report: s.report,
        });
    }
    state.applied = applied;
    state.staged = staged;
    (changed, notes)
}

/// Adds a staged entry for a copy already in the store, appending the `stage` step
/// (`note` says when it applies, given the stage ID). `report` must be accepted.
pub fn stage_entry(
    store: &GmStore,
    state: &mut GmState,
    rel: &Path,
    mut report: ValidationReport,
    source: &str,
    note: impl FnOnce(&str) -> String,
) -> Result<StagedPack, (u8, String)> {
    if !report.accepted() {
        return Err((5, "only an accepted report can be staged".into()));
    }
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err((
            4,
            format!("{} isn't a folder in the pack store", rel.display()),
        ));
    }
    let pack = store.read_copy(rel, &report.revision).map_err(|e| (4, e))?;
    if pack.manifest.id != report.pack {
        return Err((
            4,
            format!(
                "the copy is {}, the report {}",
                pack.manifest.id, report.pack
            ),
        ));
    }
    if state
        .staged
        .iter()
        .any(|s| s.pack == report.pack && s.revision == report.revision)
    {
        return Err((
            4,
            format!(
                "{} {} is already staged",
                report.pack,
                short(&report.revision)
            ),
        ));
    }
    let stage = state.next_stage();
    report
        .steps
        .push(propose::stage_step(vec![propose::stage_info(
            "staged",
            note(&stage),
        )]));
    let entry = StagedPack {
        stage,
        pack: report.pack.clone(),
        revision: report.revision.clone(),
        name: pack.manifest.name.clone(),
        dir: rel.to_path_buf(),
        source: source.to_owned(),
        report,
    };
    state.staged.push(entry.clone());
    Ok(entry)
}

fn failed_report(why: &str) -> ValidationReport {
    use galaxy_contract::pack::{Diagnostic, Severity, StepOutcome, StepReport, ValidationStep};
    ValidationReport {
        pack: PackId::from_static("unknown"),
        revision: String::new(),
        status: ReportStatus::Rejected,
        steps: vec![StepReport {
            step: ValidationStep::Sandbox,
            outcome: StepOutcome::Failed,
            diagnostics: vec![Diagnostic {
                severity: Severity::Error,
                code: "sandbox_failed".into(),
                message: why.to_owned(),
                location: None,
            }],
        }],
    }
}

/// Pack folders waiting in an inbox, by name: complete ones, with their [`READY`]
/// marker written. Hidden folders (`.name`) are never looked at.
pub fn inbox_entries(inbox: &Path) -> Vec<(String, PathBuf)> {
    inbox_folders(inbox, true)
}

/// Folders in an inbox without their [`READY`] marker yet: still being copied.
pub fn inbox_incomplete(inbox: &Path) -> Vec<String> {
    inbox_folders(inbox, false)
        .into_iter()
        .map(|(n, _)| n)
        .collect()
}

fn inbox_folders(inbox: &Path, ready: bool) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(inbox) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join(READY).is_file() == ready)
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            (!name.starts_with('.')).then_some((name, p))
        })
        .collect();
    out.sort();
    out
}

fn short(revision: &str) -> &str {
    &revision[..revision.len().min(12)]
}

/// The packs that run on a world: the newest applied revision of each pack, unless it
/// was rolled back.
pub fn running(state: &GmState, store: &GmStore, world: &WorldState) -> Vec<(PackId, PathBuf)> {
    state
        .current()
        .into_iter()
        .filter(|a| !retired(world, &a.pack, &a.revision))
        .map(|a| (a.pack.clone(), store.abs(&a.dir)))
        .collect()
}

/// `gm status` for a world, with or without a running host.
pub fn status_value(
    state: &GmState,
    store: &GmStore,
    world: &WorldState,
    host: Option<&ScriptHost>,
) -> Value {
    let current: Vec<&AppliedPack> = state.current();
    let applied: Vec<Value> = state
        .applied
        .iter()
        .map(|a| {
            let is_current = current.iter().any(|c| c.stage == a.stage);
            let health = world.packs.get(&a.pack);
            let quarantined: Vec<String> = health
                .map(|p| {
                    p.health
                        .iter()
                        .filter(|(_, h)| h.revision == a.revision && h.quarantined)
                        .map(|(s, _)| s.clone())
                        .collect()
                })
                .unwrap_or_default();
            let state = if retired(world, &a.pack, &a.revision) {
                "rolled_back"
            } else if !is_current {
                "superseded"
            } else if a.change.is_none() {
                "applying"
            } else if host.is_some_and(|h| h.revision(&a.pack).as_deref() != Some(&a.revision)) {
                "not_loaded"
            } else {
                "running"
            };
            json!({
                "stage": a.stage, "pack": a.pack, "revision": a.revision, "name": a.name,
                "state": state, "change": a.change, "snapshot": a.snapshot, "tick": a.tick,
                "source": a.source, "dir": store.abs(&a.dir),
                "quarantined_scripts": quarantined,
            })
        })
        .collect();
    let staged: Vec<Value> = state
        .staged
        .iter()
        .map(|s| {
            json!({ "stage": s.stage, "pack": s.pack, "revision": s.revision, "name": s.name,
                "source": s.source, "dir": store.abs(&s.dir) })
        })
        .collect();
    let mut quarantined: Vec<Value> = state
        .quarantined
        .iter()
        .map(|q| {
            json!({ "stage": q.stage, "pack": q.pack, "revision": q.revision,
                "tick": q.tick, "reason": q.reason })
        })
        .collect();
    for (pack, p) in &world.packs {
        for (script, h) in &p.health {
            if h.quarantined {
                quarantined.push(json!({ "pack": pack, "script": script, "revision": h.revision,
                    "aborts": h.aborts, "reason": "the script was quarantined after repeated aborted calls" }));
            }
        }
    }
    let waiting: Vec<String> = inbox_entries(&store.inbox())
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let incomplete = inbox_incomplete(&store.inbox());
    json!({
        "save": store.save(),
        "gm_dir": store.dir(),
        "staged": staged,
        "applied": applied,
        "quarantined": quarantined,
        "inbox": { "dir": store.inbox(), "waiting": waiting, "incomplete": incomplete },
    })
}

/// In-world rollback of a change: `cause.N` (any cause; a script cause rolls back
/// everything its pack revision did) or `stage.N`.
pub fn rollback_change(
    sim: &mut Sim,
    store: &GmStore,
    state: &GmState,
    change: &str,
) -> Result<Value, (u8, String)> {
    let (causes, script): (Vec<CauseId>, Option<(PackId, String)>) =
        if let Ok(c) = change.parse::<CauseId>() {
            let rc = sim
                .history()
                .cause(c)
                .ok_or((4, format!("no {c} in this save")))?;
            match &rc.cause.kind {
                CauseKind::Script { pack, revision } => {
                    let (p, r) = (pack.clone(), revision.clone());
                    (causes_of(sim, &p, &r), Some((p, r)))
                }
                _ => (vec![c], None),
            }
        } else if let Some(a) = state.find_applied(change) {
            (
                causes_of(sim, &a.pack, &a.revision),
                Some((a.pack.clone(), a.revision.clone())),
            )
        } else {
            return Err((
                4,
                format!("`{change}` is neither a cause (cause.N) nor an applied change (stage.N)"),
            ));
        };
    if causes.is_empty() {
        return Err((
            4,
            format!("{change} hasn't written anything yet; nothing to roll back"),
        ));
    }
    let done = &sim.state().kernel.rolled_back;
    if causes.iter().all(|c| done.contains(c)) {
        return Err((4, format!("{change} was already rolled back")));
    }
    let line = script.as_ref().and_then(|(pack, revision)| {
        state
            .applied
            .iter()
            .find(|a| &a.pack == pack && &a.revision == revision)
            .and_then(|a| store.read_copy(&a.dir, revision).ok())
            .and_then(|p| p.manifest.rollback)
    });
    let outcome = galaxy_script::rollback(sim, &causes, line.as_ref());
    Ok(json!({
        "change": change,
        "pack": script.as_ref().map(|(p, _)| p),
        "revision": script.as_ref().map(|(_, r)| r),
        "causes": causes,
        "outcome": outcome,
        "applies": "at the end of the next tick",
    }))
}
