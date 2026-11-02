//! The model generator: parametric recipes in a module's data, made into glTF models.
//!
//! A recipe is `recipes/<name>.toml` in a module; it makes `models/<name>.gltf`. It lists
//! parts, each a [shape](shapes::SHAPES) with its keys (or another recipe of the module,
//! used as a preset), placed, repeated and painted from the module's palette
//! (`recipes/palette.toml`, slot = "#rrggbb"), plus markers: empty named nodes such as a
//! room's `camera` and `spot.<name>`. Each part is one glTF node, so one part of the model
//! contract (`engine/docs/renderer-and-style.md`).
//!
//! What a recipe makes (a station, a hull, a fitted part, a rock) is the module's business:
//! the shapes are generic. The same recipe gives the same bytes on every OS: the maths is
//! plain arithmetic and `libm`, random variation comes from the recipe's seed through
//! [`rng::Rng`], and the file is written in a fixed order. How it works:
//! `engine/docs/model-generator.md`.

pub mod params;
pub mod rng;
mod rock;
pub mod shapes;

use std::collections::BTreeMap;
use std::fmt;

use galaxy_mesh::gltf::{Marker, Part, write_nodes};
use galaxy_mesh::{Color, MeshBuilder, MeshData};
use galaxy_module::ids;
use glam::{Mat4, Vec3};
use sha2::{Digest, Sha256};
use toml::{Table, Value};

use crate::params::{ranged, show, triple};
use crate::rng::Rng;
use crate::shapes::{Ctx, PAINTS, Paints, rot_x, rot_y, rot_z};

/// Where recipes live in a module, and the palette file among them.
pub const RECIPES: &str = "recipes/";
pub const PALETTE_FILE: &str = "recipes/palette.toml";

/// Limits a recipe stays within.
pub const MAX_PARTS: usize = 64;
pub const MAX_MARKERS: usize = 64;
pub const MAX_COPIES: u32 = 32;
pub const MAX_TRIANGLES: usize = 100_000;
/// How deep recipes may use each other.
pub const MAX_DEPTH: usize = 4;

/// The top-level keys of a recipe: `(key, example, about)`.
pub const TOP_KEYS: &[(&str, &str, &str)] = &[
    (
        "seed",
        "7",
        "whole number 0 or more; default 0: the random variation (rock lumps, chips); a new seed is a new variant",
    ),
    (
        "about",
        "\"a small outpost\"",
        "text, optional: what the recipe makes, for people",
    ),
    (
        "palette",
        "{ main = \"#efdcb4\" }",
        "table of slot = \"#rrggbb\", optional: this recipe's colours, over the module's recipes/palette.toml; a recipe that uses this one paints it with these first",
    ),
    (
        "part",
        "[[part]]",
        "the parts, in order: each one glTF node; at least one",
    ),
    (
        "marker",
        "[[marker]]",
        "empty named nodes, optional: a room's camera and spot.<name> nodes, or any named point",
    ),
];

