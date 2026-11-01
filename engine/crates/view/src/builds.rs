//! Films drawn by another installed build: the recording's (`--as-recorded`) or any two
//! (`--builds a,b`), from the current build, which runs the other's executable to draw and
//! then brings the film up to its own format.
//!
//! Older builds lack newer commands and options, and always will. So the other build is
//! asked only for what every build since films began takes (`film` with `--from`, `--to`,
//! `--note`, `--before`, `--after`, `--fps`, `--size`, `--out` and `--probes-only`), and
//! what it doesn't record (which build drew it, the film's changes and orders, things' reach and
//! outlines) is added afterwards from a probes-only film of the same span made by this
//! build: the world is the same on both, so its probes are too. The pictures and the
//! camera stay the other build's own.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_module::Failure;
use galaxy_module::failure::code;
use galaxy_module::install::{self, Build};
use galaxy_motion::film::{FORMAT, Film, Manifest};
use galaxy_sim::BuildStamp;
use serde_json::{Value, json};

/// Who draws a film: this build, or another installed one.
#[derive(Clone, Debug)]
pub enum Drawer {
    This,
    Other(Build),
}

impl Drawer {
    /// The build's name (its label when this one is not an installed build).
    pub fn name(&self) -> String {
        match self {
            Drawer::This => BuildStamp::current().label(),
            Drawer::Other(b) => b.name.clone(),
        }
    }
}

/// The stamp of an installed build, as a recording keeps it.
pub fn stamp(build: &Build) -> BuildStamp {
    BuildStamp {
        version: build.info.version.clone(),
        commit: Some(build.info.commit.clone()),
        name: Some(build.name.clone()),
    }
}

/// The installed builds, newest first: in this build's install, else the configured one.
fn installed() -> Vec<Build> {
    let root = install::current().map_or_else(install::root, |(root, _)| root);
    install::list(&root)
}

