//! The rooms as staged sets: a floor, three walls (the front is open to the camera, like
//! a stage) and the props of the trade, built from the mesh builder's primitives. Each
//! set also says where its fixed camera stands and where its lamps are.
//!
//! Set-local metres: the floor is centred on the origin, x to the right, y away from the
//! open front, z up; the back wall is at `y = depth / 2`. Spots for people come from the
//! content (`lab/places.luau`) and are placed to suit these props.

use render::glam::{Mat4, Vec2, Vec3};
use render::{Color, Glow, MeshBuilder, MeshData, PointLight, palette};

pub struct SetArt {
    pub mesh: MeshData,
    pub lights: Vec<PointLight>,
    pub glows: Vec<Glow>,
    /// The fixed camera: eye, target and vertical field of view in degrees.
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
    /// Where a ship rests in the set, and its length (the cradle).
    pub ship: Option<(Vec2, f32)>,
    pub clear: Color,
}

pub const WALL_H: f32 = 3.2;
const WALL_T: f32 = 0.25;

fn hex(h: u32) -> Color {
    Color::hex(h)
}

fn cube(b: &mut MeshBuilder, at: Vec3, size: Vec3) {
    b.with(Mat4::from_translation(at), |b| b.cuboid(size));
}

/// A box standing on the floor at `at` (x, y), `size` high in z.
fn block(b: &mut MeshBuilder, at: Vec2, z: f32, size: Vec3) {
    cube(b, at.extend(z + size.z * 0.5), size);
}

struct Shell {
    floor: Color,
    wall: Color,
    trim: Color,
    /// The door: on the left (-1) or right (+1) wall, at this y.
    door: (f32, f32),
}

/// The floor, the back and side walls, a skirting trim and the door.
fn shell(b: &mut MeshBuilder, size: Vec2, s: &Shell) {
    let (hw, hd) = (size.x * 0.5, size.y * 0.5);
    b.paint(s.floor, 0.0);
    cube(b, Vec3::new(0.0, 0.0, -0.15), Vec3::new(size.x + WALL_T * 2.0, size.y, 0.3));
    // Floor boards: a few darker strips across.
    b.paint(s.floor.scale(0.82), 0.0);
    let n = (size.y / 1.2).floor() as i32;
    for i in 0..n {
        let y = -hd + 0.6 + i as f32 * 1.2;
        cube(b, Vec3::new(0.0, y, 0.005), Vec3::new(size.x, 0.05, 0.01));
    }
    b.paint(s.wall, 0.0);
    cube(
        b,
        Vec3::new(0.0, hd + WALL_T * 0.5, WALL_H * 0.5),
        Vec3::new(size.x + WALL_T * 2.0, WALL_T, WALL_H),
    );
    for side in [-1.0f32, 1.0] {
        let x = side * (hw + WALL_T * 0.5);
        if side == s.door.0 {
            // A wall with a doorway: two pieces and a lintel.
            let (dy, dw, dh) = (s.door.1, 1.1, 2.2);
            let a = (-hd, dy - dw * 0.5);
            let c = (dy + dw * 0.5, hd);
            for (y0, y1) in [a, c] {
                if y1 > y0 {
                    cube(
                        b,
                        Vec3::new(x, (y0 + y1) * 0.5, WALL_H * 0.5),
                        Vec3::new(WALL_T, y1 - y0, WALL_H),
                    );
                }
            }
            cube(
                b,
                Vec3::new(x, dy, (dh + WALL_H) * 0.5),
                Vec3::new(WALL_T, dw, WALL_H - dh),
            );
            // The door frame and a dark passage beyond.
            b.paint(s.trim, 0.0);
            for e in [-0.5, 0.5] {
                cube(
                    b,
                    Vec3::new(x - side * 0.02, dy + e * (dw + 0.1), dh * 0.5),
                    Vec3::new(WALL_T + 0.08, 0.1, dh),
                );
            }
            cube(
                b,
                Vec3::new(x - side * 0.02, dy, dh + 0.05),
                Vec3::new(WALL_T + 0.08, dw + 0.2, 0.1),
            );
            b.paint(hex(0x2a1f36), 0.0);
            cube(
                b,
                Vec3::new(x + side * 0.3, dy, dh * 0.5),
                Vec3::new(0.05, dw, dh),
            );
            b.paint(s.wall, 0.0);
        } else {
            cube(b, Vec3::new(x, 0.0, WALL_H * 0.5), Vec3::new(WALL_T, size.y, WALL_H));
        }
    }
    // Skirting and a rail along the top of the walls.
    b.paint(s.trim, 0.0);
    cube(
        b,
        Vec3::new(0.0, hd - 0.03, 0.12),
        Vec3::new(size.x, 0.08, 0.24),
    );
    cube(
        b,
        Vec3::new(0.0, hd + WALL_T * 0.5, WALL_H + 0.05),
        Vec3::new(size.x + WALL_T * 2.2, WALL_T + 0.1, 0.12),
    );
    for side in [-1.0f32, 1.0] {
        cube(
            b,
            Vec3::new(side * (hw + WALL_T * 0.5), 0.0, WALL_H + 0.05),
            Vec3::new(WALL_T + 0.1, size.y, 0.12),
        );
    }
}

