//! The shapes a recipe's parts are made of: plain primitives, and the bigger pieces the
//! slice's station and hull kits were built from (tiers, spoked rings, trusses, masts,
//! hull bodies, wings, nozzles, rocks), as generic shapes. Which shapes make a station or a
//! ship is module data; nothing here is one particular thing.
//!
//! Each shape builds in its own frame: +x is its front (or outwards, for a part placed
//! round a centre), +y its left, +z up. All maths is plain arithmetic and `libm`, so a
//! recipe gives the same bits on every OS.

use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use galaxy_mesh::{Color, MeshBuilder};
use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::params::{Kind, Param, Values};
use crate::rng::Rng;

/// A paint a shape uses, by the part key that picks its colour. The default slot is the
/// palette slot of the same name, except `paint`, which defaults to `main`.
pub const PAINTS: &[(&str, &str)] = &[
    (
        "paint",
        "the main surface; default slot main, glowing by the part's glow",
    ),
    (
        "trim",
        "bands, collars, frames and spokes; default slot trim",
    ),
    (
        "accent",
        "caps, roofs, dishes and lips; default slot accent",
    ),
    (
        "lights",
        "windows, lamps and engine cores, fully self-lit; default slot lights",
    ),
    ("glass", "glazing, a little self-lit; default slot glass"),
];

/// How much each paint glows (the main paint glows by the part's `glow`).
fn glow_of(role: &str) -> f32 {
    match role {
        "lights" => 1.0,
        "glass" => 0.3,
        _ => 0.0,
    }
}

/// The paints one part resolved, by role.
pub type Paints = BTreeMap<&'static str, Color>;

/// What a shape builds with: the builder in the part's frame, its paints and a random
/// stream of its own.
pub struct Ctx<'a> {
    pub b: &'a mut MeshBuilder,
    pub paints: &'a Paints,
    pub glow: f32,
    pub rng: Rng,
}

impl Ctx<'_> {
    fn color(&self, role: &str) -> Color {
        *self
            .paints
            .get(role)
            .unwrap_or_else(|| panic!("a shape painted `{role}`, which its table doesn't list"))
    }

    /// Paints the following geometry with `role`.
    fn paint(&mut self, role: &str) {
        let glow = if role == "paint" {
            self.glow
        } else {
            glow_of(role)
        };
        let c = self.color(role);
        self.b.paint(c, glow);
    }

    /// Paints with `role` mixed `t` of the way to `other`.
    fn paint_mix(&mut self, role: &str, other: &str, t: f32) {
        let glow = if role == "paint" {
            self.glow
        } else {
            glow_of(role)
        };
        let c = self.color(role).lerp(self.color(other), t);
        self.b.paint(c, glow);
    }

    fn at(&mut self, m: Mat4, f: impl FnOnce(&mut MeshBuilder)) {
        self.b.with(m, f);
    }
}

/// A refusal from a shape: the key and what is wrong with it.
pub type Refusal = (&'static str, String);

/// One shape: its name, what it is, its keys, the paints it uses and its builder.
pub struct Shape {
    pub name: &'static str,
    pub about: &'static str,
    pub params: &'static [Param],
    pub paints: &'static [&'static str],
    pub build: fn(&mut Ctx, &Values) -> Result<(), Refusal>,
}

const fn num(key: &'static str, lo: f32, hi: f32, d: Option<f32>, about: &'static str) -> Param {
    Param {
        key,
        kind: Kind::Num(lo, hi, d),
        about,
    }
}

const fn int(key: &'static str, lo: u32, hi: u32, d: u32, about: &'static str) -> Param {
    Param {
        key,
        kind: Kind::Int(lo, hi, Some(d)),
        about,
    }
}

const fn flag(key: &'static str, d: bool, about: &'static str) -> Param {
    Param {
        key,
        kind: Kind::Flag(d),
        about,
    }
}

