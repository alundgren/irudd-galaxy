//! `check`: motion expectations that must keep holding. A spec file says what to film and
//! what each tool must report about it; `galaxy-gm motion check <spec>` films it, runs the
//! tools and fails when a value is out of bounds.
//!
//! ```toml
//! modules = "../drift"      # a module folder or a folder of modules, from this file
//! inputs = "route.toml"     # optional, from this file
//! seed = 1                  # optional
//! from = 4                  # ticks to film
//! to = 34
//! fps = 60                  # optional
//! size = "960x540"          # optional
//! probes_only = false       # optional: true films without a GPU (track and dots only)
//!
//! [[expect]]
//! tool = "strip"            # the tool to run
//! value = "drift_px_per_frame"   # a field of its report; nested with dots (gap.overshoot)
//! at_least = -0.1           # bounds: at_least, at_most, or equals
//! at_most = 0.1
//! along = "course"          # any other key is an option for the tool
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Opts;

/// What to film.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub modules: PathBuf,
    pub inputs: Option<PathBuf>,
    pub seed: u64,
    pub from: u64,
    pub to: u64,
    pub fps: u32,
    pub size: (u32, u32),
    pub probes_only: bool,
    pub expects: Vec<Expect>,
}

/// One expectation.
#[derive(Clone, Debug, PartialEq)]
pub struct Expect {
    pub tool: String,
    pub value: String,
    pub at_least: Option<f64>,
    pub at_most: Option<f64>,
    pub equals: Option<Value>,
    pub opts: Opts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpec {
    modules: PathBuf,
    #[serde(default)]
    inputs: Option<PathBuf>,
    #[serde(default)]
    seed: Option<u64>,
    from: u64,
    to: u64,
    #[serde(default)]
    fps: Option<u32>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    probes_only: bool,
    #[serde(default)]
    expect: Vec<toml::Table>,
}

impl Spec {
    pub fn read(path: &Path) -> Result<Spec, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let raw: RawSpec = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new(""));
        let size = match raw.size {
            None => (960, 540),
            Some(s) => s
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .filter(|&(w, h): &(u32, u32)| (64..=8192).contains(&w) && (64..=8192).contains(&h))
                .ok_or_else(|| {
                    format!(
                        "{}: size is width x height, such as 960x540, not `{s}`",
                        path.display()
                    )
                })?,
        };
        let mut expects = Vec::new();
        for (n, mut table) in raw.expect.into_iter().enumerate() {
            let at = |m: &str| format!("{}: expect {}: {m}", path.display(), n + 1);
            let mut text = |k: &str| -> Result<Option<String>, String> {
                match table.remove(k) {
                    None => Ok(None),
                    Some(toml::Value::String(s)) => Ok(Some(s)),
                    Some(other) => Err(at(&format!("{k} must be text, not {other}"))),
                }
            };
            let tool = text("tool")?.ok_or_else(|| at("needs a tool"))?;
            let value = text("value")?
                .ok_or_else(|| at("needs the value to check, such as drift_px_per_frame"))?;
            let mut number = |k: &str| -> Result<Option<f64>, String> {
                match table.remove(k) {
                    None => Ok(None),
                    Some(toml::Value::Integer(i)) => Ok(Some(i as f64)),
                    Some(toml::Value::Float(f)) => Ok(Some(f)),
                    Some(other) => Err(at(&format!("{k} must be a number, not {other}"))),
                }
            };
            let at_least = number("at_least")?;
            let at_most = number("at_most")?;
            let equals = table.remove("equals").map(|v| toml_json(&v));
            if at_least.is_none() && at_most.is_none() && equals.is_none() {
                return Err(at("needs at_least, at_most or equals"));
            }
            let mut opts = Opts::new();
            for (k, v) in table {
                let v = match v {
                    toml::Value::String(s) => s,
                    toml::Value::Boolean(b) => b.to_string(),
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    toml::Value::Array(a) => a
                        .iter()
                        .map(|x| match x {
                            toml::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                    other => return Err(at(&format!("option {k} can't be {other}"))),
                };
                // A second film is named from the spec's folder.
                let v = if k == "with" {
                    base.join(v).display().to_string()
                } else {
                    v
                };
                opts.set(&k, v);
            }
            expects.push(Expect {
                tool,
                value,
                at_least,
                at_most,
                equals,
                opts,
            });
        }
        if expects.is_empty() {
            return Err(format!("{}: no [[expect]] entries", path.display()));
        }
        Ok(Spec {
            modules: base.join(raw.modules),
            inputs: raw.inputs.map(|p| base.join(p)),
            seed: raw.seed.unwrap_or(1),
            from: raw.from,
            to: raw.to,
            fps: raw.fps.unwrap_or(60),
            size,
            probes_only: raw.probes_only,
            expects,
        })
    }
}

fn toml_json(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => json!(s),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => json!(b),
        toml::Value::Array(a) => Value::Array(a.iter().map(toml_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.iter().map(|(k, v)| (k.clone(), toml_json(v))).collect())
        }
        toml::Value::Datetime(d) => json!(d.to_string()),
    }
}

