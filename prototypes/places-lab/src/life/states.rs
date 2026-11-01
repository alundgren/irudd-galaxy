//! States: people do things in their places, and you see them do it. Each person has a
//! day of acts (at work, eating, on a break, visiting; `day.rs`), each with a spot and a
//! pose. When the day moves them on in the room you are in, or a story moment does, you
//! watch them get up and walk: to another spot, out of the door, or in through it.
//! Elsewhere it happens out of sight. The station shows, on each module's sign, who is
//! there who doesn't belong there.

use std::cell::RefCell;
use std::collections::VecDeque;

use render::glam::Vec2;
use serde_json::{Value as Json, json};

use super::Life;
use super::day::{self, Shared, Target};
use crate::figures::Pose;
use crate::places::{Person, Places};

/// Walking pace, metres a second.
const PACE: f32 = 1.5;
/// A beat before someone gets up: you see them notice it's time.
const BEAT: f32 = 0.9;
/// How long someone coming to your room takes from elsewhere before they come in.
const ON_THE_WAY: f32 = 1.6;
/// A story moment out of sight waits this many of the player's moves for them to come
/// to the room it starts or ends in (to see it), then happens out of sight.
const STORY_WAITS: u32 = 3;

#[derive(Debug)]
enum Leg {
    Wait(f32),
    /// Walk on this place's floor to here.
    Walk(Vec2),
    /// Step into `place` (through its door, out of sight of where they were).
    Enter(String, Vec2),
    /// Arrived: take up the act.
    Settle(Target),
}

#[derive(Debug)]
struct Job {
    who: String,
    legs: VecDeque<Leg>,
    /// Waits for the player's view to land before it starts (changes made as the player
    /// sets off play once they are there to see them).
    after_landing: bool,
    /// The way they face while walking, radians from facing the camera.
    heading: f32,
    walking: bool,
}

#[derive(Default)]
pub struct States {
    clock: Shared,
    jobs: Vec<Job>,
    /// Story moments out of sight: who, where to, and the player's moves left to come
    /// and see it.
    pending: Vec<(String, Target, u32)>,
    /// The player's view has landed since they last set off.
    landed: bool,
    /// Who was talking at the last frame: nobody walks off mid-line.
    talking: RefCell<Vec<String>>,
}

/// The temporary floor spot someone walking stands on.
fn walk_spot(who: &str) -> String {
    format!("~{who}")
}

/// Puts `who` at `pos` on `place`'s floor, mid-walk.
fn stand(model: &mut Places, who: &str, place: &str, pos: Vec2) {
    let name = walk_spot(who);
    let Some(pl) = model.places.iter_mut().find(|p| p.id == place) else {
        return;
    };
    match pl.spots.iter_mut().find(|(n, _)| *n == name) {
        Some(s) => s.1 = pos,
        None => pl.spots.push((name.clone(), pos)),
    }
    let mut moved = false;
    if let Some(p) = model.people.iter_mut().find(|p| p.id == who) {
        moved = p.place != place;
        p.place = place.to_string();
        p.spot = name;
    }
    if moved {
        model.version += 1;
    }
}

fn clear_walk_spots(model: &mut Places, who: &str) {
    let name = walk_spot(who);
    for p in &mut model.places {
        p.spots.retain(|(n, _)| *n != name);
    }
}

fn door(model: &Places, place: &str) -> Vec2 {
    day::spot_pos(model, place, "door")
        .or(model.place(place).map(|p| p.arrive))
        .unwrap_or(Vec2::ZERO)
}

impl States {
    pub fn new(clock: Shared) -> States {
        States {
            clock,
            ..States::default()
        }
    }

    /// How `who` gets to `to`, seen from `here`: a walk the player watches, or at once.
    fn change(&mut self, model: &mut Places, who: &str, to: Target, after_landing: bool) {
        let Some(p) = model.person(who) else { return };
        let (from, here) = (p.place.clone(), model.here.clone());
        let spot = day::spot_pos(model, &to.place, &to.spot);
        let mut legs = VecDeque::new();
        if from == here && to.place == here {
            legs.push_back(Leg::Wait(BEAT));
            // Only a walk if they're going somewhere (eating at the stall they work at
            // isn't a walk).
            legs.extend(spot.filter(|s| *s != model.floor_pos(p)).map(Leg::Walk));
        } else if from == here {
            legs.push_back(Leg::Wait(BEAT));
            legs.push_back(Leg::Walk(door(model, &from)));
        } else if to.place == here {
            let door = door(model, &to.place);
            legs.push_back(Leg::Wait(ON_THE_WAY));
            legs.push_back(Leg::Enter(to.place.clone(), door));
            legs.extend(spot.map(Leg::Walk));
        } else {
            day::put(model, who, &to);
            return;
        }
        legs.push_back(Leg::Settle(to));
        self.jobs.retain(|j| j.who != who);
        self.jobs.push(Job {
            who: who.to_string(),
            legs,
            after_landing,
            heading: 0.0,
            walking: false,
        });
    }

