//! Session recordings and replay.
//!
//! A recording holds everything needed to run a session again and get the same world:
//!
//! - `build`: the engine that made it (version, and the commit and build name when it is
//!   an installed build), so the launcher can run that exact build again;
//! - `modules`: the exact module files the session started with;
//! - `seed` and `start`: the world it started from, whole;
//! - `events`, in order: inputs (tick, source, name, payload), hot-loads (tick, module and
//!   its exact new files), notes (tick, text, the world's hash then, and from a played
//!   session the camera and the frame that was shown) and zooms (the view height the
//!   player's wheel set, on the tick it was turned; presentation only, so a replay of the
//!   world ignores them and a film of the session uses them);
//! - `end`: the last tick and the world's hash there.
//!
//! [`replay`] runs it again from `start` with the recorded files, never the module folders,
//! and compares the end hash. The sim reads no clock and no OS state, so a replay on any
//! OS gives the same hash.
//!
//! The file is one JSON document. JSON keeps it readable for people and agents; module
//! files are text today, and binary assets will need a different encoding when they come.
//! It is laid out a record a line ([`Recording::to_text`]): the build, seed and end first,
//! then each event on its own line, then the module files and the start world, so a
//! search for `"type":"note"` finds every note. Recordings written compact, on one line,
//! read the same.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_module::module::{Module, ModuleFiles};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::{Input, Session};
use crate::world::World;

/// The recording format this engine writes and reads.
pub const FORMAT: u32 = 1;

/// Which engine build made a recording.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildStamp {
    pub version: String,
    /// The commit, for an installed build.
    #[serde(default)]
    pub commit: Option<String>,
    /// The installed build's folder name (`0.2.0-<commit>`), which the launcher runs.
    #[serde(default)]
    pub name: Option<String>,
}

impl BuildStamp {
    /// This engine.
    pub fn current() -> BuildStamp {
        let installed = galaxy_module::install::current();
        BuildStamp {
            version: galaxy_module::version::engine().to_string(),
            commit: installed.as_ref().map(|(_, b)| b.info.commit.clone()),
            name: installed.map(|(_, b)| b.name),
        }
    }

    /// Its installed name, or its version when it was not an installed build.
    pub fn label(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("{} (not an installed build)", self.version),
        }
    }

    /// Whether `self` and `other` are the same build: none when either was not an
    /// installed build, so it can't be told.
    pub fn same_as(&self, other: &BuildStamp) -> Option<bool> {
        match (&self.name, &other.name) {
            (Some(a), Some(b)) => Some(a == b),
            _ if self.version != other.version => Some(false),
            _ => None,
        }
    }
}

