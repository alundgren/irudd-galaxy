//! One live Jev choice and one live line, timed and costed: `galaxy mind try` and the
//! `#[ignore]` live tests. Needs `OPENROUTER_API_KEY`; nothing else uses the network.

use std::collections::BTreeSet;
use std::time::Instant;

use galaxy_contract::ids::FactId;
use serde::Serialize;
use serde_json::json;

use crate::config::MindConfig;
use crate::host::system_prompt;
use crate::mind::{Decision, DecisionRequest, LineRequest, Mind, MindError, Question, Reply};
use crate::openrouter::OpenRouterMind;
use crate::validate::{self, LineRules};

/// A typical trader menu: the Ninefold docked at Cinder Hook.
pub fn sample_decision() -> DecisionRequest {
    let options = [
        (
            "plan.1".to_owned(),
            "Sell 10 glassite here, buy 10 iron ore, carry it to Brassmouth Exchange".to_owned(),
        ),
        (
            "plan.2".to_owned(),
            "Buy 10 iron ore here and carry it to Lantern Wharf".to_owned(),
        ),
        (
            "plan.3".to_owned(),
            "Sell 6 rations here and wait for a better ore price".to_owned(),
        ),
    ];
    DecisionRequest {
        key: "sk.probe_decision".into(),
        instructions: "You decide for Ninefold, a wide brass Tollway Compact barge. Ninefold \
                       has docked. Pick the trade plan this captain makes, given their goals \
                       and temperament. Every option is allowed; choose the one that fits best."
            .into(),
        state: json!({
            "who": {
                "id": "craft.ninefold",
                "name": "Ninefold",
                "goals": ["work the Cinder ore trade for the Compact", "bring glassite to Brassmouth"],
                "temperament": {"greed": 0.8, "caution": 0.4, "warmth": 0.3},
            },
            "place": {"id": "station.cinder_hook", "text": "Cinder Hook in Cinder Drift"},
            "state": [
                {"id": "craft.ninefold", "text": "Ninefold: 10 glassite, 6 rations"},
                {"id": "station.cinder_hook/ware.iron_ore", "text": "Iron ore: price 9, stock 80"},
            ],
            "facts": [
                {"id": "fact.41", "kind": "sell", "text": "Wren sold 30 glassite at Brassmouth Exchange for 31 each"},
                {"id": "fact.42", "kind": "dock", "text": "Ninefold docked at Cinder Hook"},
            ],
            "options": options.iter().enumerate()
                .map(|(i, (_, t))| json!({"id": crate::mind::wire_id(i), "text": t}))
                .collect::<Vec<_>>(),
        }),
        question: Question::choice(&options),
    }
}

/// Ilse's first line to say in her voice, with what the answer is checked against.
pub struct SampleLine {
    pub req: LineRequest,
    packet: Vec<FactId>,
    names: BTreeSet<String>,
    forbidden: Vec<String>,
    max_chars: u32,
}

impl SampleLine {
    pub fn rules(&self) -> LineRules<'_> {
        LineRules {
            cite: FactId(40),
            packet: &self.packet,
            max_words: self.req.max_words,
            max_chars: self.max_chars,
            forbidden: &self.forbidden,
            names: &self.names,
            authored: &self.req.authored,
        }
    }
}

