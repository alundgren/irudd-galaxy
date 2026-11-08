//! Exact inputs to the pixel focus test. No world tick, talk-line heuristic or hash:
//! reuse is possible only when every value the visibility test reads is identical.
use std::{cell::RefCell, ops::Range};

use galaxy_render::{Camera, Projection, Rect, glam::Vec2};

use super::Geometry;
use crate::screens::{Boxed, Screens};

#[derive(Default)]
pub(crate) struct FocusCache {
    last: RefCell<Option<(Key, Result<(), String>)>>,
    /// The screen-independent half of recent tests, so a test that differs only in
    /// its screens (another talk line's bubble, say) casts no ray twice.
    scans: RefCell<Vec<(Rays, FocusScan)>>,
    #[cfg(test)]
    evaluations: std::cell::Cell<u64>,
}

impl FocusCache {
    /// Poses whose rays are kept.
    const KEEP: usize = 8;

    #[cfg(test)]
    pub(super) fn evaluations(&self) -> u64 {
        self.evaluations.get()
    }

    /// The verdict for `key`: the last one when every input is the same, else
    /// `evaluate` with the scan these poses left, which it carries on.
    pub(super) fn check(
        &self,
        key: Key,
        evaluate: impl FnOnce(&mut FocusScan) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some((previous, result)) = self.last.borrow().as_ref()
            && previous == &key
        {
            return result.clone();
        }
        #[cfg(test)]
        self.evaluations.set(self.evaluations.get() + 1);
        let mut scans = self.scans.borrow_mut();
        let mut scan = match scans.iter().position(|(rays, _)| *rays == key.rays) {
            Some(i) => scans.remove(i).1,
            None => FocusScan::default(),
        };
        let result = evaluate(&mut scan);
        if scans.len() >= Self::KEEP {
            scans.remove(0);
        }
        scans.push((key.rays.clone(), scan));
        self.last.replace(Some((key, result.clone())));
        result
    }
}

/// How far the pixel test got for one speaker part and set of poses, in pixel order:
/// whether the part is on screen, and each pixel that shows it with the first mesh
/// in front of it. Only the widget test is left to run against the screens.
#[derive(Default)]
pub(super) struct FocusScan {
    /// The part's screen bounds, or why it is off screen; `None` before the first test.
    pub(super) bounds: Option<Result<(Vec2, Vec2), String>>,
    /// The speaker's mesh cut to the part.
    pub(super) mesh: Option<galaxy_mesh::MeshData>,
    /// Pixels that show the part, and the id of the mesh covering each, if any.
    pub(super) pixels: Vec<(Vec2, Option<String>)>,
    /// The next pixel of the bounds, row by row, not tested yet.
    pub(super) next: usize,
}

#[derive(PartialEq, Eq)]
pub(super) struct Key {
    rays: Rays,
    screens: Vec<Surface>,
}

/// Everything the test reads but the screens.
#[derive(PartialEq, Eq, Clone)]
pub(super) struct Rays {
    speaker: String,
    part: String,
    vertices: Range<usize>,
    posed: Posed,
}

/// Exactly what a ray cast from the screen reads: the camera, the frame size and every
/// posed mesh. Equal values give equal hits for every pixel.
#[derive(PartialEq, Eq, Clone)]
pub(crate) struct Posed {
    camera: [u32; 13],
    size: [u32; 2],
    geometry: Vec<Mesh>,
}

#[derive(PartialEq, Eq, Clone)]
struct Mesh {
    id: String,
    positions: Vec<[u32; 3]>,
    indices: Vec<u32>,
    transform: [u32; 16],
    inverse: [u32; 16],
}

/// Exactly the fields `Screens::hit` reads, preserving parent/child and paint order.
/// In particular, a bubble maps its non-button children's hits back to the bubble.
#[derive(PartialEq, Eq)]
struct Surface {
    kind: String,
    path: String,
    rect: [u32; 4],
    clip: Option<[u32; 4]>,
    hidden: bool,
    children: Vec<Surface>,
}

impl Surface {
    fn of(b: &Boxed) -> Self {
        let rect = |r: Rect| [r.x, r.y, r.w, r.h].map(f32::to_bits);
        Self {
            kind: b.widget.kind.clone(),
            path: b.widget.path.clone(),
            rect: rect(b.rect),
            clip: b.clip.map(rect),
            hidden: b.hidden,
            children: b.children.iter().map(Self::of).collect(),
        }
    }
}

impl Key {
    pub(super) fn new(
        speaker: &str,
        part: &str,
        vertices: Range<usize>,
        camera: Camera,
        size: Vec2,
        geometry: &[Geometry],
        screens: &Screens,
    ) -> Self {
        Self {
            rays: Rays {
                speaker: speaker.into(),
                part: part.into(),
                vertices,
                posed: Posed::new(camera, size, geometry),
            },
            screens: screens.boxes.iter().map(Surface::of).collect(),
        }
    }
}

