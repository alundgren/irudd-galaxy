//! Scripted runs: the inputs file `galaxy sim run --inputs` takes, and scenario tests, a
//! scripted run plus the outcomes it must reach.
//!
//! An inputs file (TOML):
//!
//! ```toml
//! [[input]]
//! tick = 10                 # runs on tick 10
//! name = "example.pressed"  # a declared event kind
//! source = "keyboard"       # optional; "script" by default
//! payload = { door = "example.front" }
//!
//! [[note]]
//! tick = 90                 # a note on the world after tick 90, in the recording
//! text = "the door should be open now"
//!
//! [[hot_load]]
//! tick = 150                # loaded just before tick 150 runs
//! from = "changed/example"  # a module folder, relative to this file
//!
//! [[zoom]]
//! tick = 200                # the wheel, turned once tick 200 has run
//! steps = 2.0               # positive is closer; presentation only, not a sim input
//! ```
//!
//! A scenario test is a module's `tests/<name>.toml`: a `seed`, a number of `ticks`, the
//! same `[[input]]` entries (a plain name is the module's own event kind), and
//! `[[expect]]` entries checked after their `tick` (the last one by default):
//!
//! ```toml
//! [[expect]]
//! entity = "front"          # this module's entity, or a full ID
//! component = "door"
//! field = "pushes"
//! at_least = 2              # or equals / at_most; or `absent = true` without a field
//!
//! [[expect]]
//! facts = "opened"          # how many facts of this kind were recorded
//! equals = 1
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use galaxy_api::command::Fields;
use galaxy_module::ids;
use galaxy_module::manifest::toml_to_json;
use galaxy_module::module::Module;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::{Input, Session};

