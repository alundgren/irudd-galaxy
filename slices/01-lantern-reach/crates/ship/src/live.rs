//! The reserved rock-lamp input and survey-buoy interaction. Survey rewards are paid
//! by `galaxy-progress`, which owns standing deeds.
use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};

/// The flag the rock-lamp verb needs.
pub const ROCK_TAGGING: &str = "mech.rock_tagging";

/// Whether a batch already queued this tick (a rollback, a retirement, an earlier
/// system's) holds a command `f` matches. Input worked out from the tick's start waits
/// a tick rather than overwrite it.
fn queued_change(ctx: &TickCtx<'_>, f: impl Fn(&Command) -> bool) -> bool {
    ctx.queued().iter().flat_map(|b| &b.commands).any(f)
}

pub struct LiveInteractions;
impl System for LiveInteractions {
    fn name(&self) -> &'static str {
        "live_interactions"
    }
    fn step(&self, ctx: &mut TickCtx<'_>) {
        let w = ctx.world;
        let Some(ship) = w.player.ship.as_ref().and_then(|id| w.ships.get(id)) else {
            return;
        };
        if ship.docked_at.is_some() || ship.activity.transit.is_some() {
            return;
        }
        if ctx.input.tag
            && w.player.flags.contains(ROCK_TAGGING)
            && !queued_change(ctx, |c| match c {
                Command::SetRockTag { .. } => true,
                Command::SetFlag { flag, .. } => flag.as_str() == ROCK_TAGGING,
                _ => false,
            })
            && let Some(rock) = w
                .rocks
                .values()
                .filter(|r| {
                    w.fields
                        .get(&r.field)
                        .is_some_and(|f| f.region == ship.region)
                        && r.position.distance(ship.position) <= w.ship_rules.mining.reach
                })
                .min_by(|a, b| {
                    a.position
                        .distance(ship.position)
                        .total_cmp(&b.position.distance(ship.position))
                })
        {
            let on = !w.rock_tags.contains(&rock.id);
            if !on || w.rock_tags.len() < 6 {
                let mut batch =
                    CommandBatch::new(Cause::system("rock_tagging", "the pilot tends a rock lamp"));
                batch.push(Command::ExpectFlag {
                    flag: FlagId::new(ROCK_TAGGING).unwrap(),
                    on: true,
                });
                batch.push(Command::SetRockTag {
                    rock: rock.id.clone(),
                    on,
                });
                batch.push(Command::RecordFact {
                    fact: Box::new(FactDraft {
                        kind: "tagged".parse().unwrap(),
                        subjects: Subjects {
                            ship: Some(ship.id.clone()),
                            rock: Some(rock.id.clone()),
                            region: Some(ship.region.clone()),
                            ..Default::default()
                        },
                        amount: Some(i64::from(on)),
                        note: "A rock lamp was tended".into(),
                    }),
                });
                ctx.submit(batch);
            }
        }
        if ctx.input.action {
            for (id, buoy) in &w.survey_buoys {
                if !buoy.read
                    && buoy.region == ship.region
                    && buoy.position.distance(ship.position) <= buoy.radius
                {
                    if queued_change(ctx, |c| match c {
                        Command::PutSurveyBuoy { id: other, .. }
                        | Command::UpdateSurveyBuoy { id: other, .. } => other == id,
                        _ => false,
                    }) {
                        break;
                    }
                    let mut read = buoy.clone();
                    read.read = true;
                    let mut batch = CommandBatch::new(Cause::system(
                        "survey_buoy",
                        "the pilot reads a survey buoy log",
                    ));
                    batch.push(Command::UpdateSurveyBuoy {
                        id: id.clone(),
                        from: Box::new(buoy.clone()),
                        to: Box::new(read),
                    });
                    batch.push(Command::RecordFact {
                        fact: Box::new(FactDraft {
                            kind: "survey_buoy_read".parse().unwrap(),
                            subjects: Subjects {
                                ship: Some(ship.id.clone()),
                                faction: Some(buoy.planted_by.clone()),
                                other: Some(id.parse().unwrap()),
                                region: Some(ship.region.clone()),
                                ..Default::default()
                            },
                            amount: None,
                            note: "The survey log identifies who planted the stale claim".into(),
                        }),
                    });
                    ctx.submit(batch);
                    break;
                }
            }
        }
    }
}
