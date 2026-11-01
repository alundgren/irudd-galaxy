//! The model contract: glTF in and out.
//!
//! A model is a `.gltf` file with its buffers embedded as `data:` URIs, so it is one text
//! file that a module, a recording and a hot-load carry like any other.
//!
//! What the engine reads:
//!
//! - Every node with a mesh in the default scene (or the first scene), through its
//!   transform. Each node is one **part**: its spans (the minor and major extent) size the
//!   ink, so a thin antenna stays a hairline and a hull keeps its full silhouette weight.
//! - Triangles only. `POSITION` is required; `NORMAL` is the shading normal (flat face
//!   normals when absent); `COLOR_0` (RGB or RGBA) times the material's base colour is the
//!   paint.
//! - Glow, how self-lit a surface is (`0..=1`): the material's largest emissive factor,
//!   or the `_GLOW` attribute (a float per vertex) where present.
//! - `_BITE`, a fresh cut (`VEC2`: now and at the morph target, or a float for both): the
//!   shader keeps a cut out of the shadow tint and hatching.
//! - The first morph target's `POSITION` and `NORMAL` displacements: the shape the model
//!   takes at morph weight 1 (a rock mined down, a door open).
//!
//! What it derives at load: outline normals (welded over equal positions), part spans and
//! facet spans. Axes: glTF is +Y up with the front facing +Z; the world is +Z up and a
//! thing at angle 0 faces +X. So glTF `(x, y, z)` is world `(z, x, y)`: the front of a
//! model faces where its entity heads. One glTF unit is one world unit.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use glam::{Mat4, Vec3};
use gltf::mesh::Mode;
use gltf::{Gltf, Semantic};
use serde_json::{Value, json};

use crate::mesh::{MeshData, Vertex, compute_outline_normals, extent_span, facet_span};

/// glTF axes to world axes.
fn to_world(p: Vec3) -> Vec3 {
    Vec3::new(p.z, p.x, p.y)
}

/// World axes to glTF axes.
fn to_gltf(p: Vec3) -> Vec3 {
    Vec3::new(p.y, p.z, p.x)
}

/// Reads a model from `.gltf` text.
pub fn read(text: &str) -> Result<MeshData, String> {
    let doc = Gltf::from_slice(text.as_bytes()).map_err(|e| format!("not a glTF file: {e}"))?;
    let mut buffers = Vec::new();
    for buffer in doc.buffers() {
        let gltf::buffer::Source::Uri(uri) = buffer.source() else {
            return Err("binary glTF chunks are not read; embed buffers as data: URIs".into());
        };
        let Some((_, data)) = uri
            .split_once(";base64,")
            .filter(|_| uri.starts_with("data:"))
        else {
            return Err(format!(
                "buffer {} points to a file ({uri}); embed buffers as data: URIs",
                buffer.index()
            ));
        };
        let bytes = BASE64
            .decode(data)
            .map_err(|e| format!("buffer {}: bad base64: {e}", buffer.index()))?;
        if bytes.len() < buffer.length() {
            return Err(format!(
                "buffer {} has {} bytes, and says it has {}",
                buffer.index(),
                bytes.len(),
                buffer.length()
            ));
        }
        buffers.push(bytes);
    }
    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .ok_or("the model has no scene")?;
    let mut out = MeshData::default();
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            read_part(&mesh, transform, &buffers, &mut out)
                .map_err(|e| format!("node {}: {e}", node.name().unwrap_or("(unnamed)")))?;
        }
        stack.extend(node.children().map(|c| (c, transform)));
    }
    if out.is_empty() {
        return Err("the model has no triangles".into());
    }
    compute_outline_normals(&mut out);
    Ok(out)
}

