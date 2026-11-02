//! A day selects acts; the recorded transition rises, walks, then settles.
//! The clock never advances here. Model morphs and all activity names belong to modules.
use std::collections::BTreeSet;

use crate::{World, places, talk::text};
use galaxy_api::{
    Registry,
    command::{Batch, Cause, Command, Fields},
    engine::*,
};
use serde_json::{Value, json};

pub const POSE_TICKS: u64 = 9;
fn n(f: &Fields, k: &str) -> f64 {
    f.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}
fn fields<'a>(w: &'a World, id: &str, c: &str) -> Result<&'a Fields, String> {
    w.component(id, c)
        .map(|c| &c.fields)
        .ok_or_else(|| format!("`{id}` has no `{c}`"))
}
pub fn acts(w: &World, first: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    let mut at = first;
    while !at.is_empty() {
        if !seen.insert(at) {
            return Err(format!("day `{first}` has a cycle; end with an empty next"));
        }
        out.push(at.to_owned());
        at = text(fields(w, at, ACT)?, "next");
    }
    if out.is_empty() {
        return Err("a day needs a first act".into());
    }
    Ok(out)
}
pub fn act_at(w: &World, life: &Fields, clock: i64) -> Result<String, String> {
    let acts = acts(w, text(life, "first"))?;
    let total = acts.iter().try_fold(0i64, |sum, id| {
        sum.checked_add(
            w.component(id, ACT).unwrap().fields["duration"]
                .as_i64()
                .unwrap_or(0),
        )
        .ok_or("day duration overflow")
    })?;
    if total <= 0 {
        return Err("a day needs positive durations".into());
    }
    let mut at = clock.rem_euclid(total);
    for id in acts {
        let duration = w.component(&id, ACT).unwrap().fields["duration"]
            .as_i64()
            .unwrap();
        if at < duration {
            return Ok(id);
        }
        at -= duration;
    }
    unreachable!()
}
pub fn validate(w: &World) -> Result<(), String> {
    for (id, c) in &w.entities {
        if let Some(a) = c.get(ACT) {
            let a = &a.fields;
            let p = fields(w, text(a, "place"), PLACE)?;
            if !text(p, "enter").is_empty() {
                return Err(format!("act `{id}` needs a terminal place"));
            }
            let s = fields(w, text(a, "spot"), SPOT)?;
            if s["place"] != a["place"]
                || n(a, "duration") <= 0.0
                || !(0.0..=1.0).contains(&n(a, "morph"))
            {
                return Err(format!(
                    "act `{id}` needs a spot in its place, positive duration and morph in 0..1"
                ));
            }
            if !text(a, "next").is_empty() {
                fields(w, text(a, "next"), ACT)?;
            }
        }
        if let Some(l) = c.get(LIFE) {
            let l = &l.fields;
            let being = fields(w, id, BEING)?;
            fields(w, id, POSITION)?;
            if being["player"] == true || c.contains_key(FLIGHT) {
                return Err(format!(
                    "life `{id}` needs a non-player being without flight"
                ));
            }
            fields(w, text(l, "clock"), CLOCK)?;
            if n(l, "speed") <= 0.0 || !matches!(text(l, "presentation"), "states" | "instant") {
                return Err(format!(
                    "life `{id}` needs positive speed and states or instant presentation"
                ));
            }
            let chain = acts(w, text(l, "first"))?;
            act_at(w, l, 0)?;
            let places: BTreeSet<_> = chain
                .iter()
                .map(|a| text(&w.component(a, ACT).unwrap().fields, "place"))
                .collect();
            if places.len() > 1 {
                for a in &chain {
                    let a = fields(w, a, ACT)?;
                    let p = fields(w, text(a, "place"), PLACE)?;
                    let arrival = fields(w, text(p, "arrival"), SPOT)?;
                    if arrival["layer"] != fields(w, text(a, "spot"), SPOT)?["layer"] {
                        return Err(format!(
                            "life `{id}` needs arrivals on the act's floor layer"
                        ));
                    }
                }
            }
            let current = text(l, "current");
            if !current.is_empty() && !chain.iter().any(|a| a == current) {
                return Err(format!("life `{id}` current act is outside its day"));
            }
            if !matches!(text(l, "phase"), "" | "rise" | "walk" | "settle") {
                return Err(format!("life `{id}` has an invalid phase"));
            }
            if !text(l, "phase").is_empty() {
                fields(w, current, ACT)?;
                let spot = fields(w, text(l, "walk_spot"), SPOT)?;
                if text(spot, "place") != places::place_of(w, id)
                    || n(l, "start_tick") < 0.0
                    || n(l, "start_tick") > w.tick as f64
                {
                    return Err(format!("life `{id}` has an invalid transition"));
                }
            }
            // A straight walk cannot change floors within a room.
            for from in &chain {
                for to in &chain {
                    let a = fields(w, from, ACT)?;
                    let b = fields(w, to, ACT)?;
                    if a["place"] == b["place"]
                        && fields(w, text(a, "spot"), SPOT)?["layer"]
                            != fields(w, text(b, "spot"), SPOT)?["layer"]
                    {
                        return Err(format!("day `{id}` crosses floor layers inside a room"));
                    }
                    if a["place"] != b["place"] {
                        let door = w.entities.values().filter_map(|c| c.get(DOOR)).find(|d| {
                            d.fields["from"] == a["place"] && d.fields["to"] == b["place"]
                        });
                        if let Some(door) = door
                            && !text(&door.fields, "spot").is_empty()
                            && fields(w, text(&door.fields, "spot"), SPOT)?["layer"]
                                != fields(w, text(a, "spot"), SPOT)?["layer"]
                        {
                            return Err(format!(
                                "day `{id}` needs its departure door on the act's floor layer"
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
fn patch(id: &str, c: &str, f: Value) -> Command {
    Command::Patch {
        entity: id.into(),
        component: c.into(),
        fields: f.as_object().unwrap().clone(),
    }
}
fn apply(w: &mut World, r: &Registry, commands: Vec<Command>) -> Result<(), String> {
    w.apply(
        &Batch {
            cause: Cause::Engine {
                service: "life".into(),
            },
            commands,
        },
        r,
    )
}
fn destination(id: &str, act: &Fields) -> Command {
    Command::Move {
        entity: id.into(),
        place: text(act, "place").into(),
        spot: Some(text(act, "spot").into()),
        state: Some(text(act, "state").into()),
    }
}
fn finish(w: &mut World, r: &Registry, id: &str) -> Result<(), String> {
    let l = fields(w, id, LIFE)?;
    let a = fields(w, text(l, "current"), ACT)?;
    let mut commands = vec![
        destination(id, a),
        patch(id, POSITION, json!({"angle":a["angle"]})),
        patch(id, LIFE, json!({"phase":""})),
    ];
    let source = places::place_of(w, id);
    if crate::travel::player(w).is_some_and(|(_, here)| here != source) {
        let p = fields(w, id, POSITION)?;
        // The cut starts at alpha zero: the displayed pose is one sim tick behind.
        // Capture that boundary, not the newly integrated position (a one-frame leap).
        let (x, y) =
            if let Some(walk) = w.component(id, WALK).filter(|c| c.fields["active"] == true) {
                let f = &walk.fields;
                let dx = n(f, "to_x") - n(f, "from_x");
                let dy = n(f, "to_y") - n(f, "from_y");
                let distance = libm::hypot(dx, dy);
                let elapsed = w
                    .tick
                    .saturating_sub(1)
                    .saturating_sub(f["start_tick"].as_u64().unwrap_or(w.tick));
                let t = if distance == 0.0 {
                    1.0
                } else {
                    (elapsed as f64 * TICK_SECONDS * n(f, "speed") / distance).min(1.0)
                };
                (n(f, "from_x") + dx * t, n(f, "from_y") + dy * t)
            } else {
                (n(p, "x"), n(p, "y"))
            };
        let height =
            crate::services::floor_height(w, source, p["layer"].as_i64().unwrap_or(0), x, y);
        let (morph, angle) = pose(w, id, 0.0).unwrap_or((0.0, 0.0));
        commands.push(patch(id,LIFE,json!({"departed_tick":w.tick,"departed_place":source,"departed_x":x,"departed_y":y,"departed_height":height,"departed_angle":angle,"departed_morph":morph})));
    }
    if places::walking(w, id) {
        commands.push(Command::Record {
            kind: WALK_FINISHED.into(),
            payload: json!({"entity":id,"place":a["place"],"spot":a["spot"]})
                .as_object()
                .unwrap()
                .clone(),
        });
    }
    apply(w, r, commands)
}
/// The pose is an absolute function of saved phase and tick. No view-owned timer.
pub fn pose(w: &World, id: &str, alpha: f32) -> Option<(f32, f32)> {
    let l = &w.component(id, LIFE)?.fields;
    let a = &w.component(text(l, "current"), ACT)?.fields;
    let elapsed = (w.tick.saturating_sub(1) as f64 + alpha as f64 - n(l, "start_tick")).max(0.0);
    let t = (elapsed / POSE_TICKS as f64).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let (m, angle) = match text(l, "phase") {
        "rise" => (n(l, "from_morph") * (1.0 - t), n(l, "from_angle")),
        "walk" => (0.0, n(l, "from_angle")),
        "settle" => {
            let turn = (n(a, "angle") - n(l, "from_angle") + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            (n(a, "morph") * t, n(l, "from_angle") + turn * t)
        }
        _ => (n(a, "morph"), n(a, "angle")),
    };
    Some((m as f32, angle as f32))
}
/// Reconcile authored days after script commands, and complete offscreen transitions.
pub fn update(w: &mut World, r: &Registry) -> Result<(), String> {
    let ids: Vec<_> = w
        .entities
        .iter()
        .filter(|(_, c)| c.contains_key(LIFE))
        .map(|(id, _)| id.clone())
        .collect();
    let here = crate::travel::player(w)
        .map(|(_, p)| p.to_owned())
        .unwrap_or_default();
    for id in ids {
        let l = fields(w, &id, LIFE)?.clone();
        let clock = fields(w, text(&l, "clock"), CLOCK)?["value"]
            .as_i64()
            .unwrap();
        let target = act_at(w, &l, clock)?;
        let current = text(&l, "current");
        let phase = text(&l, "phase");
        if !phase.is_empty()
            && (here != places::place_of(w, &id) || text(&l, "presentation") != "states")
        {
            finish(w, r, &id)?;
        }
        if target != current {
            let a = fields(w, &target, ACT)?.clone();
            let old_place = places::place_of(w, &id).to_owned();
            let visible = !current.is_empty()
                && text(&l, "presentation") == "states"
                && (old_place == here || text(&a, "place") == here);
            if !visible {
                apply(
                    w,
                    r,
                    vec![
                        patch(&id, LIFE, json!({"current":target,"phase":""})),
                        destination(&id, &a),
                        patch(&id, POSITION, json!({"angle":a["angle"]})),
                    ],
                )?;
                continue;
            }
            let (m, angle) = pose(w, &id, 1.0).unwrap_or((0.0, 0.0));
            let mut commands = Vec::new();
            if places::walking(w, &id) {
                commands.push(patch(&id, WALK, json!({"active":false})));
            }
            let walk_spot = if old_place == text(&a, "place") {
                text(&a, "spot").to_owned()
            } else if text(&a, "place") == here {
                let arrival = text(fields(w, &here, PLACE)?, "arrival");
                if arrival.is_empty() {
                    return Err(format!("life arrival into `{here}` needs an arrival spot"));
                }
                commands.push(Command::Move {
                    entity: id.clone(),
                    place: here.clone(),
                    spot: Some(arrival.into()),
                    state: None,
                });
                text(&a, "spot").to_owned()
            } else {
                let door = w
                    .entities
                    .values()
                    .filter_map(|c| c.get(DOOR))
                    .find(|d| d.fields["from"] == old_place && d.fields["to"] == a["place"]);
                let exit = door
                    .map(|d| text(&d.fields, "spot"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or(text(fields(w, &old_place, PLACE)?, "arrival"));
                if exit.is_empty() {
                    return Err(format!(
                        "life departure from `{old_place}` needs a door or arrival spot"
                    ));
                }
                exit.to_owned()
            };
            commands.push(patch(&id,LIFE,json!({"current":target,"phase":"rise","start_tick":w.tick,"from_morph":m,"from_angle":angle,"walk_spot":walk_spot})));
            apply(w, r, commands)?;
        } else {
            let l = fields(w, &id, LIFE)?.clone();
            let elapsed = w
                .tick
                .saturating_sub(l["start_tick"].as_u64().unwrap_or(w.tick));
            match text(&l, "phase") {
                "rise" if elapsed >= POSE_TICKS => apply(
                    w,
                    r,
                    vec![
                        Command::Walk {
                            entity: id.clone(),
                            spot: text(&l, "walk_spot").into(),
                            speed: n(&l, "speed"),
                            state: None,
                        },
                        patch(&id, LIFE, json!({"phase":"walk","start_tick":w.tick})),
                    ],
                )?,
                "walk" if !places::walking(w, &id) => {
                    let a = fields(w, text(&l, "current"), ACT)?;
                    if text(a, "place") != here {
                        finish(w, r, &id)?;
                    } else {
                        apply(
                            w,
                            r,
                            vec![patch(
                                &id,
                                LIFE,
                                json!({"phase":"settle","start_tick":w.tick}),
                            )],
                        )?;
                    }
                }
                "settle" if elapsed >= POSE_TICKS => finish(w, r, &id)?,
                _ => {}
            }
        }
    }
    Ok(())
}