/// The keys every `[[part]]` takes besides its shape's: `(key, example, about)`.
pub const PART_KEYS: &[(&str, &str, &str)] = &[
    (
        "shape",
        "\"tiers\"",
        "the shape (galaxy-gm fields recipe lists them); a part has a shape or a use",
    ),
    (
        "use",
        "\"thruster\"",
        "another recipe of this module (recipes/thruster.toml), built into this one part as a preset",
    ),
    (
        "name",
        "\"core\"",
        "the glTF node's name, the part's name in galaxy-gm model; default the shape (or used recipe), with .2, .3 after the first; lowercase letters, digits and _, with . between",
    ),
    (
        "at",
        "[0.0, 0.0, 2.0]",
        "[x, y, z], each -500 to 500; default [0, 0, 0]: where the part's origin sits; x the model's front, y its left, z up",
    ),
    (
        "angle",
        "90.0",
        "degrees -720 to 720; default 0: with distance, places the part round the centre, turned to face outwards (0 is +x, 90 is +y)",
    ),
    (
        "distance",
        "8.0",
        "number 0 to 500; default 0: how far out along angle",
    ),
    (
        "turn",
        "45.0",
        "degrees -720 to 720; default 0: turns the part about its up",
    ),
    (
        "pitch",
        "10.0",
        "degrees -180 to 180; default 0: tips its front up",
    ),
    (
        "roll",
        "0.0",
        "degrees -180 to 180; default 0: rolls it about its front",
    ),
    (
        "scale",
        "1.5",
        "number 0.01 to 100, or [x, y, z]; default 1: scales the part in its own frame",
    ),
    (
        "copies",
        "4",
        "whole number 1 to 32; default 1: copies round the model's up axis",
    ),
    (
        "spread",
        "90.0",
        "degrees -360 to 360; default 360 / copies: the turn between copies",
    ),
    (
        "mirror",
        "true",
        "true or false; default false: also a copy mirrored left to right (y to -y), in the same part",
    ),
    (
        "paint",
        "\"main\"",
        "a palette slot or \"#rrggbb\"; default slot main: the main surface",
    ),
    (
        "trim",
        "\"trim\"",
        "a palette slot or \"#rrggbb\", for shapes that use it; default slot trim",
    ),
    (
        "accent",
        "\"accent\"",
        "a palette slot or \"#rrggbb\", for shapes that use it; default slot accent",
    ),
    (
        "lights",
        "\"lights\"",
        "a palette slot or \"#rrggbb\", for shapes that use it; default slot lights; fully self-lit",
    ),
    (
        "glass",
        "\"glass\"",
        "a palette slot or \"#rrggbb\", for shapes that use it; default slot glass; a little self-lit",
    ),
    (
        "glow",
        "0.0",
        "number 0 to 1; default 0: how self-lit the main paint is",
    ),
    (
        "palette",
        "{ main = \"#3fa7a0\" }",
        "with use only: slots to repaint the used recipe with",
    ),
];

/// The keys a `[[marker]]` takes: `(key, example, about)`.
pub const MARKER_KEYS: &[(&str, &str, &str)] = &[
    (
        "name",
        "\"spot.door\"",
        "required: the node's name (camera, spot.<name>, or any); not a part's name",
    ),
    (
        "at",
        "[4.0, 0.0, 1.5]",
        "[x, y, z]; default [0, 0, 0]: where it is",
    ),
    (
        "angle",
        "90.0",
        "degrees; with distance, places it round the centre",
    ),
    ("distance", "8.0", "how far out along angle"),
    (
        "turn",
        "180.0",
        "degrees; default 0: which way it faces in the plane (0 is the model's front, +x)",
    ),
    (
        "pitch",
        "-20.0",
        "degrees -89 to 89; default 0: tips it up (a camera looking down is below 0)",
    ),
];

/// Something wrong with a recipe: the file, the field (`part[2].radius`) and what.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub file: String,
    pub field: String,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "{}: {}", self.file, self.message)
        } else {
            write!(f, "{}: {} {}", self.file, self.field, self.message)
        }
    }
}

/// A model made from a recipe.
#[derive(Clone, Debug)]
pub struct Made {
    /// The recipe's name: `outpost` for `recipes/outpost.toml`.
    pub name: String,
    pub recipe: String,
    /// Where it goes in the module: `models/outpost.gltf`.
    pub file: String,
    /// The `.gltf` text.
    pub text: String,
    /// Its parts' node names, in order.
    pub parts: Vec<String>,
    pub markers: Vec<String>,
    pub triangles: usize,
    /// SHA-256 of `text`, as hex.
    pub sha256: String,
    /// The box round it, world axes: `(min, max)`.
    pub bounds: (Vec3, Vec3),
}

/// The recipe names a module has, from its files (path inside the module to text).
pub fn names(files: &BTreeMap<String, String>) -> Vec<String> {
    files
        .keys()
        .filter(|p| p.as_str() != PALETTE_FILE)
        .filter_map(|p| p.strip_prefix(RECIPES)?.strip_suffix(".toml"))
        .filter(|n| !n.contains('/'))
        .map(str::to_owned)
        .collect()
}

