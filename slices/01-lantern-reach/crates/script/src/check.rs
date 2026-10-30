//! The fast validation pipeline: `galaxy pack check` and dev hot reload.
//!
//! Steps, in order, each writing into one [`ValidationReport`]:
//!
//! 1. `manifest`: `manifest.toml` and data files against their schemas; files exist;
//!    `uses` are known and implemented.
//! 2. `types`: strict Luau typecheck against `galaxy.d.luau` (needs `luau-analyze`).
//!    Even without the analyzer, every file must start with `--!strict` and may not
//!    contain `!nocheck` or `!nonstrict` anywhere: source can't turn the strict check
//!    off.
//!
//! Every step that reads Luau reads exactly the files `require` can load
//! ([`Pack::loadable`]): the script roots, never `tests/`.
//! 3. `hooks`: every `ctx` hook a script calls is declared in `uses`; each script
//!    loads and defines exactly the handlers its manifest lists.
//! 4. `references`: every ID a script or the manifest names exists in the world or is
//!    created by this pack.
//! 5. `invariants`: no clock or timer APIs, no unseeded randomness, every line cites a
//!    fact, shocks are paired with opportunities.
//! 6. `smoke`: each handler runs once on a sample event against the world, within
//!    budget, on a VM of its own (the VM the check returns has run no handler); its
//!    writes are checked like the kernel checks them, on a copy, and thrown away.
//!
//! The game-master path appends `canon`, `sandbox` (fork the save, fast-forward,
//! compare), `legibility` and `stage` to the same report later.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use galaxy_contract::events::{Event, EventEnvelope, EventKind};
use galaxy_contract::history::{Fact, FactPayload};
use galaxy_contract::pack::{
    Diagnostic, ReportStatus, Severity, StepOutcome, StepReport, ValidationReport, ValidationStep,
};
use galaxy_contract::prelude::*;
use galaxy_core::History;
use galaxy_core::rng::RngBank;

use crate::pack::{self, Pack, diag, error};
use crate::scan;
use crate::vm::{PackVm, Target, View};

/// What the checks run against.
#[derive(Clone, Copy, Default)]
pub struct CheckOptions<'a> {
    /// The world IDs resolve against and the smoke run reads. Without one, those
    /// steps are skipped.
    pub world: Option<&'a WorldState>,
    pub history: Option<&'a History>,
    /// `luau-analyze`. Without it the typecheck is skipped with a warning, unless
    /// `require_typecheck` is set.
    pub analyzer: Option<&'a Path>,
    pub require_typecheck: bool,
}

/// A checked pack: the report, and the pack and its loaded VM when they got that far.
pub struct Checked {
    pub report: ValidationReport,
    pub pack: Option<Rc<Pack>>,
    pub vm: Option<PackVm>,
}

