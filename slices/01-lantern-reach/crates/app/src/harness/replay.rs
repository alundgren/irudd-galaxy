//! Versioned, run-length encoded player actions with canonical per-store checkpoints.
use super::Profile;
use crate::game::{Game, GameOptions};
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::{Order, OrderQueue};
use galaxy_core::{SimState, TickReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
pub enum Action {
    Input(PlayerInput),
    Trade(TradeRequest),
    Pause,
    Orders(Vec<Order>),
    SaveLoop(String),
    RunLoop(String),
    Assign {
        name: String,
        ship: ShipId,
        floor: u32,
    },
}

/// Replay verbs through the same boundaries as the pilot, never recorded state writes.
pub fn apply(game: &mut Game, action: &Action) -> Result<TickReport, String> {
    let w = game.sim().world();
    let controls = galaxy_ship::controls::Controls::of(w);
    let batch = match action {
        Action::Input(_) => None,
        // Like Session's dock desk, submit ahead of ship/NPC orders. A player
        // clicking Sell must not silently lose the market line to a passing NPC.
        Action::Trade(request) => {
            Some(galaxy_economy::trade_batch(w, request).map_err(|e| e.to_string())?)
        }
        Action::Pause => Some(
            CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Wait,
                "the player opened the PAUSED card",
            ))
            .with(Command::Emit {
                event: Event::Paused { session: 1 },
            }),
        ),
        Action::Orders(orders) => {
            if !orders.is_empty() && !controls.has(galaxy_ship::controls::Capability::Queues) {
                return Err("queues are locked".into());
            }
            let limit = if controls.has(galaxy_ship::controls::Capability::Loops) {
                w.ship_rules.controls.loop_limit
            } else {
                w.ship_rules.controls.queue_limit
            };
            if orders.len() > limit as usize {
                return Err("queue is too long".into());
            }
            Some(
                CommandBatch::new(Cause::verb(
                    Actor::Player,
                    VerbKind::Fly,
                    "the player's orders",
                ))
                .with(Command::SetOrders {
                    ship: w.player.ship.clone().ok_or("no flagship")?,
                    orders: OrderQueue {
                        orders: orders.clone(),
                        ..OrderQueue::default()
                    },
                }),
            )
        }
        Action::SaveLoop(name) => {
            Some(galaxy_ship::controls::save_loop(w, name).map_err(|e| e.to_string())?)
        }
        Action::RunLoop(name) => Some(
            galaxy_ship::controls::run_loop(w, w.player.ship.as_ref().ok_or("no flagship")?, name)
                .map_err(|e| e.to_string())?,
        ),
        Action::Assign { name, ship, floor } => Some(
            galaxy_ship::controls::assign_loop(
                w,
                &galaxy_ship::EconomyDesk,
                ship,
                name,
                Some(*floor),
            )
            .map_err(|e| e.to_string())?,
        ),
    };
    if let Some(batch) = batch {
        game.sim_mut().submit(batch);
    }
    if matches!(action, Action::Pause) {
        game.sim_mut().set_paused(true);
        if game.step(&PlayerInput::default()).is_some() {
            return Err("paused game stepped".into());
        }
        game.sim_mut().set_paused(false);
    }
    let idle = PlayerInput::default();
    game.step(match action {
        Action::Input(input) => input,
        _ => &idle,
    })
    .ok_or("game is paused".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputRun {
    pub count: u32,
    pub input: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    pub tick: u64,
    pub hash: String,
    pub stores: BTreeMap<String, String>,
}
impl Checkpoint {
    pub fn of(game: &Game) -> Self {
        let state = game.sim().state();
        let value = serde_json::to_value(state).expect("serializable game");
        let mut stores = BTreeMap::new();
        for (name, value) in value.as_object().expect("state object") {
            if name == "world" {
                for (name, store) in value.as_object().expect("world object") {
                    stores.insert(format!("world.{name}"), digest(store));
                }
            } else {
                stores.insert(name.clone(), digest(value));
            }
        }
        Self {
            tick: game.sim().tick().0,
            hash: game.sim().hash(),
            stores,
        }
    }
    fn verify(&self, game: &Game) -> Result<(), String> {
        let actual = Self::of(game);
        if self.hash == actual.hash {
            return Ok(());
        }
        let differing: Vec<_> = self
            .stores
            .keys()
            .chain(actual.stores.keys())
            .filter(|k| self.stores.get(*k) != actual.stores.get(*k))
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Err(format!(
            "golden diverged at tick {}: store(s) {}; expected {}, actual {}",
            self.tick,
            differing.join(", "),
            self.hash,
            actual.hash
        ))
    }
}
fn digest(value: &serde_json::Value) -> String {
    blake3::hash(&serde_json::to_vec(value).expect("JSON"))
        .to_hex()
        .to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub version: u32,
    pub seed: u64,
    pub gm: bool,
    pub profile: Profile,
    /// Present for a recording that starts from an existing save. Fresh fixtures
    /// regenerate from seed and profile, so changes to authored data are detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<SimState>,
    pub inputs: Vec<InputRun>,
    pub checkpoints: Vec<Checkpoint>,
}
impl Recording {
    pub fn new(game: &Game, record: bool) -> Self {
        Self {
            version: 1,
            seed: game.sim().world().meta.seed,
            gm: game
                .reports()
                .iter()
                .any(|r| r.accepted() && r.pack.as_str().starts_with("gm.")),
            profile: Profile::Slice,
            initial: record.then(|| game.sim().state().clone()),
            inputs: vec![],
            checkpoints: vec![],
        }
    }
    pub fn push(&mut self, input: Action) {
        if let Some(last) = self.inputs.last_mut()
            && last.input == input
            && last.count < u32::MAX
        {
            last.count += 1;
        } else {
            self.inputs.push(InputRun { count: 1, input });
        }
    }
}

pub fn replay(packs: &std::path::Path, recording: &Recording) -> Result<Game, String> {
    if recording.version != 1 {
        return Err("unsupported golden version".into());
    }
    if recording.checkpoints.is_empty() {
        return Err("golden has no checkpoints".into());
    }
    let mut game = if let Some(state) = &recording.initial {
        let mut opts = GameOptions::headless(packs.to_owned(), recording.seed);
        opts.gm = recording.gm;
        Game::open(state.clone(), &opts)?
    } else {
        super::fresh(packs, recording.seed, recording.gm, recording.profile)?
    };
    let mut checks = recording.checkpoints.iter().peekable();
    for run in &recording.inputs {
        if run.count == 0 {
            return Err("zero-length golden input run".into());
        }
        for _ in 0..run.count {
            apply(&mut game, &run.input)?;
            while let Some(check) = checks.peek()
                && check.tick <= game.sim().tick().0
            {
                if check.tick != game.sim().tick().0 {
                    return Err("golden checkpoints are not ordered within the input stream".into());
                }
                check.verify(&game)?;
                checks.next();
            }
        }
    }
    if checks.next().is_some() {
        return Err("golden ends before its checkpoints".into());
    }
    if recording.checkpoints.last().expect("checkpoint").tick != game.sim().tick().0 {
        return Err("golden lacks a final checkpoint".into());
    }
    Ok(game)
}
