//! Live calls, timed and costed: a Jev trader choice, a Jev dock-talk pick among
//! authored lines (the shipped mode) and, opt-in, a free-phrased gpt-oss line. For
//! `galaxy mind try` and the `#[ignore]` live tests. Needs `OPENROUTER_API_KEY`;
//! nothing else uses the network.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use galaxy_contract::ids::FactId;
use serde::Serialize;
use serde_json::json;

use crate::calllog::CallSink;
use crate::config::MindConfig;
use crate::host::system_prompt;
use crate::mind::{
    Decision, DecisionRequest, LineRequest, MAX_ATTEMPTS, Mind, MindError, Question, Reply,
};
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
        max_attempts: MAX_ATTEMPTS,
        deadline_ms: 10_000,
    }
}

/// Dock talk as shipped (pick and fill): the player docks at Lantern Wharf after
/// delivering ice and ending the Rimewater shortage. Ilse's authored lines that fit,
/// each tied to the fact it cites, as one Jev choice. Returns the request and, per
/// option, the line ID and its text with slots filled.
pub fn sample_line_pick() -> (DecisionRequest, Vec<(String, String)>) {
    let fill = |t: &str| t.replace("{qty}", "12");
    let lines = [
        (
            "line.ilse_marrow.14",
            "fact.41",
            "Rimewater came through, and so did you. Odo's lamps are bright again.",
        ),
        (
            "line.ilse_marrow.01",
            "fact.40",
            "Odo's been counting your ice. He counts slower than I do, but he gets there.",
        ),
        (
            "line.ilse_marrow.05",
            "fact.38",
            "Rimewater's thin. A lot of your ice is in our tanks, and not much is left out there.",
        ),
    ];
    let options: Vec<(String, String)> = lines
        .iter()
        .map(|(id, _, text)| {
            (
                (*id).to_owned(),
                format!("npc.ilse_marrow says: \"{}\"", fill(text)),
            )
        })
        .collect();
    let req = DecisionRequest {
        key: "sk.probe_line_pick".into(),
        instructions: "The player has just docked. Pick the one line the player most needs \
                       to hear. Every option is allowed; choose the one that fits best."
            .into(),
        state: json!({
            "place": {"id": "station.lantern_wharf", "text": "Lantern Wharf in Lantern Shallows"},
            "facts": [
                {"id": "fact.38", "kind": "shortage_started", "text": "Rimewater ran short of ice"},
                {"id": "fact.40", "kind": "deliver", "text": "Wren delivered 12 ice to Kettle Dock"},
                {"id": "fact.41", "kind": "shortage_resolved", "text": "The player ended the Rimewater ice shortage"},
                {"id": "fact.42", "kind": "dock", "text": "Wren docked at Lantern Wharf"},
            ],
            "options": options.iter().enumerate()
                .map(|(i, (_, t))| json!({"id": crate::mind::wire_id(i), "text": t}))
                .collect::<Vec<_>>(),
        }),
        question: Question::choice(&options),
        max_attempts: MAX_ATTEMPTS,
        deadline_ms: 10_000,
    };
    let said = lines
        .iter()
        .map(|(id, _, text)| ((*id).to_owned(), fill(text)))
        .collect();
    (req, said)
}

/// Ilse's first line to say in her voice, with what the answer is checked against.
/// Free phrasing only (opt-in).
pub struct SampleLine {
    pub req: LineRequest,
    facts: BTreeMap<FactId, String>,
    context: Vec<String>,
    names: BTreeSet<String>,
    wares: BTreeSet<String>,
    forbidden: Vec<String>,
    claims: Vec<String>,
    max_chars: u32,
}