/// Every shape, in the order `galaxy-gm fields recipe` lists them.
pub static SHAPES: &[Shape] = &[
    Shape {
        name: "box",
        about: "a box centred on the part's origin",
        params: &[Param {
            key: "size",
            kind: Kind::Vec3(0.01, 500.0, None),
            about: "its sides along x, y and z",
        }],
        paints: &["paint"],
        build: |c, v| {
            c.paint("paint");
            c.b.cuboid(v.vec3("size"));
            Ok(())
        },
    },
    Shape {
        name: "cylinder",
        about: "a cylinder or cone standing on the origin, up along +z",
        params: &[
            num("radius", 0.01, 200.0, None, "radius at the bottom"),
            num("height", 0.01, 500.0, None, "height"),
            num(
                "taper",
                0.0,
                4.0,
                Some(1.0),
                "top radius over bottom radius; 0 is a cone",
            ),
            int("sides", 3, 64, 8, "sides round"),
        ],
        paints: &["paint"],
        build: |c, v| {
            c.paint("paint");
            let r = v.num("radius");
            c.b.frustum(r, r * v.num("taper"), v.num("height"), v.int("sides"));
            Ok(())
        },
    },
    Shape {
        name: "lathe",
        about: "a surface turned round +z from a profile of [radius, z] pairs, bottom to top; a radius of 0 closes that end",
        params: &[
            Param {
                key: "profile",
                kind: Kind::Pairs(2, 64),
                about: "[radius, z] pairs from bottom to top; radius 0 or more",
            },
            int("sides", 3, 64, 12, "sides round"),
        ],
        paints: &["paint"],
        build: |c, v| {
            let profile: Vec<(f32, f32)> = v.pairs("profile").iter().map(|p| (p.x, p.y)).collect();
            if profile.iter().any(|p| p.0 < 0.0) {
                return Err((
                    "profile",
                    "has a negative radius; radii are 0 or more".into(),
                ));
            }
            c.paint("paint");
            c.b.lathe(&profile, v.int("sides"), 0.0);
            Ok(())
        },
    },
    Shape {
        name: "ring",
        about: "a ring (torus) lying flat round the origin",
        params: &[
            num(
                "radius",
                0.05,
                300.0,
                None,
                "from the centre to the middle of the tube",
            ),
            num("tube", 0.01, 100.0, None, "the tube's width"),
            num(
                "flat",
                0.05,
                4.0,
                Some(0.7),
                "the tube's height over its width",
            ),
            int("segments", 3, 96, 24, "segments round the ring"),
            int("sides", 3, 16, 6, "sides round the tube"),
        ],
        paints: &["paint"],
        build: |c, v| {
            let (r, t) = (v.num("radius"), v.num("tube"));
            if t > r * 2.0 {
                return Err((
                    "tube",
                    format!("is {t}; at most twice the radius ({})", r * 2.0),
                ));
            }
            c.paint("paint");
            c.b.torus(r, t, t * v.num("flat"), v.int("segments"), v.int("sides"));
            Ok(())
        },
    },
    Shape {
        name: "sphere",
        about: "a ball centred on the origin, or with half = true a dome standing on it",
        params: &[
            num("radius", 0.01, 300.0, None, "radius"),
            int("sides", 3, 48, 12, "sides round"),
            int(
                "rings",
                2,
                24,
                6,
                "bands from pole to pole (or base to top for a dome)",
            ),
            flag("half", false, "only the top half, standing on z = 0"),
        ],
        paints: &["paint"],
        build: |c, v| {
            let (r, rings) = (v.num("radius"), v.int("rings"));
            let (from, span) = if v.flag("half") {
                (0.0, FRAC_PI_2)
            } else {
                (-FRAC_PI_2, PI)
            };
            let profile: Vec<(f32, f32)> = (0..=rings)
                .map(|i| {
                    let a = from + span * i as f32 / rings as f32;
                    let rad = if i == rings { 0.0 } else { r * libm::cosf(a) };
                    let rad = if i == 0 && !v.flag("half") { 0.0 } else { rad };
                    (rad.max(0.0), r * libm::sinf(a))
                })
                .collect();
            c.paint("paint");
            c.b.lathe(&profile, v.int("sides"), 0.0);
            Ok(())
        },
    },
    Shape {
        name: "plate",
        about: "a flat outline of [x, y] points extruded up from z = 0, its top shrunk by bevel",
        params: &[
            Param {
                key: "outline",
                kind: Kind::Pairs(3, 64),
                about: "[x, y] corners in order round the outline (either way round); star-shaped round its centre",
            },
            num("height", 0.01, 100.0, None, "thickness, up from z = 0"),
            num(
                "bevel",
                0.05,
                1.5,
                Some(1.0),
                "the top's size over the bottom's",
            ),
        ],
        paints: &["paint"],
        build: |c, v| {
            let outline = ccw(v.pairs("outline"));
            if signed_area(&outline).abs() < 1e-6 {
                return Err(("outline", "encloses no area".into()));
            }
            c.paint("paint");
            c.b.extrude(&outline, v.num("height"), v.num("bevel"));
            Ok(())
        },
    },
    Shape {
        name: "strut",
        about: "a square beam from one point to another",
        params: &[
            Param {
                key: "from",
                kind: Kind::Vec3(-500.0, 500.0, None),
                about: "one end",
            },
            Param {
                key: "to",
                kind: Kind::Vec3(-500.0, 500.0, None),
                about: "the other end",
            },
            num("thickness", 0.01, 50.0, Some(0.3), "its side"),
        ],
        paints: &["paint"],
        build: |c, v| {
            if (v.vec3("to") - v.vec3("from")).length() < 1e-3 {
                return Err(("to", "is where from is; a strut needs two ends".into()));
            }
            c.paint("paint");
            strut(c.b, v.vec3("from"), v.vec3("to"), v.num("thickness"));
            Ok(())
        },
    },
    Shape {
        name: "tiers",
        about: "a core of stacked tiers centred on the origin, each with a trim band and a ring of lit windows, under a cap with a light",
        params: &[
            num("radius", 0.5, 60.0, None, "the bottom tier's radius"),
            num("height", 0.5, 60.0, None, "all tiers together"),
            int(
                "tiers",
                1,
                6,
                2,
                "how many tiers; each is a little narrower",
            ),
            int("sides", 5, 24, 8, "sides round"),
            num(
                "cap",
                0.0,
                2.0,
                Some(0.4),
                "the cap's height over its radius; 0 for none",
            ),
        ],
        paints: &["paint", "trim", "accent", "lights"],
        build: |c, v| {
            tiers(
                c,
                v.num("radius"),
                v.num("height"),
                v.int("tiers"),
                v.int("sides"),
                v.num("cap"),
            );
            Ok(())
        },
    },
    Shape {
        name: "spoked_ring",
        about: "a ring lying flat round the origin with a lighter inner lip, lamps on top and spokes running in to inner",
        params: &[
            num(
                "radius",
                1.0,
                120.0,
                None,
                "from the centre to the tube's middle",
            ),
            num(
                "tube",
                0.1,
                30.0,
                Some(1.6),
                "the tube's height; at most half the radius",
            ),
            int("spokes", 0, 16, 4, "spokes"),
            num(
                "inner",
                0.0,
                120.0,
                Some(0.0),
                "where the spokes start, from the centre",
            ),
            int("lamps", 0, 16, 4, "lamps round the top"),
        ],
        paints: &["paint", "trim", "accent", "lights"],
        build: |c, v| {
            let (radius, tube, inner) = (v.num("radius"), v.num("tube"), v.num("inner"));
            if tube > radius * 0.5 {
                return Err((
                    "tube",
                    format!("is {tube}; at most half the radius ({})", radius * 0.5),
                ));
            }
            if inner > radius {
                return Err((
                    "inner",
                    format!("is {inner}; at most the radius ({radius})"),
                ));
            }
            let segments = ((radius * 1.2) as u32).clamp(16, 48);
            c.paint("paint");
            c.b.torus(radius, tube * 1.3, tube, segments, 6);
            c.paint("accent");
            c.at(Mat4::from_translation(Vec3::Z * tube * 0.45), |b| {
                b.torus(radius - tube * 0.2, tube * 0.6, tube * 0.3, segments, 4)
            });
            let lamps = v.int("lamps");
            c.paint("lights");
            for k in 0..lamps {
                let a = k as f32 / lamps as f32 * TAU + TAU / 8.0;
                c.at(
                    Mat4::from_translation(Vec3::new(
                        libm::cosf(a) * radius,
                        libm::sinf(a) * radius,
                        tube * 0.55,
                    )) * rot_z(a),
                    |b| b.cuboid(Vec3::new(tube * 0.7, tube * 0.7, tube * 0.5)),
                );
            }
            let spokes = v.int("spokes");
            c.paint("trim");
            for k in 0..spokes {
                let a = k as f32 / spokes as f32 * TAU;
                let dir = Vec3::new(libm::cosf(a), libm::sinf(a), 0.0);
                strut(c.b, dir * inner, dir * (radius - tube * 0.5), tube * 0.6);
            }
            Ok(())
        },
    },
    Shape {
        name: "truss",
        about: "a beam reaching out along +x from the origin, with trim collars along it",
        params: &[
            num("length", 0.5, 120.0, None, "how far it reaches"),
            num(
                "width",
                0.1,
                20.0,
                None,
                "its width; its height is 0.6 of it",
            ),
            num(
                "collar_every",
                0.5,
                120.0,
                Some(5.0),
                "the space between collars",
            ),
        ],
        paints: &["paint", "trim"],
        build: |c, v| {
            let (l, w) = (v.num("length"), v.num("width"));
            c.paint("paint");
            c.at(Mat4::from_translation(Vec3::new(l * 0.5, 0.0, 0.0)), |b| {
                b.cuboid(Vec3::new(l, w, w * 0.6))
            });
            c.paint("trim");
            let n = ((l / v.num("collar_every")) as u32).max(1);
            let pad = (w * 0.2).min(l * 0.5);
            for k in 0..=n {
                let x = (l * k as f32 / n as f32).clamp(pad, l - pad);
                c.at(Mat4::from_translation(Vec3::new(x, 0.0, 0.0)), |b| {
                    b.cuboid(Vec3::new(w * 0.35, w * 1.25, w * 0.8))
                });
            }
            Ok(())
        },
    },
    Shape {
        name: "mast",
        about: "a thin mast up from the origin with a dish part-way up and a lamp at the tip",
        params: &[
            num("height", 0.5, 60.0, None, "height"),
            flag("dish", true, "a dish on it"),
            num(
                "lean",
                -45.0,
                45.0,
                Some(0.0),
                "degrees it leans towards +x",
            ),
        ],
        paints: &["paint", "accent", "lights"],
        build: |c, v| {
            let h = v.num("height");
            let dish = v.flag("dish");
            let (body, accent, lights) = (c.color("paint"), c.color("accent"), c.color("lights"));
            let glow = c.glow;
            c.at(rot_y(v.num("lean").to_radians()), |b| {
                b.paint(body, glow);
                b.frustum(h * 0.06, h * 0.03, h, 5);
                if dish {
                    b.paint(accent, 0.0);
                    b.with(
                        Mat4::from_translation(Vec3::Z * h * 0.62) * rot_y(0.9),
                        |b| {
                            b.lathe(
                                &[(0.0, 0.0), (h * 0.16, h * 0.04), (h * 0.32, h * 0.14)],
                                8,
                                0.0,
                            )
                        },
                    );
                }
                b.paint(lights, 1.0);
                b.with(Mat4::from_translation(Vec3::Z * h), |b| {
                    b.cuboid(Vec3::splat(h * 0.09))
                });
            });
            Ok(())
        },
    },
    Shape {
        name: "tanks",
        about: "rounded capsules lying side by side along x, centred on the origin, with trim bands",
        params: &[
            num("radius", 0.1, 20.0, None, "each capsule's radius"),
            num("length", 0.2, 60.0, None, "each capsule's straight length"),
            int("count", 1, 8, 1, "capsules side by side along y"),
        ],
        paints: &["paint", "trim"],
        build: |c, v| {
            let (r, l, n) = (v.num("radius"), v.num("length"), v.int("count"));
            let half = l * 0.5;
            for k in 0..n {
                let y = (k as f32 - (n - 1) as f32 * 0.5) * r * 2.2;
                let at = Mat4::from_translation(Vec3::new(0.0, y, 0.0)) * rot_y(FRAC_PI_2);
                c.paint("paint");
                c.at(at, |b| {
                    b.lathe(
                        &[
                            (0.0, -half - r * 0.6),
                            (r * 0.75, -half - r * 0.25),
                            (r, -half + r * 0.2),
                            (r, half - r * 0.2),
                            (r * 0.75, half + r * 0.25),
                            (0.0, half + r * 0.6),
                        ],
                        8,
                        0.0,
                    )
                });
                c.paint("trim");
                for z in [-half * 0.45, half * 0.45] {
                    c.at(at * Mat4::from_translation(Vec3::Z * z), |b| {
                        b.lathe(&[(r * 1.07, -r * 0.2), (r * 1.07, r * 0.2)], 8, 0.0)
                    });
                }
            }
            Ok(())
        },
    },
    Shape {
        name: "panel",
        about: "a flat panel of cells reaching out along +x on a spine",
        params: &[
            num("length", 0.5, 120.0, None, "how far it reaches"),
            num("width", 0.2, 60.0, None, "its width along y"),
            num("cell", 0.5, 60.0, Some(3.0), "about how long each cell is"),
        ],
        paints: &["paint", "trim"],
        build: |c, v| {
            let (l, w) = (v.num("length"), v.num("width"));
            c.paint("trim");
            strut(c.b, Vec3::ZERO, Vec3::new(l, 0.0, 0.0), w * 0.12);
            let cells = ((l / v.num("cell")) as u32).max(1);
            let cell = l / cells as f32;
            for k in 0..cells {
                if k % 2 == 0 {
                    c.paint("paint");
                } else {
                    c.paint_mix("paint", "trim", 0.35);
                }
                c.at(
                    Mat4::from_translation(Vec3::new(cell * (k as f32 + 0.5), 0.0, 0.2)),
                    |b| b.cuboid(Vec3::new(cell * 0.9, w, 0.25)),
                );
            }
            Ok(())
        },
    },
    Shape {
        name: "dome",
        about: "a glazed dome on a rim, standing on the origin, with ribs over it",
        params: &[
            num("radius", 0.5, 60.0, None, "radius"),
            int("ribs", 0, 12, 6, "ribs over the glass"),
        ],
        paints: &["paint", "trim", "glass"],
        build: |c, v| {
            let radius = v.num("radius");
            let base = radius * 0.18;
            c.paint("paint");
            c.b.lathe(
                &[
                    (radius * 0.9, 0.0),
                    (radius * 1.04, base * 0.6),
                    (radius * 0.92, base),
                ],
                12,
                0.0,
            );
            c.paint("glass");
            c.b.lathe(
                &[
                    (radius * 0.94, base),
                    (radius * 0.82, base + radius * 0.38),
                    (radius * 0.48, base + radius * 0.72),
                    (0.0, base + radius * 0.98),
                ],
                10,
                0.0,
            );
            c.paint("trim");
            let rib = (radius * 0.07).max(0.05);
            let ribs = v.int("ribs");
            for k in 0..ribs {
                let a = k as f32 / ribs as f32 * TAU;
                let p = |t: f32| {
                    let e = t * FRAC_PI_2;
                    Vec3::new(
                        libm::cosf(a) * libm::cosf(e) * radius * 0.98,
                        libm::sinf(a) * libm::cosf(e) * radius * 0.98,
                        base + libm::sinf(e) * radius * 0.95,
                    )
                };
                for s in 0..4 {
                    strut(c.b, p(s as f32 / 4.0), p((s + 1) as f32 / 4.0), rib);
                }
            }
            Ok(())
        },
    },
    Shape {
        name: "beacon",
        about: "a post up from the origin with a glowing head and a cap",
        params: &[num("height", 0.5, 40.0, None, "height")],
        paints: &["paint", "accent", "lights"],
        build: |c, v| {
            let h = v.num("height");
            let w = (h * 0.14).max(0.08);
            c.paint("paint");
            c.b.frustum(w * 1.6, w, h * 0.72, 5);
            c.paint("lights");
            c.at(Mat4::from_translation(Vec3::Z * h * 0.76), |b| {
                b.lathe(&[(0.0, -h * 0.1), (h * 0.14, 0.0), (0.0, h * 0.1)], 4, 0.3)
            });
            c.paint("accent");
            c.at(Mat4::from_translation(Vec3::Z * h * 0.9), |b| {
                cap(b, w * 1.4, h * 0.18, 6)
            });
            Ok(())
        },
    },
    Shape {
        name: "block",
        about: "a building standing on the origin: walls, a roof, a door and two lit windows facing +x",
        params: &[
            num("width", 0.3, 120.0, None, "along y"),
            num("depth", 0.3, 120.0, None, "along x"),
            num("height", 0.3, 120.0, None, "to the top of the roof"),
        ],
        paints: &["paint", "trim", "accent", "lights"],
        build: |c, v| {
            let (w, d, h) = (v.num("width"), v.num("depth"), v.num("height"));
            let t = (d.min(w) * 0.05).max(0.02);
            c.paint("paint");
            c.at(Mat4::from_translation(Vec3::Z * h * 0.42), |b| {
                b.cuboid(Vec3::new(d, w, h * 0.84))
            });
            c.paint("accent");
            c.at(Mat4::from_translation(Vec3::Z * h * 0.84), |b| {
                cap(b, d.min(w) * 0.42, h * 0.16, 4)
            });
            c.paint("trim");
            c.at(
                Mat4::from_translation(Vec3::new(d * 0.5, 0.0, h * 0.2)),
                |b| b.cuboid(Vec3::new(t * 2.0, w * 0.22, h * 0.4)),
            );
            c.paint("lights");
            for y in [-0.28_f32, 0.28] {
                c.at(
                    Mat4::from_translation(Vec3::new(d * 0.5, y * w, h * 0.58)),
                    |b| b.cuboid(Vec3::new(t * 1.6, w * 0.14, h * 0.14)),
                );
            }
            Ok(())
        },
    },
    Shape {
        name: "scaffold",
        about: "an open frame of posts round the origin with braces, a deck and a spike on top",
        params: &[
            num("radius", 0.3, 60.0, None, "from the centre to the posts"),
            num("height", 0.3, 120.0, None, "height"),
            int("posts", 3, 8, 5, "posts round"),
        ],
        paints: &["paint", "trim", "accent"],
        build: |c, v| {
            let (radius, height, posts) = (v.num("radius"), v.num("height"), v.int("posts"));
            let thick = (radius * 0.1).max(0.05);
            let p = |a: f32, z: f32| {
                Vec3::new(libm::cosf(a), libm::sinf(a), 0.0) * radius * 0.82 + Vec3::Z * z
            };
            c.paint("paint");
            for k in 0..posts {
                let a = k as f32 / posts as f32 * TAU;
                strut(c.b, p(a, 0.0), p(a, height), thick);
            }
            for level in [0.38, 0.78] {
                for k in 0..posts {
                    let (a0, a1) = (
                        k as f32 / posts as f32 * TAU,
                        (k + 1) as f32 / posts as f32 * TAU,
                    );
                    strut(
                        c.b,
                        p(a0, height * level),
                        p(a1, height * level),
                        thick * 0.75,
                    );
                }
            }
            c.paint("trim");
            c.at(Mat4::from_translation(Vec3::Z * height * 0.7), |b| {
                b.cuboid(Vec3::new(radius * 1.15, radius * 0.5, thick * 0.6))
            });
            c.paint("accent");
            c.at(Mat4::from_translation(Vec3::Z * height), |b| {
                b.frustum(radius * 0.12, 0.0, radius * 0.4, 4)
            });
            Ok(())
        },
    },
    Shape {
        name: "spire",
        about: "a tall tapering tower up from the origin with a band of lit windows and a cap",
        params: &[
            num("height", 1.0, 120.0, None, "height"),
            num("radius", 0.2, 30.0, None, "radius at the foot"),
        ],
        paints: &["paint", "accent", "lights"],
        build: |c, v| {
            let (height, radius) = (v.num("height"), v.num("radius"));
            c.paint("paint");
            c.b.lathe(
                &[
                    (radius * 1.05, 0.0),
                    (radius, height * 0.12),
                    (radius * 0.72, height * 0.4),
                    (radius * 0.4, height * 0.68),
                    (radius * 0.18, height * 0.84),
                ],
                8,
                0.15,
            );
            c.paint("lights");
            let r = radius * 0.62;
            for k in 0..6 {
                let a = k as f32 / 6.0 * TAU;
                c.at(
                    Mat4::from_translation(Vec3::new(
                        libm::cosf(a) * r,
                        libm::sinf(a) * r,
                        height * 0.46,
                    )) * rot_z(a),
                    |b| b.cuboid(Vec3::new(radius * 0.08, radius * 0.28, height * 0.05)),
                );
            }
            c.paint("accent");
            c.at(Mat4::from_translation(Vec3::Z * height * 0.84), |b| {
                cap(b, radius * 0.34, height * 0.18, 6)
            });
            Ok(())
        },
    },
    Shape {
        name: "petals",
        about: "flat petals fanned round a short hub on the origin",
        params: &[
            num("length", 0.5, 60.0, None, "each petal's reach"),
            int("count", 2, 12, 3, "petals"),
        ],
        paints: &["paint", "trim"],
        build: |c, v| {
            let (length, count) = (v.num("length"), v.int("count"));
            c.paint("trim");
            c.b.frustum(length * 0.1, length * 0.07, length * 0.1, 6);
            for k in 0..count {
                let a = k as f32 / count as f32 * TAU;
                if k % 2 == 0 {
                    c.paint("paint");
                } else {
                    c.paint_mix("paint", "trim", 0.45);
                }
                c.at(
                    rot_z(a) * Mat4::from_translation(Vec3::new(length * 0.55, 0.0, length * 0.06)),
                    |b| b.cuboid(Vec3::new(length * 0.82, length * 0.22, length * 0.03)),
                );
                c.paint("trim");
                c.at(
                    rot_z(a) * Mat4::from_translation(Vec3::new(length * 0.55, 0.0, length * 0.08)),
                    |b| b.cuboid(Vec3::new(length * 0.62, length * 0.05, length * 0.015)),
                );
            }
            Ok(())
        },
    },
    Shape {
        name: "cabins",
        about: "a ring lying flat round the origin carrying cabins, each with a lit window facing out",
        params: &[
            num(
                "radius",
                1.0,
                120.0,
                None,
                "from the centre to the tube's middle",
            ),
            num(
                "tube",
                0.1,
                20.0,
                None,
                "the ring's tube; cabins are about twice it",
            ),
            int("count", 3, 16, 6, "cabins"),
        ],
        paints: &["paint", "trim", "lights"],
        build: |c, v| {
            let (radius, tube, count) = (v.num("radius"), v.num("tube"), v.int("count"));
            if tube > radius * 0.5 {
                return Err((
                    "tube",
                    format!("is {tube}; at most half the radius ({})", radius * 0.5),
                ));
            }
            let segs = ((radius * 0.7) as u32).clamp(12, 28);
            c.paint("trim");
            c.b.torus(radius, tube * 1.35, tube, segs, 6);
            for k in 0..count {
                let a = k as f32 / count as f32 * TAU;
                let p =
                    Vec3::new(libm::cosf(a), libm::sinf(a), 0.0) * radius + Vec3::Z * tube * 0.15;
                let at = Mat4::from_translation(p) * rot_z(a);
                if k % 2 == 0 {
                    c.paint("paint");
                } else {
                    c.paint_mix("paint", "trim", 0.3);
                }
                c.at(at, |b| {
                    b.cuboid(Vec3::new(tube * 2.2, tube * 1.7, tube * 1.45))
                });
                c.paint("lights");
                c.at(
                    at * Mat4::from_translation(Vec3::new(tube * 1.1, 0.0, tube * 0.1)),
                    |b| b.cuboid(Vec3::new(tube * 0.12, tube * 0.7, tube * 0.4)),
                );
            }
            Ok(())
        },
    },
    Shape {
        name: "body",
        about: "a rounded body along x centred on the origin, nose at +x, oval in section, with a waist band",
        params: &[
            num("length", 0.2, 120.0, None, "nose to tail"),
            num("width", 0.1, 60.0, None, "side to side"),
            num("height", 0.1, 60.0, None, "top to bottom"),
            num(
                "nose",
                0.0,
                1.0,
                Some(0.5),
                "how pointed the nose is: 0 blunt, 1 sharp",
            ),
            num(
                "shoulder",
                0.5,
                1.0,
                Some(0.88),
                "how full the body is at its waist: 1 boxy",
            ),
            int("sides", 4, 32, 8, "sides round"),
            flag("band", true, "a trim band round the waist"),
        ],
        paints: &["paint", "trim", "accent"],
        build: |c, v| {
            let (l, w, h, nose) = (
                v.num("length"),
                v.num("width"),
                v.num("height"),
                v.num("nose"),
            );
            let shoulder = v.num("shoulder");
            let sides = v.int("sides");
            let phase = PI / sides as f32;
            let tip = 0.06 + 0.12 * (1.0 - nose);
            let tail = [(0.55, -0.5), (0.86, -0.36), (1.0, -0.12), (shoulder, 0.08)];
            let front = [
                (shoulder, 0.08),
                (0.72 - 0.25 * nose, 0.28),
                (0.42 - 0.2 * nose, 0.42),
                (tip, 0.5),
            ];
            // The lathe's axis is +z; turned so it runs along +x, scaled by the length,
            // and its radius spans y and z by the half-width and half-height.
            let frame = Mat4::from_scale(Vec3::new(l, w * 0.5, h * 0.5)) * rot_y(FRAC_PI_2);
            c.paint("paint");
            c.at(frame, |b| b.lathe(&tail, sides, phase));
            c.paint("accent");
            c.at(frame, |b| b.lathe(&front, sides, phase));
            if v.flag("band") {
                c.paint("trim");
                c.at(frame, |b| {
                    b.lathe(&[(1.04, -0.16), (1.04, -0.06)], sides, phase)
                });
            }
            Ok(())
        },
    },
    Shape {
        name: "wing",
        about: "a flat swept wing reaching out along +y from its root at the origin, leading edge at +x; mirror = true adds the other side",
        params: &[
            num("span", 0.1, 120.0, None, "root to tip, along y"),
            num("chord", 0.1, 60.0, None, "the root's length along x"),
            num(
                "tip",
                0.0,
                1.5,
                Some(0.35),
                "the tip's length over the root's",
            ),
            num(
                "sweep",
                -60.0,
                60.0,
                Some(0.5),
                "how far back the tip's leading edge sits",
            ),
            num(
                "thickness",
                0.01,
                10.0,
                Some(0.2),
                "thickness, centred on z = 0",
            ),
            flag("light", false, "a lamp on the tip"),
        ],
        paints: &["paint", "lights"],
        build: |c, v| {
            let (span, chord, sweep, t) = (
                v.num("span"),
                v.num("chord"),
                v.num("sweep"),
                v.num("thickness"),
            );
            let tip = (chord * v.num("tip")).max(0.01);
            let outline = ccw(&[
                Vec2::new(0.0, 0.0),
                Vec2::new(-chord, 0.0),
                Vec2::new(-sweep - tip, span),
                Vec2::new(-sweep, span),
            ]);
            c.paint("paint");
            c.at(Mat4::from_translation(Vec3::Z * -t * 0.5), |b| {
                b.extrude(&outline, t, 0.92)
            });
            if v.flag("light") {
                c.paint("lights");
                let s = (t * 1.6).max(0.05);
                c.at(
                    Mat4::from_translation(Vec3::new(-sweep - tip * 0.5, span - s * 0.5, t * 0.5)),
                    |b| b.cuboid(Vec3::splat(s)),
                );
            }
            Ok(())
        },
    },
    Shape {
        name: "fin",
        about: "an upright fin standing on the origin, its root running back along -x",
        params: &[
            num(
                "length",
                0.1,
                60.0,
                None,
                "the root's length, back from the origin",
            ),
            num("height", 0.1, 60.0, None, "how tall"),
            num(
                "sweep",
                0.0,
                1.0,
                Some(0.5),
                "how far back the top leans, over the length",
            ),
            num(
                "thickness",
                0.01,
                5.0,
                Some(0.24),
                "thickness, centred on y = 0",
            ),
        ],
        paints: &["paint"],
        build: |c, v| {
            let (l, h, t) = (v.num("length"), v.num("height"), v.num("thickness"));
            let lead = -l * v.num("sweep") * 0.7;
            let outline = ccw(&[
                Vec2::new(0.0, 0.0),
                Vec2::new(lead, h),
                Vec2::new(lead - l * 0.3, h * 0.92),
                Vec2::new(-l, 0.0),
            ]);
            c.paint("paint");
            // The outline is drawn in x and z: extrude's y turns to z.
            c.at(
                Mat4::from_translation(Vec3::Y * t * 0.5) * rot_x(FRAC_PI_2),
                |b| b.extrude(&outline, t, 0.85),
            );
            Ok(())
        },
    },
    Shape {
        name: "nozzle",
        about: "an engine bell opening backwards along -x from the origin, with a glowing core",
        params: &[
            num("radius", 0.05, 20.0, None, "the bell's radius"),
            int("sides", 4, 32, 8, "sides round"),
        ],
        paints: &["paint", "lights"],
        build: |c, v| {
            let (r, sides) = (v.num("radius"), v.int("sides"));
            let back = rot_y(-FRAC_PI_2);
            c.paint("paint");
            c.at(back, |b| {
                b.lathe(
                    &[(r * 0.9, -0.1 * r), (r * 1.15, r * 0.9), (r, r * 1.6)],
                    sides,
                    0.0,
                )
            });
            c.paint("lights");
            c.at(back, |b| {
                b.lathe(&[(r * 0.75, r * 1.62), (0.0, r * 1.68)], sides, 0.0)
            });
            Ok(())
        },
    },
    Shape {
        name: "rock",
        about: "a lumpy rock centred on the origin; mined above 0 gives it a morph target: chipped, cut and shrunk as if mined, which engine.model morph runs towards",
        params: &[
            num("radius", 0.1, 200.0, None, "about how far it reaches"),
            num("lumps", 0.0, 1.0, Some(0.5), "how lumpy: 0 a faceted ball"),
            num("squash", 0.3, 1.5, Some(0.85), "its height over its width"),
            int(
                "detail",
                0,
                3,
                1,
                "how finely it is faceted (each step is four times the faces)",
            ),
            num(
                "mined",
                0.0,
                0.9,
                Some(0.0),
                "how much the morph target has lost; 0 for no morph",
            ),
            int("chips", 1, 6, 4, "flat cuts in the morph target"),
        ],
        paints: &["paint", "accent"],
        build: |c, v| {
            crate::rock::build(c, v);
            Ok(())
        },
    },
];

