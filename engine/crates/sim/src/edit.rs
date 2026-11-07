//! Editing the world from outside its rules: what a test, an agent or the owner uses to
//! put a world in the state they want (a field set, a component or entity added or
//! removed, an entity moved to another place) without playing there.
//!
//! One vocabulary everywhere. An edit names an entity and does exactly one thing:
//!
//! ```toml
//! [[edit]]
//! tick = 0                    # applied once this tick has run (0: before the first)
//! entity = "example.walker"
//! component = "example.pockets"
//! set = { coins = 500 }       # these fields of a component it has
//!
//! [[edit]]
//! entity = "example.lamp2"
//! component = "example.lamp"
//! put = { lit = true }        # the whole component, added or replaced (the rest defaults);
//!                             # a new entity is made by putting its first component
//!
//! [[edit]]
//! entity = "example.rock"
//! remove = true               # the whole entity; with `component`, just that component
//!
//! [[edit]]
//! entity = "example.walker"
//! place = "example.hall"      # moved there at once (`spot`, or the place's arrival)
//! spot = "example.door"
//! ```
//!
//! `galaxy-gm live edit` takes the same edits as text, one a step:
//! `set example.walker example.pockets coins=500`, `put <entity> <component> [k=v ...]`,
//! `remove <entity> [<component>]`, `place <entity> <place> [<spot>]`.
//!
//! [`crate::Session::edit`] applies a list of edits between two ticks as one batch with an
//! `edit` cause, records an `engine.edited` fact (an event on the next tick, so scripts can
//! react) and puts the edit in the recording, so a replay makes it again.

use galaxy_api::command::{Cause, Command, Fields};
use galaxy_api::engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::world::World;

/// One change to the world from outside its rules. Exactly one of `set`, `put`, `remove`
/// and `place` is given. IDs are full when the session applies it; scenario files and
/// live requests may name things more shortly, and are qualified first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub entity: String,
    /// The component `set`, `put` or `remove` works on; none for `place`, or to remove
    /// the whole entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// These fields of a component the entity has, the others as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<Fields>,
    /// The whole component, added or replaced: fields left out get their defaults. An
    /// entity that doesn't exist yet is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put: Option<Fields>,
    /// Removes the component, or the whole entity when no component is named.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove: bool,
    /// Moves a located entity to this place at once (a teleport): to `spot`, or the
    /// place's arrival spot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spot: Option<String>,
}

/// What an edit does.
pub enum Action<'a> {
    Set(&'a str, &'a Fields),
    Put(&'a str, &'a Fields),
    Remove(Option<&'a str>),
    Place(&'a str, Option<&'a str>),
}

