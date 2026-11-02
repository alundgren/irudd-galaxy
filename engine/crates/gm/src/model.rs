//! `galaxy-gm model <module> [<model>]`: what a model is, without reading its glTF: its
//! parts (glTF nodes) with their sizes and places, its outline on the plane as collision
//! sees it, its morph target, which parts glow or carry a bite, and its markers (named
//! nodes without a mesh, such as a room's `camera` and `spot.<name>`); or, without a model,
//! every model the module has in a line each.

use std::path::Path;

use galaxy_api::engine::MODEL;
use galaxy_mesh::gltf::{PartInfo, PartRange, read_nodes, read_parts};
use galaxy_module::cli::Output;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover};
use galaxy_sim::shape::{DIRECTIONS, Profile};
use serde_json::{Map, Value, json};

/// The options `model` takes.
pub const OPTIONS: &[&str] = &[];

/// The options `model make` takes.
pub const MAKE_OPTIONS: &[&str] = &["check"];

/// `galaxy-gm model make <module> [<recipe>] [--check]`: makes `models/<name>.gltf` from
/// each `recipes/<name>.toml` (or one), all or nothing: a bad recipe is refused, naming
/// its field, and nothing is written. With `check`, writes nothing and refuses when a
/// model is not what its recipe makes.
pub fn make(dir: &Path, only: Option<&str>, check: bool) -> Result<Output, Failure> {
    let module = Module::read(dir)?;
    let files = &module.files.0;
    let mut names = galaxy_recipe::names(files);
    if names.is_empty() {
        return Err(Failure::bad_input(format!(
            "{} has no recipes (recipes/<name>.toml makes models/<name>.gltf); \
             galaxy-gm guide models shows one",
            module.id()
        ))
        .with("path", dir.display().to_string()));
    }
    if let Some(one) = only {
        let one = one
            .strip_prefix(&format!("{}.", module.id()))
            .unwrap_or(one);
        if !names.iter().any(|n| n == one) {
            return Err(Failure::bad_input(format!(
                "{} has no recipe `{one}`; it has {}",
                module.id(),
                names.join(", ")
            ))
            .with("path", dir.display().to_string()));
        }
        names = vec![one.to_owned()];
    }
    let mut made = Vec::new();
    for name in &names {
        let m = galaxy_recipe::make(files, name).map_err(|p| {
            Failure::bad_input(format!("refused: {p}"))
                .with("path", dir.display().to_string())
                .with("recipe", p.file.clone())
                .with("field", p.field.clone())
        })?;
        made.push(m);
    }
    let mut rows = Vec::new();
    let mut stale = Vec::new();
    for m in &made {
        let path = dir.join(&m.file);
        let old = std::fs::read_to_string(&path)
            .ok()
            .map(|t| t.replace("\r\n", "\n"));
        let status = match (&old, check) {
            (Some(t), _) if *t == m.text => "unchanged",
            (None, true) => "missing",
            (Some(_), true) => "stale",
            (_, false) => "written",
        };
        if matches!(status, "missing" | "stale") {
            stale.push(m.file.clone());
        }
        let (lo, hi) = m.bounds;
        rows.push(json!({
            "recipe": m.recipe,
            "model": format!("{}.{}", module.id(), m.name),
            "file": m.file,
            "status": status,
            "parts": m.parts,
            "markers": m.markers,
            "triangles": m.triangles,
            "size": place(lo.to_array(), hi.to_array()),
            "sha256": m.sha256,
        }));
    }
    if !check {
        for m in &made {
            let path = dir.join(&m.file);
            let same =
                std::fs::read_to_string(&path).is_ok_and(|t| t.replace("\r\n", "\n") == m.text);
            if same {
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    Failure::runtime(format!("can't make {}: {e}", parent.display()))
                })?;
            }
            std::fs::write(&path, &m.text)
                .map_err(|e| Failure::runtime(format!("can't write {}: {e}", path.display())))?;
        }
    }
    let out = json!({
        "ok": stale.is_empty(),
        "command": "model make",
        "module": module.id(),
        "made": rows,
        "units": "size x by y by z at (centre), world units at scale 1: x the model's front, y its left, z up",
        "next": format!(
            "galaxy-gm model {} <model> for one model's parts and markers; galaxy shot --modules <folder> or galaxy play to see it",
            dir.display()
        ),
    });
    if stale.is_empty() {
        return Ok(Output::Json(out));
    }
    let mut failure = Failure::new(
        galaxy_module::failure::code::REJECTED,
        "findings",
        format!(
            "{} model(s) are not what their recipes make: {}; run galaxy-gm model make without --check",
            stale.len(),
            stale.join(", ")
        ),
    );
    if let Value::Object(fields) = out {
        failure.fields = fields;
    }
    Err(failure)
}

