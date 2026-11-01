//! The film format: what `galaxy film` and `galaxy-gm film` write and every motion tool
//! reads.
//!
//! A film is a folder:
//!
//! ```text
//! film.json           the manifest: how it was shot, and one entry per frame
//! frames/00000.png    each frame, lossless (none for a probes-only film)
//! ```
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

/// The film format this engine writes and reads.
pub const FORMAT: u32 = 1;

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
    /// Inputs, orders and arrivals in the world while it was filmed, in tick order.
    #[serde(default)]
    pub changes: Vec<Change>,
    pub frames: Vec<Frame>,
}

impl Manifest {
    /// Writes the film's manifest into its folder.
    pub fn write(&self, dir: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let path = dir.join("film.json");
        std::fs::write(&path, text).map_err(|e| format!("can't write {}: {e}", path.display()))
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
        if raw["format"].as_u64() != Some(u64::from(FORMAT)) {
            return Err(format!(
                "{} is film format {}, and this engine reads format {FORMAT}; film it again",
                path.display(),
                raw["format"]
            ));
        }
        let manifest: Manifest = serde_json::from_value(raw)
            .map_err(|e| format!("{} is not a valid film manifest: {e}", path.display()))?;
        if manifest.frames.is_empty() {
            return Err(format!("{} has no frames", path.display()));
        }
        Ok(Film {
            dir: dir.to_path_buf(),
            manifest,
        })
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
