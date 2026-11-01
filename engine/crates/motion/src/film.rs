//! The film format: what `galaxy film` and `galaxy-gm film` write and every motion tool
//! reads.
//!
//! A film is a folder:
//!
//! ```text
//! film.json           how it was shot, the things in it and what changed (small)
//! frames.jsonl        a frame a line: time, tick, camera and where every thing is
//! lines.jsonl         a frame a line: every line and dot it drew
//! outlines.json       each thing's outline on the plane, for gaps between surfaces
//! parts.json          the models' parts as triangles, to name what changed pixels show
//! frames/00000.png    each frame, lossless (none for a probes-only film)
//! ```
//!
//! Split so an agent can read `film.json` whole and search the others a frame at a time;
//! `galaxy-gm film show` prints them compactly. Format 1 films, with every frame inside
//! `film.json`, still read.
//!
//! Each frame entry keeps when it was shown (presentation time, the tick, and `alpha`,
//! how far it was drawn from the tick before toward that tick), the camera as a matrix,
//! and probes: where every drawn thing was in the world and on screen, and every line the
//! scene drew, with the centre of each dot of a dotted line. Pixels are physical pixels of
//! the frame, origin top left, +y down.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::image::Image;

/// The film format this engine writes. It reads this and format 1, where `film.json` held
/// every frame.
pub const FORMAT: u32 = 2;

/// A film's manifest, `film.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Frames a second of presentation time.
    pub fps: u32,
    pub ticks_per_second: u32,
    /// Each frame's size in physical pixels, and how many physical pixels a logical one is.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    /// Whether frames were drawn (false: probes only, no GPU used).
    pub drawn: bool,
    /// The GPU that drew them.
    #[serde(default)]
    pub adapter: Option<String>,
    /// What was filmed: modules and inputs, or a recording and a note.
    #[serde(default)]
    pub source: Value,
    /// Which build drew the film and, for a played session, which recorded it:
    /// `{recorded, drawn, same, note}`. Null in films from before builds were kept.
    #[serde(default)]
    pub builds: Value,
    /// Inputs, orders and arrivals in the world while it was filmed, in tick order.
    #[serde(default)]
    pub changes: Vec<Change>,
    pub frames: Vec<Frame>,
    /// Each thing's outline on the plane, by entity ID (kept in `outlines.json`). Empty
    /// in films from before outlines were kept.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outlines: BTreeMap<String, Outline>,
    /// Each thing's model parts (kept in `parts.json`). Empty in films from before parts
    /// were kept.
    #[serde(default, skip_serializing_if = "Parts::is_empty")]
    pub parts: Parts,
}

/// The parts of the models things wear (kept in `parts.json`), so a tool can name the
/// part under a pixel: each model's parts (glTF nodes) as triangles in its own frame (+x
/// its front, +y its left, +z up, world units at scale 1), and which model each thing
/// wears at what scale.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Parts {
    pub models: BTreeMap<String, Vec<PartMesh>>,
    pub things: BTreeMap<String, Worn>,
}

impl Parts {
    pub fn is_empty(&self) -> bool {
        self.things.is_empty()
    }
}

/// One part of a model: its name and its triangles, nine numbers each (three corners).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartMesh {
    pub name: String,
    pub triangles: Vec<f32>,
}

/// The model a thing wears, and its scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Worn {
    pub model: String,
    pub scale: f32,
}

/// A thing's outline on the sim plane, as collision sees it: how far it reaches from its
/// centre in each of `base.len()` directions evenly round the circle from +x, in its own
/// frame (facing +x), scale applied. `morphed` is the reach at morph weight 1, when the
/// model morphs; a frame's thing says its weight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    pub base: Vec<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub morphed: Vec<f32>,
}