/// Makes the recipe `name` (`recipes/<name>.toml`) from a module's files.
pub fn make(files: &BTreeMap<String, String>, name: &str) -> Result<Made, Problem> {
    let recipe = format!("{RECIPES}{name}.toml");
    if !ids::valid_local(name) {
        return Err(Problem {
            file: recipe,
            field: String::new(),
            message: format!(
                "`{name}` is not a valid recipe name: start with a lowercase letter and use \
                 lowercase letters, digits and `_`, with `.` between parts (it names the model)"
            ),
        });
    }
    let module = match files.get(PALETTE_FILE) {
        Some(text) => {
            let table = parse(PALETTE_FILE, text)?;
            palette(PALETTE_FILE, "", &Value::Table(table))?
        }
        None => BTreeMap::new(),
    };
    let maker = Maker { files, module };
    let built = maker.recipe(name, &BTreeMap::new(), &mut Vec::new())?;
    let triangles: usize = built.parts.iter().map(|(_, m)| m.triangle_count()).sum();
    let fail = |message: String| Problem {
        file: recipe.clone(),
        field: String::new(),
        message,
    };
    if triangles > MAX_TRIANGLES {
        return Err(fail(format!(
            "makes {triangles} triangles; a model has at most {MAX_TRIANGLES} (lower sides, segments or detail)"
        )));
    }
    let parts: Vec<Part> = built
        .parts
        .iter()
        .map(|(n, m)| Part { name: n, mesh: m })
        .collect();
    let markers: Vec<Marker> = built
        .markers
        .iter()
        .map(|m| Marker {
            name: &m.name,
            position: m.position,
            forward: m.forward,
            up: m.up,
        })
        .collect();
    let text = write_nodes(&parts, &markers);
    let max = galaxy_module::module::MAX_FILE_BYTES as usize;
    if text.len() > max {
        return Err(fail(format!(
            "makes a {} byte file; module files are at most {max} (lower sides, segments or detail)",
            text.len()
        )));
    }
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for (_, m) in &built.parts {
        if let Some((a, b)) = m.bounds() {
            (lo, hi) = (lo.min(a), hi.max(b));
        }
    }
    let sha256 = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(Made {
        name: name.to_owned(),
        file: format!("models/{name}.gltf"),
        recipe,
        parts: built.parts.iter().map(|(n, _)| n.clone()).collect(),
        markers: built.markers.iter().map(|m| m.name.clone()).collect(),
        triangles,
        text,
        sha256,
        bounds: (lo, hi),
    })
}

/// A palette: slot name to colour.
type Palette = BTreeMap<String, Color>;

struct Maker<'a> {
    files: &'a BTreeMap<String, String>,
    module: Palette,
}

struct MarkerAt {
    name: String,
    position: Vec3,
    forward: Vec3,
    up: Vec3,
}

/// A recipe built: each part's node name and mesh, and its markers.
struct Built {
    parts: Vec<(String, MeshData)>,
    markers: Vec<MarkerAt>,
}

fn parse(file: &str, text: &str) -> Result<Table, Problem> {
    text.parse::<Table>().map_err(|e| Problem {
        file: file.to_owned(),
        field: String::new(),
        message: format!("is not valid TOML: {}", e.message()),
    })
}

/// Reads a palette table (`slot = "#rrggbb"`); `at` prefixes the field in messages.
fn palette(file: &str, at: &str, v: &Value) -> Result<Palette, Problem> {
    let problem = |field: String, message: String| Problem {
        file: file.to_owned(),
        field,
        message,
    };
    let Value::Table(t) = v else {
        return Err(problem(
            at.trim_end_matches('.').to_owned(),
            format!("is {}; expected a table of slot = \"#rrggbb\"", show(v)),
        ));
    };
    let mut out = Palette::new();
    for (slot, colour) in t {
        let field = format!("{at}{slot}");
        if !ids::valid_field(slot) {
            return Err(problem(field, ids::name_rule(slot)));
        }
        let c = colour
            .as_str()
            .ok_or(())
            .and_then(|s| {
                if s.len() == 7 {
                    Color::parse_hex(s).map_err(|_| ())
                } else {
                    Err(())
                }
            })
            .map_err(|_| problem(field, format!("is {}; expected \"#rrggbb\"", show(colour))))?;
        out.insert(slot.clone(), c);
    }
    Ok(out)
}

