//! Views of places. A [`Stage`] lends the variants what they draw with: the resting view
//! of any place (the station from outside, or a room as the current room variant shows
//! it), a set with its people placed anywhere, and the place's titles. A view is a
//! [`Shown`]: the scene, plus the named points the UI attaches to (`person.ilse`,
//! `person.ilse.face`, `place.office_block`, `spot.door`, `socket.nose`), projected
//! through whatever camera the view ends up with.

use render::art_bible::ArtMaterial;
use render::glam::{Mat4, Vec2, Vec3};
use render::kit::{self, CradlePose, HullParams};
use render::{
    ArtBible, Camera, Glow, InkRole, MAX_POINT_LIGHTS, Material, MeshInstance, PointLight, Scene,
};

use crate::life::Life;
use crate::places::{Kind, Place, Places};
use crate::room::Room;
use crate::ui::draw::{Anchor, Anchors};
use crate::world::{self, World};

/// A named point in the scene, world space, and how far the thing reaches round it.
#[derive(Clone, Debug)]
pub struct Point {
    pub name: String,
    pub at: Vec3,
    pub radius: f32,
}

/// A view: what to draw, and the points the UI can attach to.
#[derive(Clone, Debug)]
pub struct Shown {
    pub scene: Scene,
    pub points: Vec<Point>,
    /// A ship in a cradle in this view: its sockets become `socket.*` anchors.
    pub cradle: Option<CradlePose>,
    /// The place this view is of.
    #[allow(dead_code)] // for variants
    pub place: String,
}

impl Shown {
    pub fn new(scene: Scene, place: &str) -> Shown {
        Shown {
            scene,
            points: Vec::new(),
            cradle: None,
            place: place.to_string(),
        }
    }

    #[allow(dead_code)] // for variants
    pub fn point(&self, name: &str) -> Option<&Point> {
        self.points.iter().find(|p| p.name == name)
    }

    /// The points on screen through the scene's camera, in logical pixels.
    pub fn anchors(&self, hull: &HullParams, viewport: Vec2) -> Anchors {
        let cam = &self.scene.camera;
        let fwd = (cam.target - cam.eye).normalize_or(Vec3::NEG_Z);
        let right = fwd.cross(cam.up).normalize_or(Vec3::X);
        let mut out = Anchors::new();
        let in_front = |p: Vec3| (p - cam.eye).dot(fwd) > 0.1;
        for p in &self.points {
            if !in_front(p.at) {
                continue;
            }
            let (Some(a), Some(b)) = (
                cam.world_to_screen(p.at, viewport),
                cam.world_to_screen(p.at + right * p.radius, viewport),
            ) else {
                continue;
            };
            out.insert(
                p.name.clone(),
                Anchor {
                    at: a,
                    radius: a.distance(b).max(4.0),
                },
            );
        }
        if let Some(pose) = &self.cradle {
            for s in kit::cradle_sockets(hull, pose, cam, viewport) {
                if let Some(at) = s.screen {
                    out.insert(
                        format!("socket.{}", world::socket_name(s.socket)),
                        Anchor {
                            at,
                            radius: s.radius_px.max(22.0),
                        },
                    );
                }
            }
        }
        out
    }
}

pub struct Stage<'a> {
    pub world: &'a World,
    pub model: &'a Places,
    pub room: &'a dyn Room,
    pub life: &'a dyn Life,
    pub viewport: Vec2,
    /// Who is speaking now, if anyone: their figure talks.
    pub speaking: Option<&'a str>,
}

/// Light for rooms: a key from the front left and above, so the back wall and the
/// people's faces catch it.
pub fn room_lights(scene: &mut Scene) {
    scene.lights.sun_direction = Vec3::new(-0.45, -0.7, 0.75).normalize();
    scene.lights.ambient = render::Color::hex(0x4a3a58);
}