/// A shape by name.
pub fn find(name: &str) -> Option<&'static Shape> {
    SHAPES.iter().find(|s| s.name == name)
}

/// The cap on tiers, beacons and spires: a rounded cone `height` tall.
fn cap(b: &mut MeshBuilder, radius: f32, height: f32, sides: u32) {
    if height <= 0.0 {
        return;
    }
    b.lathe(
        &[
            (radius, 0.0),
            (radius * 0.72, height * 0.45),
            (radius * 0.28, height * 0.82),
            (0.0, height),
        ],
        sides.clamp(4, 12),
        0.0,
    );
}

fn tiers(c: &mut Ctx, radius: f32, height: f32, tiers: u32, sides: u32, cap_ratio: f32) {
    let phase = PI / sides as f32;
    let tier_h = height / tiers as f32;
    let z0 = -height * 0.5;
    for t in 0..tiers {
        let r = radius * (1.0 - t as f32 * (0.5 / tiers as f32));
        let z = z0 + t as f32 * tier_h;
        if t % 2 == 0 {
            c.paint("paint");
        } else {
            c.paint_mix("paint", "accent", 0.25);
        }
        c.b.lathe(
            &[
                (r * 0.88, z),
                (r, z + tier_h * 0.25),
                (r, z + tier_h * 0.75),
                (r * 0.9, z + tier_h),
            ],
            sides,
            phase,
        );
        c.paint("trim");
        c.b.lathe(
            &[(r * 1.03, z + tier_h * 0.3), (r * 1.03, z + tier_h * 0.5)],
            sides,
            phase,
        );
        c.paint("lights");
        for k in 0..sides {
            let a = (k as f32 + 0.5) / sides as f32 * TAU + phase;
            let at = Vec3::new(libm::cosf(a), libm::sinf(a), 0.0) * r * 0.99
                + Vec3::Z * (z + tier_h * 0.63);
            c.at(Mat4::from_translation(at) * rot_z(a), |b| {
                b.cuboid(Vec3::new(r * 0.08, r * 0.22, tier_h * 0.16))
            });
        }
    }
    if cap_ratio <= 0.0 {
        return;
    }
    let top = z0 + height;
    let rc = radius * (1.0 - (tiers - 1) as f32 * (0.5 / tiers as f32)) * 0.9;
    let cap_h = rc * cap_ratio;
    c.paint("accent");
    c.at(Mat4::from_translation(Vec3::Z * top), |b| {
        cap(b, rc * 0.85, cap_h, sides)
    });
    c.paint("lights");
    c.at(
        Mat4::from_translation(Vec3::Z * (top + cap_h * 0.92)),
        |b| {
            b.lathe(
                &[(rc * 0.14, 0.0), (rc * 0.1, rc * 0.12), (0.0, rc * 0.16)],
                6,
                0.0,
            )
        },
    );
}

