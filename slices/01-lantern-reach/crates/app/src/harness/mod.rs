//! Deterministic developer journeys. Only player inputs and validated control verbs
//! change the running game; the optional fast profile changes fresh-game data only.
mod pilot;
pub mod replay;
mod route;

use crate::game::{Game, GameOptions};
use galaxy_contract::prelude::*;
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