impl Outline {
    /// The reach along world direction `k` when turned by `angle` and morphed by `morph`.
    fn reach(&self, k: usize, angle: f32, morph: f32) -> f32 {
        let n = self.base.len();
        let step = std::f32::consts::TAU / n as f32;
        let at = (k as f32 - angle / step).rem_euclid(n as f32);
        let i = (at.floor() as usize) % n;
        let j = (i + 1) % n;
        let t = at - at.floor();
        let m = morph.clamp(0.0, 1.0);
        let mix = |q: usize| match self.morphed.get(q) {
            Some(to) if m > 0.0 => self.base[q] + (to - self.base[q]) * m,
            _ => self.base[q],
        };
        mix(i) + (mix(j) - mix(i)) * t
    }
}

/// The gap between two things' outlines along the line between their centres, as
/// collision measures it: how far `b`'s centre is from where it would just touch `a`.
/// Negative when they overlap; none when they have outlines of different sampling.
pub fn surface_gap(a: (&Outline, &Thing), b: (&Outline, &Thing)) -> Option<f32> {
    let n = a.0.base.len();
    if n < 8 || b.0.base.len() != n {
        return None;
    }
    let (dx, dy) = (b.1.x - a.1.x, b.1.y - a.1.y);
    let distance = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let (ux, uy) = (angle.cos(), angle.sin());
    let morph = |t: &Thing| t.morph.unwrap_or(0.0);
    let touching = (0..n)
        .filter_map(|k| {
            let d = std::f32::consts::TAU * k as f32 / n as f32;
            let along = ux * d.cos() + uy * d.sin();
            (along > 1e-6).then(|| {
                let ra = a.0.reach(k, a.1.angle, morph(a.1));
                let rb = b.0.reach((k + n / 2) % n, b.1.angle, morph(b.1));
                (ra + rb) / along
            })
        })
        .fold(f32::INFINITY, f32::min);
    touching.is_finite().then_some(distance - touching)
}

/// `film.json` in format 2: how the film was made, with a summary of its frames; the
/// frames themselves are in `frames.jsonl` and their lines in `lines.jsonl`.
#[derive(Serialize, Deserialize)]
struct Header {
    format: u32,
    fps: u32,
    ticks_per_second: u32,
    width: u32,
    height: u32,
    scale: f32,
    drawn: bool,
    #[serde(default)]
    adapter: Option<String>,
    #[serde(default)]
    source: Value,
    #[serde(default)]
    builds: Value,
    frame_count: usize,
    first: Value,
    last: Value,
    things: Vec<String>,
    #[serde(default)]
    changes: Vec<Change>,
    files: Value,
}