impl Checked {
    pub fn accepted(&self) -> bool {
        self.report.accepted()
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

fn skipped(step: ValidationStep, why: &str) -> StepReport {
    StepReport {
        step,
        outcome: StepOutcome::Skipped,
        diagnostics: vec![diag(Severity::Info, "skipped", why, None, None)],
    }
}

fn finish(pack: PackId, revision: String, steps: Vec<StepReport>) -> ValidationReport {
    let rejected = steps.iter().any(|s| s.outcome == StepOutcome::Failed);
    ValidationReport {
        pack,
        revision,
        status: if rejected {
            ReportStatus::Rejected
        } else {
            ReportStatus::Accepted
        },
        steps,
    }
}

/// The pack's ID from a manifest that failed to parse, if it can be read at all.
fn guess_id(dir: &Path) -> PackId {
    std::fs::read_to_string(dir.join("manifest.toml"))
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| {
            t.get("id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or_else(|| PackId::from_static("unknown"))
}

const LATER: [ValidationStep; 5] = [
    ValidationStep::Types,
    ValidationStep::Hooks,
    ValidationStep::References,
    ValidationStep::Invariants,
    ValidationStep::Smoke,
];

/// Reads and checks a pack folder.
pub fn check_dir(dir: &Path, opts: CheckOptions<'_>) -> Checked {
    match pack::read_pack(dir) {
        Ok(p) => check_pack(Rc::new(p), opts),
        Err(diags) => {
            let mut steps = vec![step(ValidationStep::Manifest, diags)];
            steps.extend(
                LATER
                    .iter()
                    .map(|s| skipped(*s, "the manifest step failed")),
            );
            Checked {
                report: finish(guess_id(dir), String::new(), steps),
                pack: None,
                vm: None,
            }
        }
    }
}

/// Checks a pack that was read successfully.
pub fn check_pack(pack: Rc<Pack>, opts: CheckOptions<'_>) -> Checked {
    let mut steps = vec![step(ValidationStep::Manifest, pack.warnings.clone())];

    // 2. Types
    let mut modes = crate::analyzer::mode_directives(&pack);
    steps.push(match opts.analyzer {
        Some(a) => match crate::analyzer::typecheck(&pack, a) {
            Ok(mut d) => {
                modes.append(&mut d);
                step(ValidationStep::Types, modes)
            }
            Err(e) => {
                modes.push(error("analyzer", e, None, None));
                step(ValidationStep::Types, modes)
            }
        },
        None if opts.require_typecheck => {
            modes.push(error(
                "no_analyzer",
                "luau-analyze not found: run `cargo xtask fetch-luau`",
                None,
                None,
            ));
            step(ValidationStep::Types, modes)
        }
        None if !modes.is_empty() => step(ValidationStep::Types, modes),
        None => StepReport {
            step: ValidationStep::Types,
            outcome: StepOutcome::Skipped,
            diagnostics: vec![diag(
                Severity::Warning,
                "no_analyzer",
                "luau-analyze not found, strict typecheck skipped: run `cargo xtask fetch-luau`",
                None,
                None,
            )],
        },
    });

    // 3. Hooks (static scan, then load the scripts)
    let scans: Vec<(String, scan::Scan)> = pack
        .loadable_files()
        .map(|(f, src)| (f.to_owned(), scan::scan(f, src)))
        .collect();
    let has_stores = |file: &str| {
        pack.manifest.scripts.iter().any(|s| {
            !s.stores.is_empty()
                && (s.file == file || !pack.manifest.scripts.iter().any(|d| d.file == file))
        })
    };
    let mut hook_diags: Vec<Diagnostic> = scans
        .iter()
        .flat_map(|(f, s)| scan::hook_diagnostics(f, s, &pack.manifest.uses, has_stores(f)))
        .collect();
    let vm = match PackVm::new(pack.clone()) {
        Ok(vm) => Some(vm),
        Err(mut d) => {
            hook_diags.append(&mut d);
            None
        }
    };
    if let Some(vm) = &vm {
        for s in &vm.scripts {
            for k in &s.events {
                let need = galaxy_contract::api::event_use(*k);
                if !pack.manifest.uses.iter().any(|u| u == need) {
                    hook_diags.push(error(
                        "undeclared_hook",
                        format!(
                            "script `{}` handles `on.{}`, which needs `{need}` in `uses`",
                            s.decl.id,
                            k.as_str()
                        ),
                        Some(&s.decl.file),
                        None,
                    ));
                }
            }
        }
    }
    steps.push(step(ValidationStep::Hooks, hook_diags));

    // 4. References
    steps.push(match opts.world {
        Some(world) => step(ValidationStep::References, references(&pack, &scans, world)),
        None => skipped(
            ValidationStep::References,
            "no world to resolve IDs against",
        ),
    });

    // 5. Invariants
    let mut inv: Vec<Diagnostic> = scans
        .iter()
        .flat_map(|(_, s)| s.diagnostics.clone())
        .collect();
    inv.extend(pack::unpaired_shocks(&pack.manifest));
    steps.push(step(ValidationStep::Invariants, inv));

    // 6. Smoke, on a VM of its own: the one returned never ran a handler.
    steps.push(match (&vm, opts.world) {
        (Some(_), Some(world)) => match PackVm::new(pack.clone()) {
            Ok(trial) => step(
                ValidationStep::Smoke,
                smoke(&trial, &scans, world, opts.history),
            ),
            Err(d) => step(ValidationStep::Smoke, d),
        },
        (None, _) => skipped(ValidationStep::Smoke, "the scripts didn't load"),
        (_, None) => skipped(ValidationStep::Smoke, "no world to run against"),
    });

    let report = finish(pack.manifest.id.clone(), pack.revision.clone(), steps);
    Checked {
        report,
        pack: Some(pack),
        vm,
    }
}

/// Does `id` exist in the world?
fn exists(world: &WorldState, id: &str) -> bool {
    match id.split('.').next().unwrap_or("") {
        "station" => world.stations.contains(id),
        "npc" => world.npcs.contains(id),
        "faction" => world.factions.contains(id),
        "region" => world.regions.contains(id),
        "ware" => world.wares.contains(id),
        "rumor" => world.rumors.contains(id),
        "ship" => world.ships.contains(id),
        "lane" => world.lanes.contains(id),
        "hull" => world.hulls.contains(id),
        "field" => world.fields.contains(id),
        "rock_type" => world.rock_types.contains(id),
        _ => true,
    }
}

fn references(pack: &Pack, scans: &[(String, scan::Scan)], world: &WorldState) -> Vec<Diagnostic> {
    let created = pack.created_ids();
    let known = |id: &str| exists(world, id) || created.contains(id);
    let mut out = Vec::new();
    for (file, s) in scans {
        for (id, line) in &s.ids {
            if !known(id) {
                out.push(error(
                    "unknown_id",
                    format!("`{id}` doesn't exist and isn't created by this pack (add it to `creates` or the pack's data)"),
                    Some(file),
                    Some(*line),
                ));
            }
        }
    }
    let at = Some("manifest.toml");
    if let Some(r) = &pack.manifest.rollback {
        for id in r
            .speaker
            .iter()
            .map(|s| s.to_string())
            .chain(r.place.iter().map(|p| p.to_string()))
        {
            if !known(&id) {
                out.push(error(
                    "unknown_id",
                    format!("rollback line names `{id}`, which doesn't exist"),
                    at,
                    None,
                ));
            }
        }
    }
    for npc in &pack.data.npcs {
        for id in npc
            .faction
            .iter()
            .map(|f| f.to_string())
            .chain(npc.location.iter().map(|l| l.to_string()))
        {
            if !known(&id) {
                out.push(error(
                    "unknown_id",
                    format!("`{}` refers to `{id}`, which doesn't exist", npc.id),
                    Some("data/npcs.toml"),
                    None,
                ));
            }
        }
    }
    out
}

/// The first ID of a kind the pack names that exists, else the world's first.
fn pick<'a>(
    scans: &'a [(String, scan::Scan)],
    kind: &str,
    mut world_first: impl Iterator<Item = &'a str>,
    world: &WorldState,
) -> String {
    scans
        .iter()
        .flat_map(|(_, s)| &s.ids)
        .map(|(id, _)| id.as_str())
        .find(|id| id.starts_with(&format!("{kind}.")) && exists(world, id))
        .or_else(|| world_first.next())
        .unwrap_or("x.sample")
        .to_owned()
}

/// A plausible event of each kind, using IDs the pack names where it can.
fn sample_event(kind: EventKind, scans: &[(String, scan::Scan)], w: &WorldState) -> Option<Event> {
    let ship: ShipId = w
        .player
        .ship
        .clone()
        .or_else(|| w.ships.keys().next().cloned())?;
    let station: StationId = pick(scans, "station", w.stations.keys().map(|k| k.as_str()), w)
        .parse()
        .ok()?;
    let ware: WareId = pick(scans, "ware", w.wares.keys().map(|k| k.as_str()), w)
        .parse()
        .ok()?;
    let rock = w
        .rocks
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| RockId::from_static("rock.sample"));
    let region = w
        .regions
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| RegionId::from_static("region.sample"));
    let hull = w
        .hulls
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| HullId::from_static("hull.sample"));
    Some(match kind {
        EventKind::Dock => Event::Dock { ship, station },
        EventKind::Undock => Event::Undock { ship, station },
        EventKind::Mine => Event::Mine {
            ship,
            rock,
            ware,
            amount: 1,
        },
        EventKind::RockDepleted => Event::RockDepleted {
            rock,
            ship: Some(ship),
        },
        EventKind::Buy => Event::Buy {
            ship,
            station,
            ware,
            quantity: 1,
            unit_price: 1,
        },
        EventKind::Sell => Event::Sell {
            ship,
            station,
            ware,
            quantity: 1,
            unit_price: 1,
        },
        EventKind::Deliver => Event::Deliver {
            ship,
            station,
            ware,
            quantity: 1,
        },
        EventKind::Jump => Event::Jump {
            ship,
            lane: w
                .lanes
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| LaneId::from_static("lane.sample")),
            from: region.clone(),
            to: region,
        },
        EventKind::Scout => Event::Scout { ship, station },
        EventKind::OrderDone => Event::OrderDone {
            ship,
            verb: VerbKind::Wait,
        },
        EventKind::QuestStep => Event::QuestStep {
            quest: QuestId::from_static("quest.sample"),
            step: "sample".into(),
        },
        EventKind::TraitEarned => Event::TraitEarned {
            trait_id: TraitId::from_static("trait.sample"),
        },
        EventKind::Prestige => Event::Prestige {
            from_hull: hull.clone(),
            to_hull: hull,
        },
        EventKind::SessionStart => Event::SessionStart {},
    })
}

