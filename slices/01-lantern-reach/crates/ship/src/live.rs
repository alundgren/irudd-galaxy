//! The reserved rock-lamp input and survey-buoy interaction.
use galaxy_contract::prelude::*;
use galaxy_core::{System, TickCtx};
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
        for event in ctx.events {
            let Event::Reply { npc, choice, .. } = &event.event else {
                continue;
            };
            for (id, buoy) in &w.survey_buoys {
                let Some(reward) = &buoy.reward else { continue };
                if !buoy.read || reward.claimed || &reward.person != npc || &reward.choice != choice
                {
                    continue;
                }
                if !w
                    .npcs
                    .get(npc)
                    .is_some_and(|n| n.location.is_some() && n.location == ship.docked_at)
                {
                    continue;
                }
                let mut paid = buoy.clone();
                paid.reward.as_mut().unwrap().claimed = true;
                let mut batch = CommandBatch::new(
                    Cause::verb(
                        Actor::Player,
                        VerbKind::Wait,
                        "the pilot claims the survey-log reward",
                    )
                    .citing([event.fact]),
                );
                batch.push(Command::ExpectBuoyReward { id: id.clone() });
                batch.push(Command::ExpectDocked {
                    ship: ship.id.clone(),
                    station: ship.docked_at.clone().unwrap(),
                });
                batch.push(Command::AdjustCredits {
                    delta: i64::from(reward.credits) * CENTS_PER_CREDIT,
                });
                for (faction, delta) in &reward.standing {
                    batch.push(Command::AdjustStanding {
                        faction: faction.clone(),
                        delta: *delta,
                    });
                }
                batch.push(Command::PutSurveyBuoy {
                    id: id.clone(),
                    value: Some(Box::new(paid)),
                });
                batch.push(Command::RecordFact {
                    fact: Box::new(FactDraft {
                        kind: "survey_reward_claimed".parse().unwrap(),
                        subjects: Subjects {
                            ship: Some(ship.id.clone()),
                            npc: Some(npc.clone()),
                            station: ship.docked_at.clone(),
                            other: Some(id.parse().unwrap()),
                            ..Default::default()
                        },
                        amount: Some(i64::from(reward.credits)),
                        note: "The pilot was paid for exposing a planted claim".into(),
                    }),
                });
                ctx.submit(batch);
            }
        }
        if ship.docked_at.is_some() || ship.activity.transit.is_some() {
            return;
        }
        if ctx.input.tag
            && w.player.flags.contains("mech.rock_tagging")
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
                    let mut read = buoy.clone();
                    read.read = true;
                    let mut batch = CommandBatch::new(Cause::system(
                        "survey_buoy",
                        "the pilot reads a survey buoy log",
                    ));
                    batch.push(Command::PutSurveyBuoy {
                        id: id.clone(),
                        value: Some(Box::new(read)),
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