impl Manifest {
    /// Writes the film's manifest into its folder: `film.json` (small: how it was made),
    /// `frames.jsonl` (a frame a line: time, tick, camera and every thing) and
    /// `lines.jsonl` (a frame a line: the lines and dots it drew).
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        let mut frames = String::new();
        let mut lines = String::new();
        for f in &self.frames {
            let mut v = serde_json::to_value(f).map_err(|e| e.to_string())?;
            if let Value::Object(m) = &mut v {
                m.remove("lines");
            }
            frames.push_str(&v.to_string());
            frames.push('\n');
            let drawn = serde_json::to_string(&f.lines).map_err(|e| e.to_string())?;
            lines.push_str(&format!("{{\"frame\":{},\"lines\":{drawn}}}\n", f.index));
        }
        let ends = |f: Option<&Frame>| f.map_or(Value::Null, when);
        let header = Header {
            format: FORMAT,
            fps: self.fps,
            ticks_per_second: self.ticks_per_second,
            width: self.width,
            height: self.height,
            scale: self.scale,
            drawn: self.drawn,
            adapter: self.adapter.clone(),
            source: self.source.clone(),
            builds: self.builds.clone(),
            frame_count: self.frames.len(),
            first: ends(self.frames.first()),
            last: ends(self.frames.last()),
            things: self
                .frames
                .first()
                .map(|f| f.things.keys().cloned().collect())
                .unwrap_or_default(),
            changes: self.changes.clone(),
            files: serde_json::json!({
                "frames.jsonl": "a frame a line: index, file, time (s), tick, alpha, hash, camera, zoom, things (x, y, height, angle, screen, on_screen, followed, reach)",
                "lines.jsonl": "a frame a line: the lines it drew (kind, closed, alpha, width, points, phase, spacing, dots)",
                "outlines.json": "each thing's outline on the plane: its reach in evenly spaced directions from +x in its own frame (base, and morphed for a model that morphs)",
                "parts.json": "models: each model's parts (glTF nodes) as triangles in its own frame, nine numbers a triangle; things: which model each thing wears and its scale",
                "frames/": "the pictures, frames/00000.png on",
                "motion/": "the motion tools' pictures",
                "show": "galaxy-gm film show <film> prints the probes compactly",
            }),
        };
        let header = serde_json::to_string_pretty(&header).map_err(|e| e.to_string())?;
        if !self.outlines.is_empty() {
            let rounded: BTreeMap<&String, Value> = self
                .outlines
                .iter()
                .map(|(id, o)| {
                    let r = |v: &[f32]| v.iter().map(|x| round(*x, 5)).collect::<Vec<_>>();
                    let mut v = serde_json::json!({ "base": r(&o.base) });
                    if !o.morphed.is_empty() {
                        v["morphed"] = serde_json::json!(r(&o.morphed));
                    }
                    (id, v)
                })
                .collect();
            let text = serde_json::to_string(&rounded).map_err(|e| e.to_string())?;
            let path = dir.join("outlines.json");
            std::fs::write(&path, text)
                .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        }
        if !self.parts.is_empty() {
            let mut parts = self.parts.clone();
            for p in parts.models.values_mut().flatten() {
                for v in &mut p.triangles {
                    *v = round(*v, 3) as f32;
                }
            }
            let text = serde_json::to_string(&parts).map_err(|e| e.to_string())?;
            let path = dir.join("parts.json");
            std::fs::write(&path, text)
                .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        }
        for (name, text) in [
            ("frames.jsonl", frames),
            ("lines.jsonl", lines),
            ("film.json", header),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, text)
                .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        }
        Ok(())
    }
}

/// Something that changed in the world during a film: an input (a key, a click), an
/// order (`engine.steer` given, changed or dropped) or an arrival.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub tick: u64,
    /// Presentation seconds when the tick is shown.
    pub t: f32,
    /// `input`, `order` or `arrived`.
    pub kind: String,
    /// What: `drift.order_dock down`, `drift.ship: dock drift.berth`.
    pub what: String,
}

/// One frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub index: usize,
    /// The picture, relative to the film folder.
    #[serde(default)]
    pub file: Option<String>,
    /// Presentation seconds since the session started.
    pub time: f32,
    /// The world's tick, and how far the frame is drawn from the tick before toward it.
    pub tick: u64,
    pub alpha: f32,
    /// The world's hash at `tick`.
    pub hash: String,
    pub camera: Cam,
    /// The view height the wheel zoomed to, if it did.
    #[serde(default)]
    pub zoom: Option<f32>,
    /// Every drawn thing, by entity ID.
    pub things: BTreeMap<String, Thing>,
    pub lines: Vec<LineProbe>,
}

/// The camera of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cam {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// World to clip space, column-major.
    pub view_proj: [f32; 16],
    /// World units one physical pixel covers at the target.
    pub per_pixel: f32,
}

impl Cam {
    /// Where world point `p` is in a `width` x `height` frame, in physical pixels; none
    /// behind a perspective camera.
    pub fn project(&self, p: [f32; 3], width: u32, height: u32) -> Option<[f32; 2]> {
        let m = &self.view_proj;
        let v = [p[0], p[1], p[2], 1.0];
        let row = |r: usize| (0..4).map(|c| m[c * 4 + r] * v[c]).sum::<f32>();
        let (x, y, w) = (row(0), row(1), row(3));
        if w <= 1e-6 {
            return None;
        }
        Some([
            (x / w + 1.0) * 0.5 * width as f32,
            (1.0 - y / w) * 0.5 * height as f32,
        ])
    }

