//! Return: the station has moved on when you come back. Nothing changes in front of
//! you; while you are elsewhere people go about their day (`day.rs`), story moments
//! happen and what you did leaves a mark (the ice you sold stacked in the market, the
//! crate your part came in, a mug for you on Bel's counter). The variant remembers each
//! room as you left it, and coming back the differences show plainly but quietly: who
//! is here now, who has gone where, what is new. The station's signs say which rooms
//! have changed, so you needn't check them all, and the person with something to say
//! about what you did says it as you come in (the Luau's part: `lab/life.luau`).

use std::collections::HashMap;

use render::glam::{Mat4, Vec3};
use render::{Color, MeshBuilder, MeshHandle, Renderer, palette};
use serde_json::{Value as Json, json};

use super::Life;
use super::day::{self, Shared, Target};
use crate::figures::Pose;
use crate::places::{Kind, Person, Places};
use crate::world::World;

/// A room as the player last left it.
struct Seen {
    /// Who was there: (person, state).
    people: Vec<(String, String)>,
    /// The clock when they left: details since then are new.
    at: u32,
}

/// Something new in a room because of what the player did.
struct Detail {
    id: String,
    room: &'static str,
    spot: &'static str,
    look: &'static str,
    /// Height above the floor.
    z: f32,
    /// The clock from which it is there: after the player has gone.
    since: u32,
}

#[derive(Default)]
pub struct Return {
    clock: Shared,
    /// Story moments where the player is: they happen once their back is turned.
    pending: Vec<(String, Target)>,
    seen: HashMap<String, Seen>,
    details: Vec<Detail>,
    meshes: HashMap<&'static str, MeshHandle>,
}

impl Return {
    pub fn new(clock: Shared) -> Return {
        Return {
            clock,
            ..Return::default()
        }
    }

    fn now(&self) -> u32 {
        self.clock.borrow().now
    }

    fn add(&mut self, id: String, room: &'static str, spot: &'static str, look: &'static str, z: f32) {
        let since = self.now() + 1;
        match self.details.iter_mut().find(|d| d.spot == spot && d.room == room) {
            // One thing to a spot: a newer one takes its place.
            Some(d) if d.id != id => {
                *d = Detail { id, room, spot, look, z, since };
            }
            Some(_) => {}
            None => self.details.push(Detail { id, room, spot, look, z, since }),
        }
    }

    /// The details there are in `room` now.
    fn shown<'a>(&'a self, room: &'a str) -> impl Iterator<Item = &'a Detail> + 'a {
        self.details
            .iter()
            .filter(move |d| d.room == room && d.since <= self.now())
    }

    /// What changed in `room` since the player last left it, and how many moves ago
    /// that was (`away`: the station marks a room once the player has been elsewhere,
    /// not the moment they step out of it).
    fn changes(&self, model: &Places, room: &str) -> Option<Json> {
        let seen = self.seen.get(room)?;
        let now: Vec<&Person> = model.people_in(room).collect();
        let was = |id: &str| seen.people.iter().find(|(w, _)| w == id);
        let came: Vec<&str> = now
            .iter()
            .filter(|p| was(&p.id).is_none())
            .map(|p| p.id.as_str())
            .collect();
        let doing: Vec<Json> = now
            .iter()
            .filter(|p| was(&p.id).is_some_and(|(_, s)| *s != p.state))
            .map(|p| json!({ "who": p.id, "state": p.state }))
            .collect();
        let gone: Vec<Json> = seen
            .people
            .iter()
            .filter(|(w, _)| !now.iter().any(|p| p.id == *w))
            .filter_map(|(w, _)| model.person(w))
            .map(|p| json!({ "who": p.id, "to": p.place }))
            .collect();
        let details: Vec<&str> = self
            .details
            .iter()
            .filter(|d| d.room == room && d.since > seen.at && d.since <= self.now())
            .map(|d| d.id.as_str())
            .collect();
        let away = self.now() - seen.at;
        Some(json!({ "away": away, "came": came, "gone": gone, "doing": doing, "details": details }))
    }
}

fn mesh(f: impl FnOnce(&mut MeshBuilder)) -> render::MeshData {
    let mut b = MeshBuilder::new();
    f(&mut b);
    b.build()
}

fn boxed(b: &mut MeshBuilder, at: Vec3, size: Vec3) {
    b.with(Mat4::from_translation(at), |b| b.cuboid(size));
}

/// A slatted crate with what was sold heaped on it.
fn crate_of(load: Color) -> render::MeshData {
    mesh(|b| {
        b.paint(Color::hex(0x8a5a2e), 0.0);
        boxed(b, Vec3::new(0.0, 0.0, 0.22), Vec3::new(0.75, 0.55, 0.44));
        b.paint(Color::hex(0x5e3a1c), 0.0);
        for x in [-0.39, 0.39] {
            boxed(b, Vec3::new(x, 0.0, 0.22), Vec3::new(0.05, 0.58, 0.48));
        }
        b.paint(load, 0.15);
        for (x, y, z) in [(-0.18, -0.08, 0.53), (0.15, 0.06, 0.53), (0.0, -0.02, 0.72)] {
            boxed(b, Vec3::new(x, y, z), Vec3::splat(0.26));
        }
    })
}

