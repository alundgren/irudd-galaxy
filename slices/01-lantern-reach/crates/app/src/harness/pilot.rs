use super::{
    BEATS, RunOptions,
    replay::{Action, Checkpoint, Recording},
};
use crate::game::Game;
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::{Order, Quantity};
use serde_json::{Value, json};

pub(super) const WHARF: StationId = StationId::from_static("station.lantern_wharf");
pub(super) const KETTLE: StationId = StationId::from_static("station.kettle_dock");
pub(super) const HOOK: StationId = StationId::from_static("station.cinder_hook");
pub(super) const GDO: StationId = StationId::from_static("station.glass_drift_observatory");
pub(super) const BRASS: StationId = StationId::from_static("station.brassmouth_exchange");
pub(super) const ICE: WareId = WareId::from_static("ware.ice");
pub(super) const ORE: WareId = WareId::from_static("ware.iron_ore");
pub(super) const PLATE: WareId = WareId::from_static("ware.refined_plate");
pub(super) const GLASS: WareId = WareId::from_static("ware.glassite");
pub(super) const RIME: FieldId = FieldId::from_static("field.shallows.ice");
pub(super) const TIN: FieldId = FieldId::from_static("field.shallows.iron");
pub(super) const GLASS_FIELD: FieldId = FieldId::from_static("field.cinder.glassite");

#[derive(Debug)]
pub(super) enum Halt {
    Reached,
    Failed(String),
}
impl From<String> for Halt {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}
pub(super) type Result<T = ()> = std::result::Result<T, Halt>;

pub(super) struct Pilot {
    pub game: Game,
    pub options: RunOptions,
    pub recording: Recording,
    start: u64,
    seen: usize,
    beats: Vec<Value>,
    credits: Vec<Value>,
    rejected: Vec<Value>,
    script_events: Vec<Value>,
    pub activity: u64,
}
impl Pilot {
    pub fn laps(&self) -> u64 {
        self.game
            .sim()
            .history()
            .facts()
            .iter()
            .filter(|f| {
                f.kind.as_str() == "loop_lap"
                    && f.subjects.ship.as_ref() == self.world().player.ship.as_ref()
            })
            .count() as u64
    }
    pub fn sold(&self) -> i64 {
        self.game
            .sim()
            .history()
            .facts()
            .iter()
            .filter(|f| {
                f.kind.as_str() == "sell"
                    && f.subjects.ship.as_ref() == self.world().player.ship.as_ref()
            })
            .filter_map(|f| f.amount)
            .sum()
    }
    pub fn new(game: Game, options: RunOptions) -> Self {
        let start = game.sim().tick().0;
        let seen = game.sim().history().len();
        let recording = Recording::new(&game, options.record);
        Self {
            game,
            options,
            recording,
            start,
            seen,
            beats: vec![],
            credits: vec![],
            rejected: vec![],
            script_events: vec![],
            activity: 0,
        }
    }
    pub fn world(&self) -> &WorldState {
        self.game.sim().world()
    }
    pub fn ship(&self) -> &Ship {
        self.world()
            .ships
            .get(self.world().player.ship.as_ref().expect("flagship"))
            .expect("ship")
    }
    pub fn cargo(&self, ware: &WareId) -> u32 {
        self.ship().hold.count(ware)
    }
    pub fn free(&self) -> u64 {
        galaxy_progress::eval::hold_free(self.world(), Some(self.ship()))
    }
    pub fn counter(&self, k: CounterKind) -> u64 {
        self.world().counters.get(k, &CounterScope::Player)
    }
    pub fn done(&self, n: usize) -> bool {
        self.world()
            .progress
            .beats
            .get(BEATS[n - 1])
            .is_some_and(|b| b.status == BeatStatus::Completed)
    }
    pub fn active(&self, n: usize) -> bool {
        self.world()
            .progress
            .beats
            .get(BEATS[n - 1])
            .is_some_and(|b| b.status == BeatStatus::Active)
    }
    pub fn step(&self, n: usize) -> u32 {
        self.world()
            .progress
            .beats
            .get(BEATS[n - 1])
            .map_or(0, |b| b.step)
    }
    pub fn has(&self, kind: &str) -> bool {
        self.game
            .sim()
            .history()
            .facts()
            .iter()
            .any(|f| f.kind.as_str() == kind)
    }
    pub fn number(&self, id: &str) -> f64 {
        self.world().progress.tunables[id]
            .value
            .as_number()
            .expect("numeric pacing")
    }
    pub fn fitted(&self, id: &str) -> bool {
        self.world().player.fittings.contains(id)
    }
    pub fn cap(&self, id: &str) -> bool {
        self.world().player.flags.contains(id)
    }