impl SampleLine {
    pub fn rules(&self) -> LineRules<'_> {
        LineRules {
            cite: FactId(40),
            facts: &self.facts,
            context: &self.context,
            max_words: self.req.max_words,
            max_chars: self.max_chars,
            forbidden: &self.forbidden,
            names: &self.names,
            wares: &self.wares,
            claims: &self.claims,
            authored: &self.req.authored,
            template: &self.req.template,
            slots: &self.req.slots,
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
    let template =
        "Odo's been counting your {qty} ice. He counts slower than I do, but he gets there.";
    let slots: BTreeMap<String, String> = [("qty".to_owned(), "12".to_owned())].into();
    let authored = template.replace("{qty}", "12");
    let facts: BTreeMap<FactId, String> = [
        (
            FactId(40),
            "Wren delivered 12 ice to Kettle Dock".to_owned(),
        ),
        (FactId(42), "Wren docked at Lantern Wharf".to_owned()),
    ]
    .into();
    let place = "Lantern Wharf in Lantern Shallows";
    let req = LineRequest {
        key: "sk.probe_line".into(),
        system: system_prompt(&brief, max_words, true),
        packet: json!({
            "place": {"id": "station.lantern_wharf", "text": place},
            "facts": facts.iter()
                .map(|(id, text)| json!({"id": id, "kind": if id.0 == 40 { "deliver" } else { "dock" }, "text": text}))
                .collect::<Vec<_>>(),
        }),
        authored,
        template: template.into(),
        slots,
        cite: "fact.40".into(),
        allowed_cites: vec!["fact.40".into(), "fact.42".into()],
        max_words,
        max_attempts: MAX_ATTEMPTS,
        deadline_ms: 10_000,
    };
    let names: BTreeSet<String> = [
        "Odo Fenn",
        "Kettle Dock",
        "Lantern Wharf",
        "Lantern Shallows",
        "Wren",
        "Ilse Marrow",
        "Saffron Vey",
        "Brassmouth Exchange",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let wares: BTreeSet<String> = ["Ice", "Rations", "Glassite", "Iron ore"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    SampleLine {
        req,
        facts,
        context: vec![brief.name.clone(), brief.role.clone(), place.to_owned()],
        names,
        wares,
        forbidden: config.talk.forbidden.clone(),
        claims: config.talk.claims.clone(),
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

fn timed<T>(f: impl FnOnce() -> Reply<T>) -> (Reply<T>, u64) {
    let start = Instant::now();
    let r = f();
    (r, start.elapsed().as_millis() as u64)
}

fn result<T, U>(r: Reply<T>, ms: u64, map: impl FnOnce(T) -> U) -> ProbeResult<U> {
    let model = (!r.model.is_empty()).then(|| r.model.clone());
    match r.result {
        Ok(value) => ProbeResult {
            ok: true,
            latency_ms: ms,
            model,
            cost_usd: r.cost_usd,
            attempts: r.attempts,
            answer: Some(map(value)),
            error: None,
        },
        Err(e) => ProbeResult {
            ok: false,
            latency_ms: ms,
            model,
            cost_usd: r.cost_usd,
            attempts: r.attempts,
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

/// One live dock-talk pick: the line ID chosen and the text the game says, every word
/// of it authored.
pub fn line_pick(mind: &mut impl Mind) -> ProbeResult<serde_json::Value> {
    let (req, said) = sample_line_pick();
    let (r, ms) = timed(|| mind.decide(&req));
    result(r, ms, |d| match d {
        Decision::Choice { choice, confidence } => {
            let picked = crate::mind::wire_index(&choice).and_then(|i| said.get(i));
            json!({
                "choice": choice,
                "line": picked.map(|(id, _)| id),
                "said": picked.map(|(_, text)| text),
                "confidence": confidence,
            })
        }
        other => json!(other),
    })
}

/// One live free-phrased line (opt-in mode), with the validator's verdict. The
/// validator is best effort; the authored line is said whenever it rejects.
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

/// The live calls against OpenRouter, as JSON for the CLI: a trader choice and a dock
/// talk pick (both Jev), and a free-phrased line only if `talk.free_phrasing` is on.
pub fn live(config: &MindConfig) -> Result<serde_json::Value, MindError> {
    live_logged(config, &CallSink::new())
}

/// As [`live`], every call logged to `log`.
pub fn live_logged(config: &MindConfig, log: &CallSink) -> Result<serde_json::Value, MindError> {
    let mut mind =
        OpenRouterMind::from_env(config.models.clone(), &config.limits)?.with_log(log.clone());
    let jev = decision(&mut mind);
    let pick = line_pick(&mut mind);
    let mut total = jev.cost_usd + pick.cost_usd;
    let mut out = json!({
        "decision": jev,
        "line_pick": pick,
    });
    if config.talk.free_phrasing {
        let gpt = line(&mut mind, config);
        total += gpt.cost_usd;
        out["line"] = json!(gpt);
    }
    out["total_cost_usd"] = json!(total);
    Ok(out)
}