    /// The clock moved on as the player set off into a room: story moments waiting to
    /// be seen, then the day's changes. Those in that room play out as they arrive.
    fn next(&mut self, model: &mut Places, changes: Vec<(String, day::Act)>) {
        let here = model.here.clone();
        for (who, to, left) in std::mem::take(&mut self.pending) {
            if changes.iter().any(|(w, _)| *w == who) {
                continue; // their day has moved on past it
            }
            let from = model.person(&who).map(|p| p.place.clone()).unwrap_or_default();
            if from == here || to.place == here || left <= 1 {
                self.change(model, &who, to, true);
            } else {
                self.pending.push((who, to, left - 1));
            }
        }
        for (who, act) in changes {
            self.change(model, &who, Target::of(act), true);
        }
    }

    /// Everyone still on their way gets there at once (the player has gone elsewhere).
    fn finish_all(&mut self, model: &mut Places) {
        for j in std::mem::take(&mut self.jobs) {
            clear_walk_spots(model, &j.who);
            if let Some(Leg::Settle(to)) = j.legs.back() {
                day::put(model, &j.who, to);
            }
        }
    }

    fn walk(&mut self, model: &mut Places, dt: f32, talking: &[String]) {
        let landed = self.landed;
        let mut done = Vec::new();
        for (i, j) in self.jobs.iter_mut().enumerate() {
            j.walking = false;
            if (j.after_landing && !landed) || talking.contains(&j.who) {
                continue;
            }
            let Some(p) = model.person(&j.who) else {
                done.push(i);
                continue;
            };
            let (place, pos) = (p.place.clone(), model.floor_pos(p));
            match j.legs.front_mut() {
                Some(Leg::Wait(t)) => {
                    *t -= dt;
                    if *t <= 0.0 {
                        j.legs.pop_front();
                    }
                }
                Some(Leg::Walk(to)) => {
                    let d = *to - pos;
                    let step = PACE * dt;
                    j.walking = true;
                    // On their feet: what they were doing is over (their sign says so).
                    if let Some(p) = model.people.iter_mut().find(|p| p.id == j.who)
                        && p.state != "walk"
                    {
                        p.state = "walk".into();
                        model.version += 1;
                    }
                    if d.length() <= step {
                        let to = *to;
                        j.legs.pop_front();
                        stand(model, &j.who, &place, to);
                    } else {
                        j.heading = d.x.atan2(-d.y);
                        stand(model, &j.who, &place, pos + d.normalize() * step);
                    }
                }
                Some(Leg::Enter(into, at)) => {
                    let (into, at) = (into.clone(), *at);
                    j.legs.pop_front();
                    clear_walk_spots(model, &j.who);
                    stand(model, &j.who, &into, at);
                }
                Some(Leg::Settle(to)) => {
                    clear_walk_spots(model, &j.who);
                    day::put(model, &j.who, &to.clone());
                    done.push(i);
                }
                None => done.push(i),
            }
        }
        for i in done.into_iter().rev() {
            self.jobs.remove(i);
        }
    }
}

