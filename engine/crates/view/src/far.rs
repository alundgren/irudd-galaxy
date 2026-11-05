//! The sky's layers and landmarks for one frame: where each is for this camera, far to
//! near ([`galaxy_render::far`]).
//!
//! Presentation only: they read the camera and the style, never the world, and their
//! random placement comes from the world's seed, so a replay, an F8 note and a film draw
//! the same sky. Reduced motion holds every layer and landmark still on screen.

use std::collections::BTreeMap;

use galaxy_render::far::{self, Far, Field, Mark, Outline, Shape};
use galaxy_render::glam::{Mat4, Quat, Vec2, Vec3};
use galaxy_render::style::{Ink, Landmark, Look, Sky, SkyLayer};
use galaxy_render::{Camera, Material, MeshHandle, MeshInstance};

/// The most models one model layer draws in a frame.
pub const MAX_LAYER_MODELS: usize = 256;
/// The most grid cells a model layer looks at in a frame.
const MAX_LAYER_CELLS: i64 = 16_384;

/// What the sky's models need from the view.
pub struct Models<'a> {
    pub handles: &'a BTreeMap<String, MeshHandle>,
    pub looks: &'a BTreeMap<String, Look>,
    /// How far each model reaches from its centre, for what may poke into view.
    pub reach: &'a BTreeMap<String, f32>,
}

/// The camera a frame's sky is placed for.
pub struct Frame {
    pub camera: Camera,
    pub logical: Vec2,
    pub seed: u64,
    /// Reduced motion: everything stays where it is with the camera at the origin.
    pub still: bool,
    /// World units a logical pixel shows at the camera's own zoom (its `view`), which a
    /// layer's density counts in.
    pub unit: f32,
}

