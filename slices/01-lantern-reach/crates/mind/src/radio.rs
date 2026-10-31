//! Radio text bits from the live layer (#47, 5.1): one short first-person line a
//! station's keeper says on its radio channel, shown as text under the radio on the
//! HUD. No voice synthesis.
//!
//! The same rules as the other live flourishes ([`crate::live`]): the model is offered
//! the station's closed world (its keepers, the wares, the stations) and answers with a
//! JSON object naming one of the keepers and the line. [`validate`] checks the speaker
//! against the list and the line with [`live::check_text`]; anything else is rejected
//! and the bit skipped. An accepted bit goes into **station history only**: a
//! `mind.radio` fact with `Cause::Mind`, about the station and the speaker, with the
//! channel in `subjects.other`. Nothing here changes the world beyond that fact.
//!
//! [`crate::MindHost`] asks when the flagship flies into a station channel's range
//! (counted per channel entry, with a cooldown in dockings and a per-run cap from the
//! radio data, never time), with the station's own `session_id`, through the same
//! executor, budget and call log as the live layer. The channels come from the app's
//! `radio.toml` ([`Setup`]); beacons carry authored bits only.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::LiveRules;
use crate::live::{self, Closed, TextRules};
use crate::mind::{LiveDraft, Purpose};
use crate::packet::Names;
use crate::validate::Invalid;

/// The fact an accepted radio bit records.
pub const RADIO_FACT: &str = "mind.radio";

/// The fact the first time a channel comes into range records (once per save), with the
/// channel in `subjects.other`. The app's radio system records it.
pub const FOUND_FACT: &str = "radio_found";

/// A station's radio channel, as the live layer needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// `radio.kettle_dock`.
    pub id: String,
    /// "Kettle Dock radio".
    pub name: String,
    pub station: StationId,
    /// World units from the station.
    pub range: f32,
}

/// How often bits are asked for: counted, never timed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    /// Dockings between two bits on the same channel.
    pub cooldown_dockings: u32,
    /// Bits asked for in one run, at most.
    pub run_calls: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            cooldown_dockings: 2,
            run_calls: 8,
        }
    }
}

/// The station channels and their rules ([`crate::MindHost::set_radio`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setup {
    pub channels: Vec<Channel>,
    pub rules: Rules,
}

/// Channels whose station the flagship is within range of: flying (not docked, not in
/// a lane), in the station's region.
pub fn in_range<'a>(world: &WorldState, setup: &'a Setup) -> Vec<&'a Channel> {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return Vec::new();
    };
    if ship.docked_at.is_some() || ship.activity.transit.is_some() {
        return Vec::new();
    }
    setup
        .channels
        .iter()
        .filter(|c| {
            world.stations.get(&c.station).is_some_and(|st| {
                st.region == ship.region && st.position.distance(ship.position) <= c.range
            })
        })
        .collect()
}

/// The task, the closed world and the answer schema for a bit on `channel`.
pub fn task(channel: &Channel, closed: &Closed) -> (Value, Value) {
    let schema = json!({
        "type": "object",
        "properties": {
            "speaker": {
                "type": "string",
                "enum": closed.keepers.iter().map(|k| k.id.as_str()).collect::<Vec<_>>(),
            },
            "text": { "type": "string" },
        },
        "required": ["speaker", "text"],
        "additionalProperties": false,
    });
    let user = json!({
        "task": format!(
            "One of the keepers speaks on the station's radio, {}, to pilots flying past: \
             a short friendly line about the dock, the lanes, the weather in the lanes, or \
             what the market wants. Pick the speaker and write the line in their voice. Name \
             no amounts or prices.",
            channel.name
        ),
        "station": closed.station,
        "keepers": closed.keepers,
        "wares": closed.wares,
        "stations": closed.stations,
        "answer_schema": schema,
    });
    (user, schema)
}

/// A bit that passed validation.
#[derive(Clone, Debug, PartialEq)]
pub struct Bit {
    pub speaker: NpcId,
    pub text: String,
}