impl Life for States {
    fn name(&self) -> &'static str {
        "states"
    }

    /// F5 starts the day again.
    fn setup(&mut self, _r: &mut render::Renderer, _world: &crate::world::World, _model: &Places) {
        *self.clock.borrow_mut() = day::Clock::default();
        *self = States::new(self.clock.clone());
    }

    fn moved(
        &mut self,
        model: &mut Places,
        person: &str,
        place: Option<&str>,
        state: Option<&str>,
        spot: Option<&str>,
    ) {
        let Some(p) = model.person(person) else { return };
        let to = Target::story(p, place, state, spot);
        self.clock.borrow_mut().story(person);
        if p.place == model.here || to.place == model.here {
            self.change(model, person, to, false);
        } else {
            // Out of sight: it waits a little for the player to come and see it.
            self.pending.retain(|(w, ..)| w != person);
            self.pending.push((person.to_string(), to, STORY_WAITS));
        }
    }

    fn arrived(&mut self, _model: &mut Places, _place: &str) {
        self.landed = true;
    }

    fn leave(&mut self, model: &mut Places) {
        self.finish_all(model);
        for (who, to, _) in std::mem::take(&mut self.pending) {
            day::put(model, &who, &to);
        }
    }

    fn step(&mut self, model: &mut Places, dt: f32) {
        let talking = self.talking.take();
        let moved = self.clock.borrow_mut().tick(model);
        if let Some(moved) = moved {
            // The player has gone: anyone still on their way gets there.
            self.finish_all(model);
            self.landed = false;
            if moved.ticked {
                self.next(model, moved.changes);
            }
        }
        self.walk(model, dt, &talking);
    }

    fn pose(&self, who: &Person, t: f32, talking: bool) -> Pose {
        if talking {
            self.talking.borrow_mut().push(who.id.clone());
        }
        match self.jobs.iter().find(|j| j.who == who.id && j.walking) {
            Some(j) if !talking => {
                let s = t * 8.5;
                let mut p = Pose::still();
                p.turn = j.heading;
                p.lift = s.sin().abs() * 0.05;
                p.sway = s.sin() * 0.05;
                p.head_nod = 0.06;
                p
            }
            _ => day::act_pose(who, t, talking),
        }
    }

    fn world(&self, _model: &Places) -> Json {
        json!({ "name": "states", "away": true })
    }

    fn settling(&self, _model: &Places) -> bool {
        let talking = self.talking.borrow();
        self.jobs.iter().any(|j| !talking.contains(&j.who))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab() -> Places {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab");
        let mut host = crate::script::Host::new(dir);
        Places::read(&host.places().unwrap()).unwrap()
    }

    /// Runs the variant until nobody is walking, a second at a time.
    fn settle(v: &mut States, m: &mut Places) {
        let here = m.here.clone();
        v.arrived(m, &here);
        for _ in 0..600 {
            v.step(m, 1.0 / 30.0);
            if v.jobs.is_empty() {
                return;
            }
        }
        panic!("still walking: {:?}", v.jobs);
    }

    #[test]
    fn every_act_of_the_day_is_a_real_spot() {
        let m = lab();
        for r in day::DAY {
            for (_, a) in r.acts {
                assert!(
                    day::spot_pos(&m, a.place, a.spot).is_some(),
                    "{}: {} has no spot {}",
                    r.who,
                    a.place,
                    a.spot
                );
            }
        }
    }

    #[test]
    fn someone_leaving_the_room_you_are_in_walks_out_and_is_gone() {
        let mut m = lab();
        let mut v = States::default();
        v.step(&mut m, 0.0);
        m.go("canteen_block");
        v.step(&mut m, 0.0);
        assert_eq!(m.person("pim").unwrap().place, "canteen");
        v.moved(&mut m, "pim", Some("market"), Some("work"), Some("crates"));
        assert!(v.settling(&m), "Pim walks out where you can see");
        let before = m.floor_pos(m.person("pim").unwrap());
        for _ in 0..60 {
            v.step(&mut m, 1.0 / 30.0);
        }
        let mid = m.person("pim").unwrap();
        assert_eq!(mid.place, "canteen", "still on his way to the door");
        assert_ne!(m.floor_pos(mid), before, "he has moved");
        settle(&mut v, &mut m);
        let p = m.person("pim").unwrap();
        assert_eq!((p.place.as_str(), p.spot.as_str()), ("market", "crates"));
        assert!(m.places.iter().all(|p| p.spots.iter().all(|(n, _)| !n.starts_with('~'))));
    }

    #[test]
    fn out_of_sight_a_story_move_waits_for_the_player_to_come_and_see() {
        let mut m = lab();
        let mut v = States::default();
        v.step(&mut m, 0.0);
        m.go("office_block");
        v.step(&mut m, 0.0);
        settle(&mut v, &mut m);
        v.moved(&mut m, "pim", Some("market"), Some("work"), Some("crates"));
        v.step(&mut m, 1.0);
        assert_eq!(m.person("pim").unwrap().place, "canteen");
        m.go("wharf");
        v.step(&mut m, 0.0);
        assert_eq!(m.person("pim").unwrap().place, "canteen", "waits for the player");
        // Going to the canteen, the player sees him get up and go.
        m.go("canteen_block");
        v.step(&mut m, 0.0);
        assert!(v.settling(&m));
        settle(&mut v, &mut m);
        assert_eq!(m.person("pim").unwrap().place, "market");
    }

    #[test]
    fn a_story_move_nobody_comes_to_see_happens_anyway() {
        let mut m = lab();
        let mut v = States::default();
        v.step(&mut m, 0.0);
        m.go("office_block");
        v.step(&mut m, 0.0);
        v.moved(&mut m, "pim", Some("market"), Some("work"), Some("crates"));
        for to in ["wharf", "office_block", "wharf", "office_block", "wharf", "office_block"] {
            m.go(to);
            v.step(&mut m, 0.0);
            settle(&mut v, &mut m);
        }
        // Gone from the canteen, out of sight: by now his day has him on his rounds.
        assert_ne!(m.person("pim").unwrap().place, "canteen");
        assert!(v.pending.is_empty());
        assert!(!v.settling(&m));
    }

    #[test]
    fn the_day_moves_people_as_the_player_goes_about() {
        let mut m = lab();
        let mut v = States::default();
        v.step(&mut m, 0.0);
        // The clock moves as the player goes into a room, not back out. Orrin goes for
        // his lunch on the fourth, and is back on the sixth.
        for visit in 1..=8 {
            for to in ["office_block", "wharf"] {
                m.go(to);
                v.step(&mut m, 0.0);
                settle(&mut v, &mut m);
            }
            let at = m.person("orrin").unwrap().place.clone();
            let want = if (4..6).contains(&visit) { "canteen" } else { "cradle" };
            assert_eq!(at, want, "after visit {visit}");
        }
    }
}