/// What a part's or marker's placement keys say: the matrices of its copies.
struct Placement {
    copies: Vec<Mat4>,
}

fn degrees(t: &Table, key: &str, lo: f32, hi: f32) -> Result<f32, (String, String)> {
    match t.get(key) {
        None => Ok(0.0),
        Some(v) => ranged(v, lo, hi).map_err(|m| (key.to_owned(), m)),
    }
}

/// The frame placement keys put a thing in, before copies: polar `angle` and `distance`
/// plus `at`, then `turn`, `pitch` and `roll`.
fn frame(t: &Table) -> Result<(Vec3, f32), (String, String)> {
    let at = match t.get("at") {
        None => Vec3::ZERO,
        Some(v) => triple(v, -500.0, 500.0).map_err(|m| ("at".to_owned(), m))?,
    };
    let angle = degrees(t, "angle", -720.0, 720.0)?.to_radians();
    let distance = match t.get("distance") {
        None => 0.0,
        Some(v) => ranged(v, 0.0, 500.0).map_err(|m| ("distance".to_owned(), m))?,
    };
    let at = at
        + Vec3::new(
            libm::cosf(angle) * distance,
            libm::sinf(angle) * distance,
            0.0,
        );
    let turn = angle + degrees(t, "turn", -720.0, 720.0)?.to_radians();
    Ok((at, turn))
}

fn placement(t: &Table) -> Result<Placement, (String, String)> {
    let (at, turn) = frame(t)?;
    let pitch = degrees(t, "pitch", -180.0, 180.0)?.to_radians();
    let roll = degrees(t, "roll", -180.0, 180.0)?.to_radians();
    let scale = match t.get("scale") {
        None => Vec3::ONE,
        Some(v @ Value::Array(_)) => triple(v, 0.01, 100.0).map_err(|m| ("scale".to_owned(), m))?,
        Some(v) => Vec3::splat(ranged(v, 0.01, 100.0).map_err(|m| ("scale".to_owned(), m))?),
    };
    let copies = match t.get("copies") {
        None => 1,
        Some(Value::Integer(n)) if (1..=i64::from(MAX_COPIES)).contains(n) => *n as u32,
        Some(v) => {
            return Err((
                "copies".into(),
                format!("is {}; expected a whole number 1 to {MAX_COPIES}", show(v)),
            ));
        }
    };
    let spread = match t.get("spread") {
        None => 360.0 / copies as f32,
        Some(v) => ranged(v, -360.0, 360.0).map_err(|m| ("spread".to_owned(), m))?,
    }
    .to_radians();
    let mirror = match t.get("mirror") {
        None => false,
        Some(Value::Boolean(b)) => *b,
        Some(v) => {
            return Err((
                "mirror".into(),
                format!("is {}; expected true or false", show(v)),
            ));
        }
    };
    // Nose up is a turn about +y by minus the pitch.
    let local = Mat4::from_translation(at)
        * rot_z(turn)
        * rot_y(-pitch)
        * rot_x(roll)
        * Mat4::from_scale(scale);
    let mut out = Vec::new();
    for k in 0..copies {
        let m = rot_z(spread * k as f32) * local;
        out.push(m);
        if mirror {
            out.push(Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * m);
        }
    }
    Ok(Placement { copies: out })
}

/// The keys a part may have besides its shape's.
fn common_keys() -> Vec<&'static str> {
    PART_KEYS.iter().map(|k| k.0).collect()
}