impl Life for Return {
    fn name(&self) -> &'static str {
        "return"
    }

    /// F5 starts the day again; the details' meshes are uploaded once more.
    fn setup(&mut self, r: &mut Renderer, _world: &World, _model: &Places) {
        *self.clock.borrow_mut() = day::Clock::default();
        *self = Return::new(self.clock.clone());
        let mut add = |name: &'static str, m: render::MeshData| {
            let h = r.upload_mesh(&m);
            self.meshes.insert(name, h);
        };
        add("ice", crate_of(Color::hex(0xbfe6f2)));
        add("iron", crate_of(Color::hex(0xb5643a)));
        add("glassite", crate_of(Color::hex(0x5fc9a8)));
        // The crate a part came in: open, straw spilling, the Wren's name stencilled.
        add(
            "part_crate",
            mesh(|b| {
                b.paint(Color::hex(0xc98a3c), 0.0);
                boxed(b, Vec3::new(0.0, 0.0, 0.25), Vec3::new(0.9, 0.6, 0.5));
                b.paint(palette::MUSTARD, 0.2);
                boxed(b, Vec3::new(0.0, 0.0, 0.53), Vec3::new(0.8, 0.5, 0.08));
                b.paint(Color::hex(0x2f7f86), 0.0);
                boxed(b, Vec3::new(0.0, -0.31, 0.28), Vec3::new(0.5, 0.02, 0.14));
                b.paint(Color::hex(0x6e4220), 0.0);
                b.with(
                    Mat4::from_translation(Vec3::new(0.15, 0.35, 0.62))
                        * Mat4::from_rotation_x(-0.9),
                    |b| b.cuboid(Vec3::new(0.95, 0.04, 0.62)),
                );
            }),
        );
        // A mug for the player on the counter: teal, a mustard band.
        add(
            "mug",
            mesh(|b| {
                b.paint(Color::hex(0x2f7f86), 0.0);
                b.frustum(0.13, 0.15, 0.3, 10);
                b.paint(palette::MUSTARD, 0.3);
                b.with(Mat4::from_translation(Vec3::Z * 0.12), |b| {
                    b.frustum(0.152, 0.152, 0.07, 10)
                });
                b.paint(Color::hex(0x2f7f86), 0.0);
                b.with(Mat4::from_translation(Vec3::new(0.18, 0.0, 0.16)), |b| {
                    b.torus(0.08, 0.025, 0.04, 8, 4)
                });
            }),
        );
        // The Wren's pennant on a stand by Ilse's desk: the ship has a berth.
        add(
            "pennant",
            mesh(|b| {
                b.paint(Color::hex(0x3a2a1e), 0.0);
                b.frustum(0.2, 0.16, 0.06, 8);
                b.frustum(0.03, 0.03, 1.7, 6);
                b.paint(palette::MUSTARD, 0.2);
                b.tri(
                    Vec3::new(0.0, 0.0, 1.66),
                    Vec3::new(0.0, 0.0, 1.22),
                    Vec3::new(0.7, -0.05, 1.44),
                );
                b.tri(
                    Vec3::new(0.0, 0.0, 1.22),
                    Vec3::new(0.0, 0.0, 1.66),
                    Vec3::new(0.7, -0.05, 1.44),
                );
                b.paint(Color::hex(0x2f7f86), 0.0);
                b.tri(
                    Vec3::new(0.0, -0.01, 1.56),
                    Vec3::new(0.0, -0.01, 1.32),
                    Vec3::new(0.38, -0.04, 1.44),
                );
            }),
        );
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
            self.pending.retain(|(w, _)| w != person);
            self.pending.push((person.to_string(), to));
        } else {
            day::put(model, person, &to);
        }
    }

    fn step(&mut self, model: &mut Places, _dt: f32) {
        let moved = self.clock.borrow_mut().tick(model);
        let Some(moved) = moved else {
            return;
        };
        // The room as the player leaves it.
        if let Some(room) = moved
            .left
            .filter(|r| model.place(r).is_some_and(|p| p.kind == Kind::Interior))
        {
            let people = model
                .people_in(&room)
                .map(|p| (p.id.clone(), p.state.clone()))
                .collect();
            let at = self.now();
            self.seen.insert(room, Seen { people, at });
        }
        // Behind their back: what waited for them to go, then everyone where the day
        // has them now (whatever another variant left half done).
        for (who, to) in std::mem::take(&mut self.pending) {
            day::put(model, &who, &to);
        }
        if moved.ticked {
            for p in &mut model.places {
                p.spots.retain(|(n, _)| !n.starts_with('~'));
            }
            let everyone = self.clock.borrow().everyone();
            for (who, act) in everyone {
                day::put(model, who, &Target::of(act));
            }
        }
        model.version += 1;
    }

    fn event(&mut self, _model: &mut Places, on: &str, args: &Json) {
        let arg = |k: &str| args.get(k).and_then(Json::as_str).unwrap_or_default();
        match on {
            "sell" => {
                let look = match arg("ware") {
                    "ice" => "ice",
                    "iron" => "iron",
                    "glassite" => "glassite",
                    _ => return,
                };
                self.add(format!("sold_{look}"), "market", "pile", look, 0.0);
            }
            "fit" if arg("from") == "rack" => {
                self.add(format!("crate_{}", arg("part")), "cradle", "boxes", "part_crate", 0.0)
            }
            "buy_bowl" => self.add("mug".into(), "canteen", "mug", "mug", 1.08),
            "arrive" if arg("place") == "office" => {
                self.add("pennant".into(), "office", "flag", "pennant", 0.0)
            }
            _ => {}
        }
    }

    fn props(&self, model: &Places, place: &str) -> Vec<(MeshHandle, Mat4)> {
        self.shown(place)
            .filter_map(|d| {
                let at = day::spot_pos(model, place, d.spot)?;
                let mesh = *self.meshes.get(d.look)?;
                Some((mesh, Mat4::from_translation(at.extend(d.z)) * Mat4::from_rotation_z(0.35)))
            })
            .collect()
    }

    fn pose(&self, who: &Person, t: f32, talking: bool) -> Pose {
        day::act_pose(who, t, talking)
    }

    fn world(&self, model: &Places) -> Json {
        let rooms: serde_json::Map<String, Json> = model
            .places
            .iter()
            .filter(|p| p.kind == Kind::Interior)
            .filter_map(|p| Some((p.id.clone(), self.changes(model, &p.id)?)))
            .collect();
        let details: Vec<Json> = self
            .details
            .iter()
            .filter(|d| d.since <= self.now())
            .map(|d| {
                let new = self.seen.get(d.room).is_none_or(|s| d.since > s.at);
                json!({ "id": d.id, "room": d.room, "spot": d.spot, "new": new })
            })
            .collect();
        json!({ "name": "return", "news": true, "rooms": rooms, "details": details })
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

    fn go(v: &mut Return, m: &mut Places, to: &str) {
        m.go(to);
        v.step(m, 0.0);
    }

    #[test]
    fn nothing_changes_in_front_of_you_and_the_room_says_what_did() {
        let mut m = lab();
        let mut v = Return::default();
        v.step(&mut m, 0.0);
        go(&mut v, &mut m, "canteen_block");
        // A story moment in the room you're in waits for you to leave.
        v.moved(&mut m, "pim", Some("market"), Some("work"), Some("crates"));
        v.event(&mut m, "buy_bowl", &Json::Null);
        assert_eq!(m.person("pim").unwrap().place, "canteen");
        assert_eq!(v.shown("canteen").count(), 0, "the mug isn't there yet");
        go(&mut v, &mut m, "wharf");
        assert_eq!(m.person("pim").unwrap().place, "market", "done once you'd gone");
        let c = &v.world(&m)["rooms"]["canteen"];
        assert_eq!(c["away"], 0, "just left");
        assert_eq!(c["gone"][0]["who"], "pim");
        // The mug turns up while the player is in another room.
        go(&mut v, &mut m, "office_block");
        let c = &v.world(&m)["rooms"]["canteen"];
        assert_eq!(c["away"], 1);
        assert_eq!(c["details"][0], "mug");
        go(&mut v, &mut m, "wharf");
        go(&mut v, &mut m, "canteen_block");
        assert_eq!(v.shown("canteen").count(), 1, "the mug is there now");
        assert_eq!(v.world(&m)["details"][0]["new"], true);
        // Seen once, it is part of the room.
        go(&mut v, &mut m, "wharf");
        go(&mut v, &mut m, "canteen_block");
        assert_eq!(v.world(&m)["details"][0]["new"], false);
    }

    #[test]
    fn switching_from_states_keeps_the_day() {
        let clock = day::Shared::default();
        let mut states = crate::life::states::States::new(clock.clone());
        let mut v = Return::new(clock);
        let mut m = lab();
        states.step(&mut m, 0.0);
        m.go("office_block");
        states.step(&mut m, 0.0);
        // Ilse's welcome sends Pim off while `states` waits for the player to see it...
        states.moved(&mut m, "pim", Some("market"), Some("work"), Some("crates"));
        // ...and the player switches (key 4) and goes on.
        m.go("wharf");
        v.step(&mut m, 0.0);
        m.go("wharf_arm");
        v.step(&mut m, 0.0);
        assert_eq!(v.now(), 2, "one clock, two room visits");
        let p = m.person("pim").unwrap();
        assert_eq!((p.place.as_str(), p.spot.as_str()), ("market", "crates"));
    }

    #[test]
    fn a_room_never_seen_is_not_marked() {
        let mut m = lab();
        let mut v = Return::default();
        v.step(&mut m, 0.0);
        v.event(&mut m, "sell", &json!({ "ware": "ice" }));
        for _ in 0..4 {
            go(&mut v, &mut m, "office_block");
            go(&mut v, &mut m, "wharf");
        }
        assert!(v.world(&m)["rooms"].get("market").is_none());
    }
}