/// Which build recorded a session and which one is working on it now (`act`: `drew`,
/// `replayed`), with what that means and, when they differ, how to use the recording's
/// (`command` is the command line that would, without `--build`):
/// `{recorded, drawn, same, note}` (the second key named after `act`).
pub fn builds(recorded: &BuildStamp, this: &BuildStamp, act: &str, command: &str) -> Value {
    let (r, t) = (recorded.label(), this.label());
    let same = recorded.same_as(this);
    let key = match act {
        "drew" => "drawn",
        "replayed" => "replayed",
        other => other,
    };
    let note = match same {
        Some(true) => format!(
            "{t} {act} it, the build that recorded the session: this is what the player saw"
        ),
        Some(false) => {
            let run = match &recorded.name {
                // galaxy-gm films with another build itself.
                Some(_) if act == "drew" => format!(
                    "{command} --as-recorded (galaxy-gm runs the recording's build to draw it; \
                     --builds recorded,this films both and compares them)"
                ),
                Some(name) => {
                    let (tool, rest) = command.split_once(' ').unwrap_or((command, ""));
                    format!(
                        "{tool} --build {name} {rest} (or leave out --build: the launcher runs \
                         the recording's build when it is installed; galaxy doctor lists them)"
                    )
                }
                None => "the recording's build was not an installed build, so it can't be \
                         run again by name"
                    .to_owned(),
            };
            format!(
                "DIFFERENT BUILDS: recorded by {r}, {key} by {t}. A defect fixed or added \
                 between them shows differently, so this may not look like what the player \
                 saw. To see what the player saw: {run}"
            )
        }
        None => format!(
            "can't tell whether {t} is the build that recorded it ({r}): one of them was not \
             an installed build"
        ),
    };
    serde_json::json!({ "recorded": r, key: t, "same": same, "note": note })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecEvent {
    /// An input, run on its tick.
    Input(Input),
    /// A module's new files, hot-loaded just before `tick` ran.
    HotLoad {
        tick: u64,
        module: String,
        files: ModuleFiles,
    },
    /// A note on the world as it was after `tick`.
    Note {
        tick: u64,
        text: String,
        /// The world's hash when the note was taken.
        #[serde(default)]
        hash: Option<String>,
        /// The camera when the note was taken (with the renderer; none headless).
        #[serde(default)]
        camera: Option<Value>,
        /// The frame that was shown (with the renderer; none headless).
        #[serde(default)]
        frame: Option<Value>,
    },
    /// The player zoomed the view to `view` world units top to bottom, after `tick`.
    /// The world never sees it.
    Zoom { tick: u64, view: f32 },
    /// Engine widget state after this tick; ignored by world replay.
    Presentation { tick: u64, state: Value },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
    pub tick: u64,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub format: u32,
    pub build: BuildStamp,
    pub seed: u64,
    /// Each module's files at the start, by module ID.
    pub modules: BTreeMap<String, ModuleFiles>,
    pub start: World,
    pub events: Vec<RecEvent>,
    pub end: End,
}

impl Recording {
    pub(crate) fn start<'a>(
        build: BuildStamp,
        world: &World,
        modules: impl Iterator<Item = &'a Module>,
    ) -> Recording {
        Recording {
            format: FORMAT,
            build,
            seed: world.seed,
            modules: modules
                .map(|m| (m.id().to_owned(), m.files.clone()))
                .collect(),
            start: world.clone(),
            events: Vec::new(),
            end: End {
                tick: world.tick,
                hash: world.hash(),
            },
        }
    }

    pub(crate) fn ended(&self, world: &World) -> Recording {
        Recording {
            end: End {
                tick: world.tick,
                hash: world.hash(),
            },
            ..self.clone()
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        crate::save::write_atomically(path, self.to_text()?.as_bytes())
    }

    /// The file's text: one JSON document laid out for reading and searching. The small
    /// fields come first, then one event a line, then the module files and the start
    /// world, each on a line of its own.
    pub fn to_text(&self) -> Result<String, String> {
        let j = |v: &dyn erased::Json| v.json();
        let mut s = format!(
            "{{
\"format\": {},
\"build\": {},
\"seed\": {},
\"end\": {},
\"events\": [",
            self.format,
            j(&self.build)?,
            self.seed,
            j(&self.end)?
        );
        for (i, e) in self.events.iter().enumerate() {
            s.push_str(if i == 0 {
                "
"
            } else {
                ",
"
            });
            s.push_str(&j(e)?);
        }
        s.push_str(if self.events.is_empty() {
            "],
"
        } else {
            "
],
"
        });
        s.push_str(&format!(
            "\"modules\": {},
\"start\": {}
}}
",
            j(&self.modules)?,
            j(&self.start)?
        ));
        Ok(s)
    }

    pub fn read(path: &Path) -> Result<Recording, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let raw: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a recording: {e}", path.display()))?;
        let format = raw["format"].as_u64();
        if format != Some(u64::from(FORMAT)) {
            return Err(format!(
                "{} is recording format {}, and this engine reads format {FORMAT}; replay it \
                 with the build that made it",
                path.display(),
                format.map_or_else(|| "unknown".to_owned(), |f| f.to_string())
            ));
        }
        serde_json::from_value(raw)
            .map_err(|e| format!("{} is not a valid recording: {e}", path.display()))
    }
}

/// The world's hash when a note was taken, in a replay.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoteAt {
    pub tick: u64,
    pub text: String,
    pub hash: String,
}

/// What a replay found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Replayed {
    pub tick: u64,
    pub hash: String,
    pub recorded_hash: String,
    pub identical: bool,
    pub notes: Vec<NoteAt>,
}

/// A recording being run again, one tick at a time, with its own module files.
pub struct Replay<'a> {
    pub session: Session,
    rec: &'a Recording,
    inputs: BTreeMap<u64, Vec<Input>>,
    loads: BTreeMap<u64, Vec<(&'a String, &'a ModuleFiles)>>,
}

