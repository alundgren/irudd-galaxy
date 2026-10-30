//! Deterministic developer journeys. Only player inputs and validated control verbs
//! change the running game; the optional fast profile changes fresh-game data only.
mod pilot;
pub mod replay;
mod route;

use crate::game::{Game, GameOptions};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::SimState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const BEATS: [&str; 17] = [
    "beat.b01_undock",
    "beat.b02_mine_by_hand",
    "beat.b03_first_delivery",
    "beat.b04_first_sale",
    "beat.b05_pause",
    "beat.b06_nav_chip",
    "beat.b07_cinder_drift",
    "beat.b08_chasing_a_gap",
    "beat.b09_hook_dispute",
    "beat.b10_rumor",
    "beat.b11_orders",
    "beat.b12_ice_shortage",
    "beat.b13_first_tender",
    "beat.b14_pilot_changes",
    "beat.b15_fragment",
    "beat.b16_hullwrights_question",
    "beat.b17_coming_home",
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Slice,
    /// Authored prices and activity gates, with faster flight and mining only.
    Travel,
    Fast,
}

/// A test data profile: quicker travel/mining, smaller economic gates. No cargo,
/// credits, capabilities, counters, beat states, facts or ship positions are seeded.
pub fn profile(state: &mut SimState, profile: Profile) {
    if profile == Profile::Slice {
        return;
    }
    let w = &mut state.world;
    for (_, h) in w.hulls.iter_mut() {
        h.max_speed *= 12.0;
        h.accel = Some(h.max_speed * 30.0);
        h.turn_rate = Some(10800.0);
        h.mining_rate *= 40.0;
    }
    w.ship_rules.lanes.speed *= 20.0;
    if profile == Profile::Travel {
        return;
    }
    for u in w.progress.rules.upgrades.values_mut() {
        u.cost /= 10;
    }
    for t in w.progress.rules.crafts.values_mut() {
        t.cost /= 10;
    }
    w.progress.rules.prestige.as_mut().expect("prestige").fee /= 10;
    for (id, value) in [
        ("b06.dockings", 4.0),
        ("b11.dockings", 12.0),
        ("b16.dockings", 30.0),
        ("shortage.rimewater_ice.trigger_left", 0.9),
        ("shortage.rimewater_ice.mined", 40.0),
        ("fragment.glassite_mined", 20.0),
    ] {
        let t = w
            .progress
            .tunables
            .get_mut(id)
            .expect("core pacing tunable");
        t.value = TunableValue::Number(value);
        t.authored = t.value.clone();
    }
    w.progress
        .rules
        .traits
        .get_mut("trait.ringwise")
        .expect("Ringwise")
        .times = 20;
}

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub until: u32,
    pub at_start: bool,
    pub max_ticks: u64,
    pub record: bool,
    pub prestige: HullId,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            until: 17,
            at_start: false,
            max_ticks: 4_000_000,
            record: false,
            prestige: HullId::from_static("hull.tandem"),
        }
    }
}

pub struct Run {
    pub game: Game,
    pub report: Value,
    pub recording: replay::Recording,
}

pub fn run(game: Game, options: RunOptions) -> Result<Run, String> {
    if !(1..=17).contains(&options.until) {
        return Err("beat must be in 1..=17".into());
    }
    let mut p = pilot::Pilot::new(game, options);
    eprintln!(
        "journey: installed game-master packs {:?}",
        CHANGE_TYPES
            .iter()
            .filter(|(_, pack)| p.installed(pack))
            .map(|(_, pack)| pack)
            .collect::<Vec<_>>()
    );
    let result = p.journey();
    let mut error = match result {
        Ok(()) | Err(pilot::Halt::Reached) => None,
        Err(pilot::Halt::Failed(e)) => Some(e),
    };
    if error.is_none()
        && p.options.until == 17
        && !p.options.at_start
        && (1..=17).any(|n| !p.done(n))
    {
        error = Some("the closing beat completed with earlier beats missing".into());
    }
    if error.is_none() && p.finished() {
        let missing: Vec<_> = p
            .change_types()
            .into_iter()
            .filter(|c| c["status"] == "missing")
            .map(|c| c["change"].clone())
            .collect();
        if !missing.is_empty() {
            error = Some(format!(
                "installed game-master changes never happened: {missing:?}"
            ));
        }
    }
    p.finish_recording();
    let report = p.report(error);
    Ok(Run {
        game: p.game,
        report,
        recording: p.recording,
    })
}