impl Maker<'_> {
    /// Builds recipe `name`. `over` is the palette of the recipes using this one (their
    /// slots win); `stack` the recipes being built, for cycles.
    fn recipe(
        &self,
        name: &str,
        over: &Palette,
        stack: &mut Vec<String>,
    ) -> Result<Built, Problem> {
        let file = format!("{RECIPES}{name}.toml");
        let problem = |field: String, message: String| Problem {
            file: file.clone(),
            field,
            message,
        };
        let Some(text) = self.files.get(&file) else {
            return Err(problem(String::new(), "does not exist".into()));
        };
        let table = parse(&file, text)?;
        for key in table.keys() {
            if !TOP_KEYS.iter().any(|k| k.0 == key) {
                let takes: Vec<&str> = TOP_KEYS.iter().map(|k| k.0).collect();
                return Err(problem(
                    key.clone(),
                    format!("is not a recipe key; a recipe takes {}", takes.join(", ")),
                ));
            }
        }
        let seed = match table.get("seed") {
            None => 0,
            Some(Value::Integer(n)) if *n >= 0 => *n as u64,
            Some(v) => {
                return Err(problem(
                    "seed".into(),
                    format!("is {}; expected a whole number 0 or more", show(v)),
                ));
            }
        };
        if let Some(v) = table.get("about")
            && !v.is_str()
        {
            return Err(problem(
                "about".into(),
                format!("is {}; expected text", show(v)),
            ));
        }
        // This recipe's colours: the module's, then its own, then its users'.
        let own = match table.get("palette") {
            Some(v) => palette(&file, "palette.", v)?,
            None => Palette::new(),
        };
        let mut explicit = own;
        explicit.extend(over.iter().map(|(k, v)| (k.clone(), *v)));
        let mut colours = self.module.clone();
        colours.extend(explicit.iter().map(|(k, v)| (k.clone(), *v)));

        let parts = match table.get("part") {
            Some(Value::Array(a)) if !a.is_empty() => a,
            Some(Value::Array(_)) | None => {
                return Err(problem(
                    "part".into(),
                    "is missing; a recipe needs at least one [[part]]".into(),
                ));
            }
            Some(v) => {
                return Err(problem(
                    "part".into(),
                    format!("is {}; expected [[part]] tables", show(v)),
                ));
            }
        };
        if parts.len() > MAX_PARTS {
            return Err(problem(
                "part".into(),
                format!(
                    "has {} parts; a recipe has at most {MAX_PARTS}",
                    parts.len()
                ),
            ));
        }
        stack.push(name.to_owned());
        let rng = Rng::new(seed);
        let mut out = Built {
            parts: Vec::new(),
            markers: Vec::new(),
        };
        let mut defaults: BTreeMap<String, usize> = BTreeMap::new();
        for (i, part) in parts.iter().enumerate() {
            let at = format!("part[{i}]");
            let Value::Table(t) = part else {
                return Err(problem(at, format!("is {}; expected a table", show(part))));
            };
            let field = |(k, m): (String, String)| problem(format!("{at}.{k}"), m);
            let (mesh, default_name) = self.part(
                t,
                &colours,
                &explicit,
                rng.fork(i as u64 + 1),
                stack,
                &file,
                &at,
            )?;
            let node = match t.get("name") {
                Some(Value::String(n)) => {
                    if !ids::valid_local(n) {
                        return Err(field(("name".into(), ids::local_rule(n))));
                    }
                    if out.parts.iter().any(|(p, _)| p == n) {
                        return Err(field((
                            "name".into(),
                            format!("is `{n}`, which an earlier part has; each part is one node"),
                        )));
                    }
                    n.clone()
                }
                Some(v) => {
                    return Err(field((
                        "name".into(),
                        format!("is {}; expected text", show(v)),
                    )));
                }
                None => {
                    let base = default_name.replace('-', "_");
                    let n = defaults.entry(base.clone()).or_insert(0);
                    *n += 1;
                    let mut candidate = if *n == 1 {
                        base.clone()
                    } else {
                        format!("{base}.{n}")
                    };
                    while out.parts.iter().any(|(p, _)| *p == candidate) {
                        *n += 1;
                        candidate = format!("{base}.{n}");
                    }
                    candidate
                }
            };
            if mesh.is_empty() {
                return Err(problem(at, "makes no triangles".into()));
            }
            out.parts.push((node, mesh));
        }
        stack.pop();

        if let Some(v) = table.get("marker") {
            let Value::Array(markers) = v else {
                return Err(problem(
                    "marker".into(),
                    format!("is {}; expected [[marker]] tables", show(v)),
                ));
            };
            if markers.len() > MAX_MARKERS {
                return Err(problem(
                    "marker".into(),
                    format!("has {}; a recipe has at most {MAX_MARKERS}", markers.len()),
                ));
            }
            for (i, m) in markers.iter().enumerate() {
                let at = format!("marker[{i}]");
                let Value::Table(t) = m else {
                    return Err(problem(at, format!("is {}; expected a table", show(m))));
                };
                let field = |(k, m): (String, String)| problem(format!("{at}.{k}"), m);
                for key in t.keys() {
                    if !MARKER_KEYS.iter().any(|k| k.0 == key) {
                        let takes: Vec<&str> = MARKER_KEYS.iter().map(|k| k.0).collect();
                        return Err(field((
                            key.clone(),
                            format!("is not a marker key; a marker takes {}", takes.join(", ")),
                        )));
                    }
                }
                let name = match t.get("name") {
                    Some(Value::String(n)) if ids::valid_local(n) => n.clone(),
                    Some(Value::String(n)) => {
                        return Err(field(("name".into(), ids::local_rule(n))));
                    }
                    Some(v) => {
                        return Err(field((
                            "name".into(),
                            format!("is {}; expected text", show(v)),
                        )));
                    }
                    None => {
                        return Err(field((
                            "name".into(),
                            "is missing; a marker needs a name".into(),
                        )));
                    }
                };
                if out.parts.iter().any(|(p, _)| *p == name)
                    || out.markers.iter().any(|m| m.name == name)
                {
                    return Err(field((
                        "name".into(),
                        format!("is `{name}`, which a part or an earlier marker has"),
                    )));
                }
                let (position, turn) = frame(t).map_err(field)?;
                let pitch = degrees(t, "pitch", -89.0, 89.0)
                    .map_err(field)?
                    .to_radians();
                let (sp, cp, st, ct) = (
                    libm::sinf(pitch),
                    libm::cosf(pitch),
                    libm::sinf(turn),
                    libm::cosf(turn),
                );
                out.markers.push(MarkerAt {
                    name,
                    position,
                    forward: Vec3::new(cp * ct, cp * st, sp),
                    up: Vec3::new(-sp * ct, -sp * st, cp),
                });
            }
        }
        Ok(out)
    }

    /// Builds one `[[part]]`: its mesh, and the name it takes when it has none.
    #[allow(clippy::too_many_arguments)]
    fn part(
        &self,
        t: &Table,
        colours: &Palette,
        explicit: &Palette,
        rng: Rng,
        stack: &mut Vec<String>,
        file: &str,
        at: &str,
    ) -> Result<(MeshData, String), Problem> {
        let problem = |k: &str, message: String| Problem {
            file: file.to_owned(),
            field: format!("{at}.{k}"),
            message,
        };
        let field = |(k, m): (String, String)| problem(&k, m);
        let place = placement(t).map_err(field)?;
        let mut b = MeshBuilder::new();
        match (t.get("shape"), t.get("use")) {
            (Some(_), Some(_)) => Err(problem("use", "is given with shape; a part has one or the other".into())),
            (None, None) => Err(problem(
                "shape",
                "is missing; a part has a shape (galaxy-gm fields recipe lists them) or uses another recipe".into(),
            )),
            (None, Some(u)) => {
                let Some(name) = u.as_str() else {
                    return Err(problem("use", format!("is {}; expected a recipe name", show(u))));
                };
                for key in t.keys() {
                    if PAINTS.iter().any(|p| p.0 == key) || key == "glow" {
                        return Err(problem(
                            key,
                            "is not taken with use: repaint a used recipe with palette = { slot = \"#rrggbb\" }".into(),
                        ));
                    }
                    if !common_keys().contains(&key.as_str()) {
                        return Err(problem(key, "is not a part key (galaxy-gm fields recipe lists them)".into()));
                    }
                }
                if !self.files.contains_key(&format!("{RECIPES}{name}.toml")) {
                    let have = names(self.files);
                    return Err(problem(
                        "use",
                        format!("is `{name}`, which is not a recipe here; the recipes are {}", have.join(", ")),
                    ));
                }
                if stack.iter().any(|s| s == name) {
                    return Err(problem(
                        "use",
                        format!("is `{name}`, which is already being built: {} -> {name} goes round", stack.join(" -> ")),
                    ));
                }
                if stack.len() >= MAX_DEPTH {
                    return Err(problem("use", format!("goes deeper than {MAX_DEPTH} recipes using each other")));
                }
                let mut over = explicit.clone();
                if let Some(v) = t.get("palette") {
                    over.extend(palette(file, &format!("{at}.palette."), v)?);
                }
                let inner = self.recipe(name, &over, stack)?;
                for m in &place.copies {
                    b.with(*m, |b| {
                        for (_, mesh) in &inner.parts {
                            b.append(mesh);
                        }
                    });
                }
                Ok((b.build(), name.to_owned()))
            }
            (Some(s), None) => {
                let Some(shape) = s.as_str().and_then(shapes::find) else {
                    let all: Vec<&str> = shapes::SHAPES.iter().map(|s| s.name).collect();
                    return Err(problem(
                        "shape",
                        format!("is {}, which is not a shape; the shapes are {}", show(s), all.join(", ")),
                    ));
                };
                if t.contains_key("palette") {
                    return Err(problem("palette", "goes with use only; a shape is painted with paint, trim, accent, lights and glass".into()));
                }
                let values = params::read(shape.params, t, &common_keys()).map_err(field)?;
                let mut paints = Paints::new();
                for &(role, _) in PAINTS {
                    let given = t.get(role);
                    if !shape.paints.contains(&role) {
                        if given.is_some() {
                            return Err(problem(
                                role,
                                format!("is not a paint {} uses; it uses {}", shape.name, shape.paints.join(", ")),
                            ));
                        }
                        continue;
                    }
                    let slot = match given {
                        None if role == "paint" => "main",
                        None => role,
                        Some(Value::String(s)) => s.as_str(),
                        Some(v) => return Err(problem(role, format!("is {}; expected a palette slot or \"#rrggbb\"", show(v)))),
                    };
                    let colour = if slot.starts_with('#') {
                        Color::parse_hex(slot)
                            .ok()
                            .filter(|_| slot.len() == 7)
                            .ok_or_else(|| problem(role, format!("is \"{slot}\"; expected \"#rrggbb\"")))?
                    } else {
                        *colours.get(slot).ok_or_else(|| {
                            let have: Vec<&str> = colours.keys().map(String::as_str).collect();
                            problem(
                                role,
                                format!(
                                    "{}paints with slot `{slot}`, which the palette doesn't have; it has {} \
                                     ({PALETTE_FILE} and the recipe's [palette]); give the slot or a \"#rrggbb\"",
                                    if given.is_none() { "is not given, so it " } else { "" },
                                    if have.is_empty() { "no slots".to_owned() } else { have.join(", ") }
                                ),
                            )
                        })?
                    };
                    paints.insert(role, colour);
                }
                let glow = match t.get("glow") {
                    None => 0.0,
                    Some(v) => ranged(v, 0.0, 1.0).map_err(|m| problem("glow", m))?,
                };
                for m in &place.copies {
                    let mut refused = None;
                    b.with(*m, |b| {
                        let mut ctx = Ctx {
                            b,
                            paints: &paints,
                            glow,
                            rng: rng.clone(),
                        };
                        if let Err(e) = (shape.build)(&mut ctx, &values) {
                            refused = Some(e);
                        }
                    });
                    if let Some((k, m)) = refused {
                        return Err(problem(k, m));
                    }
                }
                Ok((b.build(), shape.name.to_owned()))
            }
        }
    }
}
