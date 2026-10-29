//! [`MindHost`]: system 17, the fast-model layer's driver.
//!
//! It runs outside the tick, like the game-master bridge: call [`MindHost::pump`] once
//! per frame (or after each `Sim::step`). Each pump
//!
//! 1. reads the history facts committed since the last pump and lets each
//!    [`MenuProvider`] open menus (requests fire from events, never from a clock);
//! 2. answers each menu from the situation cache, or queues a model call if the
//!    region's budget allows, or takes the utility default at once;
//! 3. collects finished calls, validates them and falls back where they fail;
//! 4. takes the default for any call past its deadline (in ticks, so a pause loses
//!    nothing);
//! 5. submits one `CommandBatch` per outcome with `Cause::Mind { npc, decision }`. Its
//!    `mind.decision` or `mind.line` fact records the request key, the source, the
//!    calls and the cost, so dock talk can cite it, rumors can carry it and the game
//!    master can read it.
//!
//! Pumping never blocks. With an [`crate::Inline`] executor and a deterministic mind
//! every answer lands in the same pump, so runs repeat exactly.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::{History, Sim};
use serde::Serialize;
use serde_json::json;

use crate::config::{Brief, MindConfig};
use crate::executor::{Done, Executor, Job, JobResult};
use crate::menu::{DOCK_TALK, LineCandidate, Menu, MenuCtx, MenuProvider, RumorSpread};
use crate::mind::{DecisionRequest, LineRequest, MindError, Question};
use crate::packet::{self, Names};
use crate::situation::{self, Cached, Ledger, LineNote, OutcomeNote, Source, micros};
use crate::validate::{self, LineRules};
use crate::{DECISION_FACT, LINE_FACT};

/// A line the layer decided someone says. Show it through the talk channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpokenLine {
    /// The authored line it came from.
    pub line: String,
    pub talk: TalkLine,
    pub source: Source,
}

/// What one pump did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PumpReport {
    pub menus: usize,
    pub batches: usize,
    pub lines: Vec<SpokenLine>,
    pub in_flight: usize,
}

/// Running totals for this session (not saved; history holds the record).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub menus: u64,
    pub requests: u64,
    pub model: u64,
    pub cache: u64,
    pub defaults: u64,
    pub skipped: u64,
    /// Answers that arrived after their deadline and were dropped.
    pub late_dropped: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
}

/// An outcome being put together across stages.
#[derive(Clone, Debug)]
struct Acc {
    key: String,
    source: Source,
    why: Vec<String>,
    models: Vec<String>,
    calls: u32,
    cost: i64,
    region: Option<RegionId>,
    window: u64,
}

impl Acc {
    fn spend(&mut self, model: &str, attempts: u8, cost_usd: f64) {
        self.models.push(model.to_owned());
        self.calls += u32::from(attempts);
        self.cost += micros(cost_usd);
    }

    fn failed(&mut self, e: &MindError) {
        // A retried failure made two calls; a missing key or recording made none.
        self.calls += match e {
            MindError::NoKey | MindError::NotRecorded => 0,
            e if e.retryable() => 2,
            _ => 1,
        };
        self.why.push(e.tag().to_owned());
    }
}

#[derive(Clone, Debug)]
enum Stage {
    Pick,
    Phrase {
        chosen: usize,
        key: String,
        packet: Vec<FactId>,
    },
}

struct Pending {
    provider: usize,
    menu: Menu,
    deadline: Tick,
    stage: Stage,
    acc: Acc,
}

pub struct MindHost {
    config: MindConfig,
    exec: Box<dyn Executor>,
    providers: Vec<Box<dyn MenuProvider>>,
    ledger: Ledger,
    /// Outcomes submitted but not yet seen in history, so a burst can't overspend.
    posted: Vec<(OutcomeNote, i64)>,
    seen: Option<FactId>,
    pending: BTreeMap<u64, Pending>,
    next_ticket: u64,
    stats: Stats,
}

/// Work produced during a pump, submitted at its end.
#[derive(Default)]
struct Out {
    batches: Vec<CommandBatch>,
    lines: Vec<SpokenLine>,
}