pub fn sample_line(config: &MindConfig) -> SampleLine {
    let brief = config
        .brief("npc.ilse_marrow")
        .cloned()
        .unwrap_or_else(|| crate::config::Brief {
            id: galaxy_contract::ids::AnyId::new("npc.ilse_marrow").expect("valid"),
            revision: 1,
            name: "Ilse Marrow".into(),
            role: "dockmaster at Lantern Wharf".into(),
            npc: None,
            goals: Vec::new(),
            temperament: Default::default(),
            allowed: vec!["dock_talk".into()],
            voice: "Dry and warm.".into(),
            max_words: None,
            knows: Vec::new(),
            off_limits: Vec::new(),
        });
    let max_words = config.talk.max_words;
    let authored = "Odo's been counting your ice. He counts slower than I do, but he gets there.";
    let req = LineRequest {
        key: "sk.probe_line".into(),
        system: system_prompt(&brief, max_words),
        packet: json!({
            "place": {"id": "station.lantern_wharf", "text": "Lantern Wharf in Lantern Shallows"},
            "facts": [
                {"id": "fact.40", "kind": "deliver", "text": "Wren delivered 12 ice to Kettle Dock"},
                {"id": "fact.42", "kind": "dock", "text": "Wren docked at Lantern Wharf"},
            ],
        }),
        authored: authored.into(),
        cite: "fact.40".into(),
        allowed_cites: vec!["fact.40".into(), "fact.42".into()],
        max_words,
    };
    let names: BTreeSet<String> = ["Odo Fenn", "Kettle Dock", "Lantern Wharf", "Wren", "Ilse"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    SampleLine {
        req,
        packet: vec![FactId(40), FactId(42)],
        names,
        forbidden: config.talk.forbidden.clone(),
        max_chars: config.talk.max_chars,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProbeResult<T> {
    pub ok: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub cost_usd: f64,
    pub attempts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn timed<T>(f: impl FnOnce() -> Result<Reply<T>, MindError>) -> (Result<Reply<T>, MindError>, u64) {
    let start = Instant::now();
    let r = f();
    (r, start.elapsed().as_millis() as u64)
}

fn result<T, U>(
    r: Result<Reply<T>, MindError>,
    ms: u64,
    map: impl FnOnce(T) -> U,
) -> ProbeResult<U> {
    match r {
        Ok(reply) => ProbeResult {
            ok: true,
            latency_ms: ms,
            model: Some(reply.model),
            cost_usd: reply.cost_usd,
            attempts: reply.attempts,
            answer: Some(map(reply.value)),
            error: None,
        },
        Err(e) => ProbeResult {
            ok: false,
            latency_ms: ms,
            model: None,
            cost_usd: 0.0,
            attempts: 0,
            answer: None,
            error: Some(e.to_string()),
        },
    }
}

/// One live choice. Its answer is the option ID picked and the model's confidence.
pub fn decision(mind: &mut impl Mind) -> ProbeResult<serde_json::Value> {
    let req = sample_decision();
    let (r, ms) = timed(|| mind.decide(&req));
    result(r, ms, |d| match d {
        Decision::Choice { choice, confidence } => {
            let index = crate::mind::wire_index(&choice);
            let option = match (&req.question, index) {
                (Question::Choice { options, .. }, Some(i)) => options.get(i).cloned(),
                _ => None,
            };
            json!({"choice": choice, "option": option, "confidence": confidence})
        }
        other => json!(other),
    })
}

/// One live line, with the validator's verdict.
pub fn line(mind: &mut impl Mind, config: &MindConfig) -> ProbeResult<serde_json::Value> {
    let sample = sample_line(config);
    let req = &sample.req;
    let (r, ms) = timed(|| mind.line(req));
    result(r, ms, |draft| {
        let verdict = validate::line(&draft, &sample.rules());
        json!({
            "text": draft.text,
            "cites": draft.cites,
            "valid": verdict.is_ok(),
            "said": verdict.as_ref().map_or(req.authored.clone(), |c| c.text.clone()),
            "rejected": verdict.err().map(|e| e.to_string()),
        })
    })
}

/// Both calls against OpenRouter, as JSON for the CLI.
pub fn live(config: &MindConfig) -> Result<serde_json::Value, MindError> {
    let mut mind = OpenRouterMind::from_env(config.models.clone(), &config.limits)?;
    let jev = decision(&mut mind);
    let gpt = line(&mut mind, config);
    let total = jev.cost_usd + gpt.cost_usd;
    Ok(json!({
        "decision": jev,
        "line": gpt,
        "total_cost_usd": total,
    }))
}