/// A round window in the back wall onto space, with a few stars.
fn porthole(b: &mut MeshBuilder, size: Vec2, x: f32, z: f32, r: f32, rim: Color) {
    let y = size.y * 0.5 - 0.02;
    let face = Mat4::from_translation(Vec3::new(x, y, z)) * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    b.with(face, |b| {
        b.paint(rim, 0.0);
        b.torus(r, 0.22, 0.14, 20, 4);
        b.paint(palette::SPACE_DEEP, 0.15);
        b.frustum(r, r, 0.02, 20);
        b.paint(palette::WINDOW, 1.0);
        for (sx, sy, s) in [(0.3, 0.2, 0.05), (-0.4, -0.1, 0.04), (0.05, -0.45, 0.035), (-0.15, 0.5, 0.03)] {
            cube(b, Vec3::new(sx * r, sy * r, -0.03), Vec3::splat(s * r * 2.0));
        }
    });
}

/// Where the screen has room for the scene: room panels sit on the right, so the stage
/// is framed into the left part, centred this far across (of 1280 at 16:9).
const STAGE_X: f32 = 340.0 / 1280.0;

/// A camera in front of the open side, `back` metres out and `height` up, looking a
/// little down, moved sideways so the room's `focus_x` lands in the free part of the
/// screen rather than under the room's panel.
fn camera_for(size: Vec2, focus_x: f32, height: f32, back: f32, fov: f32) -> (Vec3, Vec3) {
    let target_y = size.y * 0.15;
    let eye = Vec3::new(0.0, -size.y * 0.5 - back, height);
    let target = Vec3::new(0.0, target_y, 1.25);
    let dist = (target - eye).length();
    let width = 2.0 * dist * (fov.to_radians() * 0.5).tan() * 16.0 / 9.0;
    let shift = focus_x + (0.5 - STAGE_X) * width;
    (eye + Vec3::X * shift, target + Vec3::X * shift)
}

fn lamp(at: Vec3, color: Color, radius: f32) -> (PointLight, Glow) {
    (
        PointLight {
            position: at,
            color,
            radius,
        },
        Glow {
            position: at,
            radius: 0.5,
            color: color.with_alpha(0.6),
        },
    )
}