impl MindHost {
    /// A host that reacts to facts committed after `history`'s last one. Rumor spread
    /// is built in; add trader menus and dock talk with [`MindHost::add_provider`].
    pub fn new(config: MindConfig, exec: impl Executor + 'static, history: &History) -> Self {
        let mut host = Self {
            config,
            exec: Box::new(exec),
            providers: vec![Box::new(RumorSpread)],
            ledger: Ledger::default(),
            posted: Vec::new(),
            seen: history.last_fact_id(),
            pending: BTreeMap::new(),
            next_ticket: 1,
            stats: Stats::default(),
        };
        host.catch_up(history);
        host
    }

    pub fn add_provider(&mut self, provider: impl MenuProvider + 'static) -> &mut Self {
        self.providers.push(Box::new(provider));
        self
    }

    pub fn config(&self) -> &MindConfig {
        &self.config
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Requests waiting for an answer.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// Calls counted in a region's current window, committed or just submitted.
    pub fn calls_in_window(&self, region: Option<&RegionId>, window: u64) -> u32 {
        self.ledger.calls(region, window)
            + self
                .posted
                .iter()
                .filter(|(n, _)| n.region.as_ref() == region && n.window == window)
                .map(|(n, _)| n.calls)
                .sum::<u32>()
    }

    /// Dollars spent in the whole save so far.
    pub fn spent_usd(&self) -> f64 {
        let posted: i64 = self.posted.iter().map(|(_, c)| c).sum();
        (self.ledger.total() + posted) as f64 / 1_000_000.0
    }

    /// One pass: new facts to menus, answers to batches. Never blocks.
    pub fn pump(&mut self, sim: &mut Sim) -> PumpReport {
        let tick = sim.tick();
        let mut out = Out::default();
        let menus;
        {
            let world = sim.world();
            let history = sim.history();
            self.catch_up(history);

            let facts = history.facts();
            let from = self
                .seen
                .map_or(0, |id| facts.partition_point(|f| f.id <= id));
            let cx = MenuCtx {
                world,
                history,
                config: &self.config,
            };
            let mut opened = Vec::new();
            for f in &facts[from..] {
                if f.kind.as_str().starts_with("mind.") {
                    continue;
                }
                for (i, p) in self.providers.iter().enumerate() {
                    opened.extend(p.menus(&cx, f).into_iter().map(|m| (i, m)));
                }
            }
            self.seen = history.last_fact_id();
            menus = opened.len();

            for (provider, menu) in opened {
                self.open(world, history, tick, provider, menu, &mut out);
            }
            for done in self.exec.poll() {
                self.answer(world, history, tick, done, &mut out);
            }
            let late: Vec<u64> = self
                .pending
                .iter()
                .filter(|(_, p)| p.deadline <= tick)
                .map(|(t, _)| *t)
                .collect();
            for ticket in late {
                let p = self.pending.remove(&ticket).expect("listed above");
                self.give_up(world, history, tick, p, "late", &mut out);
            }
        }
        let report = PumpReport {
            menus,
            batches: out.batches.len(),
            lines: out.lines,
            in_flight: self.pending.len(),
        };
        for b in out.batches {
            sim.submit(b);
        }
        report
    }

    /// Folds new `mind.*` facts into the ledger; a fact we posted ourselves stops
    /// being counted as posted once it has committed.
    fn catch_up(&mut self, history: &History) {
        let facts = history.facts();
        let from = self
            .ledger
            .cursor()
            .map_or(0, |id| facts.partition_point(|f| f.id <= id));
        for f in &facts[from..] {
            self.ledger.observe(f);
            if let Some(note) = OutcomeNote::parse(f)
                && let Some(i) = self.posted.iter().position(|(n, _)| *n == note)
            {
                self.posted.remove(i);
            }
        }
    }

    fn window(&self, world: &WorldState, region: Option<&RegionId>) -> u64 {
        let w = self.config.budget.for_region(region).window_dockings;
        if w == 0 {
            0
        } else {
            world
                .counters
                .get(CounterKind::Dockings, &CounterScope::Player)
                / u64::from(w)
        }
    }

    fn budget(&self, acc: &Acc) -> Result<(), &'static str> {
        let region = acc.region.as_ref();
        let b = self.config.budget.for_region(region);
        let reserved = self
            .pending
            .values()
            .filter(|p| p.acc.region.as_ref() == region)
            .count() as u32;
        if b.calls == 0 || self.calls_in_window(region, acc.window) + reserved >= b.calls {
            return Err("budget");
        }
        let posted: i64 = self
            .posted
            .iter()
            .filter(|(n, _)| n.region.as_ref() == region)
            .map(|(_, c)| c)
            .sum();
        if self.ledger.spent(region) + posted >= micros(b.spend_cap_usd)
            || self.spent_usd() >= self.config.budget.spend_cap_usd
        {
            return Err("spend_cap");
        }
        Ok(())
    }

