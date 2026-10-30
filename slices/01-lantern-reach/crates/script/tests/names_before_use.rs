//! Names before use (the first playtest, PR #11; the review of PR #36): nobody at a dock
//! names a person, faction or place the player hasn't met yet. A dock line names someone
//! else either in full, with enough context to stand alone ("Saffron Vey at Brassmouth"),
//! or only once an earlier beat's introduction or goal has named them. The second kind
//! is listed below with the beat that introduces the name, so a new bare name in a line
//! fails until someone checks the order. Headless.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::pilot::*;
use common::slice;
use galaxy_contract::prelude::*;

/// Bare names that other words can't stand in for, and the full names that introduce
/// them. People come from `npcs.toml` (their first names).
const NAMES: [(&str, &[&str]); 9] = [
    ("Compact", &["Tollway Compact"]),
    ("Cooperative", &["Wharf Cooperative"]),
    ("Lamplighter", &["Lamplighter Society"]),
    ("Society", &["Lamplighter Society"]),
    ("Hook", &["Cinder Hook"]),
    ("Brassmouth", &["Brassmouth Exchange", "Brassmouth Narrows"]),
    ("Exchange", &["Brassmouth Exchange"]),
    ("Narrows", &["Brassmouth Narrows"]),
    ("Shoals", &["Ember Shoals"]),
];

/// Bare names in dock lines, each introduced by a beat that always comes before the
/// line can be said: `(line, name, why)`.
const INTRODUCED: &[(&str, &str, &str)] = &[
    (
        "line.ilse_marrow.18",
        "Odo",
        "b01: the greeting itself, \"Odo at Kettle Dock\"",
    ),
    (
        "line.ilse_marrow.01",
        "Odo",
        "b01: Ilse's greeting names Odo",
    ),
    (
        "line.ilse_marrow.03",
        "Rook",
        "territory_changed needs b09, after b07 names Rook",
    ),
    (
        "line.ilse_marrow.03",
        "Cooperative",
        "territory_changed needs b09, whose goal names both sides",
    ),
    (
        "line.ilse_marrow.03",
        "Hook",
        "territory_changed needs b09, after b07 names Cinder Hook",
    ),
    (
        "line.ilse_marrow.04",
        "Compact",
        "territory_changed needs b09, whose goal names both sides",
    ),
    (
        "line.ilse_marrow.04",
        "Hook",
        "territory_changed needs b09, after b07 names Cinder Hook",
    ),
    (
        "line.ilse_marrow.07",
        "Brassmouth",
        "cites the player flooding Brassmouth's market",
    ),
    (
        "line.ilse_marrow.11",
        "Odo",
        "b01: Ilse's greeting names Odo",
    ),
    (
        "line.ilse_marrow.12",
        "Tamsin",
        "tender_acquired needs b13, whose goal names Tamsin Oddway",
    ),
    (
        "line.ilse_marrow.13",
        "Orrin",
        "trait_earned comes long after b04 names Orrin Vale",
    ),
    (
        "line.ilse_marrow.14",
        "Odo",
        "b01: Ilse's greeting names Odo",
    ),
    (
        "line.orrin_vale.08",
        "Tamsin",
        "tender_acquired needs b13, whose goal names Tamsin Oddway",
    ),
    ("line.rook_adeyemi.01", "Ilse", "b01: Ilse greets the pilot"),
    (
        "line.rook_adeyemi.03",
        "Compact",
        "side_chosen needs b09, whose goal names both sides",
    ),
    (
        "line.rook_adeyemi.03",
        "Brassmouth",
        "side_chosen needs b09, after b07; b08 starts with b07 done and names Brassmouth Exchange",
    ),
    (
        "line.rook_adeyemi.02",
        "Cooperative",
        "side_chosen needs b09, whose goal names both sides",
    ),
    (
        "line.rook_adeyemi.05",
        "Shoals",
        "rumor_resolved needs b10, whose goal names the Ember Shoals",
    ),
    (
        "line.saffron_vey.03",
        "Compact",
        "side_chosen needs b09, whose goal names both sides",
    ),
    (
        "line.saffron_vey.04",
        "Cooperative",
        "side_chosen needs b09, whose goal names both sides",
    ),
    (
        "line.saffron_vey.04",
        "Hook",
        "side_chosen needs b09, after b07 names Cinder Hook",
    ),
    (
        "line.ilse_marrow.15",
        "Shoals",
        "rumor_resolved needs b10, whose goal names the Ember Shoals",
    ),
    (
        "line.hesper_lune.04",
        "Hook",
        "territory_changed needs b09, after b07 names Cinder Hook",
    ),
    (
        "line.hesper_lune.06",
        "Shoals",
        "rumor_resolved needs b10, whose goal names the Ember Shoals",
    ),
];

struct DockLine {
    id: String,
    speaker: Option<String>,
    text: String,
}