/// A square beam from `from` to `to`, `thick` on a side.
pub fn strut(b: &mut MeshBuilder, from: Vec3, to: Vec3, thick: f32) {
    let d = to - from;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let x = d / len;
    let helper = if x.z.abs() > 0.9 { Vec3::Y } else { Vec3::Z };
    let y = helper.cross(x).normalize();
    let z = x.cross(y);
    let mid = (from + to) * 0.5;
    let m = Mat4::from_cols(
        (x * len).extend(0.0),
        (y * thick).extend(0.0),
        (z * thick).extend(0.0),
        mid.extend(1.0),
    );
    b.with(m, |b| b.cuboid(Vec3::ONE));
}

/// Turns about +z, +y and +x by `a` radians, with portable sines.
pub fn rot_z(a: f32) -> Mat4 {
    let (s, c) = (libm::sinf(a), libm::cosf(a));
    Mat4::from_cols(
        Vec4::new(c, s, 0.0, 0.0),
        Vec4::new(-s, c, 0.0, 0.0),
        Vec4::Z,
        Vec4::W,
    )
}

pub fn rot_y(a: f32) -> Mat4 {
    let (s, c) = (libm::sinf(a), libm::cosf(a));
    Mat4::from_cols(
        Vec4::new(c, 0.0, -s, 0.0),
        Vec4::Y,
        Vec4::new(s, 0.0, c, 0.0),
        Vec4::W,
    )
}

pub fn rot_x(a: f32) -> Mat4 {
    let (s, c) = (libm::sinf(a), libm::cosf(a));
    Mat4::from_cols(
        Vec4::X,
        Vec4::new(0.0, c, s, 0.0),
        Vec4::new(0.0, -s, c, 0.0),
        Vec4::W,
    )
}

fn signed_area(pts: &[Vec2]) -> f32 {
    (0..pts.len())
        .map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()]))
        .sum::<f32>()
        * 0.5
}

/// The outline counter-clockwise, as extrude wants it.
fn ccw(pts: &[Vec2]) -> Vec<Vec2> {
    let mut v = pts.to_vec();
    if signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
}