/// The sky's layers and landmarks for `frame`, far to near, and the models and looks
/// they name that no module has.
pub fn items(sky: &Sky, ink: &Ink, models: &Models, frame: &Frame) -> (Vec<Far>, Vec<String>) {
    let mut missing = Vec::new();
    let mut out: Vec<(f32, Far)> = Vec::new();
    let target = frame.camera.target.truncate();
    // Landmarks first, so a layer at the same depth draws in front of them.
    for (i, l) in sky.landmarks.iter().enumerate() {
        let seed = mix(frame.seed, 0x1a2d_0000 + i as u64);
        if let Some(item) = landmark(sky, ink, models, l, target, frame.still, seed, &mut missing) {
            out.push((l.parallax, item));
        }
    }
    for (i, l) in sky.layers.iter().enumerate() {
        let seed = mix(frame.seed, 0x1a7e_0000 + i as u64);
        if let Some(item) = layer(sky, ink, models, l, frame, seed, &mut missing) {
            out.push((l.parallax, item));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    (out.into_iter().map(|(_, f)| f).collect(), missing)
}

fn rad(degrees: f32) -> f32 {
    degrees.to_radians()
}

/// The material a sky model wears: its look, or the default with the style's ink.
fn material(models: &Models, ink: &Ink, look: &str, missing: &mut Vec<String>) -> Material {
    if look.is_empty() {
        return Material {
            outline: ink.color,
            ..Material::default()
        };
    }
    match models.looks.get(look) {
        Some(l) => l.material(ink),
        None => {
            missing.push(format!("look `{look}` (the sky wears it)"));
            Material::default()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn landmark(
    sky: &Sky,
    ink: &Ink,
    models: &Models,
    l: &Landmark,
    target: Vec2,
    still: bool,
    seed: u64,
    missing: &mut Vec<String>,
) -> Option<Far> {
    let parallax = if still { 0.0 } else { l.parallax };
    let at = far::placed(target, Vec2::from(l.at), parallax);
    let haze = sky.haze().with_alpha(sky.fade(l.parallax, l.fade));
    let outline = match l.shape.as_str() {
        "disc" => Outline::Disc,
        "wash" => Outline::Wash,
        _ => {
            let Some(&handle) = models.handles.get(&l.model) else {
                missing.push(format!("model `{}` (the sky wears it)", l.model));
                return None;
            };
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(l.scale.max(1e-3)),
                Quat::from_rotation_z(rad(l.angle)),
                at.extend(0.0),
            );
            let material = material(models, ink, &l.look, missing);
            return Some(Far::Meshes(vec![
                MeshInstance::new(handle, transform)
                    .with_material(material)
                    .with_haze(haze),
            ]));
        }
    };
    Some(Far::Shape(Shape {
        outline,
        center: at.extend(0.0),
        radius: l.size,
        stretch: l.stretch.max(0.05),
        angle: rad(l.angle),
        color: l.color.with_alpha(l.alpha()),
        shadow: l.shadow,
        ink: ink.color,
        outline_px: l.outline_px,
        hatch: l.hatch,
        seed: seed as u32,
        haze,
    }))
}

fn layer(
    sky: &Sky,
    ink: &Ink,
    models: &Models,
    l: &SkyLayer,
    frame: &Frame,
    seed: u64,
    missing: &mut Vec<String>,
) -> Option<Far> {
    let target = frame.camera.target.truncate();
    let parallax = if frame.still { 0.0 } else { l.parallax };
    let offset = far::offset(target, parallax);
    let cell = far::field_cell(l.density)? * frame.unit.max(1e-6);
    let haze = sky.haze().with_alpha(sky.fade(l.parallax, l.fade));
    let mark = match l.shape.as_str() {
        "dust" => Mark::Dust,
        "speck" => Mark::Speck,
        "hatch" => Mark::Hatch,
        _ => return model_layer(ink, models, l, frame, offset, cell, seed, haze, missing),
    };
    Some(Far::Field(Field {
        mark,
        offset,
        cell,
        chance: far::FIELD_CHANCE,
        size: l.size,
        color: l.tint.with_alpha(l.alpha()),
        angle: rad(l.angle),
        seed: seed as u32,
        haze,
    }))
}

/// A model scattered on the layer's grid: the cells in view, each holding a model with
/// [`far::FIELD_CHANCE`], at a place, scale and turn hashed from the cell.
#[allow(clippy::too_many_arguments)]
fn model_layer(
    ink: &Ink,
    models: &Models,
    l: &SkyLayer,
    frame: &Frame,
    offset: Vec2,
    cell: f32,
    seed: u64,
    haze: galaxy_render::Color,
    missing: &mut Vec<String>,
) -> Option<Far> {
    let Some(&handle) = models.handles.get(&l.model) else {
        missing.push(format!("model `{}` (the sky wears it)", l.model));
        return None;
    };
    let target = frame.camera.target.truncate();
    let (lo, hi) = in_view(&frame.camera, frame.logical);
    let reach = models.reach.get(&l.model).copied().unwrap_or(1.0) * l.size[1];
    let (lo, hi) = (offset + lo - reach, offset + hi + reach);
    let (x0, x1) = ((lo.x / cell).floor() as i64, (hi.x / cell).floor() as i64);
    let (y0, y1) = ((lo.y / cell).floor() as i64, (hi.y / cell).floor() as i64);
    if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_LAYER_CELLS {
        return None;
    }
    let material = material(models, ink, &l.look, missing);
    let mut meshes = Vec::new();
    'cells: for ky in y0..=y1 {
        for kx in x0..=x1 {
            let r = |n: u64| unit(mix(mix(mix(seed, kx as u64), ky as u64), n));
            if r(0) >= far::FIELD_CHANCE {
                continue;
            }
            let p = (Vec2::new(kx as f32, ky as f32) + Vec2::new(r(1), r(2))) * cell;
            let scale = l.size[0] + (l.size[1] - l.size[0]) * r(3);
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_z(r(4) * std::f32::consts::TAU),
                (target + p - offset).extend(0.0),
            );
            meshes.push(
                MeshInstance::new(handle, transform)
                    .with_material(material)
                    .with_haze(haze),
            );
            if meshes.len() >= MAX_LAYER_MODELS {
                break 'cells;
            }
        }
    }
    Some(Far::Meshes(meshes))
}

/// The part of the play plane in view, relative to the camera's target.
fn in_view(camera: &Camera, logical: Vec2) -> (Vec2, Vec2) {
    let target = camera.target.truncate();
    let corners = [
        Vec2::ZERO,
        Vec2::new(logical.x, 0.0),
        Vec2::new(0.0, logical.y),
        logical,
    ];
    let points: Option<Vec<Vec2>> = corners
        .iter()
        .map(|&c| {
            camera
                .screen_to_plane(c, logical, 0.0)
                .map(|p| p.truncate() - target)
        })
        .collect();
    match points {
        Some(p) => (
            p.iter().copied().fold(Vec2::INFINITY, Vec2::min),
            p.iter().copied().fold(Vec2::NEG_INFINITY, Vec2::max),
        ),
        // Looking at the horizon: the frame's height around the target.
        None => {
            let half = logical * camera.world_per_pixel(logical);
            (-half, half)
        }
    }
}

/// SplitMix64 of `a` and `b`: the same on every platform.
fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Uniform in `[0, 1)`.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_render::style::Style;

    fn sky(text: &str) -> Sky {
        Style::parse(text).unwrap().sky.unwrap()
    }

    fn frame(at: Vec2, still: bool) -> Frame {
        Frame {
            camera: Camera::top_down(at, 400.0).with_tilt(30.0),
            logical: Vec2::new(1280.0, 720.0),
            seed: 7,
            still,
            unit: 400.0 / 720.0,
        }
    }

    fn handles() -> BTreeMap<String, MeshHandle> {
        BTreeMap::from([("m.rock".to_owned(), MeshHandle::DETACHED)])
    }

    #[test]
    fn layers_and_landmarks_draw_far_to_near() {
        let s = sky("[[sky.layers]]\nparallax = 0.3\n\
             [[sky.layers]]\nshape = \"hatch\"\nparallax = 0.0\n\
             [[sky.landmarks]]\nparallax = 0.3\n\
             [[sky.landmarks]]\nshape = \"wash\"\nparallax = 0.1\n");
        let h = handles();
        let m = Models {
            handles: &h,
            looks: &BTreeMap::new(),
            reach: &BTreeMap::new(),
        };
        let (far, missing) = items(&s, &Ink::default(), &m, &frame(Vec2::ZERO, false));
        assert!(missing.is_empty());
        let kinds: Vec<&str> = far
            .iter()
            .map(|f| match f {
                Far::Field(f) if f.mark == Mark::Hatch => "hatch",
                Far::Field(_) => "dust",
                Far::Shape(s) if s.outline == Outline::Wash => "wash",
                Far::Shape(_) => "disc",
                Far::Meshes(_) => "meshes",
            })
            .collect();
        assert_eq!(kinds, ["hatch", "wash", "disc", "dust"]);
    }

    #[test]
    fn parallax_slides_and_reduced_motion_holds_still() {
        let s = sky("[[sky.layers]]\nparallax = 0.25\n\
             [[sky.landmarks]]\nat = [100.0, 40.0]\nparallax = 0.1\n");
        let h = handles();
        let m = Models {
            handles: &h,
            looks: &BTreeMap::new(),
            reach: &BTreeMap::new(),
        };
        let at = |camera: Vec2, still: bool| {
            let (far, _) = items(&s, &Ink::default(), &m, &frame(camera, still));
            let disc = far.iter().find_map(|f| match f {
                Far::Shape(s) => Some(s.center.truncate() - camera),
                _ => None,
            });
            let dust = far.iter().find_map(|f| match f {
                Far::Field(f) => Some(f.offset),
                _ => None,
            });
            (disc.unwrap(), dust.unwrap())
        };
        let (d0, f0) = at(Vec2::ZERO, false);
        let (d1, f1) = at(Vec2::new(1000.0, -500.0), false);
        // The landmark slides back a tenth of the camera's travel on screen.
        assert!(
            (d1 - d0 - Vec2::new(-100.0, 50.0)).length() < 1e-3,
            "{d0} {d1}"
        );
        // The dust's grid moves a quarter as far as the camera.
        assert!((f1 - f0 - Vec2::new(250.0, -125.0)).length() < 1e-3);
        // Reduced motion: nothing moves on screen.
        let (s0, g0) = at(Vec2::ZERO, true);
        let (s1, g1) = at(Vec2::new(1000.0, -500.0), true);
        assert!((s1 - s0).length() < 1e-3 && (g1 - g0).length() < 1e-6);
        // Far things fade into the haze; a nearer one less.
        let (far, _) = items(&s, &Ink::default(), &m, &frame(Vec2::ZERO, false));
        let fades: Vec<f32> = far
            .iter()
            .map(|f| match f {
                Far::Shape(s) => s.haze.a,
                Far::Field(f) => f.haze.a,
                Far::Meshes(_) => 0.0,
            })
            .collect();
        assert!((fades[0] - 0.6 * 0.9).abs() < 1e-5 && (fades[1] - 0.6 * 0.75).abs() < 1e-5);
    }

    #[test]
    fn a_model_layer_is_seeded_bounded_and_wraps_without_end() {
        let s = sky(
            "[[sky.layers]]\nshape = \"model\"\nmodel = \"m.rock\"\ndensity = 1.0\n\
             parallax = 0.5\nsize = [0.5, 2.0]\n\
             [[sky.landmarks]]\nshape = \"model\"\nmodel = \"m.gone\"\n",
        );
        let h = handles();
        let m = Models {
            handles: &h,
            looks: &BTreeMap::new(),
            reach: &BTreeMap::from([("m.rock".to_owned(), 3.0)]),
        };
        let meshes = |camera: Vec2| {
            let (far, missing) = items(&s, &Ink::default(), &m, &frame(camera, false));
            assert_eq!(missing, ["model `m.gone` (the sky wears it)"]);
            match &far[..] {
                [Far::Meshes(m)] => m.clone(),
                other => panic!("{other:?}"),
            }
        };
        let a = meshes(Vec2::ZERO);
        assert_eq!(
            a,
            meshes(Vec2::ZERO),
            "the same seed scatters the same models"
        );
        // About one model each 100 by 100 logical pixels over 1280 by 720, with a
        // margin.
        assert!((60..=140).contains(&a.len()), "{}", a.len());
        // Far away there are still models, all near the camera.
        let far_off = Vec2::new(1.0e5, -3.0e4);
        let b = meshes(far_off);
        assert!(!b.is_empty());
        for inst in &b {
            let p = inst.transform.w_axis.truncate().truncate();
            assert!((p - far_off).length() < 700.0, "{p}");
            assert!(inst.haze.a > 0.0);
        }
    }
}