    /// Where the pixel `(px, py)` of a `width` x `height` frame lands on the plane
    /// `z = plane`, by inverting the projection.
    pub fn unproject(
        &self,
        px: f32,
        py: f32,
        width: u32,
        height: u32,
        plane: f32,
    ) -> Option<[f32; 2]> {
        let inv = invert(&self.view_proj)?;
        let nx = px / width as f32 * 2.0 - 1.0;
        let ny = 1.0 - py / height as f32 * 2.0;
        let at = |z: f32| {
            let v = [nx, ny, z, 1.0];
            let r = |row: usize| (0..4).map(|c| inv[c * 4 + row] * v[c]).sum::<f32>();
            let w = r(3);
            [r(0) / w, r(1) / w, r(2) / w]
        };
        let (a, b) = (at(0.0), at(1.0));
        let dz = b[2] - a[2];
        if dz.abs() < 1e-9 {
            return None;
        }
        let t = (plane - a[2]) / dz;
        Some([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    }
}

/// The inverse of a column-major 4x4 matrix, by cofactors.
pub(crate) fn invert(m: &[f32; 16]) -> Option<[f32; 16]> {
    let m: Vec<f64> = m.iter().map(|&v| f64::from(v)).collect();
    let mut inv = [0f64; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    if det.abs() < 1e-30 {
        return None;
    }
    let mut out = [0f32; 16];
    for (o, v) in out.iter_mut().zip(inv) {
        *o = (v / det) as f32;
    }
    Some(out)
}

/// A drawn thing in a frame: its interpolated pose and where it is on screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thing {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub angle: f32,
    #[serde(default)]
    pub screen: Option<[f32; 2]>,
    pub on_screen: bool,
    /// The thing the camera follows.
    #[serde(default)]
    pub followed: bool,
    /// How far its model reaches from its centre on the plane, world units.
    #[serde(default)]
    pub reach: Option<f32>,
    /// Its model's morph weight, when it has one above 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morph: Option<f32>,
}

impl Thing {
    pub fn point(&self) -> [f32; 3] {
        [self.x, self.y, self.height]
    }
}

/// A line the scene drew.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineProbe {
    /// `solid`, `dashed`, `dotted` or `beam`.
    pub kind: String,
    /// A ring or other closed loop.
    pub closed: bool,
    pub alpha: f32,
    /// Width in world units.
    pub width: f32,
    /// World points, in drawing order.
    pub points: Vec<[f32; 3]>,
    /// Where the pattern starts along the first point (world units).
    #[serde(default)]
    pub phase: f32,
    /// Dot spacing, for dotted lines (world units).
    #[serde(default)]
    pub spacing: Option<f32>,
    /// Each dot's centre in the world and how visible the line's end fade leaves it
    /// (`[x, y, z, visible]`, visible 0 to 1).
    #[serde(default)]
    pub dots: Vec<[f32; 4]>,
}

/// A film read from its folder.
pub struct Film {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Film {
    pub fn read(dir: &Path) -> Result<Film, String> {
        let path = dir.join("film.json");
        let text = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "{} is not a film (no readable film.json: {e}); make one with `galaxy film` \
                 or `galaxy-gm film`",
                dir.display()
            )
        })?;
        let raw: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a film manifest: {e}", path.display()))?;
        let bad = |e: String| format!("{} is not a valid film manifest: {e}", path.display());
        let manifest: Manifest = match raw["format"].as_u64() {
            Some(1) => serde_json::from_value(raw).map_err(|e| bad(e.to_string()))?,
            Some(2) => {
                let h: Header = serde_json::from_value(raw).map_err(|e| bad(e.to_string()))?;
                Manifest {
                    format: h.format,
                    fps: h.fps,
                    ticks_per_second: h.ticks_per_second,
                    width: h.width,
                    height: h.height,
                    scale: h.scale,
                    drawn: h.drawn,
                    adapter: h.adapter,
                    source: h.source,
                    builds: h.builds,
                    changes: h.changes,
                    frames: read_frames(dir)?,
                    outlines: read_outlines(dir)?,
                    parts: read_parts(dir)?,
                }
            }
            _ => {
                return Err(format!(
                    "{} is film format {}, and this engine reads formats 1 and {FORMAT}; film \
                     it again",
                    path.display(),
                    raw["format"]
                ));
            }
        };
        if manifest.frames.is_empty() {
            return Err(format!("{} has no frames", path.display()));
        }
        Ok(Film {
            dir: dir.to_path_buf(),
            manifest,
        })
    }

    /// Which build drew the film, next to the one that recorded the session, for every
    /// tool's report.
    pub fn builds(&self) -> Value {
        match &self.manifest.builds {
            Value::Null => {
                let recorded = self.manifest.source["recorded_build"]["name"].as_str();
                serde_json::json!({
                    "recorded": recorded,
                    "drawn": null,
                    "note": "this film does not say which build drew it (filmed by an older build); film it again to know whether it shows what the player saw",
                })
            }
            b => b.clone(),
        }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.manifest.frames
    }

    pub fn size(&self) -> (u32, u32) {
        (self.manifest.width, self.manifest.height)
    }

    /// Seconds between frames.
    pub fn dt(&self) -> f32 {
        1.0 / self.manifest.fps.max(1) as f32
    }

    /// Frame `i`'s picture.
    pub fn image(&self, i: usize) -> Result<Image, String> {
        let frame = self
            .manifest
            .frames
            .get(i)
            .ok_or_else(|| format!("the film has no frame {i}"))?;
        let file = frame.file.as_ref().ok_or_else(|| {
            "this film has probes only, no pictures; film it again without --probes-only".to_owned()
        })?;
        Image::load_png(&self.dir.join(file))
    }

    /// Requires pictures, for a tool that reads pixels.
    pub fn need_pictures(&self, tool: &str) -> Result<(), String> {
        if self.manifest.drawn {
            Ok(())
        } else {
            Err(format!(
                "`{tool}` reads pixels, and this film has probes only; film it again without \
                 --probes-only"
            ))
        }
    }

    /// The frame index nearest presentation time `t`.
    pub fn frame_at(&self, t: f32) -> usize {
        let frames = &self.manifest.frames;
        let mut best = 0;
        for (i, f) in frames.iter().enumerate() {
            if (f.time - t).abs() < (frames[best].time - t).abs() {
                best = i;
            }
        }
        best
    }

    /// The film's thing named `id`, accepting a bare name when one module has it
    /// (`ship` for `drift.ship`).
    pub fn thing_id(&self, id: &str) -> Result<String, String> {
        let first = &self.manifest.frames[0].things;
        if first.contains_key(id) {
            return Ok(id.to_owned());
        }
        let suffix = format!(".{id}");
        let matches: Vec<&String> = first.keys().filter(|k| k.ends_with(&suffix)).collect();
        match matches.as_slice() {
            [one] => Ok((*one).clone()),
            [] => Err(format!(
                "no thing `{id}` in the film; it has {}",
                first.keys().cloned().collect::<Vec<_>>().join(", ")
            )),
            many => Err(format!(
                "`{id}` could be {}; write the full ID",
                many.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" or ")
            )),
        }
    }
}