/// The build `word` names: `this`, `recorded` (the build that recorded the session), an
/// installed build's name, or a part of one only one build has (such as its commit).
pub fn resolve(word: &str, recorded: Option<&BuildStamp>) -> Result<Drawer, Failure> {
    let this = BuildStamp::current();
    let name = match word.trim() {
        "this" | "current" => return Ok(Drawer::This),
        "recorded" => match recorded {
            Some(r) => r.name.clone().ok_or_else(|| {
                Failure::bad_input(
                    "the recording's build was not an installed build, so it can't draw again; \
                     film it with this build (leave out --as-recorded)",
                )
            })?,
            None => {
                return Err(Failure::usage(
                    "`recorded` is the build that recorded a played session; a scripted film has \
                     none: name builds (galaxy doctor lists them)",
                ));
            }
        },
        w => w.to_owned(),
    };
    if this.name.as_deref() == Some(name.as_str()) {
        return Ok(Drawer::This);
    }
    let builds = installed();
    let names = || builds.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let found = match builds.iter().find(|b| b.name == name) {
        Some(b) => b.clone(),
        None => {
            let matches: Vec<&Build> = builds.iter().filter(|b| b.name.contains(&name)).collect();
            match matches.as_slice() {
                [one] => (*one).clone(),
                [] => {
                    return Err(Failure::new(
                        code::CANNOT_START,
                        "not_installed",
                        format!("build {name} is not installed"),
                    )
                    .with("builds", names()));
                }
                many => {
                    return Err(Failure::usage(format!(
                        "`{name}` could be {}; give more of the name",
                        many.iter()
                            .map(|b| b.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )));
                }
            }
        }
    };
    if this.name.as_deref() == Some(found.name.as_str()) {
        return Ok(Drawer::This);
    }
    Ok(Drawer::Other(found))
}

/// Runs `tool` (`galaxy` or `galaxy-gm`) of `build` with `args`: the JSON it printed, or
/// its failure, kept whole under `its_output`.
pub fn run(build: &Build, tool: &str, args: &[String]) -> Result<Value, Failure> {
    let exe = build.dir.join(install::exe(tool));
    let out = Command::new(&exe).args(args).output().map_err(|e| {
        Failure::runtime(format!("can't start {}: {e}", exe.display()))
            .with("path", exe.display().to_string())
    })?;
    let text = String::from_utf8_lossy(&out.stdout);
    let printed: Value = serde_json::from_str(text.trim()).unwrap_or_else(|_| {
        json!({
            "stdout": text.trim(),
            "stderr": String::from_utf8_lossy(&out.stderr).trim(),
        })
    });
    if out.status.success() {
        return Ok(printed);
    }
    let reason = printed["error"].as_str().unwrap_or("it gave no reason");
    let message = format!(
        "build {} could not film (`{tool} {}`): {reason}",
        build.name,
        args.join(" ")
    );
    let failure = match printed["kind"].as_str() {
        Some("replay_diverged") => Failure::new(code::REJECTED, "replay_diverged", message),
        Some("usage") => Failure::runtime(format!(
            "{message}. Older builds lack newer commands and options; this build asks them only \
             for what every build since films began takes, so this one may predate films"
        )),
        _ => Failure::runtime(message),
    };
    Err(failure
        .with("build", build.name.clone())
        .with("its_output", printed))
}

/// Brings the film in `dir`, drawn by `drawn`, up to this engine's format: `builds` says
/// which build drew it, and what the other build didn't record is taken from `probes`, a
/// probes-only film of the same span by this build (changes, orders, outlines, things' reach and
/// morph). Returns what it added, and whether the two worlds were the same at every frame.
pub fn adopt(
    dir: &Path,
    builds: Value,
    probes: Option<&Manifest>,
) -> Result<(Vec<String>, Option<bool>), String> {
    let mut m = Film::read(dir)?.manifest;
    let mut added = Vec::new();
    if m.format < FORMAT {
        added.push(format!("film format {} to {FORMAT}", m.format));
        m.format = FORMAT;
    }
    if m.builds.is_null() {
        added.push("builds".to_owned());
    }
    m.builds = builds;
    let mut same_world = None;
    if let Some(p) = probes {
        if m.changes.is_empty() && !p.changes.is_empty() {
            m.changes = p.changes.clone();
            added.push("changes".to_owned());
        }
        if m.orders.is_empty() && !p.orders.is_empty() {
            m.orders = p.orders.clone();
            added.push("orders".to_owned());
        }
        if m.outlines.is_empty() && !p.outlines.is_empty() {
            m.outlines = p.outlines.clone();
            added.push("outlines".to_owned());
        }
        if m.parts.is_empty() && !p.parts.is_empty() {
            m.parts = p.parts.clone();
            added.push("parts".to_owned());
        }
        let mut reach = false;
        let mut same = true;
        for (f, q) in m.frames.iter_mut().zip(&p.frames) {
            if f.tick != q.tick {
                same = false;
                break;
            }
            same &= f.hash == q.hash;
            for (id, t) in &mut f.things {
                let Some(o) = q.things.get(id) else { continue };
                if t.reach.is_none() && o.reach.is_some() {
                    t.reach = o.reach;
                    reach = true;
                }
                if t.morph.is_none() {
                    t.morph = o.morph;
                }
            }
        }
        if reach {
            added.push("reach".to_owned());
        }
        same_world = Some(same && m.frames.len() == p.frames.len());
    }
    m.write(dir)?;
    Ok((added, same_world))
}

/// A folder for this build's probes-only film beside another build's, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        Scratch(std::env::temp_dir().join(format!("galaxy-film-{name}-{}", std::process::id())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The film folder for `drawer` when several builds film into `out`: `<out>/<build>`.
pub fn folder_for(out: &Path, drawer: &Drawer) -> PathBuf {
    out.join(drawer.name().replace(' ', "-").replace(['(', ')'], ""))
}

/// Films the same span with each of `drawers` (`film` films one into a folder) and, when
/// they have pictures, compares the second with the first: one report.
pub fn film_on_each(
    drawers: &[Drawer],
    out: &Path,
    drawn: bool,
    film: &mut dyn FnMut(&Drawer, &Path) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let mut films = Vec::new();
    for d in drawers {
        let dir = folder_for(out, d);
        let report = film(d, &dir)?;
        films.push((d.name(), dir, report));
    }
    let mut compare = Value::Null;
    let mut behaviour = Value::Null;
    if films.len() == 2 {
        let read = |dir: &Path| {
            galaxy_motion::Film::read(dir)
                .map_err(|e| Failure::runtime(e).with("path", dir.display().to_string()))
        };
        let (a, b) = (read(&films[0].1)?, read(&films[1].1)?);
        behaviour = galaxy_motion::compare::behaviour([&a, &b], [&films[0].0, &films[1].0])
            .unwrap_or(Value::Null);
    }
    if drawn && films.len() == 2 {
        let opts =
            galaxy_motion::Opts::parse(&["--with".to_owned(), films[1].1.display().to_string()])
                .map_err(Failure::usage)?;
        let full = galaxy_motion::run("compare", &films[0].1, &opts)
            .map_err(|e| Failure::runtime(e).with("path", films[0].1.display().to_string()))?;
        compare = json!({
            "reading": full["reading"],
            "visibly_different_frames": full["flip"]["visibly_different_frames"],
            "image": full["image"],
            "detail": format!(
                "galaxy-gm motion compare {} {} for the worst frames and boxes",
                films[0].1.display(),
                films[1].1.display()
            ),
        });
    }
    let names: Vec<String> = films.iter().map(|f| f.0.clone()).collect();
    let next = format!(
        "any tool on each film, to compare its numbers: galaxy-gm motion <tool> {} and the same \
         on {}",
        films[0].1.display(),
        films
            .get(1)
            .map_or(String::new(), |f| f.1.display().to_string())
    );
    Ok(json!({
        "ok": true,
        "command": "film --builds",
        "builds": names,
        "films": films.iter().map(|(name, dir, report)| json!({
            "build": name,
            "out": dir.display().to_string(),
            "frames": report["frames"],
            "added": report.get("added"),
            "builds": report["builds"]["note"],
        })).collect::<Vec<_>>(),
        "behaviour": if behaviour.is_null() {
            json!("the films follow no thing in common; run track --thing <x> on each")
        } else {
            behaviour
        },
        "compare": if compare.is_null() {
            json!("probes-only films have no pictures to compare; run track or dots on each")
        } else {
            compare
        },
        "next": next,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_name_this_build_or_say_what_is_wrong() {
        assert!(matches!(resolve("this", None), Ok(Drawer::This)));
        assert_eq!(resolve("recorded", None).unwrap_err().kind, "usage");
        let unnamed = BuildStamp {
            version: "0.2.0".into(),
            commit: None,
            name: None,
        };
        assert_eq!(
            resolve("recorded", Some(&unnamed)).unwrap_err().kind,
            "bad_input"
        );
        assert_eq!(
            resolve("no-such-build-anywhere", None).unwrap_err().kind,
            "not_installed"
        );
    }

    #[test]
    fn an_older_film_is_brought_up_to_this_format() {
        use galaxy_motion::film::{Cam, Frame, Thing};
        let dir = tempfile::tempdir().unwrap();
        let cam = Cam {
            eye: [0.0; 3],
            target: [0.0; 3],
            view_proj: [
                1.0, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            per_pixel: 1.0,
        };
        let thing = |reach| Thing {
            x: 0.0,
            y: 0.0,
            height: 0.0,
            angle: 0.0,
            screen: None,
            on_screen: false,
            followed: true,
            reach,
            morph: None,
        };
        let frame = |reach| Frame {
            index: 0,
            file: None,
            time: 0.0,
            tick: 3,
            alpha: 0.0,
            hash: "h".into(),
            camera: cam,
            zoom: None,
            things: [("m.ship".to_owned(), thing(reach))].into(),
            lines: Vec::new(),
        };
        let manifest = |reach| Manifest {
            format: 1,
            fps: 60,
            ticks_per_second: 30,
            width: 64,
            height: 64,
            scale: 1.0,
            drawn: false,
            adapter: None,
            source: Value::Null,
            builds: Value::Null,
            changes: Vec::new(),
            orders: Default::default(),
            frames: vec![frame(reach)],
            outlines: Default::default(),
            parts: Default::default(),
        };
        // An older build wrote format 1: every frame inside film.json, no builds.
        let old = manifest(None);
        std::fs::write(
            dir.path().join("film.json"),
            serde_json::to_string(&old).unwrap(),
        )
        .unwrap();
        let mut ours = manifest(Some(2.5));
        ours.format = FORMAT;
        let (added, same) = adopt(dir.path(), json!({"drawn": "old"}), Some(&ours)).unwrap();
        assert_eq!(added, ["film format 1 to 2", "builds", "reach"]);
        assert_eq!(same, Some(true));
        let film = Film::read(dir.path()).unwrap();
        assert_eq!(film.manifest.format, FORMAT);
        assert_eq!(film.builds()["drawn"], "old");
        assert_eq!(film.frames()[0].things["m.ship"].reach, Some(2.5));
    }
}
