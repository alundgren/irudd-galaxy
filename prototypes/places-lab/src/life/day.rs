//! The station's day, shared by the variants that give people lives of their own
//! (`states`, `return`): what each person does, where, as the station's clock moves on,
//! and how they stand while they do it.
//!
//! The clock moves one step each time the player sets off into a room. Time passes as
//! you go about, not while you stand and look, so a room never changes under the
//! pointer; and not on the way back out, so who the station shows where is still there
//! when you go in (or you see them leave as you come).

use std::cell::RefCell;
use std::rc::Rc;

use render::glam::Vec2;

use super::idle;
use crate::figures::Pose;
use crate::places::{Kind, Person, Places};

/// What someone is doing: where, at which spot, in which state. The state names are the
/// content's (`data.chat` picks a line by them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Act {
    pub place: &'static str,
    pub spot: &'static str,
    pub state: &'static str,
}

const fn act(place: &'static str, spot: &'static str, state: &'static str) -> Act {
    Act { place, spot, state }
}

/// A person's day: acts in order, each lasting some clock steps, round and round.
pub struct Routine {
    pub who: &'static str,
    pub acts: &'static [(u32, Act)],
    /// The day only starts at the person's first story moment (Pim's starts when Ilse's
    /// welcome sends him to the market); until then they stay as the content put them.
    pub after_story: bool,
}

/// Lantern Wharf's day, twelve steps round. Where people eat says who they are: Juno eats
/// at her stall and never leaves the till, Orrin takes his lunch at the window table,
/// half turned to Bel at her counter, Bel eats last, standing at her own end of the
/// counter, Pim eats sitting down with his satchel on.
///
/// Going round the rooms in order (office, market, cradle, canteen: one step each), each
/// round shows something new where you are: first Pim turning up at the market and Orrin
/// coming in for lunch; then Ilse at the window, Juno eating at the stall and Bel stopping
/// to eat; then Pim at the office with a parcel, Juno counting stock and Orrin
/// leaving the cradle for lunch.
pub const DAY: &[Routine] = &[
    Routine {
        who: "ilse",
        acts: &[
            (5, act("office", "desk", "work")),
            (1, act("office", "window", "break")),
            (6, act("office", "desk", "work")),
        ],
        after_story: false,
    },
    Routine {
        who: "juno",
        acts: &[
            (6, act("market", "stall", "work")),
            (2, act("market", "stall", "snack")),
            (2, act("market", "stall", "work")),
            (1, act("market", "shelf", "count")),
            (1, act("market", "stall", "work")),
        ],
        after_story: false,
    },
    Routine {
        who: "orrin",
        acts: &[
            (4, act("cradle", "bench", "work")),
            (2, act("canteen", "stool", "lunch")),
            (5, act("cradle", "bench", "work")),
            (1, act("canteen", "stool", "lunch")),
        ],
        after_story: false,
    },
    Routine {
        who: "bel",
        acts: &[
            (8, act("canteen", "counter", "work")),
            (1, act("canteen", "nook", "snack")),
            (3, act("canteen", "counter", "work")),
        ],
        after_story: false,
    },
    Routine {
        who: "pim",
        acts: &[
            (6, act("market", "crates", "work")),
            (2, act("canteen", "table", "eat")),
            (1, act("office", "parcel", "visit")),
            (3, act("market", "crates", "work")),
        ],
        after_story: true,
    },
];

/// The player set off: from where, whether the clock moved, and who now does what.
pub struct Moved {
    pub left: Option<String>,
    pub ticked: bool,
    pub changes: Vec<(String, Act)>,
}

/// The clock, shared by the variants that run the day, so switching between them (key
/// 4) keeps the station's time and who is where.
pub type Shared = Rc<RefCell<Clock>>;

/// The clock and where each person's day has got to.
#[derive(Default)]
pub struct Clock {
    pub now: u32,
    /// When each started day began (by person); a day that waits for a story moment is
    /// missing until then.
    started: Vec<(&'static str, u32)>,
    /// The place the player was last seen in, to notice them setting off.
    pub last_here: Option<String>,
}

impl Clock {
    fn start_of(&self, r: &Routine) -> Option<u32> {
        if !r.after_story {
            return Some(0);
        }
        self.started.iter().find(|(w, _)| *w == r.who).map(|(_, t)| *t)
    }

    /// What `who` does at clock `t`, if their day has started.
    pub fn act_at(&self, who: &str, t: u32) -> Option<Act> {
        let r = DAY.iter().find(|r| r.who == who)?;
        let start = self.start_of(r)?;
        let total: u32 = r.acts.iter().map(|(n, _)| n).sum();
        let mut i = t.checked_sub(start)? % total.max(1);
        for (n, a) in r.acts {
            if i < *n {
                return Some(*a);
            }
            i -= n;
        }
        None
    }