/// Every key an inputs file takes: its entry, the key, an example as TOML and what it does.
/// `galaxy-gm fields inputs` prints it; a test reads a file made of every example.
pub const INPUTS_KEYS: &[(&str, &str, &str, &str)] = &[
    (
        "[[input]]",
        "tick",
        "10",
        "the tick it runs on; ticks start at 1, 30 a second",
    ),
    (
        "[[input]]",
        "name",
        "\"example.opened\"",
        "a declared event kind, as a full ID (`<module>.<kind>`)",
    ),
    (
        "[[input]]",
        "source",
        "\"keyboard\"",
        "where it came from; \"script\" by default",
    ),
    (
        "[[input]]",
        "payload",
        "{ down = true }",
        "the event's fields; a key's event has down = true when pressed, false when let go",
    ),
    (
        "[[note]]",
        "tick",
        "90",
        "a note on the world after this tick, kept in the recording like an F8 note",
    ),
    (
        "[[note]]",
        "text",
        "\"the door should be open\"",
        "the note's text",
    ),
    (
        "[[hot_load]]",
        "tick",
        "150",
        "a module is hot-loaded just before this tick",
    ),
    (
        "[[hot_load]]",
        "from",
        "\"changed/example\"",
        "the module folder, relative to the inputs file",
    ),
    (
        "[[zoom]]",
        "tick",
        "200",
        "the wheel turns once this tick has run (view only; the sim never sees it)",
    ),
    (
        "[[zoom]]",
        "steps",
        "2.0",
        "wheel steps; positive is closer",
    ),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInput {
    tick: u64,
    name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    payload: Option<toml::Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNote {
    tick: u64,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLoad {
    tick: u64,
    from: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawZoom {
    tick: u64,
    steps: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInputs {
    #[serde(default)]
    input: Vec<RawInput>,
    #[serde(default)]
    note: Vec<RawNote>,
    #[serde(default)]
    hot_load: Vec<RawLoad>,
    #[serde(default)]
    zoom: Vec<RawZoom>,
}

fn input(raw: RawInput, qualify: impl Fn(&str) -> Option<String>) -> Result<Input, String> {
    let name =
        qualify(&raw.name).ok_or_else(|| format!("input `{}`: not an event kind", raw.name))?;
    let payload = match raw.payload {
        None => Fields::new(),
        Some(table) => match toml_to_json(&toml::Value::Table(table))? {
            Value::Object(map) => map,
            _ => unreachable!("a table is an object"),
        },
    };
    if raw.tick == 0 {
        return Err(format!("input `{name}`: ticks start at 1"));
    }
    Ok(Input {
        tick: raw.tick,
        source: raw.source.unwrap_or_else(|| "script".into()),
        name,
        payload,
    })
}

/// An inputs file, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputsFile {
    pub inputs: Vec<Input>,
    pub notes: Vec<(u64, String)>,
    /// Module folders to hot-load, by tick.
    pub hot_loads: Vec<(u64, PathBuf)>,
    /// Wheel steps by tick: how the view zooms, which the sim never sees.
    pub zooms: Vec<(u64, f32)>,
}

impl InputsFile {
    pub fn read(path: &Path) -> Result<InputsFile, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let raw: RawInputs =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new(""));
        let full = |name: &str| ids::is_full_id(name).then(|| name.to_owned());
        Ok(InputsFile {
            inputs: raw
                .input
                .into_iter()
                .map(|i| input(i, full))
                .collect::<Result<_, _>>()
                .map_err(|e| {
                    format!(
                        "{}: {e} (write full IDs, such as example.pressed)",
                        path.display()
                    )
                })?,
            notes: raw.note.into_iter().map(|n| (n.tick, n.text)).collect(),
            hot_loads: raw
                .hot_load
                .into_iter()
                .map(|l| (l.tick, base.join(l.from)))
                .collect(),
            zooms: raw
                .zoom
                .into_iter()
                .map(|z| {
                    if z.steps.is_finite() {
                        Ok((z.tick, z.steps))
                    } else {
                        Err(format!(
                            "{}: zoom on tick {}: steps must be a number",
                            path.display(),
                            z.tick
                        ))
                    }
                })
                .collect::<Result<_, _>>()?,
        })
    }

    /// The inputs for each tick.
    pub fn by_tick(&self) -> BTreeMap<u64, Vec<Input>> {
        let mut out: BTreeMap<u64, Vec<Input>> = BTreeMap::new();
        for input in &self.inputs {
            out.entry(input.tick).or_default().push(input.clone());
        }
        out
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpect {
    #[serde(default)]
    tick: Option<u64>,
    #[serde(default)]
    entity: Option<String>,
    #[serde(default)]
    component: Option<String>,
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    absent: bool,
    #[serde(default)]
    facts: Option<String>,
    #[serde(default)]
    equals: Option<toml::Value>,
    #[serde(default)]
    at_least: Option<f64>,
    #[serde(default)]
    at_most: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScenario {
    #[serde(default)]
    seed: u64,
    ticks: u64,
    #[serde(default)]
    input: Vec<RawInput>,
    #[serde(default)]
    expect: Vec<RawExpect>,
}

/// How a scenario went.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScenarioReport {
    pub module: String,
    /// The test file (`tests/<name>.toml`).
    pub file: String,
    pub passed: bool,
    pub ticks: u64,
    pub hash: String,
    /// Each expectation that failed, and script failures during the run.
    pub failures: Vec<String>,
}

/// Runs every scenario in `owner`'s `tests/` with all of `modules` loaded.
pub fn run_module_tests(modules: &[Module], owner: &str) -> Result<Vec<ScenarioReport>, String> {
    let module = modules
        .iter()
        .find(|m| m.id() == owner)
        .ok_or_else(|| format!("module `{owner}` is not loaded"))?;
    module
        .files
        .0
        .iter()
        .filter(|(path, _)| path.starts_with("tests/") && path.ends_with(".toml"))
        .map(|(path, text)| run_scenario(modules, owner, path, text))
        .collect()
}

/// Runs one scenario. An error means the scenario itself is wrong; failed expectations
/// are in the report.
pub fn run_scenario(
    modules: &[Module],
    owner: &str,
    file: &str,
    text: &str,
) -> Result<ScenarioReport, String> {
    let at = |e: String| format!("{owner}: {file}: {e}");
    let raw: RawScenario = toml::from_str(text).map_err(|e| at(e.to_string()))?;
    let qualify_kind = |name: &str| ids::qualify_type(owner, name);
    let entity = |name: &str| {
        let known = |ns: &str| ns == ids::ENGINE || modules.iter().any(|m| m.id() == ns);
        match ids::split(name) {
            Some((ns, _)) if known(ns) => Some(name.to_owned()),
            _ => ids::qualify_entity(owner, name),
        }
    };
    let mut inputs: BTreeMap<u64, Vec<Input>> = BTreeMap::new();
    for raw in raw.input {
        let input = input(raw, qualify_kind).map_err(at)?;
        inputs.entry(input.tick).or_default().push(input);
    }
    let mut expects: BTreeMap<u64, Vec<RawExpect>> = BTreeMap::new();
    for e in raw.expect {
        let tick = e.tick.unwrap_or(raw.ticks);
        if tick > raw.ticks {
            return Err(at(format!(
                "an expectation at tick {tick} is after the last tick"
            )));
        }
        expects.entry(tick).or_default().push(e);
    }
    let mut session = Session::new(modules.to_vec(), raw.seed).map_err(at)?;
    for list in inputs.values() {
        for input in list {
            session.check_input(input).map_err(at)?;
        }
    }
    let mut failures = Vec::new();
    let mut check = |session: &Session, list: &[RawExpect]| -> Result<(), String> {
        for e in list {
            if let Some(problem) = expectation(session, e, &qualify_kind, &entity)? {
                failures.push(format!("after tick {}: {problem}", session.world.tick));
            }
        }
        Ok(())
    };
    check(&session, expects.get(&0).map_or(&[][..], Vec::as_slice)).map_err(at)?;
    while session.world.tick < raw.ticks {
        let tick = session.world.tick + 1;
        session
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .map_err(at)?;
        check(&session, expects.get(&tick).map_or(&[][..], Vec::as_slice)).map_err(at)?;
    }
    for fact in &session.world.facts {
        if fact.kind == galaxy_api::engine::SCRIPT_FAILED {
            failures.push(format!(
                "tick {}: a script failed: {}",
                fact.tick,
                Value::Object(fact.payload.clone())
            ));
        }
    }
    Ok(ScenarioReport {
        module: owner.to_owned(),
        file: file.to_owned(),
        passed: failures.is_empty(),
        ticks: session.world.tick,
        hash: session.world.hash(),
        failures,
    })
}

/// Checks one expectation: `None` if it holds, else what was found.
fn expectation(
    session: &Session,
    e: &RawExpect,
    kind: &dyn Fn(&str) -> Option<String>,
    entity: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>, String> {
    let value = if let Some(facts) = &e.facts {
        let kind = kind(facts).ok_or_else(|| format!("`{facts}` is not an event kind"))?;
        let n = session
            .world
            .facts
            .iter()
            .filter(|f| f.kind == kind)
            .count();
        (format!("facts `{kind}`"), Some(Value::from(n as u64)))
    } else {
        let (Some(name), Some(component)) = (&e.entity, &e.component) else {
            return Err("an expectation names `facts`, or an `entity` and a `component`".into());
        };
        let id = entity(name).ok_or_else(|| format!("`{name}` is not an entity name"))?;
        let component =
            kind(component).ok_or_else(|| format!("`{component}` is not a component type"))?;
        let found = session.world.component(&id, &component);
        if e.absent {
            return Ok(found.map(|_| format!("`{id}` still has `{component}`")));
        }
        let Some(found) = found else {
            return Ok(Some(format!("`{id}` has no `{component}`")));
        };
        match &e.field {
            None => return Ok(None),
            Some(field) => (
                format!("`{id}` `{component}.{field}`"),
                found.fields.get(field).cloned(),
            ),
        }
    };
    let (what, value) = value;
    let Some(value) = value else {
        return Ok(Some(format!("{what} doesn't exist")));
    };
    if let Some(want) = &e.equals {
        let want = toml_to_json(want)?;
        let same = match (want.as_f64(), value.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => want == value,
        };
        if !same {
            return Ok(Some(format!("{what} is {value}, expected {want}")));
        }
    }
    if let Some(min) = e.at_least
        && value.as_f64().is_none_or(|v| v < min)
    {
        return Ok(Some(format!("{what} is {value}, expected at least {min}")));
    }
    if let Some(max) = e.at_most
        && value.as_f64().is_none_or(|v| v > max)
    {
        return Ok(Some(format!("{what} is {value}, expected at most {max}")));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inputs_file_of_every_documented_key_reads() {
        let mut text = String::new();
        let mut last = "";
        for (entry, key, example, _) in INPUTS_KEYS {
            if *entry != last {
                text.push_str(&format!("{entry}\n"));
                last = entry;
            }
            text.push_str(&format!("{key} = {example}\n"));
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inputs.toml");
        std::fs::write(&path, &text).unwrap();
        let read = InputsFile::read(&path).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(
            (
                read.inputs.len(),
                read.notes.len(),
                read.hot_loads.len(),
                read.zooms.len()
            ),
            (1, 1, 1, 1)
        );
    }
}