/// The value at a dotted path in a report (`gap.overshoot`, `events.0.kind`).
pub fn lookup<'a>(report: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(report, |v, key| match key.parse::<usize>() {
            Ok(i) if v.is_array() => v.get(i),
            _ => v.get(key),
        })
}

/// Runs every expectation on the film in `dir`. Returns one line each and whether all
/// held.
pub fn evaluate(spec: &Spec, dir: &Path) -> (Vec<Value>, bool) {
    let mut lines = Vec::new();
    let mut all = true;
    for e in &spec.expects {
        let mut opts = e.opts.clone();
        if opts.text("image").is_none() {
            opts.set(
                "image",
                dir.join("motion")
                    .join(format!("{}-{}.png", e.tool, lines.len() + 1))
                    .display()
                    .to_string(),
            );
        }
        let mut line = json!({ "tool": e.tool, "value": e.value });
        if let Some(v) = e.at_least {
            line["at_least"] = json!(v);
        }
        if let Some(v) = e.at_most {
            line["at_most"] = json!(v);
        }
        if let Some(v) = &e.equals {
            line["equals"] = v.clone();
        }
        match crate::run(&e.tool, dir, &opts) {
            Err(err) => {
                line["pass"] = json!(false);
                line["error"] = json!(err);
            }
            Ok(report) => {
                let got = lookup(&report, &e.value).cloned();
                let pass = match &got {
                    None => false,
                    Some(g) => {
                        let n = g.as_f64();
                        e.at_least.is_none_or(|lo| n.is_some_and(|n| n >= lo))
                            && e.at_most.is_none_or(|hi| n.is_some_and(|n| n <= hi))
                            && e.equals.as_ref().is_none_or(|eq| {
                                eq == g || (eq.as_f64().is_some() && eq.as_f64() == n)
                            })
                    }
                };
                line["got"] = got
                    .unwrap_or_else(|| json!(format!("no `{}` in the {} report", e.value, e.tool)));
                line["pass"] = json!(pass);
                line["reading"] = report["reading"].clone();
                line["image"] = report["image"].clone();
            }
        }
        all &= line["pass"] == json!(true);
        lines.push(line);
    }
    (lines, all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_reads_with_tool_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("course.toml");
        std::fs::write(
            &path,
            "modules = \"m\"\nfrom = 4\nto = 34\n[[expect]]\ntool = \"strip\"\nvalue = \"drift_px_per_frame\"\nat_most = 0.1\nalong = \"course\"\n",
        )
        .unwrap();
        let spec = Spec::read(&path).unwrap();
        assert_eq!(spec.modules, dir.path().join("m"));
        assert_eq!(spec.expects[0].opts.text("along"), Some("course"));
        assert_eq!(spec.expects[0].at_most, Some(0.1));
        std::fs::write(
            &path,
            "modules = \"m\"\nfrom = 4\nto = 34\n[[expect]]\ntool = \"strip\"\nvalue = \"x\"\n",
        )
        .unwrap();
        assert!(
            Spec::read(&path)
                .unwrap_err()
                .contains("at_least, at_most or equals")
        );
    }

    #[test]
    fn values_are_found_by_dotted_paths() {
        let r = json!({ "gap": { "overshoot": 0.5 }, "events": [{ "kind": "pop" }] });
        assert_eq!(lookup(&r, "gap.overshoot"), Some(&json!(0.5)));
        assert_eq!(lookup(&r, "events.0.kind"), Some(&json!("pop")));
        assert_eq!(lookup(&r, "nope"), None);
    }
}