    fn ticket(&mut self) -> u64 {
        let t = self.next_ticket;
        self.next_ticket += 1;
        t
    }

    fn open(
        &mut self,
        world: &WorldState,
        history: &History,
        tick: Tick,
        provider: usize,
        menu: Menu,
        out: &mut Out,
    ) {
        if menu.options.is_empty() {
            return;
        }
        self.stats.menus += 1;
        let region = menu
            .station
            .as_ref()
            .and_then(|s| world.stations.get(s))
            .map(|s| s.region.clone());
        let window = self.window(world, region.as_ref());
        let brief = self.config.brief(menu.actor.as_str()).cloned();
        // Dock talk is a pick among authored lines, not one NPC's decision.
        let talk = menu.kind == DOCK_TALK;
        let packet = packet::build(
            world,
            history,
            &menu,
            if talk { None } else { brief.as_ref() },
            &self.config.limits,
        );
        // Keyed by what the answer depends on: the brief's revision, the facts the menu
        // is about (not the routine docking that opened it) and the menu itself.
        let facts: Vec<FactId> = menu
            .cites
            .iter()
            .copied()
            .filter(|f| *f != menu.trigger)
            .collect();
        let options: Vec<(String, String)> = menu
            .options
            .iter()
            .map(|o| (o.id.clone(), o.text.clone()))
            .collect();
        let key = situation::key(&json!({
            "menu": menu.kind,
            "actor": menu.actor,
            "brief": brief.as_ref().map_or(0, |b| b.revision),
            "facts": facts,
            "options": options,
        }));
        let mut acc = Acc {
            key: key.clone(),
            source: Source::Default,
            why: Vec::new(),
            models: Vec::new(),
            calls: 0,
            cost: 0,
            region,
            window,
        };

        let may_ask = if talk {
            self.config.talk.pick
        } else {
            brief.as_ref().is_some_and(|b| b.allows(menu.kind))
        };
        let cached = match self.ledger.cached(&key) {
            Some(Cached::Choice(id)) => menu.options.iter().position(|o| &o.id == id),
            _ => None,
        };
        let reason = if menu.options.len() == 1 {
            Some("single")
        } else if !may_ask {
            Some("not_allowed")
        } else if cached.is_some() {
            None
        } else {
            self.budget(&acc).err()
        };
        if let Some(i) = cached.filter(|_| reason.is_none()) {
            acc.source = Source::Cache;
            return self.choose(world, history, tick, provider, menu, acc, i, out);
        }
        if let Some(why) = reason {
            acc.why.push(why.to_owned());
            return self.choose(world, history, tick, provider, menu, acc, 0, out);
        }

        let mut instructions = String::new();
        if let (false, Some(b)) = (talk, &brief) {
            instructions.push_str(&format!("You decide for {}, {}. ", b.name, b.role));
        }
        instructions.push_str(&menu.instructions);
        instructions.push_str(" Every option is allowed; choose the one that fits best.");
        let req = DecisionRequest {
            key,
            instructions,
            state: packet.to_value(),
            question: Question::choice(&options),
        };
        let ticket = self.ticket();
        if !self.exec.submit(ticket, Job::Decide(req)) {
            acc.why.push("busy".into());
            return self.choose(world, history, tick, provider, menu, acc, 0, out);
        }
        self.stats.requests += 1;
        self.pending.insert(
            ticket,
            Pending {
                provider,
                menu,
                deadline: Tick(tick.0 + self.config.limits.deadline_ticks),
                stage: Stage::Pick,
                acc,
            },
        );
    }