/// Ilse's dock office: a desk with the berth book, the berth board on the wall, ledgers
/// on a shelf, a porthole onto the dock.
fn office(size: Vec2) -> SetArt {
    let mut b = MeshBuilder::new();
    shell(
        &mut b,
        size,
        &Shell {
            floor: hex(0xb8643c),
            wall: palette::HULL_CREAM,
            trim: hex(0x2f7f86),
            door: (1.0, 0.4),
        },
    );
    let hd = size.y * 0.5;
    // The desk, Ilse behind it.
    b.paint(hex(0x7a4a2a), 0.0);
    block(&mut b, Vec2::new(-0.8, 1.0), 0.0, Vec3::new(2.2, 0.9, 0.08));
    cube(&mut b, Vec3::new(-0.8, 1.0, 0.82), Vec3::new(2.2, 0.9, 0.08));
    for x in [-1.8, 0.2] {
        block(&mut b, Vec2::new(x, 1.0), 0.0, Vec3::new(0.14, 0.8, 0.78));
    }
    b.paint(hex(0x5a3420), 0.0);
    block(&mut b, Vec2::new(-0.8, 0.6), 0.0, Vec3::new(2.0, 0.05, 0.7));
    // The berth book open on the desk, and a lamp.
    b.paint(palette::PARCHMENT, 0.0);
    cube(&mut b, Vec3::new(-0.5, 0.9, 0.88), Vec3::new(0.6, 0.42, 0.04));
    b.paint(palette::BRASS, 0.0);
    b.with(Mat4::from_translation(Vec3::new(-1.6, 1.15, 0.86)), |b| {
        b.frustum(0.12, 0.08, 0.35, 8);
        b.with(Mat4::from_translation(Vec3::Z * 0.35), |b| b.frustum(0.22, 0.08, 0.18, 8));
    });
    // The berth board: four berths, pegs for the ships in.
    b.paint(hex(0x3b4a5a), 0.0);
    cube(&mut b, Vec3::new(1.6, hd - 0.05, 1.8), Vec3::new(2.4, 0.08, 1.3));
    let pegs = [palette::CORAL, palette::TEAL, palette::MUSTARD, palette::BLOSSOM];
    for (i, c) in pegs.iter().enumerate() {
        b.paint(palette::CREAM, 0.0);
        let x = 0.75 + i as f32 * 0.57;
        cube(&mut b, Vec3::new(x, hd - 0.1, 1.85), Vec3::new(0.42, 0.04, 0.9));
        if i != 2 {
            b.paint(*c, 0.2);
            cube(&mut b, Vec3::new(x, hd - 0.14, 1.95), Vec3::new(0.2, 0.06, 0.2));
        }
    }
    // Ledgers on a shelf on the left wall.
    let lx = -size.x * 0.5 + 0.2;
    b.paint(hex(0x7a4a2a), 0.0);
    cube(&mut b, Vec3::new(lx, -0.2, 1.5), Vec3::new(0.35, 1.8, 0.06));
    for i in 0..9 {
        let c = [palette::TERRACOTTA, palette::TEAL, palette::MUSTARD][i % 3];
        b.paint(c, 0.0);
        let h = 0.32 + (i % 2) as f32 * 0.06;
        cube(&mut b, Vec3::new(lx, -0.95 + i as f32 * 0.18, 1.53 + h * 0.5), Vec3::new(0.28, 0.12, h));
    }
    porthole(&mut b, size, -2.4, 1.9, 0.55, palette::BRASS);
    // A chair for visitors, in front of the desk.
    b.paint(palette::TEAL, 0.0);
    block(&mut b, Vec2::new(0.9, -0.3), 0.0, Vec3::new(0.5, 0.5, 0.45));
    block(&mut b, Vec2::new(0.9, -0.08), 0.45, Vec3::new(0.5, 0.08, 0.5));
    let (l1, g1) = lamp(Vec3::new(-1.6, 1.15, 1.3), palette::LANTERN, 7.0);
    let (eye, target) = camera_for(size, -0.4, 3.6, 5.4, 30.0);
    SetArt {
        mesh: b.build(),
        lights: vec![l1],
        glows: vec![g1],
        eye,
        target,
        fov: 30.0,
        ship: None,
        clear: hex(0x1b1424),
    }
}

