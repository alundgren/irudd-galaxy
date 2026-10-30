//! Adapter for the script's ordered candidates. No second catalog or matcher.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;

use crate::menu::{DOCK_TALK, Menu, MenuCtx, MenuOption, MenuProvider, Precondition};

pub struct ScriptDockTalk;

impl MenuProvider for ScriptDockTalk {
    fn name(&self) -> &'static str {
        "script_dock_talk"
    }

    fn resume(&self, cx: &MenuCtx<'_>, facts: &[Fact]) -> Vec<Menu> {
        // Decisions cite the request, including skipped outcomes. A result already
        // saved in the event queue belongs to the script, not a second model call.
        let settled: BTreeSet<FactId> = cx
            .history
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == crate::DECISION_FACT)
            .filter_map(|f| cx.history.cause(f.cause))
            .flat_map(|c| c.cause.cites.iter().copied())
            .collect();
        facts
            .iter()
            .filter(|f| !settled.contains(&f.id))
            .flat_map(|f| self.menus(cx, f))
            .collect()
    }

    fn menus(&self, cx: &MenuCtx<'_>, fact: &Fact) -> Vec<Menu> {
        let FactPayload::Event(Event::TalkRequested { request }) = &fact.payload else {
            return vec![];
        };
        if cx
            .world
            .packs
            .get(&request.pack)
            .is_some_and(|p| p.off || p.retired.contains(&request.revision))
        {
            return vec![];
        }
        let latest = cx.history.facts().iter().rev().find(|f| {
            f.subjects.ship.as_ref() == Some(&request.ship)
                && matches!(
                    f.payload,
                    FactPayload::Event(Event::Dock { .. } | Event::Undock { .. })
                )
        });
        if !latest.is_some_and(|f| f.id == request.docking)
            || !cx
                .world
                .ships
                .get(&request.ship)
                .is_some_and(|s| s.docked_at.as_ref() == Some(&request.station))
        {
            return vec![];
        }
        let Some(first) = request.candidates.first() else {
            return vec![];
        };
        let options = request
            .candidates
            .iter()
            .take(cx.config.limits.menu_size.max(1))
            .map(|c| MenuOption {
                id: c.line.to_string(),
                text: format!("{} says: {}", c.npc, c.text),
                score: f64::from(c.specificity),
                refs: vec![AnyId::new(c.npc.as_str()).expect("NPC ID")],
                commands: vec![Command::Emit {
                    event: Event::TalkPicked {
                        request: fact.id,
                        line: c.line.clone(),
                    },
                }],
                // Returning an ID uses the normal decision path. Even opt-in free
                // phrasing cannot rewrite a script-owned candidate.
                line: None,
            })
            .collect();
        vec![Menu {
            kind: DOCK_TALK,
            actor: AnyId::new(request.station.as_str()).expect("station ID"),
            // A station-wide menu can include several speakers. Do not attribute
            // their selection to the default's NPC when somebody else may speak.
            npc: request
                .candidates
                .iter()
                .all(|c| c.npc == first.npc)
                .then(|| first.npc.clone()),
            ship: Some(request.ship.clone()),
            station: Some(request.station.clone()),
            trigger: fact.id,
            instructions:
                "The player has just docked. Pick the one authored line they most need to hear."
                    .into(),
            options,
            cites: request.candidates.iter().map(|c| c.cite).collect(),
            preconditions: vec![Precondition::Docked {
                ship: request.ship.clone(),
                station: request.station.clone(),
            }],
        }]
    }
}