    fn answer(
        &mut self,
        world: &WorldState,
        history: &History,
        tick: Tick,
        done: Done,
        out: &mut Out,
    ) {
        let Some(p) = self.pending.remove(&done.ticket) else {
            self.stats.late_dropped += 1;
            return;
        };
        self.stats.latency_ms += done.latency_ms;
        let Pending {
            provider,
            menu,
            stage,
            mut acc,
            ..
        } = p;
        match (stage, done.result) {
            (Stage::Pick, JobResult::Decide(Ok(reply))) => {
                acc.spend(&reply.model, reply.attempts, reply.cost_usd);
                self.stats.cost_usd += reply.cost_usd;
                match validate::choice(&menu, &reply.value) {
                    Ok(i) => {
                        acc.source = Source::Model;
                        self.choose(world, history, tick, provider, menu, acc, i, out);
                    }
                    Err(e) => {
                        acc.why.push(e.to_string());
                        self.choose(world, history, tick, provider, menu, acc, 0, out);
                    }
                }
            }
            (Stage::Pick, JobResult::Decide(Err(e))) => {
                acc.failed(&e);
                self.choose(world, history, tick, provider, menu, acc, 0, out);
            }
            (
                Stage::Phrase {
                    chosen,
                    key,
                    packet,
                },
                JobResult::Line(result),
            ) => {
                let line = menu.options[chosen].line.clone().expect("phrase stage");
                let said = match result {
                    Ok(reply) => {
                        acc.spend(&reply.model, reply.attempts, reply.cost_usd);
                        self.stats.cost_usd += reply.cost_usd;
                        let names = self.names(world, &line);
                        let forbidden = self.forbidden(&line);
                        let rules = LineRules {
                            cite: line.cites,
                            packet: &packet,
                            max_words: self.max_words(&line),
                            max_chars: self.config.talk.max_chars,
                            forbidden: &forbidden,
                            names: &names,
                            authored: &line.text,
                        };
                        match validate::line(&reply.value, &rules) {
                            Ok(clean) => Some((clean.text, clean.cites)),
                            Err(e) => {
                                acc.why.push(e.to_string());
                                None
                            }
                        }
                    }
                    Err(e) => {
                        acc.failed(&e);
                        None
                    }
                };
                let source = if said.is_some() {
                    Source::Model
                } else {
                    Source::Default
                };
                let (text, cites) = said.unwrap_or((line.text.clone(), vec![line.cites]));
                self.speak(&menu, acc, chosen, &key, source, text, cites, out);
            }
            (stage, _) => {
                acc.why.push("mismatched answer".into());
                let p = Pending {
                    provider,
                    menu,
                    deadline: tick,
                    stage,
                    acc,
                };
                self.give_up(world, history, tick, p, "", out);
            }
        }
    }

    /// The answer didn't come: take the default (or the authored line).
    fn give_up(
        &mut self,
        world: &WorldState,
        history: &History,
        tick: Tick,
        p: Pending,
        why: &str,
        out: &mut Out,
    ) {
        let Pending {
            provider,
            menu,
            stage,
            mut acc,
            ..
        } = p;
        if !why.is_empty() {
            acc.why.push(why.to_owned());
        }
        match stage {
            Stage::Pick => self.choose(world, history, tick, provider, menu, acc, 0, out),
            Stage::Phrase { chosen, key, .. } => {
                let line = menu.options[chosen].line.clone().expect("phrase stage");
                let cites = vec![line.cites];
                self.speak(
                    &menu,
                    acc,
                    chosen,
                    &key,
                    Source::Default,
                    line.text,
                    cites,
                    out,
                );
            }
        }
    }

    /// Applies option `index`, falling back to the default if it no longer holds, and
    /// skipping the moment if the default doesn't either.
    #[allow(clippy::too_many_arguments)]
    fn choose(
        &mut self,
        world: &WorldState,
        history: &History,
        tick: Tick,
        provider: usize,
        menu: Menu,
        mut acc: Acc,
        index: usize,
        out: &mut Out,
    ) {
        if menu.options[index].line.is_some() {
            return self.phrase(world, history, tick, provider, menu, acc, index, out);
        }
        let mut tries = vec![index];
        if index != 0 {
            tries.push(0);
        }
        for i in tries {
            let option = &menu.options[i];
            let result = validate::option(world, &menu, option)
                .map_err(|e| e.to_string())
                .and_then(|()| self.providers[provider].resolve(world, &menu, option));
            match result {
                Ok(commands) => {
                    if i != index {
                        acc.source = Source::Default;
                    }
                    let batch = self.decision_batch(world, &menu, &acc, Some(i), commands);
                    out.batches.push(batch);
                    self.count(acc.source);
                    return;
                }
                Err(e) => acc.why.push(format!("{}: {e}", option.id)),
            }
        }
        acc.source = Source::Default;
        self.stats.skipped += 1;
        let batch = self.decision_batch(world, &menu, &acc, None, Vec::new());
        out.batches.push(batch);
    }