    /// Where everyone whose day has started should be now.
    pub fn everyone(&self) -> Vec<(&'static str, Act)> {
        DAY.iter()
            .filter_map(|r| Some((r.who, self.act_at(r.who, self.now)?)))
            .collect()
    }

    /// A story moment moved `who`: a day that waits for one starts now.
    pub fn story(&mut self, who: &str) {
        if let Some(r) = DAY.iter().find(|r| r.who == who && r.after_story)
            && self.start_of(r).is_none()
        {
            self.started.push((r.who, self.now));
        }
    }

    /// Has the player set off somewhere since the last look? Into a room, the clock
    /// moves on: who now does something else is in `changes`.
    pub fn tick(&mut self, model: &Places) -> Option<Moved> {
        let here = model.here.clone();
        if self.last_here.as_deref() == Some(here.as_str()) {
            return None;
        }
        let left = self.last_here.replace(here.clone());
        let into_room = model.place(&here).is_some_and(|p| p.kind == Kind::Interior);
        let mut moved = Moved {
            left,
            ticked: false,
            changes: Vec::new(),
        };
        if moved.left.is_none() || !into_room {
            return Some(moved);
        }
        moved.ticked = true;
        let was = self.now;
        self.now += 1;
        for r in DAY {
            let (before, after) = (self.act_at(r.who, was), self.act_at(r.who, self.now));
            if let Some(a) = after
                && before != Some(a)
                && model.person(r.who).is_some()
            {
                moved.changes.push((r.who.to_string(), a));
            }
        }
        Some(moved)
    }
}

/// Where `act` puts someone on the floor, if the place has that spot.
pub fn spot_pos(model: &Places, place: &str, spot: &str) -> Option<Vec2> {
    model.place(place)?.spot(spot)
}

/// How someone stands while doing what they do. Talking always faces the room's camera,
/// so the face the line belongs to is seen, and nods without the little hop: what hangs
/// off their head (their line) holds still.
pub fn act_pose(who: &Person, t: f32, talking: bool) -> Pose {
    let mut p = idle(who, t, talking);
    if who.state == "lunch" {
        p.lift -= 0.35; // sitting
    }
    if talking {
        p.lift = p.lift.min(0.0);
        return p;
    }
    // A spoonful every couple of seconds.
    let spoon = (t * 2.4).sin().max(0.0);
    match who.state.as_str() {
        // Busy with the work in front of them: looking down at it, a working sway.
        "work" | "count" => {
            p.head_nod = 0.28 + (t * 2.3).sin() * 0.1;
            p.head_turn = (t * 0.8).sin() * 0.25;
            p.sway = (t * 1.6).sin() * 0.05;
        }
        // Eating, sitting down (`eat`, lower: `idle` sits them) or on their feet.
        "eat" | "snack" => {
            p.head_nod = 0.18 + spoon * 0.22;
            p.head_turn *= 0.4;
        }
        // Orrin's lunch at the window table, half turned to the counter and Bel.
        "lunch" => {
            p.turn = -1.1;
            p.head_nod = 0.18 + spoon * 0.22;
            p.head_turn *= 0.4;
        }
        // Looking out, away from the room.
        "break" => {
            p.turn = 2.7;
            p.head_turn = (t * 0.3).sin() * 0.3;
            p.sway = (t * 0.4).sin() * 0.03;
        }
        // Chatting with whoever's place it is (Pim at Ilse's desk): turned to them,
        // nodding along.
        "visit" => {
            p.turn = 0.8;
            p.head_nod = (t * 3.1).sin().max(0.0) * 0.12;
            p.head_turn = 0.2 + (t * 0.5).sin() * 0.2;
        }
        _ => {}
    }
    p
}

/// Where a move takes someone (a story moment's, or the day's next act), owned.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub place: String,
    pub spot: String,
    pub state: String,
}

impl Target {
    pub fn of(a: Act) -> Target {
        Target {
            place: a.place.into(),
            spot: a.spot.into(),
            state: a.state.into(),
        }
    }

    /// A story moment's move: what it doesn't say stays as it is.
    pub fn story(
        who: &Person,
        place: Option<&str>,
        state: Option<&str>,
        spot: Option<&str>,
    ) -> Target {
        Target {
            place: place.unwrap_or(&who.place).into(),
            spot: spot.unwrap_or(&who.spot).into(),
            state: state.unwrap_or(&who.state).into(),
        }
    }
}

/// Puts someone at once where `to` says. A new place or state is one the screens must
/// hear of (who is where, what their sign says); a new spot alone isn't.
pub fn put(model: &mut Places, who: &str, to: &Target) {
    if model.place(&to.place).is_none() {
        return;
    }
    let Some(p) = model.people.iter_mut().find(|p| p.id == who) else {
        return;
    };
    let changed = p.place != to.place || p.state != to.state;
    p.place = to.place.clone();
    p.spot = to.spot.clone();
    p.state = to.state.clone();
    if changed {
        model.version += 1;
    }
}