impl Posed {
    pub(crate) fn new(camera: Camera, size: Vec2, geometry: &[Geometry]) -> Self {
        let projection = match camera.projection {
            Projection::Orthographic {
                view_height,
                near,
                far,
            } => [0, view_height.to_bits(), near.to_bits(), far.to_bits()],
            Projection::Perspective { fov_y, near, far } => {
                [1, fov_y.to_bits(), near.to_bits(), far.to_bits()]
            }
        };
        let mut camera_bits = [0; 13];
        for (dest, value) in camera_bits[..9].iter_mut().zip(
            camera
                .eye
                .to_array()
                .into_iter()
                .chain(camera.target.to_array())
                .chain(camera.up.to_array()),
        ) {
            *dest = value.to_bits();
        }
        camera_bits[9..].copy_from_slice(&projection);
        Self {
            camera: camera_bits,
            size: size.to_array().map(f32::to_bits),
            geometry: geometry
                .iter()
                .map(|g| Mesh {
                    id: g.id.clone(),
                    positions: g
                        .mesh
                        .vertices
                        .iter()
                        .map(|v| v.position.map(f32::to_bits))
                        .collect(),
                    indices: g.mesh.indices.clone(),
                    transform: g.transform.to_cols_array().map(f32::to_bits),
                    inverse: g.inverse.to_cols_array().map(f32::to_bits),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::View;
    use galaxy_api::{
        command::{Batch, Cause, Command},
        engine::*,
    };
    use galaxy_module::module::Module;
    use galaxy_sim::{Session, World};
    use std::path::Path;

    fn fixture() -> (Session, View, Camera) {
        let module =
            Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/talk/demo"))
                .unwrap();
        let mut session = Session::new(vec![module], 1).unwrap();
        session.step(&[]).unwrap();
        session
            .world
            .apply(
                &Batch {
                    cause: Cause::Engine {
                        service: "test".into(),
                    },
                    commands: vec![Command::Move {
                        entity: "talk.player".into(),
                        place: "talk.room_b".into(),
                        spot: None,
                        state: None,
                    }],
                },
                &session.registry().clone(),
            )
            .unwrap();
        session
            .world
            .entities
            .get_mut("talk.player")
            .unwrap()
            .get_mut(TRAVEL)
            .unwrap()
            .fields
            .insert("active".into(), false.into());
        let talk = &mut session
            .world
            .entities
            .get_mut("talk.third_talk")
            .unwrap()
            .get_mut(TALK)
            .unwrap()
            .fields;
        talk.insert("active".into(), true.into());
        talk.insert("current".into(), "talk.third_1".into());
        galaxy_sim::services::ground(&mut session.world);
        let mut view = View::detached(session.modules(), 1);
        view.before_step(&session.world);
        let camera = view
            .scene(&session.world, 1.0, Vec2::new(1280.0, 720.0), 0.0)
            .camera;
        assert!(
            view.live_focus(&session.world, 1.0, Vec2::new(1280.0, 720.0), camera)
                .is_empty()
        );
        (session, view, camera)
    }

    /// Compare the cached observation with the unchanged pixel test, forced to run
    /// afresh. Also ensure a repeated identical observation actually reuses its result.
    fn checked(view: &View, world: &World, camera: Camera, alpha: f32, size: Vec2) -> String {
        let cached = view.live_focus(world, alpha, size, camera);
        let evaluations = view.focus_cache.evaluations.get();
        assert_eq!(cached, view.live_focus(world, alpha, size, camera));
        assert_eq!(evaluations, view.focus_cache.evaluations.get());
        view.focus_cache.last.borrow_mut().take();
        view.focus_cache.scans.borrow_mut().clear();
        let fresh = view.live_focus(world, alpha, size, camera);
        assert_eq!(cached, fresh, "cached verdict differs from the pixel test");
        cached.join("\n")
    }

    #[test]
    fn geometry_camera_morph_and_asset_changes_never_reuse_a_stale_verdict() {
        let (mut s, mut v, camera) = fixture();
        let size = Vec2::new(1280.0, 720.0);
        let original = s.world.clone();
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
        let model = |w: &mut World, value: &str| {
            w.entities
                .get_mut("talk.third")
                .unwrap()
                .get_mut(MODEL)
                .unwrap()
                .fields
                .insert("model".into(), value.into());
        };
        model(&mut s.world, "talk.covered");
        assert!(checked(&v, &s.world, camera, 1.0, size).contains("covered by geometry"));
        model(&mut s.world, "talk.being");
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());

        let mut prop = s.world.entities["talk.third"].clone();
        prop.remove(BEING);
        prop.get_mut(MODEL)
            .unwrap()
            .fields
            .insert("model".into(), "talk.covered".into());
        s.world.entities.insert("talk.prop".into(), prop);
        assert!(checked(&v, &s.world, camera, 1.0, size).contains("geometry of `talk.prop`"));
        s.world.entities.remove("talk.prop");
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());

        v.before_step(&s.world);
        s.world
            .entities
            .get_mut("talk.third")
            .unwrap()
            .get_mut(POSITION)
            .unwrap()
            .fields
            .insert("x".into(), 1000.into());
        assert!(checked(&v, &s.world, camera, 1.0, size).contains("off screen"));
        assert!(checked(&v, &s.world, camera, 0.5, size).contains("off screen"));
        assert!(checked(&v, &s.world, camera, 0.0, size).is_empty());
        s.world = original.clone();
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
        let mut moved = camera;
        moved.target.x += 1000.0;
        assert!(checked(&v, &s.world, moved, 1.0, size).contains("off screen"));
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
        checked(&v, &s.world, camera, 1.0, Vec2::new(800.0, 600.0));
        checked(&v, &s.world, camera, 1.0, size);

        let mesh = v.assets.models["talk.being"].clone();
        for vertex in &mut v.assets.models.get_mut("talk.being").unwrap().vertices {
            vertex.morph_position[0] += 1000.0;
        }
        v.before_step(&s.world);
        s.world
            .entities
            .get_mut("talk.third")
            .unwrap()
            .get_mut(MODEL)
            .unwrap()
            .fields
            .insert("morph".into(), 1.into());
        assert!(checked(&v, &s.world, camera, 1.0, size).contains("off screen"));
        assert!(checked(&v, &s.world, camera, 0.5, size).contains("off screen"));
        assert!(checked(&v, &s.world, camera, 0.0, size).is_empty());
        v.assets.models.insert("talk.being".into(), mesh.clone());
        s.world = original;
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
        v.assets
            .models
            .get_mut("talk.being")
            .unwrap()
            .indices
            .clear();
        assert!(
            checked(&v, &s.world, camera, 1.0, size).contains("no visible front-facing pixels")
        );
        v.assets.models.insert("talk.being".into(), mesh);
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
        let parts = v.assets.parts["talk.being"].clone();
        let face = v
            .assets
            .parts
            .get_mut("talk.being")
            .unwrap()
            .iter_mut()
            .find(|p| p.name == "face")
            .unwrap();
        face.vertices = face.vertices.start..face.vertices.start;
        assert!(
            checked(&v, &s.world, camera, 1.0, size).contains("no visible front-facing pixels")
        );
        v.assets.parts.insert("talk.being".into(), parts);
        assert!(checked(&v, &s.world, camera, 1.0, size).is_empty());
    }

    #[test]
    fn screen_clipping_visibility_hierarchy_and_paint_order_change_the_verdict() {
        let (s, mut v, camera) = fixture();
        let size = Vec2::new(1280.0, 720.0);
        let check = |v: &View| checked(v, &s.world, camera, 1.0, size);
        assert!(check(&v).is_empty());
        let original = v.screens.boxes.clone();
        let mut cover = v.screens.boxes[0].clone();
        cover.children.clear();
        cover.hidden = false;
        cover.clip = None;
        cover.rect = Rect::new(0.0, 0.0, size.x, size.y);
        cover.widget.kind = "panel".into();
        cover.widget.path = "test/cover".into();
        v.screens.boxes.push(cover.clone());
        assert!(check(&v).contains("covered by test/cover"));
        v.screens.boxes.last_mut().unwrap().widget.path = "test/renamed".into();
        assert!(check(&v).contains("covered by test/renamed"));
        v.screens.boxes.last_mut().unwrap().hidden = true;
        assert!(check(&v).is_empty());
        v.screens.boxes.last_mut().unwrap().hidden = false;
        v.screens.boxes.last_mut().unwrap().clip = Some(Rect::new(-100.0, -100.0, 1.0, 1.0));
        assert!(check(&v).is_empty());
        v.screens.boxes.last_mut().unwrap().clip = None;
        v.screens.boxes.last_mut().unwrap().rect.x = 2000.0;
        assert!(check(&v).is_empty());
        *v.screens.boxes.last_mut().unwrap() = cover.clone();
        v.screens.boxes.last_mut().unwrap().widget.kind = "column".into();
        assert!(check(&v).is_empty());
        v.screens
            .boxes
            .last_mut()
            .unwrap()
            .children
            .push(cover.clone());
        assert!(check(&v).contains("covered by test/cover"));
        v.screens.boxes.last_mut().unwrap().widget.kind = "bubble".into();
        v.screens.boxes.last_mut().unwrap().widget.path = "test/bubble".into();
        assert!(check(&v).contains("covered by test/bubble"));
        v.screens.boxes = original.clone();
        assert!(check(&v).is_empty());
        v.screens.boxes.push(cover.clone());
        cover.widget.path = "test/top".into();
        v.screens.boxes.push(cover);
        assert!(check(&v).contains("covered by test/top"));
        let len = v.screens.boxes.len();
        v.screens.boxes.swap(len - 1, len - 2);
        assert!(check(&v).contains("covered by test/cover"));
        v.screens.boxes = original;
        assert!(check(&v).is_empty());
    }
}