/// `outlines.json`, if the film has one.
fn read_outlines(dir: &Path) -> Result<BTreeMap<String, Outline>, String> {
    let path = dir.join("outlines.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a film's outlines: {e}", path.display())),
        Err(_) => Ok(BTreeMap::new()),
    }
}

/// `parts.json`, if the film has one.
fn read_parts(dir: &Path) -> Result<Parts, String> {
    let path = dir.join("parts.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a film's parts: {e}", path.display())),
        Err(_) => Ok(Parts::default()),
    }
}

/// `frames.jsonl` with each frame's lines from `lines.jsonl`.
fn read_frames(dir: &Path) -> Result<Vec<Frame>, String> {
    let read = |name: &str| {
        let path = dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| format!("can't read {}: {e}", path.display()))
    };
    let (frames, lines) = (read("frames.jsonl")?, read("lines.jsonl")?);
    let mut drawn: BTreeMap<u64, Value> = BTreeMap::new();
    for (n, l) in lines.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let mut v: Value =
            serde_json::from_str(l).map_err(|e| format!("lines.jsonl line {}: {e}", n + 1))?;
        let frame = v["frame"].as_u64().unwrap_or(n as u64);
        drawn.insert(frame, v["lines"].take());
    }
    frames
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            let mut v: Value =
                serde_json::from_str(l).map_err(|e| format!("frames.jsonl line {}: {e}", n + 1))?;
            let index = v["index"].as_u64().unwrap_or(n as u64);
            v["lines"] = drawn.remove(&index).unwrap_or(Value::Array(Vec::new()));
            serde_json::from_value(v).map_err(|e| format!("frames.jsonl line {}: {e}", n + 1))
        })
        .collect()
}