/// Validates a radio answer against the closed world it was offered.
pub fn validate(draft: &LiveDraft, closed: &Closed, text: &TextRules<'_>) -> Result<Bit, Invalid> {
    let v = &draft.value;
    if !v.is_object() {
        return Err(Invalid::Shape("not a JSON object".into()));
    }
    let speaker = v
        .get("speaker")
        .and_then(Value::as_str)
        .ok_or_else(|| Invalid::Shape("`speaker` is missing or not a string".into()))?;
    if !closed.keepers.iter().any(|k| k.id == speaker) {
        return Err(Invalid::Reference(
            "`speaker` is not one of the IDs offered".into(),
        ));
    }
    let raw = v
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| Invalid::Shape("`text` is missing or not a string".into()))?;
    Ok(Bit {
        speaker: NpcId::new(speaker).map_err(|_| Invalid::Shape("not a well-formed ID".into()))?,
        text: live::check_text(raw, text)?,
    })
}

/// The note on a `mind.radio` fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadioNote {
    pub channel: String,
    /// The request key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The player's docking count when it was asked for.
    pub docking: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub speaker: NpcId,
    pub text: String,
}

impl RadioNote {
    pub fn parse(fact: &Fact) -> Option<Self> {
        if fact.kind.as_str() != RADIO_FACT {
            return None;
        }
        match &fact.payload {
            FactPayload::Note(n) => serde_json::from_str(n).ok(),
            FactPayload::Event(_) => None,
        }
    }
}

/// The batch an accepted bit becomes: its fact, with `Cause::Mind`, citing what asked
/// for it (the channel's discovery, when there is one).
pub fn accepted(
    world: &WorldState,
    channel: &Channel,
    bit: &Bit,
    key: &str,
    model: &str,
    docking: u64,
    cites: Vec<FactId>,
) -> CommandBatch {
    let note = RadioNote {
        channel: channel.id.clone(),
        key: key.to_owned(),
        docking,
        model: model.to_owned(),
        speaker: bit.speaker.clone(),
        text: bit.text.clone(),
    };
    let names = Names(world);
    CommandBatch {
        cause: Cause {
            kind: CauseKind::Mind {
                npc: Some(bit.speaker.clone()),
                decision: format!("{}:{}", Purpose::Radio.menu(), channel.id),
            },
            summary: format!(
                "{} on {}: {}",
                names.of(bit.speaker.as_str()),
                channel.name,
                bit.text
            ),
            cites,
        },
        commands: vec![Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::from_static(RADIO_FACT),
                subjects: Subjects {
                    station: Some(channel.station.clone()),
                    npc: Some(bit.speaker.clone()),
                    region: world.stations.get(&channel.station).map(|s| s.region.clone()),
                    other: AnyId::new(&channel.id).ok(),
                    ..Subjects::default()
                },
                amount: None,
                note: serde_json::to_string(&note).expect("notes serialize"),
            }),
        }],
    }
}

/// The text rules a radio bit is checked against: the live layer's.
pub fn text_rules<'a>(
    rules: &'a LiveRules,
    forbidden: &'a [String],
    names: &'a std::collections::BTreeSet<String>,
) -> TextRules<'a> {
    TextRules {
        max_chars: rules.max_chars,
        forbidden,
        markers: &rules.reject_markers,
        names,
    }
}

/// The latest accepted bits per channel, newest first, folded from history as it grows
/// (for the HUD's ticker). Keeps at most `keep` per channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bits {
    seen: Option<FactId>,
    by_channel: BTreeMap<String, Vec<(NpcId, String)>>,
}

impl Bits {
    /// How many bits per channel are kept.
    pub const KEEP: usize = 3;

    /// Folds facts recorded since the last call.
    pub fn observe(&mut self, facts: &[Fact]) {
        // A history that no longer holds what was seen (a hard restore) starts over.
        if self
            .seen
            .is_some_and(|s| facts.last().is_none_or(|f| f.id < s))
        {
            *self = Self::default();
        }
        let from = self
            .seen
            .map_or(0, |id| facts.partition_point(|f| f.id <= id));
        for f in &facts[from..] {
            if let Some(n) = RadioNote::parse(f) {
                let list = self.by_channel.entry(n.channel).or_default();
                list.insert(0, (n.speaker, n.text));
                list.truncate(Self::KEEP);
            }
        }
        self.seen = facts.last().map(|f| f.id).or(self.seen);
    }

    /// `channel`'s bits, newest first.
    pub fn on(&self, channel: &str) -> &[(NpcId, String)] {
        self.by_channel.get(channel).map_or(&[], Vec::as_slice)
    }
}
