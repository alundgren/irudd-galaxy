//! Recorded conversations. Presentation never owns progress or pending story moments.
use crate::{Input, World, travel};
use galaxy_api::{
    Fields, Registry,
    command::{Batch, Cause, Command},
    engine::*,
};
use serde_json::json;

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

/// The answers a line offers, in button order: `order`, then entity ID.
pub fn choices<'a>(world: &'a World, line: &str) -> Vec<&'a str> {
    let mut out: Vec<_> = world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let f = &c.get(TALK_CHOICE)?.fields;
            (text(f, "line") == line).then(|| (f["order"].as_i64().unwrap_or(0), id.as_str()))
        })
        .collect();
    out.sort();
    out.into_iter().map(|(_, id)| id).collect()
}

/// Every line a talk can reach from `first`, through `next` and through its choices,
/// first line first. The graph must be finite and acyclic: every path ends.
pub fn lines<'a>(world: &'a World, first: &'a str) -> Result<Vec<&'a str>, String> {
    fn visit<'a>(
        world: &'a World,
        id: &'a str,
        path: &mut Vec<&'a str>,
        out: &mut Vec<&'a str>,
    ) -> Result<(), String> {
        if path.contains(&id) {
            return Err(format!("talk line cycle at `{id}`"));
        }
        if out.contains(&id) {
            return Ok(());
        }
        let f = &world
            .component(id, TALK_LINE)
            .ok_or_else(|| format!("missing talk line `{id}`"))?
            .fields;
        if text(f, "text").trim().is_empty() {
            return Err(format!("talk line `{id}` has empty text"));
        }
        out.push(id);
        path.push(id);
        let choices = choices(world, id);
        let mut next: Vec<&str> = Vec::new();
        if choices.is_empty() {
            next.push(text(f, "next"));
        } else if !text(f, "next").is_empty() {
            return Err(format!(
                "talk line `{id}` has choices, so its next must be empty: each choice names its own next"
            ));
        }
        for c in choices {
            next.push(text(
                &world.component(c, TALK_CHOICE).unwrap().fields,
                "next",
            ));
        }
        for n in next.into_iter().filter(|n| !n.is_empty()) {
            visit(world, n, path, out)?;
        }
        path.pop();
        Ok(())
    }
    if first.is_empty() {
        return Err("talk needs a first line".into());
    }
    let mut out = Vec::new();
    visit(world, first, &mut Vec::new(), &mut out)?;
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
    for (id, c) in &world.entities {
        let Some(choice) = c.get(TALK_CHOICE) else {
            continue;
        };
        let f = &choice.fields;
        let line = text(f, "line");
        if world.component(line, TALK_LINE).is_none() {
            return Err(format!("talk choice `{id}`: missing talk line `{line}`"));
        }
        if text(f, "text").trim().is_empty() {
            return Err(format!("talk choice `{id}` has empty text"));
        }
        let next = text(f, "next");
        if !next.is_empty() && world.component(next, TALK_LINE).is_none() {
            return Err(format!("talk choice `{id}`: missing next line `{next}`"));
        }
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

/// The commands a talk input makes. Empty for a stale or unavailable one: a queued
/// click can outlive its line, room, enabled state or the talk itself (a handler may
/// remove a finished talk while a second click on it is still queued).
pub fn input(world: &World, input: &Input) -> Result<Vec<Command>, String> {
    if ![TALK_START, TALK_NEXT, TALK_CHOSEN].contains(&input.name.as_str()) {
        return Ok(vec![]);
    }
    let id = text(&input.payload, "talk");
    let Some(talk) = world.component(id, TALK) else {
        return Ok(vec![]);
    };
    let f = &talk.fields;
    if !available(world, f) {
        return Ok(vec![]);
    }
    if input.name != TALK_START {
        // Two clicks on one displayed line advance it only once.
        if f["active"] != true || text(&input.payload, "line") != text(f, "current") {
            return Ok(vec![]);
        }
        let current = text(f, "current");
        let choices = choices(world, current);
        let next = if input.name == TALK_NEXT {
            if !choices.is_empty() {
                return Ok(vec![]); // A line with choices continues only through one.
            }
            text(&world.component(current, TALK_LINE).unwrap().fields, "next")
        } else {
            let choice = text(&input.payload, "choice");
            if !choices.contains(&choice) {
                return Ok(vec![]);
            }
            text(
                &world.component(choice, TALK_CHOICE).unwrap().fields,
                "next",
            )
        };
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
