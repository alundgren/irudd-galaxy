//! Fitted parts on a hull, and the cradle pose the hullwright shows a ship in.
//!
//! Each [`LookId`] is a piece built at its socket on the hull ([`HullParams::socket`]),
//! sized from the hull, in the same low-poly, ink-outlined style. The pieces are chunky
//! on purpose: at the flight zoom a Wren is about forty pixels long, and a glance at it
//! should still tell you its fit.
//!
//! - [`fitted_hull`]: one mesh, the hull wearing its parts. Build it once per look and
//!   cache it; nothing here is per frame.
//! - [`part`]: one part alone, in hull space; [`part_ghost`] paints it as a preview
//!   for the cradle.
//! - [`Cradle`]: the ship drawn large from above and lifted over its shadow, with its
//!   four sockets on screen and a ring on each.

use super::ship::{hull_into, signed_area};
use crate::art_bible::{ArtBible, ArtMaterial, InkRole};
use crate::camera::Camera;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use crate::palette;
use crate::scene::{Glow, Line, Material, MeshHandle, MeshInstance, Scene};
pub use galaxy_kit::ship::{HullParams, LookId, ShipFitLook, Socket, long_drill_length};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::f32::consts::FRAC_PI_2;

/// The drop-target accent from the station concept (#56B4E9).
pub const SKY_BLUE: Color = Color::linear(0.0931, 0.4564, 0.8148, 1.0);
/// A fitted socket's ring (#E7B15A).
pub const FITTED_GOLD: Color = Color::linear(0.7991, 0.4397, 0.1022, 1.0);
/// A ghost part's paint (#BFE4F7).
const GHOST_PAINT: Color = Color::linear(0.521, 0.7758, 0.9301, 1.0);
/// The cradle's default shadow, a deep warm violet that reads on planks or paper
/// (#3D2A3A). Meshes are opaque, so a room on another floor sets its own.
pub const CRADLE_SHADOW: Color = Color::linear(0.0467, 0.0232, 0.0423, 1.0);
/// The seam lamp's cool light (#9EF2DC), so it never reads as an engine.
pub const SEAM_LIGHT: Color = Color::linear(0.3419, 0.8879, 0.7157, 1.0);

/// A hull wearing the parts in `look`, as one mesh. With nothing fitted it is exactly
/// [`super::hull`].
pub fn fitted_hull(params: &HullParams, seed: u64, look: &ShipFitLook) -> MeshData {
    let p = params.sanitized();
    let mut b = MeshBuilder::new();
    hull_into(&mut b, &p, seed);
    for (socket, id) in look.pairs() {
        part_into(&mut b, &p, socket, id);
    }
    b.build()
}

/// One part alone, where it sits on the hull (hull space), on the socket it is built
/// for. Draw it with the hull's transform for a preview.
pub fn part(look: LookId, params: &HullParams) -> MeshData {
    let p = params.sanitized();
    let mut b = MeshBuilder::new();
    part_into(&mut b, &p, look.socket(), look);
    b.build()
}

/// [`part`] painted all over in pale sky blue: a part shown before it's fitted. Draw
/// it with [`ghost_material`].
pub fn part_ghost(look: LookId, params: &HullParams) -> MeshData {
    let mut m = part(look, params);
    let pale = GHOST_PAINT.to_array();
    let paint = [pale[0], pale[1], pale[2], 0.15];
    for v in &mut m.vertices {
        v.color = paint;
        v.morph_color = paint;
    }
    m
}

/// Where a seam lamp's light sits in hull space, for a glow over it.
pub fn seam_lamp_light(params: &HullParams) -> Vec3 {
    let p = params.sanitized();
    p.socket(Socket::Utility) + Vec3::Z * seam_lamp_post(&p)
}

fn seam_lamp_post(p: &HullParams) -> f32 {
    (p.height * 0.5).max(0.7)
}

/// Builds `look` at `socket` on a sanitized hull. A part on another socket than its own
/// is built there anyway, so data can move parts without a code change.
fn part_into(b: &mut MeshBuilder, p: &HullParams, socket: Socket, look: LookId) {
    let at = Mat4::from_translation(p.socket(socket));
    b.with(at, |b| match look {
        LookId::Lens => lens(b, p),
        LookId::LongDrill => long_drill(b, p),
        LookId::Pods => pods(b, p),
        LookId::Fins => fins(b, p),
        LookId::SeamLamp => seam_lamp(b, p),
    });
}

