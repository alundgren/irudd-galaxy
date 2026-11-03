//! Life signs use the screens' ordinary text surface, layout and anchor path.
use galaxy_api::engine::*;
use galaxy_api::{
    Registry,
    command::{Batch, Cause, Command},
};
use galaxy_render::glam::Vec2;
use galaxy_script::screens::Widget;
use galaxy_sim::{World, talk::text};
use std::collections::BTreeMap;

/// The world completed a walk when the player left. Its source fade keeps the
/// departure pose instead of showing that offscreen completion as a teleport.
pub fn departure(w: &World, id: &str, alpha: f32) -> Option<(crate::Pose, f32)> {
    let l = &w.component(id, LIFE)?.fields;
    let (player, _) = galaxy_sim::travel::player(w)?;
    let t = &w.component(player, TRAVEL)?.fields;
    if t["active"] != true
        || t["start_tick"] != l["departed_tick"]
        || text(l, "departed_place").is_empty()
        || crate::travel::place(w, alpha) != Some(text(l, "departed_place"))
    {
        return None;
    }
    let n = |k| l[k].as_f64().unwrap_or(0.0) as f32;
    Some((
        crate::Pose {
            x: n("departed_x"),
            y: n("departed_y"),
            height: n("departed_height"),
            angle: n("departed_angle"),
        },
        n("departed_morph"),
    ))
}

pub fn widgets(w: &World) -> Vec<Widget> {
    let here = galaxy_sim::travel::player(w).map(|(_, p)| p);
    w.entities
        .iter()
        .filter_map(|(id, c)| {
            let l = &c.get(LIFE)?.fields;
            let name = text(l, "name");
            if name.is_empty()
                || !crate::rooms::visible(w, id, here)
                || w.entities.values().any(|c| {
                    c.get(TALK)
                        .is_some_and(|t| t.fields["active"] == true && t.fields["speaker"] == *id)
                })
            {
                return None;
            }
            let state = c.get(BEING).map(|b| text(&b.fields, "state")).unwrap_or("");
            let label = if text(l, "phase").is_empty() && !state.is_empty() {
                format!("{name} · {state}")
            } else {
                name.into()
            };
            Some(Widget {
                hover: None,
                kind: "sign".into(),
                id: Some(format!("life.{id}")),
                path: format!("{id}/sign"),
                text: label,
                width: Some(220.0),
                height: None,
                x: None,
                y: Some(-12.0),
                size: 18.0,
                pad: 8.0,
                explicit_pad: true,
                gap: 0.0,
                align: None,
                anchor: Some(format!("being.{id}")),
                on: None,
                payload: Default::default(),
                children: vec![],
                columns: 1,
                value: String::new(),
                delta: 0.0,
                accepts: BTreeMap::new(),
                notes: BTreeMap::new(),
                weight: 500,
                trailing: false,
                resting: None,
            })
        })
        .collect()
}

// Enumerate distinct authored clock selections, retaining synchronization between
// beings on one clock. Separate clocks form a product. Refuse an excessive check
// explicitly instead of silently approving a partially explored day.
fn selections(w: &World) -> Result<Vec<BTreeMap<String, i64>>, String> {
    let mut clocks: BTreeMap<String, Vec<&galaxy_api::command::Fields>> = BTreeMap::new();
    for c in w.entities.values() {
        if let Some(l) = c.get(LIFE) {
            clocks
                .entry(text(&l.fields, "clock").into())
                .or_default()
                .push(&l.fields);
        }
    }
    let mut combinations = vec![BTreeMap::new()];
    for (clock, lives) in clocks {
        let mut period = 1i64;
        for l in &lives {
            let total: i64 = galaxy_sim::life::acts(w, text(l, "first"))?
                .iter()
                .map(|a| {
                    w.component(a, ACT).unwrap().fields["duration"]
                        .as_i64()
                        .unwrap()
                })
                .sum();
            let (mut a, mut b) = (period, total);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            period = (period/a).checked_mul(total).filter(|p|*p <= 100_000).ok_or("life check: combined clock period exceeds 100000 units; use a shorter shared day")?;
        }
        let mut distinct = BTreeMap::new();
        for time in 0..period {
            let key = lives
                .iter()
                .map(|l| galaxy_sim::life::act_at(w, l, time))
                .collect::<Result<Vec<_>, _>>()?;
            distinct.entry(key).or_insert(time);
        }
        let mut next = Vec::new();
        for before in &combinations {
            for time in distinct.values() {
                let mut item = before.clone();
                item.insert(clock.clone(), *time);
                next.push(item);
                if next.len() > 64 {
                    return Err("life check: more than 64 distinct day arrangements; split the authored check fixture".into());
                }
            }
        }
        combinations = next;
    }
    Ok(combinations)
}
fn clock_values(w: &mut World, values: &BTreeMap<String, i64>) {
    for (id, v) in values {
        w.entities
            .get_mut(id)
            .unwrap()
            .get_mut(CLOCK)
            .unwrap()
            .fields
            .insert("value".into(), (*v).into());
    }
}
impl crate::View {
    fn check_life_pose(&mut self, world: &World, size: Vec2, alpha: f32) -> Vec<String> {
        let mut silent = world.clone();
        for c in silent.entities.values_mut() {
            if let Some(t) = c.get_mut(TALK) {
                t.fields.insert("active".into(), false.into());
            }
        }
        self.scene(&silent, alpha, size, 0.0);
        let place = galaxy_sim::travel::player(&silent)
            .map(|(_, p)| p)
            .unwrap_or("");
        let mut findings: Vec<_> = self
            .screens
            .problems
            .iter()
            .map(|p| format!("place `{place}`, without talk: {p}"))
            .collect();
        findings.extend(self.check_talk_at(world, size, alpha, false));
        findings
    }

