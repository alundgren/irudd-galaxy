//! Session recordings and replay.
//!
//! A recording holds everything needed to run a session again and get the same world:
//!
//! - `build`: the engine that made it (version, and the commit and build name when it is
//!   an installed build), so the launcher can run that exact build again;
//! - `modules`: the exact module files the session started with;
//! - `seed` and `start`: the world it started from, whole;
//! - `events`, in order: inputs (tick, source, name, payload), hot-loads (tick, module and
//!   its exact new files) and notes (tick, text, and room for the camera and the frame
//!   that was shown, which arrive with the renderer);
//! - `end`: the last tick and the world's hash there.
//!
//! [`replay`] runs it again from `start` with the recorded files, never the module folders,
//! and compares the end hash. The sim reads no clock and no OS state, so a replay on any
//! OS gives the same hash.
//!
//! The file is one JSON document. JSON keeps it readable for people and agents; module
//! files are text today, and binary assets will need a different encoding when they come.

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
        /// The camera when the note was taken (with the renderer; none headless).
        #[serde(default)]
        camera: Option<Value>,
        /// The frame that was shown (with the renderer; none headless).
        #[serde(default)]
        frame: Option<Value>,
    },
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
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::save::write_atomically(path, text.as_bytes())
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

/// Runs a recording again from its start, with its own module files, and compares the end.
pub fn replay(rec: &Recording) -> Result<(Session, Replayed), String> {
    let modules = rec
        .modules
        .iter()
        .map(|(id, files)| module_from(id, files))
        .collect::<Result<Vec<_>, _>>()?;
    let mut session = Session::resume(modules, rec.start.clone())?;
    let mut inputs: BTreeMap<u64, Vec<Input>> = BTreeMap::new();
    let mut loads: BTreeMap<u64, Vec<(&String, &ModuleFiles)>> = BTreeMap::new();
    let mut notes: BTreeMap<u64, Vec<&String>> = BTreeMap::new();
    for event in &rec.events {
        match event {
            RecEvent::Input(input) => inputs.entry(input.tick).or_default().push(input.clone()),
            RecEvent::HotLoad {
                tick,
                module,
                files,
            } => loads.entry(*tick).or_default().push((module, files)),
            RecEvent::Note { tick, text, .. } => notes.entry(*tick).or_default().push(text),
        }
    }
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
    note(&session);
    while session.world.tick < rec.end.tick {
        let tick = session.world.tick + 1;
        for (id, files) in loads.get(&tick).into_iter().flatten() {
            session
                .hot_load(module_from(id, files)?)
                .map_err(|e| format!("the hot-load of `{id}` before tick {tick} failed: {e}"))?;
        }
        session.step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))?;
        note(&session);
    }
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