    pub fn act(&mut self, action: Action) -> Result {
        let n = self.options.until as usize;
        let maintenance =
            matches!(&action, Action::Assign { name, .. } if name == "Wharf plate run");
        if !maintenance
            && ((self.options.at_start && self.active(n))
                || (!self.options.at_start && self.done(n)))
        {
            return Err(Halt::Reached);
        }
        if self.options.at_start && self.done(n) {
            return Err(Halt::Failed(format!("beat {n} has already completed")));
        }
        if self.game.sim().tick().0 - self.start >= self.options.max_ticks {
            return Err(Halt::Failed(format!(
                "journey tick budget exhausted at tick {}",
                self.game.sim().tick().0
            )));
        }
        let before = self.world().player.credits;
        let report = super::replay::apply(&mut self.game, &action)?;
        if self.options.record {
            self.recording.push(action);
        }
        self.rejected
            .extend(report.rejected.iter().map(|r| json!(r)));
        for event in self.game.scripts().take_events() {
            if matches!(
                event,
                galaxy_script::host::HostEvent::Aborted { .. }
                    | galaxy_script::host::HostEvent::Quarantined { .. }
            ) {
                self.script_events.push(json!(event));
            }
        }
        let dockings = self.counter(CounterKind::Dockings);
        let facts = &self.game.sim().history().facts()[self.seen..];
        let tender_stalled = facts.iter().any(|f| {
            matches!(f.kind.as_str(), "loop_stalled" | "patient_blocked")
                && f.subjects
                    .ship
                    .as_ref()
                    .is_some_and(|s| s.as_str() == "craft.thimble")
        });
        let player_ships: std::collections::BTreeSet<_> = self
            .world()
            .ships
            .values()
            .filter(|s| s.owner == Owner::Player)
            .map(|s| s.id.clone())
            .collect();
        let mut checkpoint = false;
        for f in facts {
            if matches!(
                f.kind.as_str(),
                "dock" | "mine" | "sell" | "buy" | "jump" | "deliver" | "loop_lap"
            ) && f
                .subjects
                .ship
                .as_ref()
                .is_some_and(|s| player_ships.contains(s))
            {
                self.activity += 1;
            }
            if f.kind.as_str() == "beat_completed" {
                eprintln!(
                    "journey: {:?} completed at tick {}, docking {}",
                    f.subjects.other, f.tick.0, dockings
                );
                self.beats
                    .push(json!({"beat":f.subjects.other,"tick":f.tick,"dockings":dockings}));
                checkpoint = true;
            }
        }
        self.seen = self.game.sim().history().len();
        if before != self.world().player.credits || self.credits.is_empty() {
            self.credits
                .push(json!({"activity":self.activity,"dockings":dockings,
                "units_mined":self.counter(CounterKind::UnitsMined),"units_sold":self.sold(),
                "centicredits":self.world().player.credits}));
        }
        if self.world().player.credits < 0 {
            return Err(Halt::Failed("negative credits".into()));
        }
        if checkpoint && self.options.record {
            self.recording.checkpoints.push(Checkpoint::of(&self.game));
        }
        if !self.script_events.is_empty() {
            return Err(Halt::Failed(
                "a script aborted or was quarantined; see script_events".into(),
            ));
        }
        // Automation remains our responsibility after its teaching beat. Respond
        // to a newly observed stall, including while earning the prestige fee.
        if tender_stalled && self.world().player.loops.contains_key("Wharf plate run") {
            self.act(Action::Assign {
                name: "Wharf plate run".into(),
                ship: ShipId::from_static("craft.thimble"),
                floor: 4000,
            })?;
        }
        Ok(())
    }
    pub fn input(&mut self, input: PlayerInput) -> Result {
        self.act(Action::Input(input))
    }
    pub fn idle(&mut self, n: u32) -> Result {
        for _ in 0..n {
            self.input(PlayerInput::default())?;
        }
        Ok(())
    }
    pub fn wait(&mut self, label: &str, done: impl Fn(&Self) -> bool) -> Result {
        for _ in 0..30_000 {
            if done(self) {
                return Ok(());
            }
            self.idle(1)?;
        }
        Err(Halt::Failed(format!(
            "{label}: bounded wait at tick {}, position {:?}, orders {:?}",
            self.game.sim().tick().0,
            self.ship().position,
            self.ship().orders
        )))
    }
    pub fn press(&mut self) -> Result {
        self.input(PlayerInput {
            action: true,
            ..PlayerInput::default()
        })?;
        self.idle(2)
    }
    pub fn undock(&mut self) -> Result {
        if self.ship().docked_at.is_some() {
            self.press()?;
        }
        Ok(())
    }

