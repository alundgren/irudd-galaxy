//! #40: directions in dialogue may be vague, but they must be correct. Every compass
//! word in the packs' scripts (lines, goals, notes) has a claim here: what it points
//! from and to. The test checks each claim against the map (+y is north), within 45
//! degrees of the word, and fails on a compass word nobody has claimed, so a new
//! direction gets checked the day it's written.

mod common;

use std::path::Path;

use galaxy_contract::prelude::*;

/// What a direction points at on the map.
enum Place {
    Station(&'static str),
    Field(&'static str),
    /// A lane's mouth on the side of `from`'s region.
    Lane(&'static str),
}

/// One direction an authored line gives: a fragment of the line, the compass word in
/// it, and where it points from and to.
struct Claim {
    fragment: &'static str,
    word: &'static str,
    from: Place,
    to: Place,
}

const CLAIMS: &[Claim] = &[
    // Ilse at the Wharf (b01).
    Claim {
        fragment: "the pale rocks east of here are Rimewater ice",
        word: "east",
        from: Place::Station("station.lantern_wharf"),
        to: Place::Field("field.shallows.ice"),
    },
    // Ilse at the Wharf (b07), and b07's goal.
    Claim {
        fragment: "The lantern current east of the Wharf carries you there",
        word: "east",
        from: Place::Station("station.lantern_wharf"),
        to: Place::Lane("lane.shallows_cinder"),
    },
    Claim {
        fragment: "The lane to Cinder Drift starts east of the Wharf",
        word: "east",
        from: Place::Station("station.lantern_wharf"),
        to: Place::Lane("lane.shallows_cinder"),
    },
    // Odo at Kettle (b04): Tinroot is mostly south of the Wharf, not west.
    Claim {
        fragment: "The Wharf buys ore from the dark rocks south of it",
        word: "south",
        from: Place::Station("station.lantern_wharf"),
        to: Place::Field("field.shallows.iron"),
    },
    // Rook at the Hook (b08): glassite is in Glass Drift, not the Cinder Belt.
    Claim {
        fragment: "out in Glass Drift, east of here",
        word: "east",
        from: Place::Station("station.cinder_hook"),
        to: Place::Field("field.cinder.glassite"),
    },
    Claim {
        fragment: "Brassmouth Exchange, up the north lane",
        word: "north",
        from: Place::Station("station.cinder_hook"),
        to: Place::Lane("lane.cinder_brassmouth"),
    },
];

const WORDS: [&str; 4] = ["north", "south", "east", "west"];

fn dot(a: Vec2, b: Vec2) -> f32 {
    a.x * b.x + a.y * b.y
}

fn unit(word: &str) -> Vec2 {
    match word {
        "north" => Vec2::new(0.0, 1.0),
        "south" => Vec2::new(0.0, -1.0),
        "east" => Vec2::new(1.0, 0.0),
        "west" => Vec2::new(-1.0, 0.0),
        other => panic!("not a compass word: {other}"),
    }
}

/// Where a place is, and its region (a lane's mouth on `region`'s side).
fn locate(world: &WorldState, place: &Place, region: Option<&RegionId>) -> (Vec2, RegionId) {
    match place {
        Place::Station(id) => {
            let s = world.stations.get(*id).unwrap_or_else(|| panic!("{id}"));
            (s.position, s.region.clone())
        }
        Place::Field(id) => {
            let f = world.fields.get(*id).unwrap_or_else(|| panic!("{id}"));
            (f.center, f.region.clone())
        }
        Place::Lane(id) => {
            let l = world.lanes.get(*id).unwrap_or_else(|| panic!("{id}"));
            let region = region.expect("a lane is seen from somewhere");
            if l.a == *region {
                (l.a_gate, l.a.clone())
            } else {
                assert_eq!(l.b, *region, "{id} doesn't open into {region}");
                (l.b_gate, l.b.clone())
            }
        }
    }
}

/// Compass words in `text` (lower case), as whole words.
fn compass_words(text: &str) -> Vec<&'static str> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for word in WORDS {
        let mut at = 0;
        while let Some(i) = lower[at..].find(word) {
            let start = at + i;
            let end = start + word.len();
            let before = lower[..start].chars().next_back();
            let after = lower[end..].chars().next();
            let edge = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if edge(before) && edge(after) {
                out.push(word);
            }
            at = end;
        }
    }
    out
}

fn luau_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            luau_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "luau") {
            out.push(p);
        }
    }
}

#[test]
fn every_compass_word_in_the_packs_matches_the_map() {
    let world = galaxy_core::new_game(&common::core_data(), 1)
        .expect("core data loads")
        .world;
    for c in CLAIMS {
        let (from, region) = locate(&world, &c.from, None);
        let (to, _) = locate(&world, &c.to, Some(&region));
        let way = (to - from).normalize_or_zero();
        let cos = dot(way, unit(c.word));
        assert!(
            cos >= std::f32::consts::FRAC_1_SQRT_2,
            "\"{}\" says {}, but it's {:.0} degrees off ({from:?} to {to:?})",
            c.fragment,
            c.word,
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        );
    }

    let mut files = Vec::new();
    luau_files(&common::slice().join("packs"), &mut files);
    files.sort();
    let mut unclaimed = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            let code = line.split("--").next().unwrap_or("");
            for word in compass_words(code) {
                let claimed = CLAIMS
                    .iter()
                    .any(|c| c.word == word && code.contains(c.fragment));
                if !claimed {
                    unclaimed.push(format!("{}:{}: {word}", file.display(), n + 1));
                }
            }
        }
    }
    assert!(
        unclaimed.is_empty(),
        "compass words with no claim in this test (add one, checked against the map): {unclaimed:#?}"
    );
}

#[test]
fn the_old_wrong_directions_would_fail() {
    let world = galaxy_core::new_game(&common::core_data(), 1)
        .expect("core data loads")
        .world;
    // Odo's old "the dark rocks west of it": Tinroot from the Wharf.
    let (wharf, _) = locate(&world, &Place::Station("station.lantern_wharf"), None);
    let (tinroot, _) = locate(&world, &Place::Field("field.shallows.iron"), None);
    let cos = dot((tinroot - wharf).normalize_or_zero(), unit("west"));
    assert!(cos < std::f32::consts::FRAC_1_SQRT_2, "{cos}");
}