/// Runs each handler once. Over-budget and errors are rejections; writes that
/// wouldn't apply are warnings (the sample event may not fit the script).
fn smoke(
    vm: &PackVm,
    scans: &[(String, scan::Scan)],
    world: &WorldState,
    history: Option<&History>,
) -> Vec<Diagnostic> {
    let mut world = world.clone();
    for batch in crate::host::install_batches(&vm.pack, &world) {
        if let Ok(next) = crate::host::preview(&world, &batch) {
            world = next;
        }
    }
    let mut out = Vec::new();
    let mut rng = RngBank::new(world.meta.seed);
    for (i, s) in vm.scripts.iter().enumerate() {
        let mut targets: Vec<(String, Option<EventEnvelope>)> = Vec::new();
        for kind in &s.events {
            let Some(event) = sample_event(*kind, scans, &world) else {
                continue;
            };
            targets.push((
                format!("on.{}", kind.as_str()),
                Some(EventEnvelope {
                    tick: world.meta.tick,
                    fact: FactId(0),
                    cause: CauseId(0),
                    event,
                }),
            ));
        }
        for t in &s.triggers {
            targets.push((format!("after.{}", t.name), None));
        }
        for (handler, envelope) in targets {
            // A history with the sample event recorded, so handlers can cite it.
            let mut facts: Vec<Fact> = history.map(|h| h.facts().to_vec()).unwrap_or_default();
            let causes: Vec<RecordedCause> = history
                .map(|h| h.causes().values().cloned().collect())
                .unwrap_or_default();
            let envelope = envelope.map(|mut e| {
                e.fact = FactId(facts.last().map_or(1, |f| f.id.0 + 1));
                facts.push(Fact {
                    id: e.fact,
                    tick: e.tick,
                    kind: e.event.kind().fact_kind(),
                    cause: e.cause,
                    subjects: e.event.subjects(),
                    amount: e.event.amount(),
                    payload: FactPayload::Event(e.event.clone()),
                });
                e
            });
            let hist = History::from_parts(facts, causes);
            let target = match &envelope {
                Some(e) => Target::Event(e),
                None => Target::Trigger {
                    name: handler.trim_start_matches("after."),
                    fired: 0,
                },
            };
            let view = View {
                world: &world,
                history: &hist,
            };
            match vm.call(i, target, view, &mut rng) {
                Ok(batch) => {
                    if let Some(Err(reason)) = batch.map(|b| crate::host::preview(&world, &b)) {
                        out.push(diag(
                            Severity::Warning,
                            "apply_failed",
                            format!("`{handler}` on a sample event wrote a batch the engine would reject: {reason}"),
                            Some(&s.decl.file),
                            vm.handler_line(i, target),
                        ));
                    }
                }
                Err(abort) => {
                    let mut d = abort.diagnostic();
                    d.message = format!("`{handler}` on a sample event: {}", d.message);
                    if d.location.is_none() {
                        d.location = Some(galaxy_contract::pack::Location {
                            file: s.decl.file.clone(),
                            line: vm.handler_line(i, target),
                            column: None,
                        });
                    }
                    out.push(d);
                }
            }
        }
    }
    out
}

/// Where `cargo xtask fetch-luau` puts the analyzer, if it's there.
pub fn default_analyzer() -> Option<PathBuf> {
    crate::analyzer::find(None)
}
