use super::pilot::*;
use super::replay::Action;
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::Order;

impl Pilot {
    fn ore_run(&mut self) -> Result {
        if self.rock(&TIN).is_none() {
            self.renewable_rock(&TIN)?;
        }
        self.fill(&TIN)?;
        self.go(&WHARF)?;
        self.sell_all(&ORE)
    }
    fn ice_run(&mut self) -> Result {
        self.fill(&RIME)?;
        self.go(&KETTLE)?;
        self.sell_all(&ICE)
    }
    fn upgrades(&mut self) -> Result {
        if self.ship().docked_at.as_ref() != Some(&WHARF) {
            return Ok(());
        }
        for id in [
            "upgrade.rock_cradle",
            "upgrade.docking_clamp",
            "upgrade.hold_extender",
        ] {
            let req = ServiceRequest::Fit {
                upgrade: id.parse().expect("upgrade"),
            };
            if !self.fitted(id) && self.offered(&req) {
                self.service(req)?;
            }
        }
        Ok(())
    }
    fn afford_fit(&mut self, id: &str) -> Result {
        if id == "upgrade.mining_lens" && !self.has("sell") {
            self.ore_run()?;
        }
        let req = ServiceRequest::Fit {
            upgrade: id.parse().expect("upgrade"),
        };
        for _ in 0..200 {
            self.go(&WHARF)?;
            if self.offered(&req) {
                return self.fit(id);
            }
            self.ore_run()?;
        }
        Err(Halt::Failed(format!("could not earn {id}")))
    }
    fn side(&mut self) -> Result {
        if self.active(9) {
            self.reply("npc.rook_adeyemi", "choice.side_coop")?;
        }
        Ok(())
    }
    /// Split glassite across buyers, then carry Brassmouth plate to the Hook.
    fn circuit(&mut self) -> Result {
        self.renewable_rock(&GLASS_FIELD)?;
        self.fill(&GLASS_FIELD)?;
        self.go(&BRASS)?;
        self.sell_all(&ORE)?;
        self.sell(&GLASS, 12)?;
        self.go(&GDO)?;
        self.sell_all(&GLASS)?;
        self.go(&BRASS)?;
        // Activity and other traders can saturate the Hook before we arrive.
        // Size this leg to the profitable part of today's quotes; a blind full
        // hold can consume the mining income needed for the next service.
        let ask = galaxy_economy::quote(
            self.world(),
            &BRASS,
            &PLATE,
            TradeSide::Buy,
            self.free() as u32,
        )
        .map_err(|e| e.to_string())?;
        let bid = galaxy_economy::quote(self.world(), &HOOK, &PLATE, TradeSide::Sell, ask.units())
            .map_err(|e| e.to_string())?;
        let quantity = ask
            .unit_prices
            .iter()
            .zip(&bid.unit_prices)
            .take_while(|(ask, bid)| bid > ask)
            .count() as u32;
        // Supply the chosen side while the dispute is open, even when that
        // contribution costs money. Afterwards the circuit is for earning.
        if self.done(9) {
            self.trade(&PLATE, TradeSide::Buy, quantity)?;
        } else {
            self.buy(&PLATE)?;
        }
        self.go(&HOOK)?;
        self.side()?;
        self.sell_all(&PLATE)?;
        Ok(())
    }
    fn shuttle(&mut self) -> Result {
        self.go(&WHARF)?;
        self.go(&KETTLE)
    }
    fn glass_loop(&mut self) -> Result {
        let rock = self.renewable_rock(&GLASS_FIELD)?;
        // The current Order contract mines a rock, not a field. Refresh the target
        // between laps so depletion doesn't turn the field policy into a dead loop.
        self.save_loop(
            "glass run",
            vec![
                Order::Mine { rock },
                dock(GDO),
                sell(GLASS),
                dock(HOOK),
                buy(GLASS),
                dock(BRASS),
                sell(GLASS),
            ],
        )?;
        let laps = self.laps();
        self.act(Action::RunLoop("glass run".into()))?;
        self.wait("glass run lap", |p| p.laps() > laps)?;
        self.orders(vec![])?;
        self.idle(4)
    }
    pub fn journey(&mut self) -> Result {
        self.idle(3)?;
        if !self.done(1) {
            self.undock()?;
            self.idle(3)?;
        }
        if !self.done(3) {
            self.fill(&RIME)?;
            self.go(&KETTLE)?;
            self.idle(4)?;
        }
        if !self.done(4) {
            self.afford_fit("upgrade.mining_lens")?;
        }
        if !self.done(5) {
            self.act(Action::Pause)?;
            self.idle(4)?;
        }
        if !self.done(6) {
            for _ in 0..150 {
                self.ore_run()?;
                let req = ServiceRequest::Fit {
                    upgrade: "upgrade.nav_chip".parse().expect("id"),
                };
                if self.offered(&req) {
                    self.service(req)?;
                    break;
                }
                self.ice_run()?;
            }
        }
        if !self.done(7) {
            self.go(&WHARF)?;
            self.sell_all(&ORE)?;
            self.sell_all(&ICE)?;
            self.idle(5)?;
            self.go(&HOOK)?;
            self.idle(4)?;
        }
        if !self.done(9) || !self.done(8) {
            for _ in 0..100 {
                if self.done(8) && self.done(9) {
                    break;
                }
                self.circuit()?;
                self.go(&WHARF)?;
                self.upgrades()?;
            }
        }
        if !self.done(10) {
            for _ in 0..12 {
                if self.has("core.rumor_heard") {
                    break;
                }
                self.circuit()?;
            }
            self.go(&HOOK)?;
            let at = Vec2::new(
                self.number("rumor.ember_shoals.x") as f32,
                self.number("rumor.ember_shoals.y") as f32,
            );
            self.click(ClickTarget::Point { at })?;
            self.idle(4)?;
            // Mine the revealed vein, whether rich glassite or stale iron.
            let field = self
                .world()
                .fields
                .values()
                .find(|f| f.id.as_str().contains("ember"))
                .map(|f| f.id.clone());
            if let Some(field) = field {
                self.fill(&field)?;
            }
            self.go(&HOOK)?;
            self.go(&BRASS)?;
            self.sell_all(&ORE)?;
            self.go(&GDO)?;
            self.sell_all(&GLASS)?;
        }
        // Run A's installed growth pack reads real sales/deliveries at Kettle.
        if self.cap("flag.gm_examples")
            && self
                .world()
                .stations
                .get(&KETTLE)
                .expect("Kettle")
                .build
                .stage
                < 2
        {
            for _ in 0..4 {
                self.ice_run()?;
            }
            self.go(&WHARF)?;
            self.buy(&PLATE)?;
            self.go(&KETTLE)?;
            self.sell_all(&PLATE)?;
        }
        if !self.done(11) {
            for _ in 0..150 {
                if self.counter(CounterKind::Dockings) as f64 >= self.number("b11.dockings") {
                    break;
                }
                self.circuit()?;
            }
            self.go(&WHARF)?;
            self.upgrades()?;
            if !self.fitted("upgrade.order_slate") {
                self.afford_fit("upgrade.order_slate")?;
            }
            if !self.fitted("upgrade.loop_log") {
                for _ in 0..4 {
                    let rock = self.renewable_rock(&TIN)?;
                    self.queue(vec![Order::Mine { rock }, dock(WHARF), sell(ORE)])?;
                }
                self.fit("upgrade.loop_log")?;
            }
        }
        while (self.laps() as f64) < self.number("b13.loop_laps") {
            self.glass_loop()?;
        }
        self.orders(vec![])?;
        if !self.done(12) {
            for _ in 0..150 {
                if self.active(12) {
                    break;
                }
                self.ice_run()?;
            }
            for _ in 0..50 {
                if self.done(12) {
                    break;
                }
                self.go(&BRASS)?;
                self.buy(&ICE)?;
                self.go(&KETTLE)?;
                self.sell_all(&ICE)?;
            }
        }
        if !self.done(13) {
            eprintln!(
                "journey: tender lesson at tick {}",
                self.game.sim().tick().0
            );
            for _ in 0..100 {
                if self.step(13) >= 1 {
                    break;
                }
                self.ore_run()?;
                self.go(&KETTLE)?;
            }
            if self.step(13) < 1 {
                return Err(Halt::Failed(
                    "Little Pail's demonstration did not finish".into(),
                ));
            }
            eprintln!(
                "journey: tender lesson finished at tick {}",
                self.game.sim().tick().0
            );
            let thimble = ShipId::from_static("craft.thimble");
            if !self.world().ships.contains(&thimble) {
                let req = ServiceRequest::Build {
                    craft: thimble.clone(),
                };
                for _ in 0..100 {
                    self.go(&KETTLE)?;
                    let cost = self
                        .world()
                        .progress
                        .rules
                        .crafts
                        .get(&thimble)
                        .expect("Thimble")
                        .cost;
                    if self.offered(&req) && self.world().player.credits >= cost + 100_000 {
                        break;
                    }
                    self.circuit()?;
                }
                self.go(&KETTLE)?;
                self.service(req)?;
            }
            self.save_loop(
                "Wharf plate run",
                vec![dock(WHARF), buy(PLATE), dock(KETTLE), sell(PLATE)],
            )?;
            self.save_loop(
                "plate run",
                vec![dock(BRASS), buy(PLATE), dock(HOOK), sell(PLATE)],
            )?;
            self.act(Action::Assign {
                name: "plate run".into(),
                ship: thimble.clone(),
                floor: 5000,
            })?;
            let mut fallback = false;
            for _ in 0..200 {
                if self.done(13) {
                    break;
                }
                if !fallback
                    && self.game.sim().history().facts().iter().any(|f| {
                        f.subjects.ship.as_ref() == Some(&thimble)
                            && matches!(f.kind.as_str(), "loop_stalled" | "patient_blocked")
                    })
                {
                    self.save_loop(
                        "Wharf plate run",
                        vec![dock(WHARF), buy(PLATE), dock(KETTLE), sell(PLATE)],
                    )?;
                    self.act(Action::Assign {
                        name: "Wharf plate run".into(),
                        ship: thimble.clone(),
                        floor: 4000,
                    })?;
                    fallback = true;
                }
                if fallback {
                    self.shuttle()?;
                } else {
                    // Prices recover on local activity. Keep visiting the tender's
                    // buyer and its neighbours while it waits for the floor.
                    self.go(&GDO)?;
                    self.go(&HOOK)?;
                    self.go(&BRASS)?;
                }
            }
            if !self.done(13) {
                return Err(Halt::Failed(format!(
                    "Thimble did not finish a patient lap: {:?}",
                    self.world().ships.get(&thimble).map(|s| &s.orders)
                )));
            }
        }
        if !self.done(14) {
            for _ in 0..100 {
                if !self.world().player.traits.is_empty() {
                    break;
                }
                self.circuit()?;
            }
            self.go(&WHARF)?;
            self.reply("npc.orrin_vale", "choice.ask_orrin")?;
        }
        if !self.done(15) {
            for _ in 0..150 {
                if self.has("core.fragment_found") {
                    break;
                }
                let rock = self.renewable_rock(&GLASS_FIELD)?;
                self.mine(rock)?;
                self.go(&GDO)?;
                self.sell_all(&GLASS)?;
            }
            self.go(&HOOK)?;
            self.go(&GDO)?;
            self.go(&KETTLE)?;
            self.go(&GDO)?;
            self.reply("npc.nell_quillon", "choice.fragment_leave")?;
        }
        if !self.done(16) {
            let req = ServiceRequest::Prestige {
                to: self.options.prestige.clone(),
            };
            for _ in 0..250 {
                self.go(&WHARF)?;
                if self.offered(&req) {
                    break;
                }
                self.circuit()?;
            }
            self.service(req)?;
        }
        if !self.done(17) {
            for s in [KETTLE, HOOK, BRASS, WHARF] {
                self.go(&s)?;
            }
            self.reply("npc.ilse_marrow", "choice.what_changed")?;
        }
        self.idle(5)?;
        if (1..=17).any(|n| !self.done(n)) {
            return Err(Halt::Failed("route ended with incomplete beats".into()));
        }
        Ok(())
    }
}
