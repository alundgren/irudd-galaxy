//! `TalkLines`' conditions against real facts over a new game: a pack's `lines.toml`
//! (the core pack's own lines live in its Luau line pools; these two are copies of its
//! stage-1 lines). Headless and offline.

use std::path::{Path, PathBuf};

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_mind::TalkLines;

const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ILSE: NpcId = NpcId::from_static("npc.ilse_marrow");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// Records a `stage_built` fact for Kettle Dock with `stage` as its amount.
fn stage_built(sim: &mut Sim, kind: &'static str, stage: i64) -> FactId {
    sim.submit(
        CommandBatch::new(Cause::system("test", "a stage")).with(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::from_static(kind),
                subjects: Subjects {
                    station: Some(KETTLE),
                    ..Subjects::default()
                },
                amount: Some(stage),
                note: format!("Kettle Dock stage {stage}"),
            }),
        }),
    );
    sim.step(&PlayerInput::default()).unwrap();
    sim.history().last_fact_id().unwrap()
}

/// Ilse's and Odo's stage-1 lines, as a pack's `lines.toml` would carry them.
const STAGE_ONE: &str = r#"
[[line]]
id = "line.ilse_marrow.11"
npc = "npc.ilse_marrow"
cites = "stage_built"
amount = 1
station = "station.kettle_dock"
text = "Odo's got a pane up at Kettle. One dome, and it fogs. He's never been prouder of anything."

[[line]]
id = "line.odo_fenn.02"
npc = "npc.odo_fenn"
cites = "stage_built"
amount = 1
station = "station.kettle_dock"
text = "First pane's up. It fogs in the mornings. Well. When we call it morning."
"#;

fn answer(sim: &Sim, lines: &TalkLines, npc: &NpcId) -> Vec<(String, FactId)> {
    lines
        .what_changed(sim.world(), sim.history(), &[], FactId(0), npc, 3)
        .into_iter()
        .map(|c| (c.line, c.cites))
        .collect()
}

#[test]
fn stage_one_lines_cite_only_the_stage_one_fact() {
    let lines = TalkLines::from_toml(STAGE_ONE).unwrap();
    let mut sim = Sim::new(galaxy_core::new_game(&data_dir(), 1).unwrap());
    // A later stage alone: the "one dome" and "first pane" lines would misreport it.
    let two = stage_built(&mut sim, "gm.stage_built", 2);
    for npc in [&ILSE, &ODO] {
        let said = answer(&sim, &lines, npc);
        assert!(
            said.iter().all(|(_, f)| *f != two),
            "{npc} cites stage 2 with a stage-1 line: {said:?}"
        );
    }
    // Stage 1 built: each says theirs, citing that fact, even with a newer stage 3.
    let one = stage_built(&mut sim, "core.stage_built", 1);
    stage_built(&mut sim, "gm.stage_built", 3);
    assert!(answer(&sim, &lines, &ILSE).contains(&("line.ilse_marrow.11".to_owned(), one)));
    assert!(answer(&sim, &lines, &ODO).contains(&("line.odo_fenn.02".to_owned(), one)));
}