impl<'a> Replay<'a> {
    /// The recording's start, ready to step.
    pub fn new(rec: &'a Recording) -> Result<Replay<'a>, String> {
        let modules = rec
            .modules
            .iter()
            .map(|(id, files)| module_from(id, files))
            .collect::<Result<Vec<_>, _>>()?;
        let session = Session::resume(modules, rec.start.clone())?;
        let mut inputs: BTreeMap<u64, Vec<Input>> = BTreeMap::new();
        let mut loads: BTreeMap<u64, Vec<(&String, &ModuleFiles)>> = BTreeMap::new();
        for event in &rec.events {
            match event {
                RecEvent::Input(input) => inputs.entry(input.tick).or_default().push(input.clone()),
                RecEvent::HotLoad {
                    tick,
                    module,
                    files,
                } => loads.entry(*tick).or_default().push((module, files)),
                RecEvent::Note { .. } | RecEvent::Zoom { .. } | RecEvent::Presentation { .. } => {}
            }
        }
        Ok(Replay {
            session,
            rec,
            inputs,
            loads,
        })
    }

    /// The inputs recorded for `tick`.
    pub fn inputs_at(&self, tick: u64) -> &[Input] {
        self.inputs.get(&tick).map_or(&[][..], Vec::as_slice)
    }

    /// The tick the recording ends on.
    pub fn end(&self) -> u64 {
        self.rec.end.tick
    }

    /// Runs the next tick, with its hot-loads and inputs. False once at the end.
    pub fn step(&mut self) -> Result<bool, String> {
        if self.session.world.tick >= self.rec.end.tick {
            return Ok(false);
        }
        let tick = self.session.world.tick + 1;
        for (id, files) in self.loads.get(&tick).into_iter().flatten() {
            self.session
                .hot_load(module_from(id, files)?)
                .map_err(|e| format!("the hot-load of `{id}` before tick {tick} failed: {e}"))?;
        }
        self.session
            .step(self.inputs.get(&tick).map_or(&[][..], Vec::as_slice))?;
        Ok(true)
    }
}

/// Runs a recording again from its start, with its own module files, and compares the end.
pub fn replay(rec: &Recording) -> Result<(Session, Replayed), String> {
    let mut notes: BTreeMap<u64, Vec<&String>> = BTreeMap::new();
    for event in &rec.events {
        if let RecEvent::Note { tick, text, .. } = event {
            notes.entry(*tick).or_default().push(text);
        }
    }
    let mut run = Replay::new(rec)?;
    let mut seen = Vec::new();
    let mut note = |session: &Session| {
        for text in notes.get(&session.world.tick).into_iter().flatten() {
            seen.push(NoteAt {
                tick: session.world.tick,
                text: (*text).clone(),
                hash: session.world.hash(),
            });
        }
    };
    note(&run.session);
    while run.step()? {
        note(&run.session);
    }
    let session = run.session;
    let hash = session.world.hash();
    let replayed = Replayed {
        tick: session.world.tick,
        identical: hash == rec.end.hash,
        hash,
        recorded_hash: rec.end.hash.clone(),
        notes: seen,
    };
    Ok((session, replayed))
}

fn module_from(id: &str, files: &ModuleFiles) -> Result<Module, String> {
    Module::from_files(&Path::new("recording").join(id), files.clone())
        .map_err(|f| format!("the recorded module `{id}`: {}", f.message))
}

/// One JSON line of anything serializable, behind a trait object.
mod erased {
    pub trait Json {
        fn json(&self) -> Result<String, String>;
    }

    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> Result<String, String> {
            serde_json::to_string(self).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(name: Option<&str>) -> BuildStamp {
        BuildStamp {
            version: "0.2.0".into(),
            commit: None,
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn builds_say_whether_this_is_what_the_player_saw() {
        let b = builds(
            &stamp(Some("a")),
            &stamp(Some("a")),
            "drew",
            "galaxy-gm film <rec>",
        );
        assert_eq!(b["same"], true);
        assert_eq!(b["drawn"], "a");
        let b = builds(
            &stamp(Some("a")),
            &stamp(Some("b")),
            "drew",
            "galaxy-gm film <rec>",
        );
        assert_eq!(b["same"], false);
        let note = b["note"].as_str().unwrap();
        assert!(
            note.contains("may not look like what the player saw"),
            "{note}"
        );
        assert!(
            note.contains("galaxy-gm film <rec> --as-recorded"),
            "{note}"
        );
        assert!(note.contains("recorded by a, drawn by b"), "{note}");
        let b = builds(
            &stamp(None),
            &stamp(Some("b")),
            "replayed",
            "galaxy sim replay <rec>",
        );
        assert_eq!(b["same"], Value::Null);
        assert!(b.get("replayed").is_some());
    }
}