/// The core pack's lines: ID, speaker and the text they can say (literals only).
fn dock_lines() -> Vec<DockLine> {
    let core = slice().join("packs/core/lib");
    // Windows checkouts may have CRLF endings (core.autocrlf).
    let ids_src = std::fs::read_to_string(core.join("ids.luau"))
        .unwrap()
        .replace("\r\n", "\n");
    let ids: BTreeMap<&str, &str> = ids_src
        .lines()
        .filter_map(|l| {
            let (k, v) = l.trim().trim_end_matches(',').split_once(" = ")?;
            Some((k, v.strip_prefix('"')?.strip_suffix('"')?))
        })
        .collect();
    let src = std::fs::read_to_string(core.join("lines.luau"))
        .unwrap()
        .replace("\r\n", "\n");
    let mut out = Vec::new();
    for block in src.split("\t{\n").skip(1) {
        let field = |name: &str| {
            block
                .lines()
                .find_map(|l| l.trim().strip_prefix(name)?.strip_prefix(" = "))
                .map(|v| v.trim_end_matches(','))
        };
        let Some(id) = field("id").and_then(|v| v.strip_prefix('"')?.strip_suffix('"')) else {
            continue;
        };
        let npc = field("npc").unwrap();
        let speaker = npc.strip_prefix("ids.").map(|key| ids[key].to_string());
        let body = &block[block.find("text = function").unwrap()..];
        let mut text = String::new();
        let mut quote = None;
        for c in body.chars() {
            match quote {
                None if c == '"' || c == '`' => quote = Some(c),
                Some(q) if c == q => {
                    quote = None;
                    text.push_str(" | ");
                }
                Some(_) => text.push(c),
                None => {}
            }
        }
        out.push(DockLine {
            id: id.to_string(),
            speaker,
            text,
        });
    }
    assert!(out.len() > 60, "found only {} lines", out.len());
    out
}

fn mentions(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphabetic)
    })
}

#[test]
fn dock_lines_name_nobody_before_they_are_introduced() {
    let data = |file: &str| -> toml::Table {
        let path = slice().join("packs/core/data").join(file);
        std::fs::read_to_string(path).unwrap().parse().unwrap()
    };
    let named = |table: &toml::Table, key: &str| -> BTreeMap<String, toml::Table> {
        table[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let r = r.as_table().unwrap().clone();
                (r["id"].as_str().unwrap().to_string(), r)
            })
            .collect()
    };
    let npcs = named(&data("npcs.toml"), "npc");
    let stations = named(&data("stations.toml"), "station");
    let regions = named(&data("regions.toml"), "region");
    let name_of = |r: &toml::Table| r["name"].as_str().unwrap().to_string();

    let mut names: Vec<(String, Vec<String>)> = NAMES
        .iter()
        .map(|(w, full)| (w.to_string(), full.iter().map(|f| f.to_string()).collect()))
        .collect();
    for npc in npcs.values() {
        let full = name_of(npc);
        let first = full.split(' ').next().unwrap().to_string();
        names.push((first, vec![full]));
    }

    let mut bare = BTreeSet::new();
    for line in dock_lines() {
        // The speaker, their station and its region are on the dock screen.
        let mut own = String::new();
        if let Some(npc) = line.speaker.as_ref().map(|s| &npcs[s]) {
            own.push_str(&name_of(npc));
            let station = &stations[npc["location"].as_str().unwrap()];
            own.push_str(&format!(" {} ", name_of(station)));
            own.push_str(&name_of(&regions[station["region"].as_str().unwrap()]));
        }
        for (word, full) in &names {
            if mentions(&line.text, word)
                && !mentions(&own, word)
                && !full.iter().any(|f| line.text.contains(f.as_str()))
            {
                bare.insert((line.id.clone(), word.clone()));
            }
        }
    }

    let listed: BTreeSet<(String, String)> = INTRODUCED
        .iter()
        .map(|(l, w, _)| (l.to_string(), w.to_string()))
        .collect();
    let unlisted: Vec<_> = bare.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&bare).collect();
    assert!(
        unlisted.is_empty(),
        "dock lines name someone bare: say the name in full, or list the beat that introduces it first: {unlisted:?}"
    );
    assert!(stale.is_empty(), "listed but no longer bare: {stale:?}");
}

/// The review's route: b09 starts on the third Hook docking, before the player has ever
/// been to Brassmouth (b08 isn't needed). Choosing the Compact and docking at the Hook
/// again has Rook mention Saffron, who must come with her context.
#[test]
fn rook_introduces_saffron_when_the_compact_is_chosen_before_brassmouth() {
    let mut p = Pilot::with(36, |s| {
        s.done(&[
            "beat.b01_undock",
            "beat.b02_mine_by_hand",
            "beat.b03_first_delivery",
            "beat.b04_first_sale",
            "beat.b05_pause",
            "beat.b06_nav_chip",
            "beat.b07_cinder_drift",
        ]);
        s.fitted(&[
            "upgrade.mining_lens",
            "upgrade.nav_chip",
            "upgrade.lane_chart",
        ]);
        s.flags(&["cap.click_fly", "cap.click_dock", "cap.map_travel"]);
        s.counter(CounterKind::Dockings, 12);
        s.station_counter(CounterKind::Dockings, &HOOK, 3);
        s.docked_at(&HOOK);
    });
    p.idle(3);
    assert_eq!(p.status("beat.b09_hook_dispute"), Some(BeatStatus::Active));
    p.reply("npc.rook_adeyemi", "choice.side_compact");
    assert!(p.has_fact("core.side_chosen"));
    p.dock_by_hand(&HOOK);
    p.idle(3);
    assert!(
        p.facts("dock")
            .iter()
            .all(|f| f.subjects.ship.as_ref() != Some(&PLAYER)
                || f.subjects.station.as_ref() != Some(&BRASS)),
        "never at Brassmouth"
    );
    assert!(
        p.heard("npc.rook_adeyemi", "Compact name on your plate"),
        "Rook remarks on the side: {:?}",
        p.lines_said()
    );
    assert!(p.heard(
        "npc.rook_adeyemi",
        "Saffron Vey, their factor at Brassmouth"
    ));
}