/// Juno's market: a counter with wares on it, jars on the shelves, crates stacked by the
/// door, bunting along the top of the back wall.
fn market(size: Vec2) -> SetArt {
    let mut b = MeshBuilder::new();
    shell(
        &mut b,
        size,
        &Shell {
            floor: hex(0x9a6a3a),
            wall: palette::SAGE,
            trim: hex(0x7a4a2a),
            door: (1.0, -0.6),
        },
    );
    let (hw, hd) = (size.x * 0.5, size.y * 0.5);
    // The counter.
    b.paint(hex(0x7a4a2a), 0.0);
    block(&mut b, Vec2::new(-1.3, 0.8), 0.0, Vec3::new(3.6, 0.8, 0.85));
    b.paint(palette::MUSTARD, 0.0);
    cube(&mut b, Vec3::new(-1.3, 0.8, 0.89), Vec3::new(3.8, 0.95, 0.08));
    // Wares on the counter: ice blocks, an ore heap, a stack of plate.
    b.paint(palette::GLASS, 0.3);
    for (x, s) in [(-2.6, 0.32), (-2.2, 0.26)] {
        cube(&mut b, Vec3::new(x, 0.75, 0.93 + s * 0.5), Vec3::splat(s));
    }
    b.paint(hex(0x8a5a4a), 0.0);
    b.with(Mat4::from_translation(Vec3::new(-0.3, 0.75, 0.93)), |b| {
        b.lathe(&[(0.35, 0.0), (0.2, 0.18), (0.0, 0.28)], 6, 0.3)
    });
    b.paint(hex(0xb0b8c0), 0.0);
    for i in 0..4 {
        cube(&mut b, Vec3::new(0.3, 0.8, 0.96 + i as f32 * 0.05), Vec3::new(0.5, 0.36, 0.04));
    }
    // Shelves of jars on the back wall.
    for row in 0..3 {
        let z = 1.1 + row as f32 * 0.55;
        b.paint(hex(0x7a4a2a), 0.0);
        cube(&mut b, Vec3::new(-1.3, hd - 0.2, z), Vec3::new(4.0, 0.35, 0.06));
        for i in 0..8 {
            let c = [palette::CORAL, palette::MUSTARD, palette::LEAF, palette::BLOSSOM][(i + row) % 4];
            b.paint(c, 0.1);
            b.with(Mat4::from_translation(Vec3::new(-3.0 + i as f32 * 0.48, hd - 0.2, z + 0.03)), |b| {
                b.lathe(&[(0.1, 0.0), (0.12, 0.2), (0.07, 0.3), (0.08, 0.36)], 8, 0.0)
            });
        }
    }
    // Crates by the door.
    for (x, y, z, c) in [
        (2.6, 1.6, 0.0, palette::TERRACOTTA),
        (3.4, 1.6, 0.0, palette::TEAL),
        (3.0, 1.6, 0.8, palette::MUSTARD),
        (3.4, 0.7, 0.0, palette::TERRACOTTA),
    ] {
        b.paint(c, 0.0);
        block(&mut b, Vec2::new(x, y), z, Vec3::splat(0.78));
        b.paint(c.scale(0.7), 0.0);
        cube(&mut b, Vec3::new(x, y - 0.4, z + 0.39), Vec3::new(0.8, 0.02, 0.12));
    }
    // Bunting.
    for i in 0..14 {
        let c = [palette::CORAL, palette::MUSTARD, palette::TEAL][i % 3];
        b.paint(c, 0.0);
        let x = -hw + 0.4 + i as f32 * (size.x - 0.8) / 13.0;
        b.with(
            Mat4::from_translation(Vec3::new(x, hd - 0.1, WALL_H - 0.3)) * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            |b| b.extrude(&[Vec2::new(-0.18, 0.18), Vec2::new(0.0, -0.22), Vec2::new(0.18, 0.18)], 0.03, 1.0),
        );
    }
    let (l1, g1) = lamp(Vec3::new(-1.3, 0.2, 2.6), palette::LANTERN, 8.0);
    let (eye, target) = camera_for(size, -1.6, 3.6, 5.6, 30.0);
    SetArt {
        mesh: b.build(),
        lights: vec![l1],
        glows: vec![g1],
        eye,
        target,
        fov: 30.0,
        ship: None,
        clear: hex(0x1b1424),
    }
}

