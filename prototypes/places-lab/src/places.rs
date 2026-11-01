//! The place model every variant shares: where the player is, the places that nest
//! (station > module > interior), and the people in them. The lab's Luau declares the
//! station's places and people (`lab/places.luau`); this is the game state, and the view
//! follows it.

use render::glam::Vec2;
use serde_json::{Value as Json, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Station,
    Module,
    Interior,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "station" => Kind::Station,
            "module" => Kind::Module,
            "interior" => Kind::Interior,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Station => "station",
            Kind::Module => "module",
            Kind::Interior => "interior",
        }
    }
}

/// A place: a station, a module on it, or a room in a module.
#[derive(Clone, Debug)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub parent: Option<String>,
    /// Places you can go to from here besides the parent and the children.
    pub connects: Vec<String>,
    /// Going to this place lands you in this child (a module's main room).
    pub enter: Option<String>,
    /// Where you arrive on the floor, place-local metres (x right, y away from the
    /// front, z up).
    pub arrive: Vec2,
    /// The floor's size in metres (interiors).
    pub floor: Vec2,
    /// Named floor positions: where people stand or sit, the door.
    pub spots: Vec<(String, Vec2)>,
    /// A module's look on the station: the kit's module kind, its bearing round the core
    /// (degrees counter-clockwise from east), and whether it is being built.
    pub art: Option<String>,
    pub bearing: f32,
    pub building: bool,
    /// The set an interior is staged in (`sets.rs`).
    pub set: Option<String>,
    /// One line about the place, for signs.
    pub line: String,
}

impl Place {
    pub fn spot(&self, name: &str) -> Option<Vec2> {
        self.spots.iter().find(|(n, _)| n == name).map(|(_, p)| *p)
    }
}

/// A person: a being in a place, doing something.
#[derive(Clone, Debug)]
pub struct Person {
    pub id: String,
    pub name: String,
    /// What they do, shown beside the name.
    pub role: String,
    pub place: String,
    /// At work, eating, away... Content names the states.
    pub state: String,
    /// The floor spot in their place they stand or sit at.
    pub spot: String,
    /// Which figure they are drawn as (`figures.rs`).
    pub look: String,
    /// The interior they belong to: their work.
    pub home: String,
}

#[derive(Clone, Debug)]
pub struct Places {
    pub places: Vec<Place>,
    pub people: Vec<Person>,
    /// Where the player is.
    pub here: String,
    /// Where the player was before the last move.
    pub was: Option<String>,
    /// Bumped on every change, so views know to look again.
    pub version: u64,
}

fn vec2(v: &Json) -> Option<Vec2> {
    let a = v.as_array()?;
    Some(Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))
}

fn text(v: &Json, k: &str) -> Option<String> {
    v.get(k).and_then(Json::as_str).map(str::to_string)
}