pub fn fresh(
    packs: &std::path::Path,
    seed: u64,
    gm: bool,
    pacing: Profile,
) -> Result<Game, String> {
    let mut state = crate::game::new_state(packs, seed, gm)?;
    profile(&mut state, pacing);
    let mut opts = GameOptions::headless(packs.to_owned(), seed);
    opts.gm = gm;
    Game::open(state, &opts)
}

pub fn jump(packs: &std::path::Path, seed: u64, gm: bool, beat: u32) -> Result<Run, String> {
    run(
        fresh(packs, seed, gm, Profile::Slice)?,
        RunOptions {
            until: beat,
            at_start: true,
            ..RunOptions::default()
        },
    )
}

/// Run A's change types (`07-pacing.md`, "What each run checks"), each with the
/// game-master pack that makes it. A type is only expected while its pack is installed.
pub const CHANGE_TYPES: [(&str, &str); 8] = [
    ("stage_built", "gm.kettle_growth"),
    ("shortage_started", "gm.shortage_tuning"),
    ("border_moved", "gm.territory_map"),
    ("rumor_exposed", "gm.compact_plant"),
    ("mechanic_taught", "gm.rock_tagging"),
    ("canon_opened", "gm.ancients_bearing"),
    ("station_founded", "gm.sixth_station"),
    ("chronicle", "gm.chronicle"),
];

/// Whether history (or, for the chronicle, the talk log) holds a change type's evidence.
pub fn change_seen(change: &str, world: &WorldState, history: &galaxy_core::History) -> bool {
    let facts = history.facts();
    let any = |kind: &str, ok: &dyn Fn(&Fact) -> bool| {
        facts.iter().any(|f| f.kind.as_str() == kind && ok(f))
    };
    let other = |f: &Fact, id: &str| f.subjects.other.as_ref().is_some_and(|o| o.as_str() == id);
    match change {
        "stage_built" => any("gm.stage_built", &|f| {
            f.subjects.station.as_ref() == Some(&pilot::KETTLE) && f.amount.is_some_and(|n| n >= 2)
        }),
        // Core marks a start it took from tuned pacing; the tuning pack's fact precedes it.
        "shortage_started" => any("core.shortage_started", &|s| {
            matches!(&s.payload, FactPayload::Note(n) if n.ends_with("(tuned)"))
                && any("gm.shortage_tuned", &|t| t.tick <= s.tick)
        }),
        "border_moved" => any("gm.border_moved", &|_| true),
        "rumor_exposed" => any("gm.rumor_exposed", &|_| true),
        "mechanic_taught" => any("gm.mechanic_taught", &|f| other(f, "mech.rock_tagging")),
        "canon_opened" => any("gm.canon_opened", &|f| other(f, "canon.second_fragment")),
        "station_founded" => any("gm.station_founded", &|_| true),
        // The chronicle hook writes talk lines on its channel, not a fact kind.
        "chronicle" => world.talk.on(Channel::Chronicle).next().is_some(),
        _ => false,
    }
}

/// Each change type as `checked`, `missing` (installed, not seen) or `not_installed`.
/// A journey stopped before b17 reports an unseen type as `not_reached`.
pub fn change_types(
    world: &WorldState,
    history: &galaxy_core::History,
    installed: &dyn Fn(&str) -> bool,
    finished: bool,
) -> Vec<Value> {
    CHANGE_TYPES
        .iter()
        .map(|(change, pack)| {
            let status = if !installed(pack) {
                "not_installed"
            } else if change_seen(change, world, history) {
                "checked"
            } else if finished {
                "missing"
            } else {
                "not_reached"
            };
            json!({"change":change,"pack":pack,"status":status})
        })
        .collect()
}

pub(super) fn health(game: &Game) -> Value {
    let mut scripts = Vec::new();
    for (pack, state) in &game.sim().world().packs {
        for (script, h) in &state.health {
            scripts.push(
                json!({"pack":pack,"script":script,"aborts":h.aborts,"quarantined":h.quarantined}),
            );
        }
    }
    json!(scripts)
}