impl Edit {
    /// What it does, or why it is not one edit.
    pub fn action(&self) -> Result<Action<'_>, String> {
        let given = [
            self.set.is_some(),
            self.put.is_some(),
            self.remove,
            self.place.is_some(),
        ]
        .iter()
        .filter(|g| **g)
        .count();
        if given != 1 {
            return Err(format!(
                "an edit of `{}` does exactly one of set, put, remove or place (it has {given})",
                self.entity
            ));
        }
        if self.spot.is_some() && self.place.is_none() {
            return Err(format!(
                "an edit of `{}` names a spot without a place",
                self.entity
            ));
        }
        let component = self.component.as_deref();
        let needs = |what: &str| {
            format!(
                "{what} on `{}` needs the component it works on",
                self.entity
            )
        };
        Ok(if let Some(fields) = &self.set {
            if fields.is_empty() {
                return Err(format!("set on `{}` names no field", self.entity));
            }
            Action::Set(component.ok_or_else(|| needs("set"))?, fields)
        } else if let Some(fields) = &self.put {
            Action::Put(component.ok_or_else(|| needs("put"))?, fields)
        } else if self.remove {
            Action::Remove(component)
        } else {
            if component.is_some() {
                return Err(format!(
                    "place moves the whole of `{}`; it takes no component",
                    self.entity
                ));
            }
            Action::Place(
                self.place.as_deref().unwrap_or_default(),
                self.spot.as_deref(),
            )
        })
    }

    /// The world command it is.
    pub fn command(&self) -> Result<Command, String> {
        let entity = self.entity.clone();
        Ok(match self.action()? {
            Action::Set(component, fields) => Command::Patch {
                entity,
                component: component.to_owned(),
                fields: fields.clone(),
            },
            Action::Put(component, fields) => Command::Put {
                entity,
                component: component.to_owned(),
                fields: fields.clone(),
            },
            Action::Remove(component) => Command::Remove {
                entity,
                component: component.map(str::to_owned),
            },
            Action::Place(place, spot) => Command::Move {
                entity,
                place: place.to_owned(),
                spot: spot.map(str::to_owned),
                state: None,
            },
        })
    }

    /// The same edit with its names made full: `entity` for entities, places and spots,
    /// `kind` for the component. A name either can't make full is an error naming it.
    pub fn qualify(
        &self,
        entity: &dyn Fn(&str) -> Result<String, String>,
        kind: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<Edit, String> {
        Ok(Edit {
            entity: entity(&self.entity)?,
            component: self.component.as_deref().map(kind).transpose()?,
            place: self.place.as_deref().map(entity).transpose()?,
            spot: self.spot.as_deref().map(entity).transpose()?,
            ..self.clone()
        })
    }

    /// The edit as `galaxy-gm live edit` writes it: `set <entity> <component> k=v ...`,
    /// `put ...`, `remove <entity> [<component>]`, `place <entity> <place> [<spot>]`.
    pub fn text(&self) -> String {
        let pairs = |fields: &Fields| {
            fields
                .iter()
                .map(|(k, v)| format!(" {k}={v}"))
                .collect::<String>()
        };
        let component = self.component.as_deref().unwrap_or("");
        match self.action() {
            Ok(Action::Set(_, f)) => format!("set {} {component}{}", self.entity, pairs(f)),
            Ok(Action::Put(_, f)) => format!("put {} {component}{}", self.entity, pairs(f)),
            Ok(Action::Remove(c)) => match c {
                Some(c) => format!("remove {} {c}", self.entity),
                None => format!("remove {}", self.entity),
            },
            Ok(Action::Place(p, s)) => match s {
                Some(s) => format!("place {} {p} {s}", self.entity),
                None => format!("place {} {p}", self.entity),
            },
            Err(_) => serde_json::to_string(self).unwrap_or_default(),
        }
    }

    /// Reads one edit written as [`Edit::text`] writes it. A value is read as TOML (`99`,
    /// `true`, `"two words"`, `[1, 2]`); a bare word that isn't TOML is text.
    pub fn parse_text(line: &str) -> Result<Edit, String> {
        let words = split_words(line)?;
        let usage = "an edit is set <entity> <component> <field>=<value>..., put <entity> \
                     <component> [<field>=<value>...], remove <entity> [<component>] or \
                     place <entity> <place> [<spot>]";
        let Some((verb, rest)) = words.split_first() else {
            return Err(usage.into());
        };
        let blank = |entity: &str| Edit {
            entity: entity.to_owned(),
            component: None,
            set: None,
            put: None,
            remove: false,
            place: None,
            spot: None,
        };
        let edit = match (verb.as_str(), rest) {
            ("set" | "put", [entity, component, pairs @ ..]) => {
                let fields = read_pairs(pairs)?;
                let mut e = blank(entity);
                e.component = Some(component.clone());
                if verb == "set" {
                    e.set = Some(fields);
                } else {
                    e.put = Some(fields);
                }
                e
            }
            ("remove", [entity]) => Edit {
                remove: true,
                ..blank(entity)
            },
            ("remove", [entity, component]) => Edit {
                remove: true,
                component: Some(component.clone()),
                ..blank(entity)
            },
            ("place", [entity, place]) => Edit {
                place: Some(place.clone()),
                ..blank(entity)
            },
            ("place", [entity, place, spot]) => Edit {
                place: Some(place.clone()),
                spot: Some(spot.clone()),
                ..blank(entity)
            },
            _ => return Err(format!("`{line}`: {usage}")),
        };
        edit.action().map_err(|e| format!("`{line}`: {e}"))?;
        Ok(edit)
    }

    /// Reads one `[[edit]]` table of a TOML file: its `tick` (0 when left out) and the
    /// edit.
    pub fn from_toml(table: &toml::Table) -> Result<(u64, Edit), String> {
        let mut table = table.clone();
        let tick = match table.remove("tick") {
            None => 0,
            Some(toml::Value::Integer(t)) if t >= 0 => t as u64,
            Some(other) => return Err(format!("an edit's tick is a tick number, not {other}")),
        };
        let json = galaxy_module::manifest::toml_to_json(&toml::Value::Table(table))?;
        let edit: Edit = serde_json::from_value(json).map_err(|e| format!("an edit: {e}"))?;
        edit.action()?;
        Ok((tick, edit))
    }
}

