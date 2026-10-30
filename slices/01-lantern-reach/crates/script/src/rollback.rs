//! In-world rollback by cause, with a compensating line.
//!
//! The kernel inverts every journaled write of the causes (`Sim::queue_rollback`).
//! This adds what the design asks for on top: the pack revisions that made those
//! causes are retired (their handlers stop, so a script whose condition still holds
//! doesn't redo the change at once), and a person says why, in dock talk and in the
//! chronicle, citing the facts the rolled-back change was based on or recorded.
//!
//! Three batches, each caused by the `script_rollback` system:
//!
//! 1. the retirement, queued before the inverses: the revisions are retired and their
//!    packs switched off (`PackState::off`), so no revision of them runs again in this
//!    save until the game master applies a new one. The script host stops a pack as
//!    soon as this is queued (it reads `TickCtx::queued`), so it runs no handler in the
//!    tick its change is undone;
//! 2. the kernel's inverse batches;
//! 3. the compensating lines, checked against the world the inverses leave: a speaker
//!    or place the rollback removes is left out, and the chronicle line is cut to fit.
//!    If no line can be said, the rollback and retirement still happen.

use std::collections::BTreeSet;

use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::RollbackLine;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, MAX_LINE_LEN, TalkLine};
use galaxy_core::{RollbackPlan, Sim};
use serde::Serialize;

use crate::host::preview;

/// The system the retirement and compensation batches are caused by.
pub const SYSTEM_NAME: &str = "script_rollback";

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

/// `text` cut to at most `max` bytes at a character boundary, with an ellipsis.
fn clip(text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
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

    // 1. Retire, on its own: nothing about the lines can undo it.
    let mut retire = CommandBatch::new(Cause::system(SYSTEM_NAME, summary.clone()));
    for (pack, revision) in &retired {
        retire.push(Command::SetRevisionRetired {
            pack: pack.clone(),
            revision: revision.clone(),
            retired: true,
        });
    }
    // The whole pack goes off, not just these revisions: an older revision of it (the
    // bundled `gm-examples` copy, an earlier applied one) would otherwise run again on
    // the next load and redo the change. Only applying a new revision switches it on.
    let packs: BTreeSet<&PackId> = retired.iter().map(|(p, _)| p).collect();
    for pack in packs {
        retire.push(Command::SetPackOff {
            pack: pack.clone(),
            off: true,
        });
    }
    sim.submit(retire);

    // 2. The inverses.
    let plan = sim.queue_rollback(causes, &summary);

    // 3. The lines, against the world everything queued so far leaves behind.
    let mut after = sim.world().clone();
    for b in &sim.state().kernel.pending {
        if let Ok(next) = preview(&after, b) {
            after = next;
        }
    }
    let speaker = line
        .and_then(|l| l.speaker.clone())
        .filter(|s| after.npcs.contains(s));
    let place = line
        .and_then(|l| l.place.clone())
        .into_iter()
        .chain(place)
        .find(|p| after.stations.contains(p));
    let talk = TalkLine {
        channel: Channel::DockTalk,
        speaker,
        place,
        text: line.map_or_else(
            || "That change has been taken back.".to_owned(),
            |l| l.text.clone(),
        ),
        cites: cites.clone(),
        about: causes.to_vec(),
    };
    let chronicle = TalkLine {
        channel: Channel::Chronicle,
        text: clip(format!("Undone: {}.", summaries.join("; ")), MAX_LINE_LEN),
        ..talk.clone()
    };
    let mut said = None;
    let mut lines =
        CommandBatch::new(Cause::system(SYSTEM_NAME, summary).citing(cites.iter().copied()));
    if !cites.is_empty() && talk.validate().is_ok() && chronicle.validate().is_ok() {
        lines.push(Command::Say {
            line: Box::new(talk.clone()),
        });
        lines.push(Command::Say {
            line: Box::new(chronicle),
        });
        if preview(&after, &lines).is_ok() {
            said = Some(talk);
            sim.submit(lines);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_at_a_char_boundary() {
        assert_eq!(clip("short".into(), 400), "short");
        let long = "é".repeat(300); // 600 bytes
        let c = clip(long, 400);
        assert!(c.len() <= 400 && c.ends_with('…'), "{}", c.len());
    }
}