/// A lathe whose axis runs along +X.
fn along_x() -> Mat4 {
    Mat4::from_rotation_y(FRAC_PI_2)
}

/// A glass lens lying flat in a brass ring, held ahead of the nose on two struts. From
/// above it is a bright round eye on the ship's nose.
fn lens(b: &mut MeshBuilder, p: &HullParams) {
    let r = (p.width * 0.3).max(0.6);
    let ahead = r * 0.75;
    b.paint(palette::BRASS, 0.0);
    for side in [1.0_f32, -1.0] {
        b.with(
            Mat4::from_translation(Vec3::new(ahead * 0.35, side * r * 0.55, 0.0)),
            |b| b.cuboid(Vec3::new(ahead * 1.1, r * 0.22, r * 0.22)),
        );
    }
    b.with(
        Mat4::from_translation(Vec3::new(ahead + r * 0.2, 0.0, 0.0)),
        |b| {
            b.paint(palette::BRASS, 0.0);
            b.torus(r, r * 0.34, r * 0.4, 12, 4);
            b.paint(palette::GLASS, 0.45);
            b.lathe(
                &[
                    (r * 0.86, -r * 0.14),
                    (r * 0.8, r * 0.1),
                    (r * 0.45, r * 0.26),
                    (0.0, r * 0.3),
                ],
                10,
                0.0,
            );
            // The glint: a bright bead up and left, as the concept draws it.
            b.paint(palette::CREAM, 1.0);
            b.with(
                Mat4::from_translation(Vec3::new(r * 0.25, r * 0.3, r * 0.26)),
                |b| b.lathe(&[(r * 0.16, 0.0), (0.0, r * 0.12)], 5, 0.0),
            );
        },
    );
}

/// A long drill out of the nose: a brass collar, a banded shaft and a bright bit.
fn long_drill(b: &mut MeshBuilder, p: &HullParams) {
    let len = long_drill_length(p);
    let r = (p.width * 0.17).clamp(0.3, 1.2);
    b.with(
        Mat4::from_translation(Vec3::X * -r * 0.6) * along_x(),
        |b| {
            b.paint(palette::BRASS, 0.0);
            b.lathe(
                &[(r * 1.5, 0.0), (r * 1.55, r * 0.5), (r * 1.2, r * 1.1)],
                8,
                0.0,
            );
            // The shaft in bands, so it reads as turning even when still.
            let bands = 4;
            let shaft = len * 0.72 - r * 1.1;
            for i in 0..bands {
                let z0 = r * 1.1 + shaft * i as f32 / bands as f32;
                let z1 = r * 1.1 + shaft * (i + 1) as f32 / bands as f32;
                let taper = |z: f32| r * (1.0 - 0.35 * (z / len));
                b.paint(
                    if i % 2 == 0 {
                        palette::TEAL
                    } else {
                        palette::HULL_CREAM
                    },
                    0.0,
                );
                b.lathe(&[(taper(z0), z0), (taper(z1), z1)], 6, 0.3 * i as f32);
            }
            let base = r * 1.1 + shaft;
            b.paint(palette::MUSTARD, 0.35);
            b.lathe(
                &[
                    (r * 0.85, base),
                    (r * 0.6, base + len * 0.16),
                    (0.0, base + len * 0.28),
                ],
                6,
                0.0,
            );
        },
    );
}

/// Two hold pods hanging off the sides on short struts, banded in brass.
fn pods(b: &mut MeshBuilder, p: &HullParams) {
    let r = (p.width * 0.17).max(0.4);
    let len = (p.length * 0.36).max(r * 3.0);
    let out = p.width * 0.5 + r * 0.85;
    let z = p.height * 0.08;
    let capsule = [
        (0.0, -len * 0.5),
        (r * 0.72, -len * 0.5 + r * 0.32),
        (r, -len * 0.5 + r),
        (r, len * 0.5 - r),
        (r * 0.72, len * 0.5 - r * 0.32),
        (0.0, len * 0.5),
    ];
    for side in [1.0_f32, -1.0] {
        b.with(Mat4::from_translation(Vec3::new(0.0, side * out, z)), |b| {
            b.paint(palette::TEAL, 0.0);
            b.with(along_x(), |b| b.lathe(&capsule, 8, 0.0));
            b.paint(palette::BRASS, 0.0);
            for x in [-len * 0.22, len * 0.22] {
                b.with(Mat4::from_translation(Vec3::X * x) * along_x(), |b| {
                    b.lathe(&[(r * 1.08, -r * 0.16), (r * 1.08, r * 0.16)], 8, 0.0)
                });
            }
            // A strut back to the hull.
            b.with(
                Mat4::from_translation(Vec3::new(0.0, -side * r * 0.9, -z * 0.5)),
                |b| b.cuboid(Vec3::new(len * 0.3, r * 0.9, r * 0.45)),
            );
            // A lamp on the pod's nose, so a pair of pods reads at a glance.
            b.paint(palette::WINDOW, 0.9);
            b.with(
                Mat4::from_translation(Vec3::new(len * 0.3, 0.0, r * 0.82)),
                |b| b.cuboid(Vec3::new(r * 0.5, r * 0.5, r * 0.3)),
            );
        });
    }
}