/// How often the world was changed from outside its rules: each edit batch and each
/// debug event (an input with source `debug`), over its whole life, saves and resumes
/// included. More than 0 marks a session as edited; it is read from the world's causes,
/// so a save keeps it with no field of its own.
pub fn edits(world: &World) -> usize {
    world
        .causes
        .iter()
        .filter(|c| match &c.cause {
            Cause::Edit { .. } => true,
            Cause::Input { source, .. } => source == engine::DEBUG_SOURCE,
            _ => false,
        })
        .count()
}

/// Engine state the engine keeps consistent itself, which an edit may not write: the
/// component, the fields of it the engine owns (none listed: all of them), whether a
/// thing that has none may be given one, and what to do instead.
pub const KEPT: &[(&str, &[&str], bool, &str)] = &[
    (
        engine::WALK,
        &[],
        false,
        "start a walk with an input or a module's debug event, or teleport with `place`",
    ),
    (
        engine::TRAVEL,
        &[],
        false,
        "go through a door with an input or a module's debug event, or teleport with `place`",
    ),
    (
        engine::LOCATION,
        &[],
        true,
        "teleport with `place`, which moves the position, location and any walk together",
    ),
    (
        engine::TALK,
        &["current", "seen", "active"],
        true,
        "start or answer a talk with an input or a module's debug event; set its other fields",
    ),
    (
        engine::LIFE,
        &[
            "current",
            "phase",
            "start_tick",
            "from_morph",
            "from_angle",
            "walk_spot",
            "departed_tick",
            "departed_place",
            "departed_x",
            "departed_y",
            "departed_height",
            "departed_angle",
            "departed_morph",
        ],
        true,
        "change its acts or advance its clock; the engine picks the current act",
    ),
];

/// Refuses an edit that would write engine-kept state ([`KEPT`]), or remove a thing in
/// the middle of a walk or a trip.
pub fn refuse_kept(entity: &str, action: &Action, world: &World) -> Result<(), String> {
    let refuse = |what: String, how: &str| {
        Err(format!(
            "{what} is kept by the engine, so an edit can't write it: {how}"
        ))
    };
    let (component, fields, put) = match action {
        Action::Set(c, f) => (*c, Some(*f), false),
        Action::Put(c, f) => (*c, Some(*f), true),
        Action::Remove(Some(c)) => (*c, None, false),
        Action::Remove(None) => {
            for (kept, why, then) in [
                (
                    engine::WALK,
                    "walking",
                    "teleport it with `place` first, then remove it",
                ),
                (
                    engine::TRAVEL,
                    "travelling",
                    "remove it once the trip has ended",
                ),
            ] {
                if world
                    .component(entity, kept)
                    .is_some_and(|c| c.fields.get("active") == Some(&Value::Bool(true)))
                {
                    return Err(format!("`{entity}` is {why}; {then}"));
                }
            }
            return Ok(());
        }
        Action::Place(..) => {
            if world
                .component(entity, engine::TRAVEL)
                .is_some_and(|c| c.fields.get("active") == Some(&Value::Bool(true)))
            {
                return Err(format!(
                    "`{entity}` is travelling, and the trip moves it when it ends: edit after \
                     the trip, or end it with an input"
                ));
            }
            return Ok(());
        }
    };
    let Some((_, owned, new_ok, how)) = KEPT.iter().find(|k| k.0 == component) else {
        return Ok(());
    };
    let has = world.component(entity, component).is_some();
    if owned.is_empty() {
        return match (put, fields) {
            (true, _) if *new_ok && !has => Ok(()),
            (_, None) if *new_ok => Ok(()),
            _ => refuse(format!("`{component}`"), how),
        };
    }
    if let Some(field) = fields
        .into_iter()
        .flat_map(|f| f.keys())
        .find(|k| owned.contains(&k.as_str()))
    {
        return refuse(format!("`{field}` of `{component}`"), how);
    }
    if put && has {
        return refuse(
            format!("part of `{component}` (put would reset it)"),
            &format!("set the fields you mean instead; {how}"),
        );
    }
    Ok(())
}