/// Reads one node's mesh as one part, appending to `out`.
fn read_part(
    mesh: &gltf::Mesh,
    transform: Mat4,
    buffers: &[Vec<u8>],
    out: &mut MeshData,
) -> Result<(), String> {
    let get = |b: gltf::Buffer| buffers.get(b.index()).map(Vec::as_slice);
    let normal_matrix = glam::Mat3::from_mat4(transform).inverse().transpose();
    let flip = transform.determinant() < 0.0;
    let start = out.vertices.len();
    let mut local_lo = Vec3::splat(f32::MAX);
    let mut local_hi = Vec3::splat(f32::MIN);
    for primitive in mesh.primitives() {
        if primitive.mode() != Mode::Triangles {
            return Err(format!(
                "primitive {} is {:?}; models are triangles",
                primitive.index(),
                primitive.mode()
            ));
        }
        let reader = primitive.reader(get);
        let positions: Vec<Vec3> = reader
            .read_positions()
            .ok_or("a primitive has no POSITION")?
            .map(Vec3::from)
            .collect();
        let n = positions.len();
        let count_ok = |what: &str, len: usize| {
            if len == n {
                Ok(())
            } else {
                Err(format!("{what} has {len} values for {n} positions"))
            }
        };
        let normals: Option<Vec<Vec3>> = reader.read_normals().map(|r| r.map(Vec3::from).collect());
        if let Some(v) = &normals {
            count_ok("NORMAL", v.len())?;
        }
        let pbr = primitive.material().pbr_metallic_roughness();
        let base = pbr.base_color_factor();
        let colors: Vec<[f32; 4]> = match reader.read_colors(0) {
            Some(c) => c.into_rgba_f32().collect(),
            None => vec![[1.0; 4]; n],
        };
        count_ok("COLOR_0", colors.len())?;
        let emissive = primitive.material().emissive_factor();
        let material_glow = emissive[0]
            .max(emissive[1])
            .max(emissive[2])
            .clamp(0.0, 1.0);
        let glow: Vec<f32> = match primitive.get(&Semantic::Extras("GLOW".into())) {
            Some(a) => gltf::accessor::Iter::<f32>::new(a, get)
                .ok_or("_GLOW must be a float per vertex")?
                .collect(),
            None => vec![material_glow; n],
        };
        count_ok("_GLOW", glow.len())?;
        let bite: Vec<[f32; 2]> = match primitive.get(&Semantic::Extras("BITE".into())) {
            Some(a) if a.dimensions() == gltf::accessor::Dimensions::Scalar => {
                gltf::accessor::Iter::<f32>::new(a, get)
                    .ok_or("_BITE must be floats")?
                    .map(|b| [b, b])
                    .collect()
            }
            Some(a) => gltf::accessor::Iter::<[f32; 2]>::new(a, get)
                .ok_or("_BITE must be a float or a VEC2 of floats")?
                .collect(),
            None => vec![[0.0; 2]; n],
        };
        count_ok("_BITE", bite.len())?;
        let (morph_pos, morph_nrm) = match reader.read_morph_targets().next() {
            Some((p, nr, _)) => (
                p.map(|i| i.map(Vec3::from).collect::<Vec<_>>()),
                nr.map(|i| i.map(Vec3::from).collect::<Vec<_>>()),
            ),
            None => (None, None),
        };
        if let Some(v) = &morph_pos {
            count_ok("the morph target's POSITION", v.len())?;
        }
        if let Some(v) = &morph_nrm {
            count_ok("the morph target's NORMAL", v.len())?;
        }
        let indices: Vec<u32> = match reader.read_indices() {
            Some(i) => i.into_u32().collect(),
            None => (0..n as u32).collect(),
        };
        if !indices.len().is_multiple_of(3) {
            return Err(format!("{} indices is not whole triangles", indices.len()));
        }
        if let Some(bad) = indices.iter().find(|&&i| i as usize >= n) {
            return Err(format!("index {bad} is past the {n} positions"));
        }
        let place = |p: Vec3| to_world(transform.transform_point3(p));
        let turn = |v: Vec3| to_world(normal_matrix * v).normalize_or(Vec3::Z);
        for tri in indices.as_chunks::<3>().0 {
            let mut corners = tri.map(|i| i as usize);
            if flip {
                corners.swap(1, 2);
            }
            let p = corners.map(|i| place(positions[i]));
            let face = (p[1] - p[0]).cross(p[2] - p[0]);
            if face.length_squared() < 1e-12 {
                continue;
            }
            let face = face.normalize();
            let target = corners.map(|i| match &morph_pos {
                Some(d) => place(positions[i] + d[i]),
                None => place(positions[i]),
            });
            let target_face = (target[1] - target[0])
                .cross(target[2] - target[0])
                .normalize_or(face);
            let facet = facet_span(p[0], p[1], p[2])[0];
            for (k, &i) in corners.iter().enumerate() {
                let local = positions[i];
                local_lo = local_lo.min(local);
                local_hi = local_hi.max(local);
                let normal = normals.as_ref().map_or(face, |v| turn(v[i]));
                let morph_normal = match (&normals, &morph_nrm, &morph_pos) {
                    (Some(v), Some(d), _) => turn(v[i] + d[i]),
                    (Some(_), None, _) => normal,
                    (None, _, Some(_)) => target_face,
                    (None, _, None) => face,
                };
                let c = colors[i];
                let color = [c[0] * base[0], c[1] * base[1], c[2] * base[2], glow[i]];
                out.vertices.push(Vertex {
                    position: p[k].into(),
                    normal: normal.into(),
                    outline_normal: normal.into(),
                    color,
                    span: [0.0; 2],
                    facet,
                    morph_position: target[k].into(),
                    morph_normal: morph_normal.into(),
                    morph_color: color,
                    bite: bite[i].map(|b| {
                        if b.is_finite() {
                            b.clamp(0.0, 1.0)
                        } else {
                            0.0
                        }
                    }),
                });
            }
            let at = (out.vertices.len() - 3) as u32;
            out.indices.extend([at, at + 1, at + 2]);
        }
    }
    if out.vertices.len() > start {
        let axis = |v: glam::Vec4| Vec3::new(v.x, v.y, v.z).length();
        let scale = Vec3::new(
            axis(transform.x_axis),
            axis(transform.y_axis),
            axis(transform.z_axis),
        );
        let span = extent_span((local_hi - local_lo) * scale);
        for v in &mut out.vertices[start..] {
            v.span = span;
        }
    }
    Ok(())
}