impl Stage<'_> {
    pub fn aspect(&self) -> f32 {
        self.viewport.x / self.viewport.y.max(1.0)
    }

    pub fn place(&self, id: &str) -> Option<&Place> {
        self.model.place(id)
    }

    /// A place's resting view: the station from outside (whole, or close on a module), or
    /// an interior as the current room variant shows it.
    pub fn view(&self, id: &str) -> Shown {
        match self.place(id) {
            Some(p) if p.kind == Kind::Interior => self.room.view(self, p),
            Some(p) if p.kind == Kind::Module => self.exterior(Some(id), None),
            _ => self.exterior(None, None),
        }
    }

    /// The station from outside, with `focus` (a module) raised, through `camera` or
    /// the station's own framing.
    pub fn exterior(&self, focus: Option<&str>, camera: Option<Camera>) -> Shown {
        let w = self.world;
        let cam = camera.unwrap_or_else(|| w.exterior_camera(focus, self.aspect()));
        let place = focus.unwrap_or(&self.model.station().id).to_string();
        let mut shown = Shown::new(Scene::new(cam), &place);
        let scene = &mut shown.scene;
        scene.backdrop = Some(w.backdrop);
        scene.time = w.time;
        scene.meshes.push(MeshInstance::new(w.station, Mat4::IDENTITY));
        for l in kit::station_lights(&w.recipe) {
            scene.glows.push(Glow {
                position: l.position + Vec3::Z * 2.0,
                radius: l.radius,
                color: l.color.with_alpha(0.55),
            });
            if l.radius > 5.0 && scene.lights.points.len() < MAX_POINT_LIGHTS {
                scene.lights.points.push(PointLight {
                    position: l.position + Vec3::Z * 3.0,
                    color: l.color.scale(0.8),
                    radius: l.radius * 2.5,
                });
            }
        }
        for m in &w.modules {
            let lift = Mat4::from_translation(Vec3::Z * m.lift * world::LIFT);
            let grow = Mat4::from_scale(Vec3::splat(1.0 + (world::LIFT_SCALE - 1.0) * m.lift));
            let mut mat = Material::default();
            if w.hover.as_deref() == Some(m.id.as_str()) || focus == Some(m.id.as_str()) {
                mat = mat.with_outline(render::Color::hex(0xE69F00), 6.0);
            }
            scene
                .meshes
                .push(MeshInstance::new(m.mesh, lift * m.at * grow).with_material(mat));
            let at = m.pos() + Vec3::Z * (3.0 + m.lift * world::LIFT);
            let radius = world::MODULE_REACH * world::MODULE_SCALE * 1.3;
            shown.points.push(Point {
                name: format!("place.{}", m.id),
                at,
                radius,
            });
            // A module's rooms are found at the module, seen from outside.
            for c in self.model.children(&m.id) {
                shown.points.push(Point {
                    name: format!("place.{}", c.id),
                    at,
                    radius,
                });
            }
        }
        shown
    }

    /// An interior's own fixed camera, in set space.
    pub fn set_camera(&self, id: &str) -> Option<Camera> {
        self.world.sets.get(id).map(|s| s.camera())
    }

    /// Draws an interior's set and the people in it into `shown`, through `at` (set space
    /// to world), and adds their points: `person.<id>` over each head, `person.<id>.face`,
    /// `spot.<name>` for the floor spots, and the cradle's sockets.
    pub fn set(&self, id: &str, at: Mat4, shown: &mut Shown) {
        let Some(set) = self.world.sets.get(id) else {
            return;
        };
        let Some(place) = self.place(id) else { return };
        let bible = ArtBible::bundled();
        let scene = &mut shown.scene;
        scene.time = self.world.time;
        scene
            .meshes
            .push(MeshInstance::new(set.mesh, at).with_material(bible.material(InkRole::Station)));
        for l in &set.lights {
            if scene.lights.points.len() < MAX_POINT_LIGHTS {
                scene.lights.points.push(PointLight {
                    position: at.transform_point3(l.position),
                    ..*l
                });
            }
        }
        for g in &set.glows {
            scene.glows.push(Glow {
                position: at.transform_point3(g.position),
                ..*g
            });
        }
        let scale = at.x_axis.truncate().length();
        let people_mat = bible.material(InkRole::Prop);
        for who in self.model.people_in(id) {
            let Some(fig) = self.world.figures.get(&who.id) else {
                continue;
            };
            let talking = self.speaking == Some(who.id.as_str());
            let pose = self
                .life
                .pose(who, self.world.time + fig.phase, talking);
            let feet = self.model.floor_pos(who).extend(0.0);
            let (body, head) = pose.transforms(feet, 0.0, &fig.build);
            let (body, head) = (at * body, at * head);
            scene
                .meshes
                .push(MeshInstance::new(fig.body, body).with_material(people_mat));
            scene
                .meshes
                .push(MeshInstance::new(fig.head, head).with_material(people_mat));
            let top = at.transform_point3(feet + Vec3::Z * (fig.build.top + 0.12 + pose.lift));
            shown.points.push(Point {
                name: format!("person.{}", who.id),
                at: top,
                radius: fig.build.width * 0.45 * scale,
            });
            shown.points.push(Point {
                name: format!("person.{}.face", who.id),
                at: head.transform_point3(Vec3::Z * fig.build.face),
                radius: 0.16 * scale,
            });
        }
        for (mesh, m) in self.life.props(self.model, id) {
            scene
                .meshes
                .push(MeshInstance::new(mesh, at * m).with_material(people_mat));
        }
        for (name, p) in &place.spots {
            shown.points.push(Point {
                name: format!("spot.{name}"),
                at: at.transform_point3(p.extend(1.0)),
                radius: 0.5 * scale,
            });
        }
        if let Some((center, span)) = set.ship {
            let local = self.world.cradle_pose(center, span);
            let (s, rot, t) = at.to_scale_rotation_translation();
            let yaw = (rot * Vec3::X).truncate().to_angle();
            let pose = CradlePose {
                center: at.transform_point3(local.center.extend(0.0)).truncate(),
                scale: local.scale * s.x,
                heading: local.heading + yaw,
                lift: local.lift * s.x + t.z,
            };
            self.world.cradle(pose).draw(scene, self.viewport);
            shown.cradle = Some(pose);
        }
    }

    /// A place's name, and where it is: "Dock office", "Office block · Lantern Wharf".
    pub fn title(&self, id: &str) -> (String, String) {
        let Some(p) = self.place(id) else {
            return (id.to_string(), String::new());
        };
        let mut trail = Vec::new();
        let mut up = p.parent.as_deref().and_then(|u| self.place(u));
        while let Some(u) = up {
            trail.push(u.name.clone());
            up = u.parent.as_deref().and_then(|x| self.place(x));
        }
        (p.name.clone(), trail.join(" · "))
    }
}
