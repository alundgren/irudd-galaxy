//! Survey-buoy rewards. A pack places a buoy with a reward (`ctx.kit:buoy`, which needs
//! the pack's credit and standing capabilities); once the pilot has read it, answering
//! the person it names with its choice pays it once. The payment is the player's own
//! (a verb, not journaled), so rolling the pack back keeps what was earned.

use galaxy_contract::prelude::*;

use crate::standing::Deeds;

/// Fact kind recorded when a reward is paid.
pub const CLAIMED: &str = "survey_reward_claimed";

/// One batch per reward the replies committed last tick claim. Each is worked out
/// after the standing changes in `earlier` (queued this tick) and the claims before
/// it, and carries a compare-and-set on its buoy, so a buoy removed, replaced or
/// already paid by a batch ahead of it rejects the claim whole.
pub fn claims(
    world: &WorldState,
    events: &[EventEnvelope],
    earlier: &[Command],
) -> Vec<CommandBatch> {
    let Some(ship) = world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
    else {
        return Vec::new();
    };
    let Some(station) = ship.docked_at.clone() else {
        return Vec::new();
    };
    let mut written: Vec<Command> = earlier.to_vec();
    let mut out = Vec::new();
    for event in events {
        let Event::Reply { npc, choice, .. } = &event.event else {
            continue;
        };
        if !world
            .npcs
            .get(npc)
            .is_some_and(|n| n.location.as_ref() == Some(&station))
        {
            continue;
        }
        for (id, buoy) in &world.survey_buoys {
            let Some(reward) = &buoy.reward else { continue };
            if !buoy.read || reward.claimed || &reward.person != npc || &reward.choice != choice {
                continue;
            }
            let mut paid = buoy.clone();
            if let Some(r) = paid.reward.as_mut() {
                r.claimed = true;
            }
            let mut batch = CommandBatch::new(
                Cause::verb(
                    Actor::Player,
                    VerbKind::Wait,
                    "the pilot claims the survey-log reward",
                )
                .citing([event.fact]),
            );
            batch.push(Command::ExpectDocked {
                ship: ship.id.clone(),
                station: station.clone(),
            });
            batch.push(Command::UpdateSurveyBuoy {
                id: id.clone(),
                from: Box::new(buoy.clone()),
                to: Box::new(paid),
            });
            if reward.credits > 0 {
                batch.push(Command::AdjustCredits {
                    delta: i64::from(reward.credits) * CENTS_PER_CREDIT,
                });
            }
            let mut deeds = Deeds::after(world, &written);
            for (faction, delta) in &reward.standing {
                deeds.deed(faction, &reward.choice, *delta, &[]);
            }
            batch.commands.extend(deeds.commands);
            batch.push(Command::RecordFact {
                fact: Box::new(FactDraft {
                    kind: CLAIMED.parse().unwrap(),
                    subjects: Subjects {
                        ship: Some(ship.id.clone()),
                        npc: Some(npc.clone()),
                        station: Some(station.clone()),
                        other: id.parse().ok(),
                        ..Default::default()
                    },
                    amount: Some(i64::from(reward.credits)),
                    note: "The pilot was paid for exposing a planted claim".into(),
                }),
            });
            written.extend(batch.commands.iter().cloned());
            out.push(batch);
        }
    }
    out
}