    /// Manual steering uses the same thrust axes as the keyboard. Approach slowly
    /// and choose a deterministic lateral waypoint around a nearby moving craft.
    pub fn fly(&mut self, to: Vec2, close: f32) -> Result {
        self.undock()?;
        for _ in 0..30_000 {
            let ship = self.ship();
            let off = to - ship.position;
            if off.length() <= close {
                return self.idle(3);
            }
            let mut direction = off.normalize_or_zero();
            for other in self.world().ships.values() {
                if other.id == ship.id || other.region != ship.region || other.docked_at.is_some() {
                    continue;
                }
                let delta = other.position - ship.position;
                let ahead = delta.x * direction.x + delta.y * direction.y;
                if ahead > 0.0 && ahead < 100.0 && (delta - direction * ahead).length() < 35.0 {
                    direction = (direction + Vec2::new(-direction.y, direction.x) * 1.5)
                        .normalize_or_zero();
                    break;
                }
            }
            let speed = self.world().hulls.get(&ship.hull).expect("hull").max_speed
                * galaxy_progress::modifiers::for_ship(self.world(), &ship.id).speed as f32;
            let stick = direction * (off.length() / (speed / 30.0 * 3.0).max(60.0)).min(1.0);
            self.input(PlayerInput {
                thrust: stick,
                ..PlayerInput::default()
            })?;
        }
        Err(Halt::Failed(format!("cannot fly to {to:?}")))
    }
    pub fn region(&mut self, target: &RegionId) -> Result {
        for _ in 0..8 {
            if &self.ship().region == target {
                return Ok(());
            }
            let way = galaxy_ship::space::route(self.world(), &self.ship().region, target)
                .ok_or_else(|| format!("no lane to {target}"))?;
            self.fly(way.start, 10.0)?;
            self.press()?;
            self.wait("lane transit", |p| {
                p.ship().region == way.to && p.ship().activity.transit.is_none()
            })?;
        }
        Err(Halt::Failed("too many lane hops".into()))
    }
    pub fn click(&mut self, target: ClickTarget) -> Result {
        self.input(PlayerInput {
            click: Some(Click {
                target,
                queue: false,
            }),
            ..PlayerInput::default()
        })?;
        self.wait("click order", |p| {
            p.ship().orders.current >= p.ship().orders.orders.len()
        })?;
        self.idle(4)
    }
    pub fn go(&mut self, station: &StationId) -> Result {
        if self.ship().docked_at.as_ref() == Some(station) {
            return Ok(());
        }
        if self.cap("cap.click_dock") && self.cap("cap.map_travel") {
            self.click(ClickTarget::Station {
                station: station.clone(),
            })?;
        } else {
            let s = self.world().stations.get(station).expect("station").clone();
            self.region(&s.region)?;
            self.fly(s.position, (s.dock_radius * 0.5).max(5.0))?;
            self.press()?;
            self.idle(4)?;
        }
        if self.ship().docked_at.as_ref() != Some(station) {
            return Err(Halt::Failed(format!("did not dock at {station}")));
        }
        Ok(())
    }
    pub fn rock(&self, field: &FieldId) -> Option<RockId> {
        // Finish rocks before starting another: depletion is a real fragment trigger.
        self.world()
            .rocks
            .values()
            .filter(|r| &r.field == field && r.remaining > 0)
            .min_by_key(|r| (r.remaining, r.id.clone()))
            .map(|r| r.id.clone())
    }
    /// A depleted renewable field needs region activity, not idle ticks. Visit its
    /// stations until regrowth exposes a rock again, always within the run budget.
    pub fn renewable_rock(&mut self, field: &FieldId) -> Result<RockId> {
        let region = self
            .world()
            .fields
            .get(field)
            .ok_or_else(|| format!("unknown field {field}"))?
            .region
            .clone();
        let stations: Vec<_> = self
            .world()
            .stations
            .values()
            .filter(|s| s.region == region)
            .take(2)
            .map(|s| s.id.clone())
            .collect();
        for _ in 0..512 {
            if let Some(rock) = self.rock(field) {
                return Ok(rock);
            }
            if stations.len() < 2 {
                break;
            }
            for station in &stations {
                self.go(station)?;
            }
        }
        Err(Halt::Failed(format!(
            "{field} did not regrow after counted regional visits"
        )))
    }
    pub fn mine(&mut self, rock: RockId) -> Result {
        if self.cap("cap.click_mine") && self.cap("cap.map_travel") {
            return self.click(ClickTarget::Rock { rock });
        }
        let r = self.world().rocks.get(&rock).expect("rock").clone();
        let region = self
            .world()
            .fields
            .get(&r.field)
            .expect("field")
            .region
            .clone();
        self.region(&region)?;
        self.undock()?;
        let radius = galaxy_ship::space::rock_radius(self.world(), &r);
        let spot =
            r.position + (self.ship().position - r.position).normalize_or_zero() * (radius + 10.0);
        self.fly(spot, 3.0)?;
        for _ in 0..30_000 {
            if self.free() == 0 || self.world().rocks.get(&rock).expect("rock").remaining == 0 {
                return self.idle(3);
            }
            self.input(PlayerInput {
                action: true,
                ..PlayerInput::default()
            })?;
        }
        Err(Halt::Failed(format!("mining {rock} stalled")))
    }
    pub fn fill(&mut self, field: &FieldId) -> Result {
        for _ in 0..40 {
            if self.free() == 0 {
                return Ok(());
            }
            let Some(rock) = self.rock(field) else {
                return Ok(());
            };
            self.mine(rock)?;
        }
        Err(Halt::Failed(format!("cannot fill at {field}")))
    }
    pub fn trade(&mut self, ware: &WareId, side: TradeSide, quantity: u32) -> Result {
        if quantity == 0 {
            return Ok(());
        }
        let station = self
            .ship()
            .docked_at
            .clone()
            .ok_or("trade away from a dock".to_owned())?;
        let mut request = TradeRequest {
            ship: self.ship().id.clone(),
            station,
            ware: ware.clone(),
            side,
            quantity,
            limit: None,
        };
        if side == TradeSide::Buy {
            // Dock UI's buy-all fits the request to stock, credits and free hold.
            request.quantity = quantity.min(self.free() as u32);
            while request.quantity > 0
                && galaxy_economy::plan_trade(self.world(), &request).is_err()
            {
                request.quantity -= 1;
            }
        }
        if request.quantity == 0 {
            return Ok(());
        }
        self.act(Action::Trade(request))?;
        self.idle(3)
    }
    pub fn sell(&mut self, ware: &WareId, n: u32) -> Result {
        self.trade(ware, TradeSide::Sell, n.min(self.cargo(ware)))
    }
    pub fn sell_all(&mut self, ware: &WareId) -> Result {
        self.sell(ware, self.cargo(ware))
    }
    pub fn buy(&mut self, ware: &WareId) -> Result {
        self.trade(ware, TradeSide::Buy, self.free() as u32)
    }
    pub fn offered(&self, req: &ServiceRequest) -> bool {
        galaxy_progress::services::check(self.world(), self.game.sim().history(), req).is_ok()
    }
    pub fn service(&mut self, req: ServiceRequest) -> Result {
        galaxy_progress::services::check(self.world(), self.game.sim().history(), &req)
            .map_err(|e| format!("{req:?}: {e}"))?;
        self.input(PlayerInput {
            services: vec![req],
            ..PlayerInput::default()
        })?;
        self.idle(4)
    }
    pub fn fit(&mut self, id: &str) -> Result {
        self.service(ServiceRequest::Fit {
            upgrade: id.parse().expect("upgrade id"),
        })
    }
    pub fn reply(&mut self, npc: &str, choice: &str) -> Result {
        self.input(PlayerInput {
            replies: vec![Reply {
                npc: npc.parse().expect("npc"),
                choice: choice.parse().expect("choice"),
            }],
            ..PlayerInput::default()
        })?;
        self.idle(4)
    }
    pub fn orders(&mut self, orders: Vec<Order>) -> Result {
        self.act(Action::Orders(orders))
    }
    pub fn queue(&mut self, orders: Vec<Order>) -> Result {
        self.orders(orders)?;
        self.wait("queue", |p| {
            p.ship().orders.current >= p.ship().orders.orders.len()
        })?;
        self.idle(4)
    }
    pub fn save_loop(&mut self, name: &str, orders: Vec<Order>) -> Result {
        self.orders(orders)?;
        self.act(Action::SaveLoop(name.into()))?;
        self.orders(vec![])
    }
    pub fn finish_recording(&mut self) {
        if self.options.record {
            self.recording.checkpoints.push(Checkpoint::of(&self.game));
        }
    }
    pub fn report(&self, error: Option<String>) -> Value {
        let facts = self.game.sim().history().facts();
        let gm_facts = facts
            .iter()
            .filter(|f| f.kind.as_str().starts_with("gm."))
            .count();
        let gm_causes = self.game.sim().history().causes().values().filter(|c|
            matches!(&c.cause.kind, CauseKind::Script { pack, .. } if pack.as_str().starts_with("gm."))).count();
        let missing: Vec<_> = [
            "gm.border_moved",
            "gm.rumor_exposed",
            "gm.mechanic_taught",
            "gm.canon_opened",
            "gm.station_founded",
            "gm.chronicle",
        ]
        .into_iter()
        .filter(|k| !self.has(k))
        .collect();
        json!({"ok":error.is_none(),"error":error,"tick":self.game.sim().tick(),
            "from_tick":self.start,"ticks_run":self.game.sim().tick().0-self.start,
            "target":{"beat":self.options.until,"at_start":self.options.at_start},
            "packs":self.game.reports().iter().filter(|r|r.accepted()).map(|r|r.pack.clone()).collect::<Vec<_>>(),
            "beats":self.beats,"completed":BEATS.iter().filter(|id|self.world().progress.beats.get(**id).is_some_and(|b|b.status==BeatStatus::Completed)).count(),
            "dockings":self.counter(CounterKind::Dockings),"credits":self.credits,
            "loop_stalled":facts.iter().filter(|f|f.kind.as_str()=="loop_stalled").collect::<Vec<_>>(),
            "script_events":self.script_events,
            "script_health":super::health(&self.game),"rejected":self.rejected,
            "gm_facts":gm_facts,"gm_causes":gm_causes,"missing_gm_change_types":missing,
            "hash":self.game.sim().hash()})
    }
}
pub(super) fn dock(s: StationId) -> Order {
    Order::Dock { station: s }
}
pub(super) fn buy(w: WareId) -> Order {
    Order::Buy {
        ware: w,
        quantity: Quantity::All,
    }
}
pub(super) fn sell(w: WareId) -> Order {
    Order::Sell {
        ware: w,
        quantity: Quantity::All,
        floor: None,
    }
}