/// Swift fins: a pair of swept blades behind the wings, flaring out from the tail.
fn fins(b: &mut MeshBuilder, p: &HullParams) {
    let (l, w) = (p.length, p.width);
    let reach = (p.wing_span * 0.5).max(w * 0.95);
    let mut blade = vec![
        Vec2::new(l * 0.14, w * 0.2),
        Vec2::new(-l * 0.12, reach * 0.98),
        Vec2::new(-l * 0.25, reach * 0.9),
        Vec2::new(-l * 0.06, w * 0.16),
    ];
    if signed_area(&blade) < 0.0 {
        blade.reverse();
    }
    let thick = (p.height * 0.14).max(0.18);
    for side in [1.0_f32, -1.0] {
        b.with(
            Mat4::from_scale(Vec3::new(1.0, side, 1.0))
                * Mat4::from_translation(Vec3::Z * p.height * 0.1)
                * Mat4::from_rotation_x(0.12),
            |b| {
                b.paint(palette::TEAL, 0.0);
                b.extrude(&blade, thick, 0.86);
                // A cream tip, so the sweep shows against a dark sky.
                b.paint(palette::HULL_CREAM, 0.0);
                b.with(
                    Mat4::from_translation(Vec3::new(-l * 0.19, reach * 0.92, thick * 0.6)),
                    |b| b.cuboid(Vec3::new(l * 0.1, reach * 0.12, thick * 0.6)),
                );
            },
        );
    }
}

/// A small lamp on a brass post, lit in the seam lamp's cool green.
fn seam_lamp(b: &mut MeshBuilder, p: &HullParams) {
    let post = seam_lamp_post(p);
    let r = (p.width * 0.24).clamp(0.6, 1.4);
    b.paint(palette::BRASS, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * -post * 0.3), |b| {
        b.lathe(&[(r * 0.75, 0.0), (r * 0.6, r * 0.3)], 6, 0.0);
        b.frustum(r * 0.24, r * 0.2, post * 1.3 - r * 0.5, 5);
    });
    b.with(Mat4::from_translation(Vec3::Z * post), |b| {
        // A lit globe under a small cap, so from above it is a bright bead.
        b.paint(SEAM_LIGHT, 1.0);
        b.lathe(
            &[
                (0.0, -r * 0.7),
                (r, -r * 0.1),
                (r * 0.8, r * 0.4),
                (r * 0.35, r * 0.6),
            ],
            7,
            0.0,
        );
        b.paint(palette::BRASS, 0.0);
        b.lathe(&[(r * 0.4, r * 0.55), (0.0, r * 0.8)], 6, 0.0);
    });
}

/// How a socket's ring is drawn in the cradle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SocketRing {
    /// No ring.
    None,
    /// A thin dashed ink ring: a socket, nothing more.
    #[default]
    Empty,
    /// A dashed sky-blue ring: the dragged part can go here.
    DropTarget,
    /// A solid gold ring: something is fitted here.
    Fitted,
}

/// How the cradle holds a ship: where, how large, which way and how high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CradlePose {
    /// The world point under the hull's centre.
    pub center: Vec2,
    /// World units per mesh unit.
    pub scale: f32,
    /// The nose's direction, radians from +X (π/2 points it up the screen).
    pub heading: f32,
    /// How high the hull is lifted, world units. The shadow falls this far off.
    pub lift: f32,
}

impl CradlePose {
    /// Nose up the screen, as long as `span` world units, lifted a little.
    pub fn fit(center: Vec2, span: f32, hull: &HullParams) -> Self {
        let p = hull.sanitized();
        let scale = span.max(0.0) / p.length;
        Self {
            center,
            scale,
            heading: FRAC_PI_2,
            lift: p.height * scale,
        }
    }

