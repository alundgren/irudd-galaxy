//! Recorded conversations. Presentation never owns progress or pending story moments.
use crate::{Input, World, travel};
use galaxy_api::{
    Fields, Registry,
    command::{Batch, Cause, Command},
    engine::*,
};
use serde_json::json;
use std::collections::BTreeSet;

pub fn text<'a>(f: &'a Fields, key: &str) -> &'a str {
    f.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

pub fn anchor(f: &Fields) -> String {
    let a = text(f, "anchor");
    if a.is_empty() {
        format!("being.{}", text(f, "speaker"))
    } else {
        a.into()
    }
}

pub fn available(world: &World, f: &Fields) -> bool {
    if f["enabled"] != true {
        return false;
    }
    let speaker = text(f, "speaker");
    let Some((player, place)) = travel::player(world) else {
        return true;
    };
    // A trip between places interrupts talk; a camera glide within the room does not.
    if world
        .component(player, TRAVEL)
        .is_some_and(|t| t.fields["active"] == true && t.fields["from"] != t.fields["to"])
    {
        return false;
    }
    if world.component(speaker, PLACE).is_some() {
        return speaker == place;
    }
    world
        .component(speaker, LOCATION)
        .is_some_and(|l| l.fields["place"] == place)
}

/// Stable priority: pending story, repeatable talk, then a previously seen story.
pub fn for_speaker<'a>(world: &'a World, speaker: &str) -> Option<&'a str> {
    world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let f = &c.get(TALK)?.fields;
            if text(f, "speaker") != speaker || !available(world, f) {
                return None;
            }
            let priority = if f["story"] == true {
                if f["seen"] == true { 2 } else { 0 }
            } else {
                1
            };
            Some((priority, id.as_str()))
        })
        .min()
        .map(|(_, id)| id)
}

pub fn lines<'a>(world: &'a World, first: &'a str) -> Result<Vec<&'a str>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut id = first;
    while !id.is_empty() {
        if !seen.insert(id) {
            return Err(format!("talk line cycle at `{id}`"));
        }
        let f = &world
            .component(id, TALK_LINE)
            .ok_or_else(|| format!("missing talk line `{id}`"))?
            .fields;
        if text(f, "text").trim().is_empty() {
            return Err(format!("talk line `{id}` has empty text"));
        }
        out.push(id);
        id = text(f, "next");
    }
    if out.is_empty() {
        return Err("talk needs a first line".into());
    }
    Ok(out)
}

pub fn validate(world: &World) -> Result<(), String> {
    let mut active = 0;
    for (id, c) in &world.entities {
        let Some(t) = c.get(TALK) else {
            continue;
        };
        let f = &t.fields;
        let speaker = text(f, "speaker");
        if !world.entities.contains_key(speaker) {
            return Err(format!("talk `{id}`: missing speaker `{speaker}`"));
        }
        let chain = lines(world, text(f, "first")).map_err(|e| format!("talk `{id}`: {e}"))?;
        if f["active"] == true {
            active += 1;
            if !chain.contains(&text(f, "current")) {
                return Err(format!("talk `{id}`: current line is outside its chain"));
            }
        }
        if f["width"].as_f64().unwrap_or(0.0) <= 0.0 {
            return Err(format!("talk `{id}`: width must be positive"));
        }
    }
    if active > 1 {
        return Err("only one talk can be active".into());
    }
    Ok(())
}

fn patch(id: &str, fields: serde_json::Value) -> Command {
    Command::Patch {
        entity: id.into(),
        component: TALK.into(),
        fields: fields.as_object().unwrap().clone(),
    }
}

pub fn input(world: &World, input: &Input) -> Result<Vec<Command>, String> {
    if input.name != TALK_START && input.name != TALK_NEXT {
        return Ok(vec![]);
    }
    let id = text(&input.payload, "talk");
    let f = &world
        .component(id, TALK)
        .ok_or_else(|| format!("missing talk `{id}`"))?
        .fields;
    if !available(world, f) {
        return Ok(vec![]); // A queued click can outlive its room or enabled state.
    }
    if input.name == TALK_NEXT {
        if f["active"] != true || text(&input.payload, "line") != text(f, "current") {
            return Ok(vec![]); // Two clicks on one displayed line advance it only once.
        }
        let line = &world
            .component(text(f, "current"), TALK_LINE)
            .unwrap()
            .fields;
        let next = text(line, "next");
        return Ok(vec![if next.is_empty() {
            patch(id, json!({"active": false, "current": null, "seen": true}))
        } else {
            patch(id, json!({"current": next}))
        }]);
    }
    if for_speaker(world, text(f, "speaker")) != Some(id) {
        return Ok(vec![]);
    }
    let mut commands = Vec::new();
    for (other, c) in &world.entities {
        if c.get(TALK).is_some_and(|t| t.fields["active"] == true) {
            commands.push(patch(other, json!({"active": false, "current": null})));
        }
    }
    commands.push(patch(id, json!({"active": true, "current": f["first"]})));
    Ok(commands)
}

/// Leaving, disabling or moving the speaker hides the line without marking it seen.
pub fn interrupt(world: &mut World, registry: &Registry) -> Result<(), String> {
    let commands = world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let f = &c.get(TALK)?.fields;
            (f["active"] == true && !available(world, f))
                .then(|| patch(id, json!({"active": false, "current": null})))
        })
        .collect();
    world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "talk".into(),
            },
            commands,
        },
        registry,
    )
}