    /// Every clock selection and every transition between selections, in each room.
    /// Each half-tick uses ordinary screen readability, both silent and talking,
    /// and the actual talk focus checker. All beings move together.
    pub fn check_life(&mut self, world: &World, registry: &Registry, size: Vec2) -> Vec<String> {
        if !world.entities.values().any(|c| c.contains_key(LIFE)) {
            return vec![];
        }
        let run = |view: &mut Self| -> Result<Vec<String>, String> {
            let selections = selections(world)?;
            let rooms: Vec<_> = world
                .entities
                .iter()
                .filter(|(_, c)| c.contains_key(PLACE))
                .map(|(id, _)| id.clone())
                .collect();
            let player = galaxy_sim::travel::player(world)
                .ok_or("life check needs a player")?
                .0
                .to_owned();
            let mut findings = Vec::new();
            for from in &selections {
                let mut rest = world.clone();
                clock_values(&mut rest, from);
                for c in rest.entities.values_mut() {
                    if let Some(l) = c.get_mut(LIFE) {
                        l.fields.insert("current".into(), serde_json::Value::Null);
                        l.fields.insert("phase".into(), "".into());
                    }
                    if let Some(w) = c.get_mut(WALK) {
                        w.fields.insert("active".into(), false.into());
                    }
                }
                galaxy_sim::life::update(&mut rest, registry)?;
                galaxy_sim::services::ground(&mut rest);
                for room in &rooms {
                    let mut in_room = rest.clone();
                    in_room.apply(
                        &Batch {
                            cause: Cause::Engine {
                                service: "life-check".into(),
                            },
                            commands: vec![Command::Move {
                                entity: player.clone(),
                                place: room.clone(),
                                spot: None,
                                state: None,
                            }],
                        },
                        registry,
                    )?;
                    if let Some(t) = in_room.entities.get_mut(&player).unwrap().get_mut(TRAVEL) {
                        t.fields.insert("active".into(), false.into());
                    }
                    galaxy_sim::services::ground(&mut in_room);
                    findings.extend(view.check_life_pose(&in_room, size, 1.0));
                    if !findings.is_empty() {
                        return Ok(findings);
                    }
                    for to in &selections {
                        if from == to {
                            continue;
                        }
                        let mut staged = in_room.clone();
                        clock_values(&mut staged, to);
                        galaxy_sim::life::update(&mut staged, registry)?;
                        let mut ticks = 0;
                        while staged.entities.values().any(|c| {
                            c.get(LIFE)
                                .is_some_and(|l| !text(&l.fields, "phase").is_empty())
                        }) {
                            view.before_step(&staged);
                            staged.tick += 1;
                            galaxy_sim::places::advance_walks(&mut staged);
                            galaxy_sim::life::update(&mut staged, registry)?;
                            galaxy_sim::services::ground(&mut staged);
                            for alpha in [0.5, 1.0] {
                                findings.extend(view.check_life_pose(&staged, size, alpha));
                            }
                            if !findings.is_empty() {
                                return Ok(findings
                                    .into_iter()
                                    .map(|f| {
                                        format!(
                                            "life transition {from:?} -> {to:?}, tick {ticks}: {f}"
                                        )
                                    })
                                    .collect());
                            }
                            ticks += 1;
                            if ticks > 18_000 {
                                return Err("life check: a transition exceeds 10 minutes; check its speed and spots".into());
                            }
                        }
                    }
                }
            }
            Ok(findings)
        };
        match run(self) {
            Ok(mut f) => {
                f.sort();
                f.dedup();
                f
            }
            Err(e) => vec![e],
        }
    }
}