/// Two decimals, for sizes and places.
fn r2(v: f32) -> f64 {
    (f64::from(v) * 100.0).round() / 100.0
}

fn size(p: &PartInfo) -> [f32; 3] {
    std::array::from_fn(|k| p.max[k] - p.min[k])
}

/// `1.20 x 0.80 x 0.40 at (0.10, 0.00, 0.20)`: a box's size and centre.
fn place(min: [f32; 3], max: [f32; 3]) -> String {
    let s: [f32; 3] = std::array::from_fn(|k| max[k] - min[k]);
    let c: [f32; 3] = std::array::from_fn(|k| (max[k] + min[k]) / 2.0);
    format!(
        "{:.2} x {:.2} x {:.2} at ({:.2}, {:.2}, {:.2})",
        s[0], s[1], s[2], c[0], c[1], c[2]
    )
}

pub fn run(dir: &Path, name: Option<&str>) -> Result<Output, Failure> {
    let module = Module::read(dir)?;
    let bad = |e: String| Failure::bad_input(e).with("path", dir.display().to_string());
    let models: Vec<(&String, &String, String)> = module
        .files
        .0
        .iter()
        .filter_map(|(path, text)| {
            let short = path.strip_prefix("models/")?.strip_suffix(".gltf")?;
            Some((path, text, format!("{}.{short}", module.id())))
        })
        .collect();
    let worn = worn_by(dir, &module);
    let Some(want) = name else {
        let mut list = Map::new();
        for (path, text, id) in &models {
            let line = match read_parts(text).and_then(|r| {
                let m = markers(text, &r.1)?;
                Ok((r, m))
            }) {
                Ok(((mesh, ranges), marks)) => {
                    let parts = PartInfo::of(&mesh, &ranges);
                    let all = whole(&parts);
                    format!(
                        "{path}: {} part(s), {} marker(s), {} triangles, {}; worn by {}",
                        parts.len(),
                        marks.len(),
                        mesh.triangle_count(),
                        place(all.0, all.1),
                        worn.get(id).map_or("nothing".to_owned(), |w| w.join(", "))
                    )
                }
                Err(e) => format!("{path}: does not read: {e}"),
            };
            list.insert(id.clone(), json!(line));
        }
        return Ok(Output::Json(json!({
            "ok": true,
            "command": "model",
            "module": module.id(),
            "models": list,
            "units": "size x by y by z at (centre), world units at scale 1: x the model's front, y its left, z up",
            "next": format!("galaxy-gm model {} <model> for one model's parts, outline, morph, glow, bite and markers", dir.display()),
        })));
    };
    let full = if want.contains('.') {
        want.to_owned()
    } else {
        format!("{}.{want}", module.id())
    };
    let Some((path, text, id)) = models.iter().find(|m| m.2 == full) else {
        return Err(bad(format!(
            "{} has no model `{want}`; it has {}",
            module.id(),
            models
                .iter()
                .map(|m| m.2.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    let (mesh, ranges) = read_parts(text).map_err(|e| bad(format!("{path} does not read: {e}")))?;
    let marks = markers(text, &ranges).map_err(|e| bad(format!("{path} does not read: {e}")))?;
    let parts = PartInfo::of(&mesh, &ranges);
    let all = whole(&parts);
    let profile = Profile::of(&mesh);
    let reach = |p: &[f64; DIRECTIONS]| {
        let q = DIRECTIONS / 4;
        json!({
            "most": r2(p.iter().copied().fold(0.0, f64::max) as f32),
            "least": r2(p.iter().copied().fold(f64::MAX, f64::min) as f32),
            "front": r2(p[0] as f32),
            "left": r2(p[q] as f32),
            "back": r2(p[2 * q] as f32),
            "right": r2(p[3 * q] as f32),
        })
    };
    let lines: Vec<String> = parts
        .iter()
        .map(|p| {
            let mut s = format!(
                "{}: {} triangles, {}",
                p.name,
                p.triangles,
                place(p.min, p.max)
            );
            if p.glow > 0.0 {
                s.push_str(&format!(", glow {:.2}", p.glow));
            }
            if p.bite > 0.0 {
                s.push_str(&format!(", bite {:.2}", p.bite));
            }
            if p.morph > 1e-4 {
                s.push_str(&format!(", morph moves it up to {:.2}", p.morph));
            }
            if size(p).iter().any(|v| *v < 0.05) {
                s.push_str(", thin (under 0.05 one way)");
            }
            s
        })
        .collect();
    let named = |f: &dyn Fn(&PartInfo) -> f32| -> Vec<String> {
        parts
            .iter()
            .filter(|p| f(p) > 1e-4)
            .map(|p| format!("{} {:.2}", p.name, f(p)))
            .collect()
    };
    let morphs = parts.iter().any(|p| p.morph > 1e-4);
    Ok(Output::Json(json!({
        "ok": true,
        "command": "model",
        "model": id,
        "file": path,
        "units": "world units at scale 1 (a thing's engine.model scale multiplies them): x the model's front (where its thing heads), y its left, z up",
        "size": place(all.0, all.1),
        "triangles": mesh.triangle_count(),
        "radius": r2(mesh.radius()),
        "parts": lines,
        "outline": {
            "reach": reach(&profile.base),
            "about": "collision's footprint on the plane: how far the model's convex outline reaches from its centre (front, left, back, right, most, least). A thing whose engine.body has a radius above 0 collides as that circle instead",
        },
        "morph": if morphs {
            json!({
                "parts": named(&|p: &PartInfo| p.morph),
                "outline_at_1": reach(&profile.morphed),
                "about": "the shape at engine.model morph 1 (each part: how far its vertices move at most)",
            })
        } else {
            json!("none: the model has no morph target")
        },
        "glow": named(&|p: &PartInfo| p.glow),
        "bite": named(&|p: &PartInfo| p.bite),
        "markers": marks,
        "markers_about": "named nodes without a mesh, by name: at (its place), looks (the way it faces; a camera looks along it) and up, in the model's frame like the sizes. A room's camera is its `camera` node (or the one engine.room camera names); engine.spot `name` stands at node `spot.<name>`",
        "worn_by": worn.get(id.as_str()).cloned().unwrap_or_default(),
        "next": "galaxy-gm motion flicker <film> names these parts under changed pixels (events.parts); galaxy shot --crop <thing> draws one thing close up",
    })))
}

/// The model's markers: named nodes that are not parts, with where they are, which way
/// they face and their up, as rooms and spots read them.
fn markers(text: &str, parts: &[PartRange]) -> Result<Map<String, Value>, String> {
    let v = |p: [f32; 3]| json!(p.map(r2));
    Ok(read_nodes(text)?
        .into_iter()
        .filter(|(name, _)| !parts.iter().any(|p| p.name == *name))
        .map(|(name, n)| {
            let m = json!({
                "at": v(n.position.to_array()),
                "looks": v(n.forward.to_array()),
                "up": v(n.up.to_array()),
            });
            (name, m)
        })
        .collect())
}

/// The box round every part.
fn whole(parts: &[PartInfo]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in parts {
        for k in 0..3 {
            lo[k] = lo[k].min(p.min[k]);
            hi[k] = hi[k].max(p.max[k]);
        }
    }
    (lo, hi)
}

/// Which things wear each model in the start states of the module and those beside it,
/// with their scale when it isn't 1.
fn worn_by(dir: &Path, module: &Module) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut all = vec![module.clone()];
    if let Some(parent) = dir.parent() {
        for other in discover::modules_in(parent) {
            if other != dir
                && let Ok(m) = Module::read(&other)
                && m.id() != module.id()
            {
                all.push(m);
            }
        }
    }
    let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for m in &all {
        for (entity, components) in &m.start {
            let Some(fields) = components.get(MODEL) else {
                continue;
            };
            let Some(model) = fields.get("model").and_then(Value::as_str) else {
                continue;
            };
            let scale = fields.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
            out.entry(model.to_owned())
                .or_default()
                .push(if scale == 1.0 {
                    entity.clone()
                } else {
                    format!("{entity} (scale {scale})")
                });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drift() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/drift")
    }

    fn json(out: Output) -> Value {
        match out {
            Output::Json(v) => v,
            _ => panic!("model prints JSON"),
        }
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let path = entry.unwrap().path();
            let dest = to.join(path.file_name().unwrap());
            if path.is_dir() {
                copy_dir(&path, &dest);
            } else {
                std::fs::copy(&path, &dest).unwrap();
            }
        }
    }

    #[test]
    fn make_writes_models_checks_them_and_refuses_a_bad_recipe_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("yard");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/yard"),
            &dir,
        );
        let statuses = |v: &Value| -> Vec<String> {
            v["made"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["status"].as_str().unwrap().to_owned())
                .collect()
        };
        // The committed models are what the recipes make.
        let out = json(make(&dir, None, true).unwrap());
        assert!(statuses(&out).iter().all(|s| s == "unchanged"), "{out}");

        // A changed recipe makes its model stale; making writes it, and only it.
        let rock = dir.join("recipes/rock.toml");
        let text = std::fs::read_to_string(&rock)
            .unwrap()
            .replace("seed = 11", "seed = 12");
        std::fs::write(&rock, text).unwrap();
        let Err(stale) = make(&dir, None, true) else {
            panic!("a stale model passes --check");
        };
        assert_eq!((stale.code, stale.kind), (5, "findings"));
        assert!(
            stale.message.contains("models/rock.gltf"),
            "{}",
            stale.message
        );
        let out = json(make(&dir, Some("rock"), false).unwrap());
        assert_eq!(statuses(&out), ["written"]);
        // The pale rock uses the rock, so it is stale too until it is made.
        let Err(stale) = make(&dir, None, true) else {
            panic!("a recipe using a changed one passes --check");
        };
        assert!(
            stale.message.contains("models/rock_pale.gltf"),
            "{}",
            stale.message
        );
        let out = json(make(&dir, None, false).unwrap());
        assert_eq!(
            statuses(&out),
            [
                "unchanged",
                "unchanged",
                "written",
                "unchanged",
                "unchanged"
            ]
        );
        assert!(
            json(make(&dir, None, true).unwrap())["ok"]
                .as_bool()
                .unwrap()
        );
        let made = json(run(&dir, Some("rock")).unwrap());
        assert!(made["morph"]["parts"].is_array(), "{made}");

        // A bad recipe is refused with its field, and nothing is written.
        std::fs::remove_file(dir.join("models/outpost.gltf")).unwrap();
        let skiff = dir.join("recipes/skiff.toml");
        let text = std::fs::read_to_string(&skiff)
            .unwrap()
            .replace("span = 1.8", "span = -1.8");
        std::fs::write(&skiff, text).unwrap();
        let Err(bad) = make(&dir, None, false) else {
            panic!("a bad recipe is made");
        };
        assert_eq!((bad.code, bad.kind), (4, "bad_input"));
        assert_eq!(bad.fields["field"], "part[1].span");
        assert_eq!(bad.fields["recipe"], "recipes/skiff.toml");
        assert!(
            !dir.join("models/outpost.gltf").exists(),
            "nothing is written"
        );
        let Err(none) = make(&dir, Some("nope"), false) else {
            panic!("an unknown recipe is made");
        };
        assert!(none.message.contains("outpost"), "{}", none.message);
    }

    #[test]
    fn a_generated_model_reports_its_camera_and_spot_markers() {
        // The yard's outpost is made from recipes/outpost.toml (make --check pins it).
        let yard = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/yard");
        let all = json(run(&yard, None).unwrap());
        let line = all["models"]["yard.outpost"].as_str().unwrap();
        assert!(line.contains(", 2 marker(s), "), "{line}");
        let out = json(run(&yard, Some("outpost")).unwrap());
        let markers = out["markers"].as_object().unwrap();
        let names: Vec<&str> = markers.keys().map(String::as_str).collect();
        assert_eq!(names, ["camera", "spot.dock"], "{out}");
        let num = |m: &str, k: &str, i: usize| out["markers"][m][k][i].as_f64().unwrap();
        // The camera stands where the recipe puts it and looks back and down.
        assert_eq!([0, 1, 2].map(|i| num("camera", "at", i)), [22.0, 0.0, 14.0]);
        assert!(num("camera", "looks", 0) < -0.8 && num("camera", "looks", 2) < -0.4);
        assert!(num("camera", "up", 2) > 0.8);
        // The dock spot is 11.5 out at 90 degrees, facing the centre.
        assert_eq!(num("spot.dock", "at", 1), 11.5);
        assert!(num("spot.dock", "looks", 1) < -0.99, "{out}");
        // Markers are not parts.
        let parts = out["parts"].as_array().unwrap();
        assert!(
            !parts
                .iter()
                .any(|p| p.as_str().unwrap().starts_with("camera:")),
            "{out}"
        );
    }

    #[test]
    fn a_model_lists_its_parts_outline_and_glow() {
        let all = json(run(&drift(), None).unwrap());
        let line = all["models"]["drift.pad"].as_str().unwrap();
        assert!(line.contains("worn by drift.berth"), "{line}");
        let hub = json(run(&drift(), Some("hub")).unwrap());
        let parts: Vec<&str> = hub["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        assert!(parts.iter().any(|p| p.starts_with("towers: ")), "{hub}");
        assert!(hub["outline"]["reach"]["most"].as_f64().unwrap() > 1.0);
        assert!(!hub["glow"].as_array().unwrap().is_empty(), "{hub}");
        let rock = json(run(&drift(), Some("drift.rock")).unwrap());
        assert!(rock["morph"]["parts"].is_array(), "{rock}");
        let Err(err) = run(&drift(), Some("nope")) else {
            panic!("a model it does not have is an error");
        };
        assert!(err.message.contains("drift.hub"), "{}", err.message);
    }
}
