//! `galaxy-gm model <module> [<model>]`: what a model is, without reading its glTF: its
//! parts (glTF nodes) with their sizes and places, its outline on the plane as collision
//! sees it, its morph target, and which parts glow or carry a bite; or, without a model,
//! every model the module has in a line each.

use std::path::Path;

use galaxy_api::engine::MODEL;
use galaxy_mesh::gltf::{PartInfo, read_parts};
use galaxy_module::cli::Output;
use galaxy_module::module::Module;
use galaxy_module::{Failure, discover};
use galaxy_sim::shape::{DIRECTIONS, Profile};
use serde_json::{Map, Value, json};

/// The options `model` takes.
pub const OPTIONS: &[&str] = &[];

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
            let line = match read_parts(text) {
                Ok((mesh, ranges)) => {
                    let parts = PartInfo::of(&mesh, &ranges);
                    let all = whole(&parts);
                    format!(
                        "{path}: {} part(s), {} triangles, {}; worn by {}",
                        parts.len(),
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
            "next": format!("galaxy-gm model {} <model> for one model's parts, outline, morph, glow and bite", dir.display()),
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
        "worn_by": worn.get(id.as_str()).cloned().unwrap_or_default(),
        "next": "galaxy-gm motion flicker <film> names these parts under changed pixels (events.parts); galaxy shot --crop <thing> draws one thing close up",
    })))
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