    fn count(&mut self, source: Source) {
        match source {
            Source::Model => self.stats.model += 1,
            Source::Cache => self.stats.cache += 1,
            Source::Default => self.stats.defaults += 1,
        }
    }

    fn line_brief(&self, line: &LineCandidate) -> Option<&Brief> {
        self.config
            .brief(line.npc.as_str())
            .filter(|b| b.allows(DOCK_TALK))
    }

    fn max_words(&self, line: &LineCandidate) -> u32 {
        self.line_brief(line)
            .and_then(|b| b.max_words)
            .unwrap_or(self.config.talk.max_words)
            .min(self.config.talk.max_words)
    }

    fn forbidden(&self, line: &LineCandidate) -> Vec<String> {
        let mut f = self.config.talk.forbidden.clone();
        if let Some(b) = self.line_brief(line) {
            f.extend(b.off_limits.iter().cloned());
        }
        f
    }

    fn names(&self, world: &WorldState, line: &LineCandidate) -> BTreeSet<String> {
        let mut names = Names(world).all();
        if let Some(b) = self.line_brief(line) {
            names.insert(b.name.clone());
        }
        names
    }

    /// Dock talk, second stage: the chosen line in the speaker's voice, or as written.
    #[allow(clippy::too_many_arguments)]
    fn phrase(
        &mut self,
        world: &WorldState,
        history: &History,
        tick: Tick,
        provider: usize,
        menu: Menu,
        acc: Acc,
        chosen: usize,
        out: &mut Out,
    ) {
        let line = menu.options[chosen]
            .line
            .clone()
            .expect("checked by caller");
        let brief = self.line_brief(&line).cloned();
        let voiced = Menu {
            npc: Some(line.npc.clone()),
            cites: vec![line.cites],
            options: Vec::new(),
            ..menu.clone()
        };
        let packet = packet::build(world, history, &voiced, brief.as_ref(), &self.config.limits);
        let ids = packet.fact_ids();
        let key = situation::key(&json!({
            "line": line.line,
            "npc": line.npc,
            "brief": brief.as_ref().map_or(0, |b| b.revision),
            "cite": line.cites,
            "authored": line.text,
        }));
        let authored = |host: &mut Self, acc: Acc, why: &str, out: &mut Out| {
            let mut acc = acc;
            acc.why.push(why.to_owned());
            let cites = vec![line.cites];
            host.speak(
                &menu,
                acc,
                chosen,
                &key,
                Source::Default,
                line.text.clone(),
                cites,
                out,
            );
        };
        let Some(brief) = brief.filter(|_| self.config.talk.phrase) else {
            return authored(self, acc, "no_voice", out);
        };
        if let Some(Cached::Line { text, cites }) = self.ledger.cached(&key).cloned() {
            return self.speak(&menu, acc, chosen, &key, Source::Cache, text, cites, out);
        }
        if let Err(why) = self.budget(&acc) {
            return authored(self, acc, why, out);
        }
        let max_words = self.max_words(&line);
        let req = LineRequest {
            key: key.clone(),
            system: system_prompt(&brief, max_words),
            packet: packet.to_value(),
            authored: line.text.clone(),
            cite: line.cites.to_string(),
            allowed_cites: ids.iter().map(ToString::to_string).collect(),
            max_words,
        };
        let ticket = self.ticket();
        if !self.exec.submit(ticket, Job::Line(req)) {
            return authored(self, acc, "busy", out);
        }
        self.stats.requests += 1;
        self.pending.insert(
            ticket,
            Pending {
                provider,
                menu,
                deadline: Tick(tick.0 + self.config.limits.deadline_ticks),
                stage: Stage::Phrase {
                    chosen,
                    key,
                    packet: ids,
                },
                acc,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn speak(
        &mut self,
        menu: &Menu,
        acc: Acc,
        chosen: usize,
        line_key: &str,
        source: Source,
        text: String,
        cites: Vec<FactId>,
        out: &mut Out,
    ) {
        let line = menu.options[chosen].line.clone().expect("a line option");
        let note = OutcomeNote {
            menu: menu.kind.to_owned(),
            actor: menu.actor.to_string(),
            key: acc.key.clone(),
            source: acc.source,
            choice: Some(menu.options[chosen].id.clone()),
            default: Some(menu.options[0].id.clone()),
            why: acc.why.clone(),
            models: acc.models.clone(),
            calls: acc.calls,
            window: acc.window,
            region: acc.region.clone(),
            line: Some(LineNote {
                id: line.line.clone(),
                key: line_key.to_owned(),
                source,
                text: text.clone(),
                cites: cites.clone(),
            }),
        };
        self.count(source);
        let fact = FactDraft {
            kind: FactKind::new(LINE_FACT).expect("valid kind"),
            subjects: Subjects {
                npc: Some(line.npc.clone()),
                station: menu.station.clone(),
                region: acc.region.clone(),
                other: AnyId::new(&line.line).ok(),
                ..Subjects::default()
            },
            amount: Some(acc.cost),
            note: serde_json::to_string(&note).expect("notes serialize"),
        };
        let cause = Cause {
            kind: CauseKind::Mind {
                npc: Some(line.npc.clone()),
                decision: format!("{}:{}", menu.kind, line.line),
            },
            summary: format!("{} says: {text}", line.npc),
            cites: cites.clone(),
        };
        self.posted.push((note, acc.cost));
        out.batches.push(CommandBatch {
            cause,
            commands: vec![Command::RecordFact {
                fact: Box::new(fact),
            }],
        });
        out.lines.push(SpokenLine {
            line: line.line,
            talk: TalkLine {
                channel: Channel::DockTalk,
                speaker: Some(line.npc),
                place: menu.station.clone(),
                text,
                cites,
            },
            source,
        });
    }

    fn decision_batch(
        &mut self,
        world: &WorldState,
        menu: &Menu,
        acc: &Acc,
        chosen: Option<usize>,
        mut commands: Vec<Command>,
    ) -> CommandBatch {
        let option = chosen.map(|i| &menu.options[i]);
        let note = OutcomeNote {
            menu: menu.kind.to_owned(),
            actor: menu.actor.to_string(),
            key: acc.key.clone(),
            source: acc.source,
            choice: option.map(|o| o.id.clone()),
            default: Some(menu.options[0].id.clone()),
            why: acc.why.clone(),
            models: acc.models.clone(),
            calls: acc.calls,
            window: acc.window,
            region: acc.region.clone(),
            line: None,
        };
        commands.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::new(DECISION_FACT).expect("valid kind"),
                subjects: Subjects {
                    npc: menu.npc.clone(),
                    ship: menu.ship.clone(),
                    station: menu.station.clone(),
                    region: acc.region.clone(),
                    ..Subjects::default()
                },
                amount: Some(acc.cost),
                note: serde_json::to_string(&note).expect("notes serialize"),
            }),
        });
        self.posted.push((note, acc.cost));
        let who = Names(world).of(menu.actor.as_str());
        let mut cites = menu.cites.clone();
        if !cites.contains(&menu.trigger) {
            cites.push(menu.trigger);
        }
        CommandBatch {
            cause: Cause {
                kind: CauseKind::Mind {
                    npc: menu.npc.clone(),
                    decision: format!("{}:{}", menu.kind, option.map_or("skip", |o| o.id.as_str())),
                },
                summary: match option {
                    Some(o) => format!("{who}: {}", o.text),
                    None => format!("{who} let the moment pass"),
                },
                cites,
            },
            commands,
        }
    }
}

/// The line model's system prompt, from the speaker's brief.
pub fn system_prompt(brief: &Brief, max_words: u32) -> String {
    let mut s = format!(
        "You are {}, {}. Voice: {}",
        brief.name, brief.role, brief.voice
    );
    if !brief.goals.is_empty() {
        s.push_str(&format!(" Goals: {}.", brief.goals.join("; ")));
    }
    s.push_str(&format!(
        " Say the given line again in your own voice, in at most {max_words} words. Keep its \
         meaning and its facts. Name only people, places, ships and wares that appear in the \
         packet or the line; invent nothing. English only. Do not put fact IDs in the text. \
         Set \"cite\" to the fact the line is about."
    ));
    if !brief.off_limits.is_empty() {
        s.push_str(&format!(" Never mention: {}.", brief.off_limits.join("; ")));
    }
    s
}