    /// Hull space to world.
    pub fn transform(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            Quat::from_rotation_z(self.heading),
            self.center.extend(self.lift),
        )
    }

    /// Where the shadow is drawn: on the plane, off the hull towards the bottom right,
    /// flattened.
    pub fn shadow_transform(&self) -> Mat4 {
        let off = Vec2::new(0.35, -0.45) * self.lift;
        Mat4::from_translation((self.center + off).extend(0.0))
            * Mat4::from_rotation_z(self.heading)
            * Mat4::from_scale(Vec3::new(self.scale, self.scale, self.scale * 0.04))
    }
}

/// A socket in the cradle, placed on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CradleSocket {
    pub socket: Socket,
    /// The ring's centre, world space: in front of the hull on the line of sight, so
    /// the ring draws on top.
    pub world: Vec3,
    /// The ring's centre in pixels, if on screen.
    pub screen: Option<Vec2>,
    /// The ring's radius, world units.
    pub radius: f32,
    /// The ring's radius in pixels: a drop or hover test is `distance <= radius_px`.
    pub radius_px: f32,
}

/// The ship in the cradle: draw it with [`Cradle::draw`], hit-test its sockets with
/// [`Cradle::sockets`].
#[derive(Clone, Debug)]
pub struct Cradle {
    /// The hull mesh, as fitted ([`fitted_hull`]).
    pub ship: MeshHandle,
    pub hull: HullParams,
    /// What the hull wears: lights the seam lamp.
    pub look: ShipFitLook,
    pub pose: CradlePose,
    /// A ring per socket, in [`Socket::ALL`] order.
    pub rings: [SocketRing; 4],
    /// A part drawn as a sky-blue ghost where it would go ([`part_ghost`]).
    pub ghost: Option<MeshHandle>,
    /// The shadow's colour, for the floor under the cradle ([`CRADLE_SHADOW`]).
    pub shadow: Color,
}

impl Cradle {
    /// How big each socket's ring is, as a share of the hull's width.
    fn ring_share(socket: Socket) -> f32 {
        match socket {
            Socket::Nose => 0.42,
            Socket::Utility => 0.14,
            Socket::Belly => 0.2,
            Socket::Tail => 0.18,
        }
    }

    /// Where a socket's ring is centred, hull space: on the socket, except the drill
    /// mount's, which circles the part held ahead of the nose.
    fn ring_center(p: &HullParams, socket: Socket) -> Vec3 {
        let at = p.socket(socket);
        match socket {
            Socket::Nose => at + Vec3::X * p.width * 0.29,
            _ => at,
        }
    }

    /// The four sockets on screen, in [`Socket::ALL`] order. Headless: no renderer.
    pub fn sockets(&self, camera: &Camera, viewport: Vec2) -> [CradleSocket; 4] {
        cradle_sockets(&self.hull, &self.pose, camera, viewport)
    }

    /// Draws the shadow, the ship, a ghost part and the socket rings into `scene`, and
    /// returns the sockets on screen.
    pub fn draw(&self, scene: &mut Scene, viewport: Vec2) -> [CradleSocket; 4] {
        let shadow = Material {
            tint: self.shadow,
            shadow: Color::WHITE,
            highlight: Color::WHITE,
            bands: 2,
            outline_px: 0.0,
            hatch: 0.0,
            ..Material::default()
        };
        scene
            .meshes
            .push(MeshInstance::new(self.ship, self.pose.shadow_transform()).with_material(shadow));
        let ship = ArtBible::bundled().material(InkRole::Ship);
        scene
            .meshes
            .push(MeshInstance::new(self.ship, self.pose.transform()).with_material(ship));
        if let Some(ghost) = self.ghost {
            scene.meshes.push(
                MeshInstance::new(ghost, self.pose.transform()).with_material(ghost_material()),
            );
        }
        if self.look.utility == Some(LookId::SeamLamp) {
            scene.glows.push(Glow {
                position: self
                    .pose
                    .transform()
                    .transform_point3(seam_lamp_light(&self.hull)),
                radius: self.hull.width * self.pose.scale * 0.45,
                color: SEAM_LIGHT.with_alpha(0.6),
            });
        }
        let sockets = self.sockets(&scene.camera, viewport);
        let px = scene.camera.world_per_pixel(viewport);
        for (s, ring) in sockets.iter().zip(self.rings) {
            let line = match ring {
                SocketRing::None => continue,
                SocketRing::Empty => Line::circle(s.world, s.radius, px * 4.0, palette::INK)
                    .dashed(px * 8.0, px * 6.0),
                SocketRing::DropTarget => {
                    // White under the sky blue, so the dashes read over any paint.
                    scene
                        .lines
                        .push(Line::circle(s.world, s.radius, px * 9.0, Color::WHITE));
                    Line::circle(s.world, s.radius, px * 5.0, SKY_BLUE).dashed(px * 12.0, px * 7.0)
                }
                SocketRing::Fitted => {
                    // Inked edges, so the gold stands off brass parts.
                    scene
                        .lines
                        .push(Line::circle(s.world, s.radius, px * 10.0, palette::INK));
                    Line::circle(s.world, s.radius, px * 6.0, FITTED_GOLD)
                }
            };
            scene.lines.push(line);
        }
        sockets
    }
}