impl Places {
    /// Reads the declaration `lab/places.luau` returns.
    pub fn read(decl: &Json) -> Result<Places, String> {
        let mut places = Vec::new();
        for p in decl
            .get("places")
            .and_then(Json::as_array)
            .ok_or("places.luau: no `places` list")?
        {
            let id = text(p, "id").ok_or("a place without an id")?;
            let kind = text(p, "kind")
                .as_deref()
                .and_then(Kind::parse)
                .ok_or(format!("place {id}: kind is station, module or interior"))?;
            let mut spots: Vec<(String, Vec2)> = p
                .get("spots")
                .and_then(Json::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), vec2(v)?)))
                        .collect()
                })
                .unwrap_or_default();
            spots.sort_by(|a, b| a.0.cmp(&b.0));
            places.push(Place {
                name: text(p, "name").unwrap_or_else(|| id.clone()),
                kind,
                parent: text(p, "parent"),
                connects: p
                    .get("connects")
                    .and_then(Json::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                enter: text(p, "enter"),
                arrive: p.get("arrive").and_then(vec2).unwrap_or(Vec2::ZERO),
                floor: p
                    .get("floor")
                    .and_then(vec2)
                    .unwrap_or(Vec2::new(10.0, 7.0)),
                spots,
                art: text(p, "art"),
                bearing: p.get("bearing").and_then(Json::as_f64).unwrap_or(0.0) as f32,
                building: p.get("building").and_then(Json::as_bool).unwrap_or(false),
                set: text(p, "set"),
                line: text(p, "line").unwrap_or_default(),
                id,
            });
        }
        let mut people = Vec::new();
        for p in decl
            .get("people")
            .and_then(Json::as_array)
            .ok_or("places.luau: no `people` list")?
        {
            let id = text(p, "id").ok_or("a person without an id")?;
            let place = text(p, "place").ok_or(format!("{id} has no place"))?;
            people.push(Person {
                name: text(p, "name").unwrap_or_else(|| id.clone()),
                role: text(p, "role").unwrap_or_default(),
                state: text(p, "state").unwrap_or_else(|| "work".into()),
                spot: text(p, "spot").unwrap_or_default(),
                look: text(p, "look").unwrap_or_else(|| id.clone()),
                home: text(p, "home").unwrap_or_else(|| place.clone()),
                place,
                id,
            });
        }
        let start = text(decl, "start")
            .or_else(|| places.first().map(|p| p.id.clone()))
            .ok_or("places.luau: no places")?;
        let model = Places {
            places,
            people,
            here: start,
            was: None,
            version: 0,
        };
        model.check()?;
        Ok(model)
    }

    /// Names that point nowhere are content mistakes: say which.
    fn check(&self) -> Result<(), String> {
        let known = |id: &str| self.place(id).is_some();
        for p in &self.places {
            for r in p.parent.iter().chain(&p.enter).chain(&p.connects) {
                if !known(r) {
                    return Err(format!("place {}: no place {r:?}", p.id));
                }
            }
            if p.kind == Kind::Interior && p.set.is_none() {
                return Err(format!("interior {} needs a `set`", p.id));
            }
        }
        for who in &self.people {
            let Some(place) = self.place(&who.place) else {
                return Err(format!("{}: no place {:?}", who.id, who.place));
            };
            if !who.spot.is_empty() && place.spot(&who.spot).is_none() {
                return Err(format!("{}: {} has no spot {:?}", who.id, place.id, who.spot));
            }
        }
        if !known(&self.here) {
            return Err(format!("start: no place {:?}", self.here));
        }
        Ok(())
    }

    /// Takes the new declaration's places, names and spots, keeping where the player
    /// and the people are when those still exist.
    pub fn reload(&mut self, mut new: Places) {
        if new.place(&self.here).is_some() {
            new.here = self.here.clone();
        }
        new.was = self.was.clone();
        let ids: Vec<String> = new.places.iter().map(|p| p.id.clone()).collect();
        for p in &mut new.people {
            if let Some(old) = self.people.iter().find(|o| o.id == p.id)
                && ids.contains(&old.place)
            {
                p.place = old.place.clone();
                p.state = old.state.clone();
                p.spot = old.spot.clone();
            }
        }
        new.version = self.version + 1;
        *self = new;
    }

    pub fn place(&self, id: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.id == id)
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.people.iter().find(|p| p.id == id)
    }

    pub fn here(&self) -> &Place {
        self.place(&self.here).expect("checked at load")
    }

    /// The station the player is on.
    pub fn station(&self) -> &Place {
        let mut p = self.here();
        while let Some(up) = p.parent.as_deref().and_then(|id| self.place(id)) {
            p = up;
        }
        p
    }

    /// The module a place is in (itself for a module), if any.
    pub fn module_of(&self, id: &str) -> Option<&Place> {
        let mut p = self.place(id)?;
        loop {
            if p.kind == Kind::Module {
                return Some(p);
            }
            p = self.place(p.parent.as_deref()?)?;
        }
    }

    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Place> + 'a {
        self.places
            .iter()
            .filter(move |p| p.parent.as_deref() == Some(id))
    }

    /// Where going to `id` lands you: down its `enter` chain.
    pub fn resolve(&self, id: &str) -> Option<String> {
        let mut p = self.place(id)?;
        for _ in 0..8 {
            match p.enter.as_deref().and_then(|e| self.place(e)) {
                Some(e) => p = e,
                None => break,
            }
        }
        Some(p.id.clone())
    }

    /// Moves the player. Returns the place they land in, or None if there is no such
    /// place or they are already there.
    pub fn go(&mut self, id: &str) -> Option<String> {
        let to = self.resolve(id)?;
        if to == self.here {
            return None;
        }
        self.was = Some(std::mem::replace(&mut self.here, to.clone()));
        self.version += 1;
        Some(to)
    }

    /// A story moment moves a person (and their state and spot).
    pub fn set_person(&mut self, id: &str, place: Option<&str>, state: Option<&str>, spot: Option<&str>) {
        let place = place.filter(|p| self.place(p).is_some()).map(str::to_string);
        let Some(who) = self.people.iter_mut().find(|p| p.id == id) else {
            return;
        };
        if let Some(p) = place {
            who.place = p;
        }
        if let Some(s) = state {
            who.state = s.to_string();
        }
        if let Some(s) = spot {
            who.spot = s.to_string();
        }
        self.version += 1;
    }

    pub fn people_in<'a>(&'a self, place: &'a str) -> impl Iterator<Item = &'a Person> + 'a {
        self.people.iter().filter(move |p| p.place == place)
    }

    /// The place's floor position of a person: their spot, else the place's arrival.
    pub fn floor_pos(&self, who: &Person) -> Vec2 {
        let place = self.place(&who.place);
        place
            .and_then(|p| p.spot(&who.spot))
            .or(place.map(|p| p.arrive))
            .unwrap_or(Vec2::ZERO)
    }

    /// Where you can go from a place: its parent, its children and its connections.
    pub fn exits(&self, id: &str) -> Vec<String> {
        let Some(p) = self.place(id) else {
            return Vec::new();
        };
        let mut out: Vec<String> = p.parent.iter().cloned().collect();
        out.extend(self.children(id).map(|c| c.id.clone()));
        out.extend(p.connects.iter().cloned());
        out.dedup();
        out
    }

    /// The model as the Luau sees it: `world.here`, `world.place`, `world.people`, ...
    pub fn snapshot(&self) -> Json {
        let place = |p: &Place| {
            json!({
                "id": p.id, "name": p.name, "kind": p.kind.name(), "parent": p.parent,
                "line": p.line, "enter": p.enter,
            })
        };
        let here = self.here();
        let people: serde_json::Map<String, Json> = self
            .people
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    json!({
                        "id": p.id, "name": p.name, "role": p.role, "place": p.place,
                        "state": p.state, "home": p.home, "here": p.place == self.here,
                    }),
                )
            })
            .collect();
        json!({
            "here": self.here,
            "was": self.was,
            "place": place(here),
            "module": self.module_of(&self.here).map(|m| m.id.clone()),
            "station": place(self.station()),
            "people": people,
            "people_here": self.people_in(&self.here).map(|p| p.id.clone()).collect::<Vec<_>>(),
            "places": self.places.iter().map(|p| (p.id.clone(), place(p))).collect::<serde_json::Map<_, _>>(),
            "modules": self.places.iter().filter(|p| p.kind == Kind::Module).map(|p| p.id.clone()).collect::<Vec<_>>(),
            "exits": self.exits(&self.here),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station() -> Places {
        Places::read(&json!({
            "start": "wharf",
            "places": [
                { "id": "wharf", "kind": "station", "name": "Wharf" },
                { "id": "block", "kind": "module", "parent": "wharf", "enter": "office" },
                { "id": "office", "kind": "interior", "parent": "block", "set": "office",
                  "spots": { "desk": [1.0, 2.0] } },
            ],
            "people": [ { "id": "ilse", "place": "office", "spot": "desk" } ],
        }))
        .unwrap()
    }

    #[test]
    fn going_to_a_module_lands_in_its_room() {
        let mut m = station();
        assert_eq!(m.go("block").as_deref(), Some("office"));
        assert_eq!(m.here, "office");
        assert_eq!(m.module_of("office").map(|p| p.id.as_str()), Some("block"));
        assert_eq!(m.station().id, "wharf");
        assert_eq!(m.go("office"), None, "already there");
        assert_eq!(m.floor_pos(m.person("ilse").unwrap()), Vec2::new(1.0, 2.0));
    }

    #[test]
    fn the_labs_station_reads_and_every_room_has_its_person() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lab");
        let mut host = crate::script::Host::new(dir);
        let m = Places::read(&host.places().unwrap()).unwrap();
        assert_eq!(m.here().kind, Kind::Station);
        for room in m.places.iter().filter(|p| p.kind == Kind::Interior) {
            assert!(m.people_in(&room.id).next().is_some(), "nobody in {}", room.id);
            assert!(room.spot("door").is_some(), "{} has no door", room.id);
        }
        for module in m.places.iter().filter(|p| p.kind == Kind::Module) {
            let to = m.resolve(&module.id).unwrap();
            assert_eq!(m.place(&to).map(|p| p.kind), Some(Kind::Interior), "{}", module.id);
        }
    }

    #[test]
    fn a_name_that_points_nowhere_is_reported() {
        let e = Places::read(&json!({
            "places": [ { "id": "wharf", "kind": "station", "enter": "nowhere" } ],
            "people": [],
        }))
        .unwrap_err();
        assert!(e.contains("nowhere"), "{e}");
    }
}
