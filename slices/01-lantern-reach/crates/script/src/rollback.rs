//! In-world rollback by cause, with a compensating line.
//!
//! The kernel inverts every journaled write of the causes (`Sim::queue_rollback`).
//! This adds what the design asks for on top: the pack revisions that made those
//! causes are retired (their handlers stop, so a script whose condition still holds
//! doesn't redo the change at once), and a person says why, in dock talk and in the
//! chronicle, citing the facts the rolled-back change was based on or recorded.

use std::collections::BTreeSet;

use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::RollbackLine;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::{RollbackPlan, Sim};
use serde::Serialize;

/// Most facts a compensating line cites.
const MAX_CITES: usize = 16;

/// What a rollback queued.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RollbackOutcome {
    pub plan: RollbackPlan,
    /// Pack revisions switched off.
    pub retired: Vec<(PackId, String)>,
    /// The facts the compensating lines cite.
    pub cites: Vec<FactId>,
    /// The compensating dock-talk line, if one could be said.
    pub line: Option<TalkLine>,
}

/// Queues the rollback of `causes`. Everything applies at the end of the next tick.
/// `line` is the pack's declared rollback line (`[rollback]` in its manifest).
pub fn rollback(sim: &mut Sim, causes: &[CauseId], line: Option<&RollbackLine>) -> RollbackOutcome {
    let history = sim.history();
    let mut cites = BTreeSet::new();
    let mut summaries = Vec::new();
    let mut retired = BTreeSet::new();
    let mut place = None;
    for &c in causes {
        let Some(rc) = history.cause(c) else { continue };
        summaries.push(rc.cause.summary.clone());
        cites.extend(rc.cause.cites.iter().copied());
        let recorded = HistoryQuery {
            cause: Some(c),
            ..HistoryQuery::default()
        };
        for f in history.find(&recorded) {
            cites.insert(f.id);
            place = place.or_else(|| f.subjects.station.clone());
        }
        if let CauseKind::Script { pack, revision } = &rc.cause.kind {
            retired.insert((pack.clone(), revision.clone()));
        }
    }
    if place.is_none() {
        place = cites
            .iter()
            .find_map(|f| history.fact(*f).and_then(|f| f.subjects.station.clone()));
    }
    let summary = format!("rolled back: {}", summaries.join("; "));
    // The newest facts are the most relevant ones to cite.
    let skip = cites.len().saturating_sub(MAX_CITES);
    let cites: Vec<FactId> = cites.into_iter().skip(skip).collect();
    let plan = sim.queue_rollback(causes, &summary);

    let mut batch = CommandBatch::new(Cause {
        kind: CauseKind::Rollback {
            of: causes.to_vec(),
        },
        summary,
        cites: cites.clone(),
    });
    for (pack, revision) in &retired {
        batch.push(Command::SetRevisionRetired {
            pack: pack.clone(),
            revision: revision.clone(),
            retired: true,
        });
    }
    let mut said = None;
    if !cites.is_empty() {
        let talk = TalkLine {
            channel: Channel::DockTalk,
            speaker: line.and_then(|l| l.speaker.clone()),
            place: line.and_then(|l| l.place.clone()).or(place),
            text: line.map_or_else(
                || "That change has been taken back.".to_owned(),
                |l| l.text.clone(),
            ),
            cites: cites.clone(),
            about: causes.to_vec(),
        };
        if talk.validate().is_ok() {
            let chronicle = TalkLine {
                channel: Channel::Chronicle,
                text: format!("Undone: {}.", summaries.join("; ")),
                ..talk.clone()
            };
            batch.push(Command::Say {
                line: Box::new(talk.clone()),
            });
            batch.push(Command::Say {
                line: Box::new(chronicle),
            });
            said = Some(talk);
        }
    }
    sim.submit(batch);
    RollbackOutcome {
        plan,
        retired: retired.into_iter().collect(),
        cites,
        line: said,
    }
}

/// Every cause one pack revision made.
pub fn causes_of(sim: &Sim, pack: &PackId, revision: &str) -> Vec<CauseId> {
    sim.causes_where(|c| {
        matches!(&c.cause.kind, CauseKind::Script { pack: p, revision: r } if p == pack && r == revision)
    })
}