/// `k=v` words as fields.
pub fn read_pairs(pairs: &[String]) -> Result<Fields, String> {
    let mut fields = Fields::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("`{pair}` is not <field>=<value>"));
        };
        fields.insert(key.trim().to_owned(), read_value(value.trim())?);
    }
    Ok(fields)
}

/// A value as TOML, or a bare word as text.
fn read_value(text: &str) -> Result<Value, String> {
    match toml::from_str::<toml::Table>(&format!("v = {text}")) {
        Ok(mut t) => galaxy_module::manifest::toml_to_json(&t.remove("v").expect("parsed")),
        Err(_) if !text.contains(['"', '\'', '[', ']', '{', '}']) => {
            Ok(Value::String(text.to_owned()))
        }
        Err(e) => Err(format!("`{text}` is not a value: {e}")),
    }
}

/// Splits on spaces outside quotes and brackets.
fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for c in line.chars() {
        match quote {
            Some(q) => {
                word.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    word.push(c);
                }
                '[' | '{' => {
                    depth += 1;
                    word.push(c);
                }
                ']' | '}' => {
                    depth -= 1;
                    word.push(c);
                }
                c if c.is_whitespace() && depth == 0 => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                c => word.push(c),
            },
        }
    }
    if quote.is_some() || depth != 0 {
        return Err(format!("`{line}`: a quote or bracket is not closed"));
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_reads_back_as_the_same_edit() {
        for line in [
            "set a.walker a.pockets coins=99 jobs=15000",
            "set a.ship a.pilot message=\"two words\" ok=true",
            "put a.ship2 a.hull",
            "put a.ship2 a.hull size=[1,2]",
            "remove a.rock",
            "remove a.ship a.beam",
            "place a.walker a.hall",
            "place a.walker a.hall a.door",
        ] {
            let edit = Edit::parse_text(line).unwrap();
            assert_eq!(Edit::parse_text(&edit.text()).unwrap(), edit, "{line}");
        }
        let e = Edit::parse_text("set a.walker a.pockets message=hello coins=3").unwrap();
        assert_eq!(e.set.unwrap()["message"], json!("hello"));
    }

    #[test]
    fn an_edit_does_one_thing() {
        for bad in [
            "set a.ship a.pilot",
            "set a.ship",
            "remove",
            "place a.walker",
            "fly a.ship",
            "set a.walker a.pockets coins",
            "set a.ship a.pilot msg=\"open",
        ] {
            assert!(Edit::parse_text(bad).is_err(), "{bad}");
        }
        let table: toml::Table =
            toml::from_str("entity = 'a.x'\nremove = true\nset = { n = 1 }\ncomponent = 'a.c'")
                .unwrap();
        let e = Edit::from_toml(&table).unwrap_err();
        assert!(e.contains("exactly one"), "{e}");
        let table: toml::Table = toml::from_str("entity = 'a.x'\nremove = true\nwhat = 1").unwrap();
        let e = Edit::from_toml(&table).unwrap_err();
        assert!(e.contains("what"), "{e}");
        let table: toml::Table = toml::from_str("tick = 5\nentity = 'a.x'\nremove = true").unwrap();
        assert_eq!(Edit::from_toml(&table).unwrap().0, 5);
    }
}