/// Orrin's cradle: a high bay with the Wren on its cradle arms, a bench of tools, a
/// pegboard and the bay doors onto space.
fn cradle(size: Vec2) -> SetArt {
    let mut b = MeshBuilder::new();
    shell(
        &mut b,
        size,
        &Shell {
            floor: hex(0x4a4a5a),
            wall: hex(0x2f6f78),
            trim: palette::MUSTARD,
            door: (-1.0, -1.8),
        },
    );
    let (hw, hd) = (size.x * 0.5, size.y * 0.5);
    let ship_at = Vec2::new(-1.3, 0.4);
    // Hazard stripes round the cradle pad.
    b.paint(palette::MUSTARD, 0.0);
    for (x, y, w, d) in [
        (ship_at.x, ship_at.y - 2.8, 4.0, 0.18),
        (ship_at.x, ship_at.y + 2.8, 4.0, 0.18),
        (ship_at.x - 2.0, ship_at.y, 0.18, 5.6),
        (ship_at.x + 2.0, ship_at.y, 0.18, 5.6),
    ] {
        cube(&mut b, Vec3::new(x, y, 0.01), Vec3::new(w, d, 0.02));
    }
    // The cradle arms.
    b.paint(hex(0x6a5a4a), 0.0);
    for y in [-1.3, 1.1] {
        block(&mut b, Vec2::new(ship_at.x, ship_at.y + y), 0.0, Vec3::new(2.4, 0.5, 0.35));
        block(&mut b, Vec2::new(ship_at.x - 0.9, ship_at.y + y), 0.35, Vec3::new(0.3, 0.4, 0.5));
        block(&mut b, Vec2::new(ship_at.x + 0.9, ship_at.y + y), 0.35, Vec3::new(0.3, 0.4, 0.5));
    }
    // The bay doors in the back wall: a frame round a window onto space.
    b.paint(palette::SPACE_DEEP, 0.15);
    cube(&mut b, Vec3::new(ship_at.x, hd - 0.02, 1.75), Vec3::new(6.0, 0.06, 2.6));
    b.paint(palette::WINDOW, 1.0);
    for (sx, sz) in [(-2.2, 2.6), (1.4, 2.2), (0.3, 1.0), (-1.0, 1.6), (2.4, 2.8)] {
        cube(&mut b, Vec3::new(ship_at.x + sx, hd - 0.06, sz), Vec3::splat(0.07));
    }
    b.paint(palette::MUSTARD, 0.0);
    for x in [-3.1, 3.1] {
        cube(&mut b, Vec3::new(ship_at.x + x, hd - 0.05, 1.6), Vec3::new(0.25, 0.15, 3.2));
    }
    for i in 0..6 {
        b.paint(if i % 2 == 0 { palette::MUSTARD } else { palette::INK }, 0.0);
        cube(
            &mut b,
            Vec3::new(ship_at.x - 2.5 + i as f32, hd - 0.05, 3.05),
            Vec3::new(1.0, 0.15, 0.2),
        );
    }
    // Orrin's bench and tools on the left.
    let bx = -hw + 1.2;
    b.paint(hex(0x7a4a2a), 0.0);
    block(&mut b, Vec2::new(bx, 1.6), 0.0, Vec3::new(1.6, 0.9, 0.9));
    b.paint(palette::BRASS, 0.0);
    cube(&mut b, Vec3::new(bx - 0.3, 1.5, 1.0), Vec3::new(0.5, 0.3, 0.2));
    b.paint(palette::CORAL, 0.0);
    cube(&mut b, Vec3::new(bx + 0.4, 1.7, 0.98), Vec3::new(0.4, 0.4, 0.16));
    // The pegboard over the bench.
    b.paint(hex(0x8a6a4a), 0.0);
    cube(&mut b, Vec3::new(bx + 0.3, hd - 0.05, 1.9), Vec3::new(2.2, 0.06, 1.3));
    b.paint(hex(0xb0b8c0), 0.0);
    for i in 0..5 {
        let x = bx - 0.5 + i as f32 * 0.4;
        cube(&mut b, Vec3::new(x, hd - 0.1, 1.95), Vec3::new(0.08, 0.04, 0.7 - (i % 2) as f32 * 0.2));
    }
    // Spare parts in a cage on the right.
    b.paint(palette::TEAL, 0.0);
    block(&mut b, Vec2::new(hw - 1.0, 2.6), 0.0, Vec3::new(1.4, 1.2, 1.4));
    b.paint(palette::CORAL, 0.0);
    block(&mut b, Vec2::new(hw - 1.0, 2.6), 1.4, Vec3::new(0.6, 0.5, 0.4));
    let (l1, g1) = lamp(Vec3::new(ship_at.x, ship_at.y - 0.5, 4.2), palette::LANTERN, 14.0);
    let (l2, g2) = lamp(Vec3::new(bx, 1.2, 2.4), palette::LANTERN, 6.0);
    let (eye, target) = camera_for(size, -2.5, 9.0, 8.5, 30.0);
    SetArt {
        mesh: b.build(),
        lights: vec![l1, l2],
        glows: vec![g1, g2],
        eye,
        target,
        fov: 30.0,
        ship: Some((ship_at, 3.5)),
        clear: hex(0x1b1424),
    }
}