/// A moment in a film, for reports: frame, time and tick.
pub fn when(frame: &Frame) -> Value {
    serde_json::json!({
        "frame": frame.index,
        "t": round(frame.time, 3),
        "tick": frame.tick,
    })
}

/// `v` rounded to `places` decimals, for compact reports.
pub fn round(v: f32, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (f64::from(v) * k).round() / k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ortho() -> Cam {
        // Orthographic: x and y scaled by 0.1 and 0.2 into clip space, z by 0.01.
        let mut m = [0.0; 16];
        m[0] = 0.1;
        m[5] = 0.2;
        m[10] = 0.01;
        m[15] = 1.0;
        Cam {
            eye: [0.0; 3],
            target: [0.0; 3],
            view_proj: m,
            per_pixel: 0.0,
        }
    }

    #[test]
    fn surfaces_are_measured_outline_to_outline() {
        let at = |x: f32, angle: f32| Thing {
            x,
            y: 0.0,
            height: 0.0,
            angle,
            screen: None,
            on_screen: false,
            followed: false,
            reach: None,
            morph: None,
        };
        let circle = |r: f32| Outline {
            base: vec![r; 128],
            morphed: Vec::new(),
        };
        let (a, b) = (circle(1.0), circle(2.0));
        let gap = surface_gap((&a, &at(0.0, 0.0)), (&b, &at(5.0, 0.0))).unwrap();
        assert!((gap - 2.0).abs() < 0.01, "{gap}");
        let overlap = surface_gap((&a, &at(0.0, 0.0)), (&b, &at(2.5, 0.0))).unwrap();
        assert!((overlap + 0.5).abs() < 0.01, "{overlap}");
        // A long thing (reaching 3 along its own +x, 1 across) turned a quarter round
        // reaches 1 towards +x.
        let long = Outline {
            base: (0..128)
                .map(|k| {
                    let t = std::f32::consts::TAU * k as f32 / 128.0;
                    (3.0 * t.cos().abs()).max(t.sin().abs())
                })
                .collect(),
            morphed: Vec::new(),
        };
        let facing = surface_gap((&long, &at(0.0, 0.0)), (&a, &at(6.0, 0.0))).unwrap();
        let turned = surface_gap(
            (&long, &at(0.0, std::f32::consts::FRAC_PI_2)),
            (&a, &at(6.0, 0.0)),
        )
        .unwrap();
        assert!((facing - 2.0).abs() < 0.05, "{facing}");
        assert!((turned - 4.0).abs() < 0.05, "{turned}");
    }

    #[test]
    fn projection_follows_the_column_major_matrix() {
        let cam = ortho();
        assert_eq!(cam.project([0.0, 0.0, 0.0], 200, 100), Some([100.0, 50.0]));
        // +x right, +y up the screen.
        assert_eq!(cam.project([10.0, 5.0, 0.0], 200, 100), Some([200.0, 0.0]));
    }

    #[test]
    fn unprojecting_a_pixel_finds_the_plane_point() {
        let cam = ortho();
        let p = cam.unproject(150.0, 25.0, 200, 100, 0.0).unwrap();
        assert!(
            (p[0] - 5.0).abs() < 1e-4 && (p[1] - 2.5).abs() < 1e-4,
            "{p:?}"
        );
    }
}