/// One part of a model to write: a node with this name and mesh.
pub struct Part<'a> {
    pub name: &'a str,
    pub mesh: &'a MeshData,
}

/// Bytes of the buffer being written, with its views and accessors.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Writer {
    /// Adds floats as one accessor of `ty` (`SCALAR`, `VEC2` ...) with `width` floats per
    /// item; returns its index.
    fn floats(&mut self, data: &[f32], width: usize, ty: &str, bounds: bool) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let offset = self.bytes.len();
        self.bytes.extend(data.iter().flat_map(|f| f.to_le_bytes()));
        self.views.push(json!({
            "buffer": 0, "byteOffset": offset, "byteLength": data.len() * 4,
        }));
        let mut accessor = json!({
            "bufferView": self.views.len() - 1,
            "componentType": 5126,
            "count": data.len() / width,
            "type": ty,
        });
        if bounds {
            let column = |k: usize| data.iter().skip(k).step_by(width).copied();
            let min: Vec<f32> = (0..width)
                .map(|k| column(k).fold(f32::MAX, f32::min))
                .collect();
            let max: Vec<f32> = (0..width)
                .map(|k| column(k).fold(f32::MIN, f32::max))
                .collect();
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn indices(&mut self, data: &[u32]) -> usize {
        let offset = self.bytes.len();
        self.bytes.extend(data.iter().flat_map(|i| i.to_le_bytes()));
        self.views.push(json!({
            "buffer": 0, "byteOffset": offset, "byteLength": data.len() * 4,
        }));
        self.accessors.push(json!({
            "bufferView": self.views.len() - 1,
            "componentType": 5125,
            "count": data.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }
}

/// Writes parts as a `.gltf` file with an embedded buffer, one node per part. Reading it
/// back gives the same triangles, paint, glow, cuts and morph target.
pub fn write(parts: &[Part]) -> String {
    let mut w = Writer::default();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    for part in parts {
        let m = part.mesh;
        let gl = |f: fn(&Vertex) -> [f32; 3]| -> Vec<f32> {
            m.vertices
                .iter()
                .flat_map(|v| to_gltf(Vec3::from(f(v))).to_array())
                .collect()
        };
        let positions = gl(|v| v.position);
        let normals = gl(|v| v.normal);
        let colors: Vec<f32> = m
            .vertices
            .iter()
            .flat_map(|v| [v.color[0], v.color[1], v.color[2]])
            .collect();
        let glow: Vec<f32> = m.vertices.iter().map(|v| v.color[3]).collect();
        let bite: Vec<f32> = m.vertices.iter().flat_map(|v| v.bite).collect();
        let mut attributes = BTreeMap::new();
        attributes.insert("POSITION", w.floats(&positions, 3, "VEC3", true));
        attributes.insert("NORMAL", w.floats(&normals, 3, "VEC3", false));
        attributes.insert("COLOR_0", w.floats(&colors, 3, "VEC3", false));
        if glow.iter().any(|&g| g != 0.0) {
            attributes.insert("_GLOW", w.floats(&glow, 1, "SCALAR", false));
        }
        if bite.iter().any(|&b| b != 0.0) {
            attributes.insert("_BITE", w.floats(&bite, 2, "VEC2", false));
        }
        let morphs = m
            .vertices
            .iter()
            .any(|v| v.morph_position != v.position || v.morph_normal != v.normal);
        let mut primitive = json!({
            "attributes": attributes,
            "indices": w.indices(&m.indices),
            "mode": 4,
        });
        if morphs {
            let delta = |f: fn(&Vertex) -> ([f32; 3], [f32; 3])| -> Vec<f32> {
                m.vertices
                    .iter()
                    .flat_map(|v| {
                        let (to, from) = f(v);
                        to_gltf(Vec3::from(to) - Vec3::from(from)).to_array()
                    })
                    .collect()
            };
            let dp = delta(|v| (v.morph_position, v.position));
            let dn = delta(|v| (v.morph_normal, v.normal));
            primitive["targets"] = json!([{
                "POSITION": w.floats(&dp, 3, "VEC3", true),
                "NORMAL": w.floats(&dn, 3, "VEC3", false),
            }]);
        }
        meshes.push(json!({ "name": part.name, "primitives": [primitive] }));
        nodes.push(json!({ "name": part.name, "mesh": meshes.len() - 1 }));
    }
    let doc = json!({
        "asset": { "version": "2.0", "generator": "galaxy-mesh" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": meshes,
        "accessors": w.accessors,
        "bufferViews": w.views,
        "buffers": [{
            "byteLength": w.bytes.len(),
            "uri": format!("data:application/octet-stream;base64,{}", BASE64.encode(&w.bytes)),
        }],
    });
    let mut text = serde_json::to_string_pretty(&doc).expect("JSON values serialise");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, MeshBuilder};

    fn hull() -> MeshData {
        let mut b = MeshBuilder::new();
        b.paint(Color::hex(0xd9774b), 0.0);
        b.cuboid(Vec3::new(4.0, 2.0, 1.0));
        b.build()
    }

    fn lamp() -> MeshData {
        let mut b = MeshBuilder::new();
        b.paint(Color::hex(0xffcf6b), 1.0);
        b.with(Mat4::from_translation(Vec3::new(2.5, 0.0, 0.0)), |b| {
            b.cuboid(Vec3::new(0.2, 0.2, 0.2));
        });
        b.build()
    }

    #[test]
    fn a_written_model_reads_back_the_same() {
        let (hull, lamp) = (hull(), lamp());
        let text = write(&[
            Part {
                name: "hull",
                mesh: &hull,
            },
            Part {
                name: "lamp",
                mesh: &lamp,
            },
        ]);
        let back = read(&text).expect("reads");
        assert!(back.is_well_formed());
        assert_eq!(
            back.triangle_count(),
            hull.triangle_count() + lamp.triangle_count()
        );
        let near = |a: [f32; 3], b: [f32; 3]| (Vec3::from(a) - Vec3::from(b)).length() < 1e-5;
        // Nodes come back in some order; match each vertex to one written.
        let written: Vec<&Vertex> = hull.vertices.iter().chain(&lamp.vertices).collect();
        for v in &back.vertices {
            let w = written
                .iter()
                .find(|w| near(w.position, v.position) && near(w.normal, v.normal))
                .expect("every vertex was written");
            assert!(
                w.color
                    .iter()
                    .zip(v.color)
                    .all(|(a, b)| (a - b).abs() < 1e-6)
            );
            assert!(near(w.outline_normal, v.outline_normal));
        }
        // The lamp glows, the hull doesn't; each part has its own span.
        let lamp_v = back.vertices.iter().find(|v| v.position[0] > 2.3).unwrap();
        let hull_v = back.vertices.iter().find(|v| v.position[0] < 0.0).unwrap();
        assert_eq!(lamp_v.color[3], 1.0);
        assert_eq!(hull_v.color[3], 0.0);
        assert!((lamp_v.span[0] - 0.2).abs() < 1e-4, "{:?}", lamp_v.span);
        assert!((hull_v.span[1] - 4.0).abs() < 1e-4, "{:?}", hull_v.span);
    }

    #[test]
    fn the_front_of_a_gltf_model_faces_world_x_and_its_up_is_world_z() {
        // A one-triangle model authored in glTF axes: its tip at +Z (the front), raised
        // along +Y (up).
        let mut m = MeshData::default();
        for p in [[0.0, 0.0, 1.0], [-0.5, 0.0, -1.0], [0.5, 1.0, -1.0]] {
            let p = to_world(Vec3::from(p)).into();
            m.vertices.push(Vertex {
                position: p,
                normal: [0.0, 0.0, 1.0],
                outline_normal: [0.0, 0.0, 1.0],
                color: [1.0; 4],
                span: [1.0; 2],
                facet: 1.0,
                morph_position: p,
                morph_normal: [0.0, 0.0, 1.0],
                morph_color: [1.0; 4],
                bite: [0.0; 2],
            });
        }
        m.indices = vec![0, 1, 2];
        let back = read(&write(&[Part {
            name: "tip",
            mesh: &m,
        }]))
        .unwrap();
        let xs: Vec<f32> = back.vertices.iter().map(|v| v.position[0]).collect();
        assert!(
            xs.iter().any(|&x| (x - 1.0).abs() < 1e-6),
            "the tip is at world +X"
        );
        assert!(
            back.vertices
                .iter()
                .any(|v| (v.position[2] - 1.0).abs() < 1e-6)
        );
    }

    #[test]
    fn a_morph_target_reads_back() {
        let mut big = hull();
        let small = {
            let mut b = MeshBuilder::new();
            b.paint(Color::hex(0xd9774b), 0.0);
            b.cuboid(Vec3::new(2.0, 1.0, 0.5));
            b.build()
        };
        assert!(big.morph_to(&small));
        let back = read(&write(&[Part {
            name: "rock",
            mesh: &big,
        }]))
        .unwrap();
        let morphed = back.morphed(1.0);
        let (lo, hi) = morphed.bounds().unwrap();
        assert!(
            ((hi - lo) - Vec3::new(2.0, 1.0, 0.5)).length() < 1e-4,
            "{lo} {hi}"
        );
    }

    #[test]
    fn bad_models_say_what_is_wrong() {
        assert!(read("{").unwrap_err().contains("not a glTF"));
        let external = r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"a.bin"}]}"#;
        assert!(read(external).unwrap_err().contains("data: URIs"));
        let empty = r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[]}]}"#;
        assert!(read(empty).unwrap_err().contains("no triangles"));
    }
}