/// Bel's canteen: a serving counter with the broth pot, two round tables with stools,
/// lamps hanging low, a porthole.
fn canteen(size: Vec2) -> SetArt {
    let mut b = MeshBuilder::new();
    shell(
        &mut b,
        size,
        &Shell {
            floor: hex(0xa8443a),
            wall: palette::MUSTARD.scale(0.85),
            trim: hex(0x7a4a2a),
            door: (1.0, 1.2),
        },
    );
    let hd = size.y * 0.5;
    // Checked tiles in front of the counter.
    b.paint(palette::CREAM, 0.0);
    for i in 0..8 {
        for j in 0..2 {
            if (i + j) % 2 == 0 {
                cube(&mut b, Vec3::new(-3.6 + i as f32 * 0.5, 1.2 + j as f32 * 0.5, 0.01), Vec3::new(0.5, 0.5, 0.02));
            }
        }
    }
    // The serving counter, Bel behind it.
    b.paint(hex(0x7a4a2a), 0.0);
    block(&mut b, Vec2::new(-2.0, 1.7), 0.0, Vec3::new(3.0, 0.8, 1.0));
    b.paint(palette::CREAM, 0.0);
    cube(&mut b, Vec3::new(-2.0, 1.7, 1.04), Vec3::new(3.2, 0.95, 0.08));
    // The broth pot, steaming.
    b.paint(hex(0x5a5a6a), 0.0);
    b.with(Mat4::from_translation(Vec3::new(-2.9, 1.7, 1.08)), |b| {
        b.lathe(&[(0.3, 0.0), (0.36, 0.2), (0.34, 0.45), (0.0, 0.46)], 10, 0.0)
    });
    b.paint(palette::CREAM, 0.0);
    for (i, z) in [1.7, 1.95, 2.2].iter().enumerate() {
        cube(&mut b, Vec3::new(-2.9 + i as f32 * 0.07, 1.7, *z), Vec3::splat(0.14 - i as f32 * 0.03));
    }
    // Bowls stacked.
    b.paint(palette::TEAL, 0.0);
    for i in 0..3 {
        b.with(Mat4::from_translation(Vec3::new(-1.2, 1.6, 1.08 + i as f32 * 0.06)), |b| {
            b.frustum(0.1, 0.16, 0.06, 8)
        });
    }
    // Tables and stools.
    for (tx, ty) in [(0.9, 0.6), (2.5, -0.9)] {
        b.paint(hex(0x7a4a2a), 0.0);
        b.with(Mat4::from_translation(Vec3::new(tx, ty, 0.0)), |b| {
            b.frustum(0.12, 0.12, 0.72, 6);
            b.with(Mat4::from_translation(Vec3::Z * 0.72), |b| b.frustum(0.6, 0.6, 0.06, 12));
        });
        b.paint(palette::TERRACOTTA, 0.0);
        for (dx, dy) in [(-0.85, 0.0), (0.85, 0.0)] {
            b.with(Mat4::from_translation(Vec3::new(tx + dx, ty + dy, 0.0)), |b| {
                b.frustum(0.18, 0.2, 0.45, 8)
            });
        }
    }
    b.paint(palette::CREAM, 0.0);
    b.with(Mat4::from_translation(Vec3::new(1.2, 0.55, 0.78)), |b| b.frustum(0.08, 0.13, 0.06, 8));
    porthole(&mut b, size, 1.8, 1.9, 0.6, hex(0x7a4a2a));
    // Lamps hanging over the tables.
    b.paint(palette::BRASS, 0.0);
    let mut lights = Vec::new();
    let mut glows = Vec::new();
    for (x, y) in [(0.9, 0.6), (2.5, -0.9), (-2.0, 1.6)] {
        cube(&mut b, Vec3::new(x, y, 2.9), Vec3::new(0.03, 0.03, 0.6));
        b.with(Mat4::from_translation(Vec3::new(x, y, 2.35)), |b| b.frustum(0.28, 0.06, 0.25, 8));
        let (l, g) = lamp(Vec3::new(x, y, 2.25), palette::LANTERN, 5.0);
        lights.push(l);
        glows.push(g);
    }
    let _ = hd;
    let (eye, target) = camera_for(size, -0.2, 3.6, 5.6, 30.0);
    SetArt {
        mesh: b.build(),
        lights,
        glows,
        eye,
        target,
        fov: 30.0,
        ship: None,
        clear: hex(0x1b1424),
    }
}

/// A bare room for a set the lab doesn't know.
fn bare(size: Vec2) -> SetArt {
    let mut b = MeshBuilder::new();
    shell(
        &mut b,
        size,
        &Shell {
            floor: hex(0x6a5a4a),
            wall: palette::HULL_CREAM,
            trim: palette::TEAL,
            door: (1.0, 0.0),
        },
    );
    let (eye, target) = camera_for(size, 0.0, 3.6, 5.6, 30.0);
    SetArt {
        mesh: b.build(),
        lights: Vec::new(),
        glows: Vec::new(),
        eye,
        target,
        fov: 34.0,
        ship: None,
        clear: hex(0x1b1424),
    }
}

pub fn build(set: &str, floor: Vec2) -> SetArt {
    match set {
        "office" => office(floor),
        "market" => market(floor),
        "cradle" => cradle(floor),
        "canteen" => canteen(floor),
        _ => bare(floor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_set_is_well_formed() {
        for s in ["office", "market", "cradle", "canteen", "unknown"] {
            let art = build(s, Vec2::new(9.0, 6.0));
            assert!(art.mesh.is_well_formed(), "{s}");
            assert!(!art.mesh.is_empty(), "{s}");
        }
    }
}