/// A ship's four sockets on screen in the cradle `pose`, in [`Socket::ALL`] order:
/// [`Cradle::sockets`] without the meshes, so hit tests run with no renderer.
pub fn cradle_sockets(
    hull: &HullParams,
    pose: &CradlePose,
    camera: &Camera,
    viewport: Vec2,
) -> [CradleSocket; 4] {
    let p = hull.sanitized();
    let m = pose.transform();
    // Rings are slid towards the camera along the line of sight: on the same pixel
    // as the socket, but in front of the hull, so the depth test never buries them.
    let toward = (camera.eye - camera.target).normalize_or(Vec3::Z);
    let clear = (p.height + p.width) * pose.scale;
    let px = camera.world_per_pixel(viewport).max(f32::EPSILON);
    Socket::ALL.map(|socket| {
        let world = m.transform_point3(Cradle::ring_center(&p, socket)) + toward * clear;
        let radius = Cradle::ring_share(socket) * p.width * pose.scale;
        CradleSocket {
            socket,
            world,
            screen: camera.world_to_screen(world, viewport),
            radius,
            radius_px: radius / px,
        }
    })
}

/// A ghost part ([`part_ghost`]): flat, a little lit, inked in sky blue.
pub fn ghost_material() -> Material {
    Material {
        emissive: SKY_BLUE.scale(0.15),
        hatch: 0.0,
        bands: 2,
        ..Material::default()
    }
    .with_outline(SKY_BLUE, 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::hull;

    fn every_look() -> ShipFitLook {
        ShipFitLook::from_pairs(LookId::ALL.map(|l| (l.socket(), l)))
    }

    #[test]
    fn accents_match_their_hex() {
        for (c, hex) in [
            (SKY_BLUE, 0x56b4e9),
            (FITTED_GOLD, 0xe7b15a),
            (SEAM_LIGHT, 0x9ef2dc),
            (GHOST_PAINT, 0xbfe4f7),
            (CRADLE_SHADOW, 0x3d2a3a),
        ] {
            assert_eq!(c.to_srgba8(), Color::hex(hex).to_srgba8());
        }
    }

    #[test]
    fn an_empty_fit_is_the_bare_hull() {
        for p in [
            HullParams::wren(),
            HullParams::tandem(),
            HullParams::heron(),
        ] {
            assert_eq!(fitted_hull(&p, 5, &ShipFitLook::default()), hull(&p, 5));
        }
    }

    #[test]
    fn every_part_builds_on_every_hull() {
        for p in [
            HullParams::wren(),
            HullParams::tandem(),
            HullParams::heron(),
        ] {
            let bare = hull(&p, 1);
            for look in LookId::ALL {
                let m = part(look, &p);
                assert!(m.is_well_formed(), "{look:?} on {:?}", p.class);
                assert!(m.triangle_count() > 10, "{look:?}");
                let fit = ShipFitLook::default().with(look.socket(), look);
                let worn = fitted_hull(&p, 1, &fit);
                assert!(worn.is_well_formed());
                assert_eq!(
                    worn.vertices.len(),
                    bare.vertices.len() + m.vertices.len(),
                    "{look:?}: the hull plus the part"
                );
            }
            assert!(fitted_hull(&p, 1, &every_look()).is_well_formed());
        }
    }

    #[test]
    fn parts_sit_where_they_say() {
        let p = HullParams::wren();
        let (_, bare_hi) = hull(&p, 1).bounds().unwrap();
        let bounds = |l: LookId| part(l, &p).bounds().unwrap();
        // The lens and the drill are ahead of the hull's middle, the drill past its nose.
        assert!(bounds(LookId::Lens).1.x > p.length * 0.5);
        assert!(bounds(LookId::LongDrill).1.x > p.length * 0.5 + 1.5);
        assert!(bounds(LookId::LongDrill).1.x > bare_hi.x);
        // Pods hang off both sides, outside the body.
        let (lo, hi) = bounds(LookId::Pods);
        assert!(hi.y > p.width * 0.6 && lo.y < -p.width * 0.6, "{lo} {hi}");
        // Fins are behind the middle and spread wide.
        let (lo, hi) = bounds(LookId::Fins);
        assert!(
            hi.x < 0.0 && hi.y > p.width * 0.9 && lo.y < -p.width * 0.9,
            "{lo} {hi}"
        );
        // The lamp stands over the back.
        assert!(bounds(LookId::SeamLamp).1.z > bare_hi.z * 0.8);
        assert!(seam_lamp_light(&p).z > p.height * 0.5);
    }

    #[test]
    fn parts_are_deterministic() {
        let p = HullParams::heron();
        assert_eq!(
            fitted_hull(&p, 9, &every_look()),
            fitted_hull(&p, 9, &every_look())
        );
    }

    #[test]
    fn the_cradle_puts_its_sockets_on_screen_nose_to_tail() {
        let hull = HullParams::wren();
        let viewport = Vec2::new(1280.0, 720.0);
        let camera = Camera::top_down(Vec2::ZERO, 120.0).with_tilt(14.0);
        let cradle = Cradle {
            ship: MeshHandle {
                index: 0,
                generation: 0,
            },
            hull: hull.clone(),
            look: ShipFitLook::default(),
            pose: CradlePose::fit(Vec2::ZERO, 80.0, &hull),
            rings: [SocketRing::Empty; 4],
            ghost: None,
            shadow: CRADLE_SHADOW,
        };
        let sockets = cradle.sockets(&camera, viewport);
        assert_eq!(sockets.map(|s| s.socket), Socket::ALL);
        let at = sockets.map(|s| s.screen.expect("on screen"));
        // Nose up: the drill mount is highest on screen, the engine lowest; all near
        // the middle column, the utility socket a little to the right.
        assert!(
            at[0].y < at[1].y && at[1].y < at[2].y && at[2].y < at[3].y,
            "{at:?}"
        );
        for p in at {
            assert!((p.x - 640.0).abs() < 40.0, "{p}");
            assert!((0.0..720.0).contains(&p.y));
        }
        assert!(at[1].x > 640.0, "utility sits right of the midline");
        // 80 world units of hull over 120 visible: rings are tens of pixels, and
        // neighbouring rings don't overlap.
        for s in sockets {
            assert!(s.radius_px > 25.0 && s.radius_px < 120.0, "{s:?}");
        }
        for w in sockets.windows(2) {
            let gap = w[0].screen.unwrap().distance(w[1].screen.unwrap());
            assert!(gap >= w[0].radius_px + w[1].radius_px - 2.0, "{w:?}");
        }
        // Drawing pushes the shadow and the ship, and a ring per socket asked for.
        let mut scene = Scene::new(camera);
        let drawn = Cradle {
            rings: [
                SocketRing::Fitted,
                SocketRing::None,
                SocketRing::DropTarget,
                SocketRing::Empty,
            ],
            ..cradle.clone()
        }
        .draw(&mut scene, viewport);
        assert_eq!(drawn, sockets);
        assert_eq!(scene.meshes.len(), 2);
        // Fitted 2 (ink under the gold), drop target 2 (white under the blue), empty 1.
        assert_eq!(scene.lines.len(), 5);
        assert!(
            scene
                .lines
                .iter()
                .any(|l| l.color == SKY_BLUE && matches!(l.style, crate::LineStyle::Dashed { .. }))
        );
        assert!(scene.lines.iter().any(|l| l.color == FITTED_GOLD));
    }

    #[test]
    fn the_shadow_falls_off_the_hull_on_the_plane() {
        let hull = HullParams::wren();
        let pose = CradlePose::fit(Vec2::new(10.0, 20.0), 60.0, &hull);
        assert!((pose.scale - 10.0).abs() < 1e-4);
        assert!(pose.lift > 0.0);
        let ship = pose.transform().transform_point3(Vec3::ZERO);
        let shadow = pose
            .shadow_transform()
            .transform_point3(Vec3::new(0.0, 0.0, 1.0));
        assert!((ship.z - pose.lift).abs() < 1e-4);
        assert!(shadow.z.abs() < 1.0, "flattened onto the plane");
        assert!(shadow.x > ship.x && shadow.y < ship.y);
    }
}
