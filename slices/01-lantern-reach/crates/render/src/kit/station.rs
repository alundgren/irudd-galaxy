//! Stations as recipes: a list of parametric parts plus a palette.
//!
//! A [`StationRecipe`] is plain serde data, so authored stations, the Kettle Dock
//! greenhouse stages and stations the game master invents later are all the same thing.
//! Validate a recipe with [`StationRecipe::validate`], then build it with [`station`].
//!
//! Local axes: the station centre is the origin, the play plane is `z = 0`, +Z is up.
//! Each part sits at a polar [`Placement`]; in a part's own frame +X points away from the
//! station centre.

use super::rng::Rng;
use crate::art_bible::ArtBible;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use crate::palette;
use glam::{Mat4, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::fmt;

/// The colours a station is painted with. Parts pick slots from it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StationPalette {
    /// Main hull plating.
    pub hull: Color,
    /// Bands, trusses and rings.
    pub trim: Color,
    /// Caps, tanks and highlights.
    pub accent: Color,
    /// Windows, beacons and lanterns (self-lit).
    pub glow: Color,
    /// Greenhouse glass.
    pub glass: Color,
    /// Plants.
    pub leaf: Color,
}

impl Default for StationPalette {
    fn default() -> Self {
        Self {
            hull: palette::HULL_CREAM,
            trim: palette::TERRACOTTA,
            accent: palette::TEAL,
            glow: palette::WINDOW,
            glass: Color::hex(0x9fe0c8),
            leaf: palette::LEAF,
        }
    }
}

impl StationPalette {
    /// The faction's authored paint, from the art bible.
    pub fn from_faction(style: &crate::art_bible::FactionStyle) -> Self {
        Self {
            hull: style.hull,
            trim: style.trim,
            accent: style.accent,
            glow: style.glow,
            glass: style.glass,
            leaf: style.leaf,
        }
    }
}

/// Where a part sits, in polar coordinates around the station centre.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    /// Direction from the centre, degrees counter-clockwise from +X.
    pub angle: f32,
    /// Distance from the centre in world units.
    pub distance: f32,
    /// Height above the play plane.
    pub z: f32,
    /// Extra turn of the part around its own Z axis, degrees.
    pub turn: f32,
    /// Uniform scale.
    pub scale: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            angle: 0.0,
            distance: 0.0,
            z: 0.0,
            turn: 0.0,
            scale: 1.0,
        }
    }
}

impl Placement {
    pub fn polar(angle: f32, distance: f32) -> Self {
        Self {
            angle,
            distance,
            ..Self::default()
        }
    }

    pub fn matrix(&self) -> Mat4 {
        let a = self.angle.to_radians();
        Mat4::from_translation(Vec3::new(
            a.cos() * self.distance,
            a.sin() * self.distance,
            self.z,
        )) * Mat4::from_rotation_z(a + self.turn.to_radians())
            * Mat4::from_scale(Vec3::splat(self.scale))
    }
}

/// A station part and its parameters. In JSON the variant is the `"part"` field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum PartKind {
    /// The core: stacked tiers with a ring of lit windows and a capped top.
    Hub {
        radius: f32,
        height: f32,
        #[serde(default = "default_tiers")]
        tiers: u32,
        #[serde(default = "default_sides")]
        sides: u32,
    },
    /// The ring a ship drifts into to dock. Spokes run inwards to `inner`.
    DockingRing {
        radius: f32,
        #[serde(default = "default_tube")]
        tube: f32,
        #[serde(default = "default_spokes")]
        spokes: u32,
        #[serde(default)]
        inner: f32,
    },
    /// A truss reaching outwards from the placement.
    Arm { length: f32, width: f32 },
    /// A greenhouse that grows through `stage` 0 (scaffold) to 3 (glazed and blooming).
    Greenhouse { radius: f32, stage: u8 },
    /// A mast with a dish and a blinking tip.
    Antenna {
        height: f32,
        #[serde(default = "default_true")]
        dish: bool,
    },
    /// Side-by-side storage capsules.
    Tank {
        radius: f32,
        length: f32,
        #[serde(default = "default_one")]
        count: u32,
    },
    /// A flat solar sail reaching outwards.
    Sail { length: f32, width: f32 },
    /// A warm lantern on a post: the Lantern Reach's signature.
    Lantern { height: f32 },
    /// A big glass lens in a brass rim on a mount, tilted outwards: an observatory's
    /// telescope.
    Lens { radius: f32 },
}

fn default_tiers() -> u32 {
    2
}
fn default_sides() -> u32 {
    8
}
fn default_tube() -> f32 {
    1.6
}
fn default_spokes() -> u32 {
    4
}
fn default_true() -> bool {
    true
}
fn default_one() -> u32 {
    1
}

/// One part of a recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    #[serde(flatten)]
    pub kind: PartKind,
    #[serde(default)]
    pub at: Placement,
    /// Overrides the part's main colour slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl Part {
    pub fn new(kind: PartKind, at: Placement) -> Self {
        Self {
            kind,
            at,
            color: None,
        }
    }

    pub fn colored(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

/// A station: a name, a seed for small variations, a palette and parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StationRecipe {
    pub name: String,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub palette: StationPalette,
    pub parts: Vec<Part>,
    /// Faction id (`faction.wharf_cooperative`). The art bible checks its colour pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    /// Region id. When set, the hull must stay lighter than that region's sky.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// Limits a recipe must stay within.
pub const MAX_PARTS: usize = 48;
/// No part may reach further than this from the station centre.
pub const MAX_STATION_RADIUS: f32 = 120.0;

/// Something wrong with a recipe. `part` is the index into `parts`, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeError {
    pub part: Option<usize>,
    pub message: String,
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.part {
            Some(i) => write!(f, "part {i}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

/// A self-lit point on a station, for glows and point lights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationLight {
    pub position: Vec3,
    pub color: Color,
    /// Suggested glow radius in world units.
    pub radius: f32,
}

impl StationRecipe {
    /// Checks part counts, ranges and reach. A recipe that validates always builds a
    /// well-formed mesh.
    pub fn validate(&self) -> Result<(), Vec<RecipeError>> {
        let mut errs = Vec::new();
        let mut err =
            |part: Option<usize>, message: String| errs.push(RecipeError { part, message });
        if self.parts.is_empty() {
            err(None, "a station needs at least one part".into());
        }
        if self.parts.len() > MAX_PARTS {
            err(
                None,
                format!("{} parts; at most {MAX_PARTS}", self.parts.len()),
            );
        }
        if !self
            .parts
            .iter()
            .any(|p| matches!(p.kind, PartKind::Hub { .. }))
        {
            err(None, "a station needs a hub".into());
        }
        for (i, part) in self.parts.iter().enumerate() {
            let at = &part.at;
            let mut check = |name: &str, v: f32, lo: f32, hi: f32| {
                if !v.is_finite() || v < lo || v > hi {
                    err(Some(i), format!("{name} is {v}; expected {lo}..={hi}"));
                }
            };
            check("at.angle", at.angle, -720.0, 720.0);
            check("at.distance", at.distance, 0.0, MAX_STATION_RADIUS);
            check("at.z", at.z, -40.0, 40.0);
            check("at.turn", at.turn, -720.0, 720.0);
            check("at.scale", at.scale, 0.1, 5.0);
            match part.kind {
                PartKind::Hub {
                    radius,
                    height,
                    tiers,
                    sides,
                } => {
                    check("radius", radius, 1.0, 40.0);
                    check("height", height, 0.5, 40.0);
                    check("tiers", tiers as f32, 1.0, 4.0);
                    check("sides", sides as f32, 5.0, 24.0);
                }
                PartKind::DockingRing {
                    radius,
                    tube,
                    spokes,
                    inner,
                } => {
                    check("radius", radius, 2.0, 60.0);
                    check("tube", tube, 0.2, radius * 0.5);
                    check("spokes", spokes as f32, 0.0, 12.0);
                    check("inner", inner, 0.0, radius);
                }
                PartKind::Arm { length, width } => {
                    check("length", length, 0.5, 80.0);
                    check("width", width, 0.2, 12.0);
                }
                PartKind::Greenhouse { radius, stage } => {
                    check("radius", radius, 1.0, 30.0);
                    check("stage", stage as f32, 0.0, 3.0);
                }
                PartKind::Antenna { height, .. } => check("height", height, 0.5, 40.0),
                PartKind::Tank {
                    radius,
                    length,
                    count,
                } => {
                    check("radius", radius, 0.3, 12.0);
                    check("length", length, 0.5, 40.0);
                    check("count", count as f32, 1.0, 4.0);
                }
                PartKind::Sail { length, width } => {
                    check("length", length, 0.5, 60.0);
                    check("width", width, 0.5, 30.0);
                }
                PartKind::Lantern { height } => check("height", height, 0.3, 20.0),
                PartKind::Lens { radius } => check("radius", radius, 1.0, 30.0),
            }
            let reach = at.distance + part_reach(&part.kind) * at.scale;
            if reach.is_finite() && reach > MAX_STATION_RADIUS {
                err(
                    Some(i),
                    format!("reaches {reach:.1} from the centre; at most {MAX_STATION_RADIUS}"),
                );
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// Checks colours, part counts and proportions against the art bible.
    ///
    /// [`Self::validate`] still checks that a recipe can build. This one checks that it
    /// belongs in the Lantern Reach: faction pairs, no grey or chrome paint, and the
    /// bible's budgets. The game master calls it before accepting a station.
    pub fn validate_art(&self, bible: &ArtBible) -> Result<(), Vec<RecipeError>> {
        let mut errs = Vec::new();
        let mut err =
            |part: Option<usize>, message: String| errs.push(RecipeError { part, message });

        if self.parts.len() < bible.budget.min_parts || self.parts.len() > bible.budget.max_parts {
            err(
                None,
                format!(
                    "{} parts; expected {}..={}",
                    self.parts.len(),
                    bible.budget.min_parts,
                    bible.budget.max_parts
                ),
            );
        }
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for (i, part) in self.parts.iter().enumerate() {
            let (kind, params) = part_params(&part.kind);
            *counts.entry(kind).or_default() += 1;
            for (name, value) in params {
                if let Some(message) = bible.proportion_issue(kind, name, value) {
                    err(Some(i), message);
                }
            }
            if let Some(color) = part.color
                && let Some(issue) = bible.check_paint(color)
            {
                err(Some(i), format!("paint {}", issue.message));
            }
        }
        for (kind, n) in &counts {
            let cap = bible.kind_cap(kind);
            if *n > cap as usize {
                err(None, format!("{n} {kind} parts; at most {cap}"));
            }
        }

        for (role, color) in [
            ("hull", self.palette.hull),
            ("trim", self.palette.trim),
            ("accent", self.palette.accent),
            ("glow", self.palette.glow),
            ("glass", self.palette.glass),
            ("leaf", self.palette.leaf),
        ] {
            if let Some(issue) = bible.check_role(role, color) {
                let slot = issue.role.unwrap_or(role);
                err(None, format!("{slot} {}", issue.message));
            }
        }

        let sky_l = self
            .region
            .as_deref()
            .and_then(|id| bible.sky_lightness(id))
            .unwrap_or_else(|| crate::art_bible::oklab_l(crate::palette::SPACE_HIGH));
        let hull_l = crate::art_bible::oklab_l(self.palette.hull);
        if hull_l + 1e-4 < sky_l + bible.ranges.hull_above_sky {
            err(
                None,
                format!(
                    "hull lightness {hull_l:.2} is under {sky_l:.2} + {:.2} above the sky",
                    bible.ranges.hull_above_sky
                ),
            );
        }

        if let Some(id) = &self.faction {
            match bible.faction(id) {
                None => err(None, format!("unknown faction {id}")),
                Some(style) => {
                    let slots = [
                        self.palette.hull,
                        self.palette.trim,
                        self.palette.accent,
                        self.palette.glow,
                    ];
                    let near = |swatch: Color| {
                        slots.iter().any(|c| {
                            ArtBible::oklab_distance(*c, swatch) <= bible.ranges.faction_distance
                        })
                    };
                    if !near(style.primary) {
                        err(
                            None,
                            format!("palette misses {id} primary {}", style.primary.to_hex()),
                        );
                    }
                    if !near(style.secondary) {
                        err(
                            None,
                            format!("palette misses {id} secondary {}", style.secondary.to_hex()),
                        );
                    }
                }
            }
        }

        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// The radius of a circle around the centre that contains every part.
    pub fn footprint(&self) -> f32 {
        self.parts
            .iter()
            .map(|p| p.at.distance + part_reach(&p.kind) * p.at.scale)
            .fold(0.0, f32::max)
    }

    /// Lantern Wharf: the home station. A big friendly hub inside its docking ring, three
    /// arms with lanterns, the hullwright's sail and a pair of tanks.
    pub fn lantern_wharf() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        Self {
            name: "Lantern Wharf".into(),
            seed: 1,
            palette: StationPalette::from_faction(coop()),
            faction: Some("faction.wharf_cooperative".into()),
            region: Some("region.lantern_shallows".into()),
            parts: vec![
                p(
                    Hub {
                        radius: 8.0,
                        height: 6.0,
                        tiers: 3,
                        sides: 10,
                    },
                    Placement::default(),
                ),
                p(
                    DockingRing {
                        radius: 17.0,
                        tube: 2.0,
                        spokes: 4,
                        inner: 7.0,
                    },
                    Placement {
                        turn: 45.0,
                        ..Placement::default()
                    },
                ),
                p(
                    Arm {
                        length: 14.0,
                        width: 2.2,
                    },
                    Placement::polar(90.0, 17.5),
                ),
                p(Lantern { height: 3.0 }, Placement::polar(90.0, 31.0)),
                p(
                    Arm {
                        length: 12.0,
                        width: 2.2,
                    },
                    Placement::polar(210.0, 17.5),
                ),
                p(Lantern { height: 3.0 }, Placement::polar(210.0, 29.0)),
                p(
                    Arm {
                        length: 10.0,
                        width: 2.2,
                    },
                    Placement::polar(330.0, 17.5),
                ),
                p(
                    Tank {
                        radius: 1.8,
                        length: 7.0,
                        count: 2,
                    },
                    Placement::polar(330.0, 25.0),
                ),
                p(
                    Sail {
                        length: 12.0,
                        width: 5.0,
                    },
                    Placement::polar(150.0, 19.0),
                ),
                p(
                    Antenna {
                        height: 7.0,
                        dish: true,
                    },
                    Placement {
                        z: 3.0,
                        ..Placement::polar(20.0, 4.5)
                    },
                ),
            ],
        }
    }

    /// Kettle Dock: the greenhouse co-op. `stage` (0..=3) is the greenhouse's growth.
    pub fn kettle_dock(stage: u8) -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        Self {
            name: "Kettle Dock".into(),
            seed: 2,
            palette: StationPalette::from_faction(coop()),
            faction: Some("faction.wharf_cooperative".into()),
            region: Some("region.lantern_shallows".into()),
            parts: vec![
                p(
                    Hub {
                        radius: 6.0,
                        height: 5.0,
                        tiers: 2,
                        sides: 8,
                    },
                    Placement::default(),
                ),
                p(
                    Arm {
                        length: 9.0,
                        width: 2.0,
                    },
                    Placement::polar(0.0, 5.5),
                ),
                p(
                    DockingRing {
                        radius: 9.0,
                        tube: 1.5,
                        spokes: 0,
                        inner: 0.0,
                    },
                    Placement::polar(0.0, 23.0),
                ),
                p(
                    Arm {
                        length: 6.0,
                        width: 2.0,
                    },
                    Placement::polar(160.0, 5.5),
                ),
                p(
                    Greenhouse {
                        radius: 7.5,
                        stage: stage.min(3),
                    },
                    Placement::polar(160.0, 18.0),
                ),
                p(
                    Tank {
                        radius: 1.6,
                        length: 5.0,
                        count: 3,
                    },
                    Placement::polar(250.0, 10.0),
                ),
                p(Lantern { height: 2.5 }, Placement::polar(85.0, 7.5)),
                p(
                    Antenna {
                        height: 5.0,
                        dish: false,
                    },
                    Placement {
                        z: 2.5,
                        ..Placement::polar(300.0, 3.0)
                    },
                ),
            ],
        }
    }

    /// Brassmouth: the Tollway Compact's trade hub. Big, brassy and busy with lanterns.
    pub fn brassmouth() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        let mut parts = vec![
            p(
                Hub {
                    radius: 10.0,
                    height: 7.0,
                    tiers: 3,
                    sides: 12,
                },
                Placement::default(),
            ),
            p(
                DockingRing {
                    radius: 22.0,
                    tube: 2.4,
                    spokes: 6,
                    inner: 9.0,
                },
                Placement::default(),
            ),
        ];
        for i in 0..4 {
            let a = 45.0 + i as f32 * 90.0;
            parts.push(p(
                Arm {
                    length: 8.0,
                    width: 2.6,
                },
                Placement::polar(a, 23.0),
            ));
            parts.push(p(
                Tank {
                    radius: 1.6,
                    length: 5.0,
                    count: 2,
                },
                Placement {
                    turn: 90.0,
                    ..Placement::polar(a, 33.0)
                },
            ));
            parts.push(p(Lantern { height: 2.5 }, Placement::polar(a + 45.0, 22.0)));
        }
        parts.push(p(
            Antenna {
                height: 9.0,
                dish: true,
            },
            Placement {
                z: 3.5,
                ..Placement::polar(200.0, 5.0)
            },
        ));
        Self {
            name: "Brassmouth".into(),
            seed: 3,
            palette: StationPalette::from_faction(faction("faction.tollway_compact")),
            faction: Some("faction.tollway_compact".into()),
            region: Some("region.brassmouth_narrows".into()),
            parts,
        }
    }
}

impl StationRecipe {
    /// Cinder Hook: a rough mining camp in the Cinder Drift. A squat hexagonal hub,
    /// mismatched arms ending in ore hoppers, a crane mast and patched plating.
    pub fn cinder_hook() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        let patch = Color::hex(0xb98a5e);
        let mut palette = StationPalette::from_faction(coop());
        // The camp is still the co-op's, but the hull is sun-baked sand.
        palette.hull = Color::hex(0xe2c09a);
        Self {
            name: "Cinder Hook".into(),
            seed: 4,
            palette,
            faction: Some("faction.wharf_cooperative".into()),
            region: Some("region.cinder_drift".into()),
            parts: vec![
                p(
                    Hub {
                        radius: 6.5,
                        height: 5.0,
                        tiers: 2,
                        sides: 6,
                    },
                    Placement::default(),
                ),
                p(
                    DockingRing {
                        radius: 13.0,
                        tube: 1.8,
                        spokes: 3,
                        inner: 6.5,
                    },
                    Placement {
                        turn: 20.0,
                        ..Placement::default()
                    },
                ),
                p(
                    Arm {
                        length: 15.0,
                        width: 2.6,
                    },
                    Placement::polar(25.0, 13.5),
                ),
                p(
                    Tank {
                        radius: 2.0,
                        length: 5.0,
                        count: 3,
                    },
                    Placement {
                        turn: 90.0,
                        ..Placement::polar(25.0, 30.0)
                    },
                )
                .colored(Color::hex(0x8a5a44)),
                p(
                    Arm {
                        length: 9.0,
                        width: 2.0,
                    },
                    Placement::polar(140.0, 13.5),
                )
                .colored(patch),
                p(
                    Antenna {
                        height: 9.0,
                        dish: false,
                    },
                    Placement {
                        z: 1.0,
                        ..Placement::polar(140.0, 22.0)
                    },
                ),
                p(
                    Arm {
                        length: 7.0,
                        width: 2.2,
                    },
                    Placement::polar(245.0, 13.5),
                ),
                p(
                    Tank {
                        radius: 1.7,
                        length: 6.0,
                        count: 2,
                    },
                    Placement {
                        turn: 90.0,
                        ..Placement::polar(245.0, 23.0)
                    },
                ),
                p(
                    Sail {
                        length: 8.0,
                        width: 3.5,
                    },
                    Placement::polar(320.0, 14.0),
                )
                .colored(patch),
                p(Lantern { height: 2.5 }, Placement::polar(85.0, 15.0)),
                p(Lantern { height: 2.5 }, Placement::polar(200.0, 15.0)),
            ],
        }
    }

    /// The Glass Drift Observatory: the Lamplighter Society's scholars. A tall, tidy
    /// hub, a great lens on an arm, a smaller lens and listening dishes.
    pub fn glass_drift_observatory() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        let mut parts = vec![
            p(
                Hub {
                    radius: 7.0,
                    height: 8.0,
                    tiers: 3,
                    sides: 12,
                },
                Placement::default(),
            ),
            p(
                DockingRing {
                    radius: 14.0,
                    tube: 1.6,
                    spokes: 6,
                    inner: 7.5,
                },
                Placement::default(),
            ),
            p(
                Arm {
                    length: 9.0,
                    width: 2.0,
                },
                Placement::polar(35.0, 14.5),
            ),
            p(Lens { radius: 8.0 }, Placement::polar(35.0, 26.0)),
            p(Lens { radius: 4.0 }, Placement::polar(215.0, 19.0)),
            p(
                Antenna {
                    height: 7.0,
                    dish: true,
                },
                Placement {
                    z: 4.0,
                    ..Placement::polar(130.0, 4.0)
                },
            ),
            p(
                Antenna {
                    height: 5.0,
                    dish: true,
                },
                Placement {
                    z: 4.0,
                    ..Placement::polar(300.0, 4.0)
                },
            ),
        ];
        for a in [100.0, 160.0, 280.0, 330.0] {
            parts.push(p(Lantern { height: 2.2 }, Placement::polar(a, 16.5)));
        }
        Self {
            name: "Glass Drift Observatory".into(),
            seed: 5,
            palette: StationPalette::from_faction(faction("faction.lamplighter_society")),
            faction: Some("faction.lamplighter_society".into()),
            region: Some("region.cinder_drift".into()),
            parts,
        }
    }
}

/// The bible's name for a part, and the parameters it checks.
fn part_params(kind: &PartKind) -> (&'static str, Vec<(&'static str, f32)>) {
    match *kind {
        PartKind::Hub {
            radius,
            height,
            tiers,
            sides,
        } => (
            "hub",
            vec![
                ("radius", radius),
                ("height", height),
                ("tiers", tiers as f32),
                ("sides", sides as f32),
            ],
        ),
        PartKind::DockingRing {
            radius,
            tube,
            spokes,
            inner,
        } => (
            "docking_ring",
            vec![
                ("radius", radius),
                ("tube", tube),
                ("spokes", spokes as f32),
                ("inner", inner),
            ],
        ),
        PartKind::Arm { length, width } => ("arm", vec![("length", length), ("width", width)]),
        PartKind::Greenhouse { radius, stage } => (
            "greenhouse",
            vec![("radius", radius), ("stage", stage as f32)],
        ),
        PartKind::Antenna { height, .. } => ("antenna", vec![("height", height)]),
        PartKind::Tank {
            radius,
            length,
            count,
        } => (
            "tank",
            vec![
                ("radius", radius),
                ("length", length),
                ("count", count as f32),
            ],
        ),
        PartKind::Sail { length, width } => ("sail", vec![("length", length), ("width", width)]),
        PartKind::Lantern { height } => ("lantern", vec![("height", height)]),
        PartKind::Lens { radius } => ("lens", vec![("radius", radius)]),
    }
}

fn faction(id: &str) -> &crate::art_bible::FactionStyle {
    ArtBible::bundled()
        .faction(id)
        .unwrap_or_else(|| panic!("art bible missing {id}"))
}

fn coop() -> &'static crate::art_bible::FactionStyle {
    faction("faction.wharf_cooperative")
}

/// How far a part reaches from its placement, before scaling.
fn part_reach(kind: &PartKind) -> f32 {
    match *kind {
        PartKind::Hub { radius, .. } => radius * 1.1,
        PartKind::DockingRing { radius, tube, .. } => radius + tube,
        PartKind::Arm { length, width } => length + width,
        PartKind::Greenhouse { radius, .. } => radius * 1.1,
        PartKind::Antenna { height, .. } => height * 0.6,
        PartKind::Tank {
            radius,
            length,
            count,
        } => length * 0.5 + radius * count as f32 * 1.2,
        PartKind::Sail { length, width } => length + width,
        PartKind::Lantern { height } => height * 0.5,
        PartKind::Lens { radius } => radius * 1.2,
    }
}

/// Builds a station mesh from a recipe. Invalid values are clamped so this never panics,
/// but only a recipe that passes [`StationRecipe::validate`] is guaranteed to look right.
pub fn station(recipe: &StationRecipe) -> MeshData {
    let mut b = MeshBuilder::new();
    let pal = recipe.palette;
    let rng = Rng::new(recipe.seed);
    for (i, part) in recipe.parts.iter().take(MAX_PARTS).enumerate() {
        let mut r = rng.fork(i as u64 + 1);
        let at = sanitize_placement(&part.at);
        b.with(at.matrix(), |b| {
            build_part(b, &part.kind, part.color, &pal, &mut r)
        });
    }
    b.build()
}

/// Self-lit points of a station (lanterns, beacons, greenhouse lamps), in station space.
pub fn station_lights(recipe: &StationRecipe) -> Vec<StationLight> {
    let mut out = Vec::new();
    let pal = recipe.palette;
    for part in recipe.parts.iter().take(MAX_PARTS) {
        let m = sanitize_placement(&part.at).matrix();
        let mut push = |local: Vec3, color: Color, radius: f32| {
            out.push(StationLight {
                position: m.transform_point3(local),
                color,
                radius: radius * part.at.scale.clamp(0.1, 5.0),
            })
        };
        match part.kind {
            PartKind::Lantern { height } => push(Vec3::Z * (height + 0.9), palette::LANTERN, 7.0),
            PartKind::Lens { radius } => {
                let r = clampf(radius, 1.0, 30.0);
                push(lens_center(r), pal.glass, r * 1.1)
            }
            PartKind::Greenhouse { radius, stage } if stage >= 2 => {
                push(Vec3::Z * radius * 0.5, Color::hex(0xc8ff9a), radius * 1.3)
            }
            PartKind::DockingRing { radius, .. } => {
                for k in 0..4 {
                    let a = k as f32 / 4.0 * TAU + TAU / 8.0;
                    push(
                        Vec3::new(a.cos() * radius, a.sin() * radius, 1.0),
                        pal.glow,
                        3.0,
                    );
                }
            }
            _ => {}
        }
    }
    out
}

fn sanitize_placement(at: &Placement) -> Placement {
    let f = |v: f32, lo: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(lo, hi) } else { d };
    Placement {
        angle: f(at.angle, -720.0, 720.0, 0.0),
        distance: f(at.distance, 0.0, MAX_STATION_RADIUS, 0.0),
        z: f(at.z, -40.0, 40.0, 0.0),
        turn: f(at.turn, -720.0, 720.0, 0.0),
        scale: f(at.scale, 0.1, 5.0, 1.0),
    }
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}

/// A thin box from `a` to `b`.
fn strut(b: &mut MeshBuilder, from: Vec3, to: Vec3, thick: f32) {
    let d = to - from;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let rot = Quat::from_rotation_arc(Vec3::X, d / len);
    b.with(
        Mat4::from_scale_rotation_translation(Vec3::new(len, thick, thick), rot, (from + to) * 0.5),
        |b| b.cuboid(Vec3::ONE),
    );
}

fn build_part(
    b: &mut MeshBuilder,
    kind: &PartKind,
    color: Option<Color>,
    pal: &StationPalette,
    rng: &mut Rng,
) {
    match *kind {
        PartKind::Hub {
            radius,
            height,
            tiers,
            sides,
        } => hub(
            b,
            clampf(radius, 1.0, 40.0),
            clampf(height, 0.5, 40.0),
            tiers.clamp(1, 4),
            sides.clamp(5, 24),
            color.unwrap_or(pal.hull),
            pal,
        ),
        PartKind::DockingRing {
            radius,
            tube,
            spokes,
            inner,
        } => {
            let radius = clampf(radius, 2.0, 60.0);
            let tube = clampf(tube, 0.2, radius * 0.5);
            let segments = ((radius * 1.2) as u32).clamp(16, 48);
            b.paint(color.unwrap_or(pal.trim), 0.0);
            b.torus(radius, tube * 1.3, tube, segments, 6);
            // A lighter inner lip makes the ring read as a ring from straight above.
            b.paint(pal.hull, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * tube * 0.45), |b| {
                b.torus(radius - tube * 0.2, tube * 0.6, tube * 0.3, segments, 4)
            });
            for k in 0..4 {
                let a = k as f32 / 4.0 * TAU + TAU / 8.0;
                b.paint(pal.glow, 1.0);
                b.with(
                    Mat4::from_translation(Vec3::new(
                        a.cos() * radius,
                        a.sin() * radius,
                        tube * 0.55,
                    )) * Mat4::from_rotation_z(a),
                    |b| b.cuboid(Vec3::new(tube * 0.7, tube * 0.7, tube * 0.5)),
                );
            }
            let inner = clampf(inner, 0.0, radius);
            b.paint(pal.trim, 0.0);
            for k in 0..spokes.min(12) {
                let a = k as f32 / spokes as f32 * TAU;
                let dir = Vec3::new(a.cos(), a.sin(), 0.0);
                strut(b, dir * inner, dir * (radius - tube * 0.5), tube * 0.6);
            }
        }
        PartKind::Arm { length, width } => {
            let (l, w) = (clampf(length, 0.5, 80.0), clampf(width, 0.2, 12.0));
            b.paint(color.unwrap_or(pal.hull), 0.0);
            b.with(Mat4::from_translation(Vec3::new(l * 0.5, 0.0, 0.0)), |b| {
                b.cuboid(Vec3::new(l, w, w * 0.6))
            });
            // Trim collars along the truss.
            b.paint(pal.trim, 0.0);
            let n = ((l / 5.0) as u32).max(1);
            for k in 0..=n {
                let x = l * k as f32 / n as f32;
                // Keep collars off the ends, but never let the padding cross over on
                // short, wide arms (that would make the clamp bounds invalid).
                let pad = (w * 0.2).min(l * 0.5);
                let x = x.clamp(pad, l - pad);
                b.with(Mat4::from_translation(Vec3::new(x, 0.0, 0.0)), |b| {
                    b.cuboid(Vec3::new(w * 0.35, w * 1.25, w * 0.8))
                });
            }
        }
        PartKind::Greenhouse { radius, stage } => {
            greenhouse(b, clampf(radius, 1.0, 30.0), stage.min(3), color, pal, rng)
        }
        PartKind::Antenna { height, dish } => {
            let h = clampf(height, 0.5, 40.0);
            let lean = rng.range(0.15, 0.3);
            b.with(Mat4::from_rotation_y(lean), |b| {
                b.paint(color.unwrap_or(pal.trim), 0.0);
                b.frustum(h * 0.06, h * 0.03, h, 5);
                if dish {
                    b.paint(pal.hull, 0.0);
                    b.with(
                        Mat4::from_translation(Vec3::Z * h * 0.62) * Mat4::from_rotation_y(0.9),
                        |b| {
                            b.lathe(
                                &[(0.0, 0.0), (h * 0.16, h * 0.04), (h * 0.32, h * 0.14)],
                                8,
                                0.0,
                            )
                        },
                    );
                }
                b.paint(pal.glow, 1.0);
                b.with(Mat4::from_translation(Vec3::Z * h), |b| {
                    b.cuboid(Vec3::splat(h * 0.09))
                });
            });
        }
        PartKind::Tank {
            radius,
            length,
            count,
        } => {
            let (r, l) = (clampf(radius, 0.3, 12.0), clampf(length, 0.5, 40.0));
            let count = count.clamp(1, 4);
            for k in 0..count {
                let y = (k as f32 - (count - 1) as f32 * 0.5) * r * 2.2;
                b.with(
                    Mat4::from_translation(Vec3::new(0.0, y, 0.0))
                        * Mat4::from_rotation_y(FRAC_PI_2),
                    |b| {
                        let half = l * 0.5;
                        b.paint(color.unwrap_or(pal.accent), 0.0);
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
                        );
                        b.paint(pal.hull, 0.0);
                        for z in [-half * 0.45, half * 0.45] {
                            b.with(Mat4::from_translation(Vec3::Z * z), |b| {
                                b.lathe(&[(r * 1.07, -r * 0.2), (r * 1.07, r * 0.2)], 8, 0.0)
                            });
                        }
                    },
                );
            }
        }
        PartKind::Sail { length, width } => {
            let (l, w) = (clampf(length, 0.5, 60.0), clampf(width, 0.5, 30.0));
            b.paint(pal.trim, 0.0);
            strut(b, Vec3::ZERO, Vec3::new(l, 0.0, 0.0), w * 0.12);
            let cells = ((l / 3.0) as u32).max(1);
            let cell = l / cells as f32;
            for k in 0..cells {
                let c = if k % 2 == 0 {
                    color.unwrap_or(pal.accent)
                } else {
                    color.unwrap_or(pal.accent).lerp(pal.hull, 0.35)
                };
                b.paint(c, 0.05);
                b.with(
                    Mat4::from_translation(Vec3::new(cell * (k as f32 + 0.5), 0.0, 0.2)),
                    |b| b.cuboid(Vec3::new(cell * 0.9, w, 0.25)),
                );
            }
        }
        PartKind::Lantern { height } => {
            let h = clampf(height, 0.3, 20.0);
            b.paint(pal.trim, 0.0);
            b.frustum(0.35, 0.25, h, 5);
            b.paint(palette::BRASS, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * h), |b| {
                b.lathe(&[(0.75, 0.0), (0.75, 0.15)], 6, 0.0);
                b.paint(palette::LANTERN, 1.0);
                b.lathe(&[(0.6, 0.15), (0.75, 0.9), (0.55, 1.5)], 6, 0.0);
                b.paint(color.unwrap_or(pal.trim), 0.0);
                b.lathe(&[(0.9, 1.5), (0.0, 2.2)], 6, 0.0);
            });
        }
        PartKind::Lens { radius } => lens(b, clampf(radius, 1.0, 30.0), color, pal),
    }
}

/// Where a lens of radius `r` is centred, in the part's frame.
fn lens_center(r: f32) -> Vec3 {
    Vec3::new(r * 0.25, 0.0, r * 0.75)
}

fn lens(b: &mut MeshBuilder, r: f32, color: Option<Color>, pal: &StationPalette) {
    // The mount: a squat post and a yoke.
    b.paint(pal.trim, 0.0);
    b.frustum(r * 0.32, r * 0.18, r * 0.55, 6);
    strut(b, Vec3::Z * r * 0.5, lens_center(r), r * 0.14);
    // The lens faces outwards and up, so it reads as an ellipse from above.
    b.with(
        Mat4::from_translation(lens_center(r)) * Mat4::from_rotation_y(0.95),
        |b| {
            b.paint(color.unwrap_or(palette::BRASS), 0.0);
            b.torus(r, r * 0.2, r * 0.22, 20, 6);
            // A biconvex glass lens inside the rim.
            b.paint(pal.glass, 0.35);
            b.lathe(
                &[(0.0, -r * 0.12), (r * 0.92, 0.0), (0.0, r * 0.12)],
                16,
                0.0,
            );
            // The eyepiece tube behind it.
            b.paint(pal.hull, 0.0);
            b.with(Mat4::from_translation(-Vec3::Z * r * 0.9), |b| {
                b.frustum(r * 0.18, r * 0.42, r * 0.8, 8)
            });
        },
    );
}

fn hub(
    b: &mut MeshBuilder,
    radius: f32,
    height: f32,
    tiers: u32,
    sides: u32,
    hull: Color,
    pal: &StationPalette,
) {
    let phase = std::f32::consts::PI / sides as f32;
    let tier_h = height / tiers as f32;
    let z0 = -height * 0.5;
    for t in 0..tiers {
        let shrink = 1.0 - t as f32 * 0.22;
        let r = radius * shrink;
        let z = z0 + t as f32 * tier_h;
        b.paint(
            if t % 2 == 0 {
                hull
            } else {
                pal.hull.lerp(pal.accent, 0.25)
            },
            0.0,
        );
        b.lathe(
            &[
                (r * 0.88, z),
                (r, z + tier_h * 0.25),
                (r, z + tier_h * 0.75),
                (r * 0.9, z + tier_h),
            ],
            sides,
            phase,
        );
        // A trim band with lit windows on each tier.
        b.paint(pal.trim, 0.0);
        b.lathe(
            &[(r * 1.03, z + tier_h * 0.3), (r * 1.03, z + tier_h * 0.5)],
            sides,
            phase,
        );
        b.paint(pal.glow, 1.0);
        for k in 0..sides {
            let a = (k as f32 + 0.5) / sides as f32 * TAU + phase;
            let at = Vec3::new(a.cos(), a.sin(), 0.0) * r * 0.99 + Vec3::Z * (z + tier_h * 0.63);
            b.with(Mat4::from_translation(at) * Mat4::from_rotation_z(a), |b| {
                b.cuboid(Vec3::new(r * 0.08, r * 0.22, tier_h * 0.16))
            });
        }
    }
    // Cap: a shallow faceted dome in the accent colour with a lamp on top.
    let top = z0 + height;
    let rc = radius * (1.0 - (tiers - 1) as f32 * 0.22) * 0.9;
    b.paint(pal.accent, 0.0);
    b.lathe(
        &[
            (rc, top),
            (rc * 0.8, top + rc * 0.22),
            (rc * 0.4, top + rc * 0.36),
            (0.0, top + rc * 0.4),
        ],
        sides,
        phase,
    );
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(Vec3::Z * (top + rc * 0.38)), |b| {
        b.lathe(
            &[(rc * 0.14, 0.0), (rc * 0.1, rc * 0.12), (0.0, rc * 0.16)],
            6,
            0.0,
        )
    });
}

fn greenhouse(
    b: &mut MeshBuilder,
    r: f32,
    stage: u8,
    color: Option<Color>,
    pal: &StationPalette,
    rng: &mut Rng,
) {
    let base_h = r * 0.18;
    let segments = 12;
    // Planter base and soil.
    b.paint(color.unwrap_or(pal.hull), 0.0);
    b.lathe(
        &[(r * 0.92, -base_h), (r, 0.0), (r, base_h * 0.5)],
        segments,
        0.0,
    );
    b.paint(Color::hex(0x6b4a3a), 0.0);
    b.lathe(
        &[(r * 0.94, base_h * 0.5), (r * 0.94, base_h * 0.6)],
        segments,
        0.0,
    );
    let soil = base_h * 0.6;

    if stage == 0 {
        // Scaffold posts and a mustard "coming soon" marker.
        b.paint(pal.trim, 0.0);
        for k in 0..4 {
            let a = k as f32 / 4.0 * TAU + TAU / 8.0;
            let p = Vec3::new(a.cos(), a.sin(), 0.0) * r * 0.8;
            strut(
                b,
                p + Vec3::Z * soil,
                p + Vec3::Z * (soil + r * 0.5),
                r * 0.07,
            );
        }
        b.paint(pal.accent, 0.0);
        b.with(Mat4::from_translation(Vec3::Z * soil), |b| {
            b.frustum(r * 0.12, 0.0, r * 0.3, 4)
        });
        return;
    }

    // Plants: more and taller with each stage.
    let count = [0, 5, 9, 12][stage as usize];
    let tallness = [0.0, 0.25, 0.38, 0.45][stage as usize];
    for k in 0..count {
        let a = k as f32 / count as f32 * TAU + rng.range(-0.2, 0.2);
        let d = if k % 3 == 0 { 0.25 } else { 0.62 } * r + rng.range(-0.08, 0.08) * r;
        let p = Vec3::new(a.cos() * d, a.sin() * d, soil);
        let h = r * tallness * rng.range(0.7, 1.15);
        let w = r * rng.range(0.1, 0.16);
        let leaf = pal.leaf.lerp(Color::hex(0x9be36b), rng.f32() * 0.5);
        b.paint(leaf, 0.0);
        b.with(
            Mat4::from_translation(p) * Mat4::from_rotation_z(rng.range(0.0, TAU)),
            |b| b.lathe(&[(w * 0.4, 0.0), (w, h * 0.45), (0.0, h)], 5, 0.0),
        );
        if stage == 3 && k % 2 == 0 {
            b.paint(palette::BLOSSOM, 0.2);
            b.with(Mat4::from_translation(p + Vec3::Z * h * 0.95), |b| {
                b.lathe(&[(0.0, -w * 0.4), (w * 0.55, 0.0), (0.0, w * 0.4)], 5, 0.0)
            });
        }
    }

    // Ribs: half the frame at stage 1, all of it from stage 2.
    let ribs = 8;
    let dome_h = r * 0.85;
    let arc = |a: f32, e: f32| {
        Vec3::new(
            a.cos() * e.cos() * r * 0.97,
            a.sin() * e.cos() * r * 0.97,
            soil + e.sin() * dome_h,
        )
    };
    b.paint(color.unwrap_or(pal.accent), 0.0);
    let steps = 5;
    for k in 0..ribs {
        if stage == 1 && k % 2 == 1 {
            continue;
        }
        let a = k as f32 / ribs as f32 * TAU;
        let top = if stage == 1 { steps - 2 } else { steps };
        for s in 0..top {
            let e0 = s as f32 / steps as f32 * FRAC_PI_2;
            let e1 = (s + 1) as f32 / steps as f32 * FRAC_PI_2;
            strut(b, arc(a, e0), arc(a, e1), r * 0.06);
        }
    }
    if stage >= 2 {
        // A ring part way up holds the ribs together.
        let e = FRAC_PI_2 * 0.45;
        for k in 0..ribs {
            let a0 = k as f32 / ribs as f32 * TAU;
            let a1 = (k + 1) as f32 / ribs as f32 * TAU;
            strut(b, arc(a0, e), arc(a1, e), r * 0.05);
        }
    }
    if stage == 3 {
        // Glazing on alternating panes so the garden peeks through, plus a lamp on top.
        b.paint(pal.glass, 0.25);
        for k in 0..ribs {
            for s in 0..steps - 1 {
                if (k + s) % 2 == 1 {
                    continue;
                }
                let a0 = k as f32 / ribs as f32 * TAU;
                let a1 = (k + 1) as f32 / ribs as f32 * TAU;
                let e0 = s as f32 / steps as f32 * FRAC_PI_2;
                let e1 = (s + 1) as f32 / steps as f32 * FRAC_PI_2;
                let shrink = |v: Vec3| (v - Vec3::Z * soil) * 0.985 + Vec3::Z * soil;
                b.quad(
                    shrink(arc(a0, e0)),
                    shrink(arc(a1, e0)),
                    shrink(arc(a1, e1)),
                    shrink(arc(a0, e1)),
                );
            }
        }
        b.paint(palette::LANTERN, 1.0);
        b.with(Mat4::from_translation(Vec3::Z * (soil + dome_h)), |b| {
            b.lathe(
                &[(0.0, -r * 0.05), (r * 0.1, r * 0.04), (0.0, r * 0.14)],
                6,
                0.0,
            )
        });
    }
}

/// The part kinds a recipe can use, for tools and validation messages.
pub const PART_KINDS: [&str; 9] = [
    "hub",
    "docking_ring",
    "arm",
    "greenhouse",
    "antenna",
    "tank",
    "sail",
    "lantern",
    "lens",
];

/// Where a docking ring of the recipe is centred, in station space (for the app to align
/// docking with the art). Returns the first ring.
pub fn docking_ring_center(recipe: &StationRecipe) -> Option<Vec2> {
    recipe.parts.iter().find_map(|p| match p.kind {
        PartKind::DockingRing { .. } => {
            let at = sanitize_placement(&p.at);
            let a = at.angle.to_radians();
            Some(Vec2::new(a.cos(), a.sin()) * at.distance)
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> Vec<StationRecipe> {
        let mut v = vec![
            StationRecipe::lantern_wharf(),
            StationRecipe::brassmouth(),
            StationRecipe::cinder_hook(),
            StationRecipe::glass_drift_observatory(),
        ];
        v.extend((0..=3).map(StationRecipe::kettle_dock));
        v
    }

    #[test]
    fn art_bible_rejects_grey_paint_and_unknown_factions() {
        let bible = ArtBible::bundled();
        let mut grey = StationRecipe::lantern_wharf();
        grey.palette.hull = Color::hex(0x808080);
        let errs = grey.validate_art(bible).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.part.is_none() && e.to_string().contains("hull")),
            "{errs:?}"
        );
        let mut unknown = StationRecipe::lantern_wharf();
        unknown.faction = Some("faction.nope".into());
        let errs = unknown.validate_art(bible).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.to_string().contains("unknown faction")),
            "{errs:?}"
        );
    }

    #[test]
    fn presets_validate_and_build() {
        for r in presets() {
            r.validate().unwrap_or_else(|e| panic!("{}: {e:?}", r.name));
            r.validate_art(ArtBible::bundled())
                .unwrap_or_else(|e| panic!("{} art: {e:?}", r.name));
            let m = station(&r);
            assert!(m.is_well_formed(), "{}", r.name);
            assert!(m.radius() <= r.footprint() * 1.2 + 4.0, "{}", r.name);
            assert!(m.radius() > 10.0, "{}", r.name);
        }
    }

    #[test]
    fn short_wide_arm_builds_without_panicking() {
        // Regression: a valid arm of length 0.5 and width 12 used to invert the
        // collar clamp bounds and panic.
        let r = StationRecipe {
            name: "stubby".into(),
            seed: 1,
            palette: StationPalette::default(),
            faction: None,
            region: None,
            parts: vec![
                Part::new(
                    PartKind::Hub {
                        radius: 6.0,
                        height: 4.0,
                        tiers: 2,
                        sides: 8,
                    },
                    Placement::default(),
                ),
                Part::new(
                    PartKind::Arm {
                        length: 0.5,
                        width: 12.0,
                    },
                    Placement::default(),
                ),
            ],
        };
        r.validate().unwrap();
        assert!(station(&r).is_well_formed());
    }

    #[test]
    fn greenhouse_grows_with_each_stage() {
        let tris: Vec<usize> = (0..=3)
            .map(|s| station(&StationRecipe::kettle_dock(s)).triangle_count())
            .collect();
        assert!(tris.windows(2).all(|w| w[0] < w[1]), "{tris:?}");
    }

    #[test]
    fn recipes_round_trip_through_json() {
        for r in presets() {
            let json = serde_json::to_string_pretty(&r).unwrap();
            let back: StationRecipe = serde_json::from_str(&json).unwrap();
            // Colours are stored as 8-bit hex, so compare the stable JSON form.
            assert_eq!(serde_json::to_string_pretty(&back).unwrap(), json);
            assert_eq!(back.parts.len(), r.parts.len());
        }
    }

    #[test]
    fn hand_written_json_uses_defaults() {
        let json = r##"{
            "name": "Sixth Station",
            "palette": { "hull": "#f0d9b5", "accent": "#c2577a" },
            "parts": [
                { "part": "hub", "radius": 5, "height": 4 },
                { "part": "arm", "length": 8, "width": 2, "at": { "angle": 45, "distance": 5 } },
                { "part": "greenhouse", "radius": 4, "stage": 2,
                  "at": { "angle": 45, "distance": 17 }, "color": "#8fbf7a" },
                { "part": "docking_ring", "radius": 10 }
            ]
        }"##;
        let r: StationRecipe = serde_json::from_str(json).unwrap();
        r.validate().unwrap();
        assert_eq!(r.parts[0].at, Placement::default());
        assert!(matches!(
            r.parts[0].kind,
            PartKind::Hub {
                tiers: 2,
                sides: 8,
                ..
            }
        ));
        assert_eq!(r.palette.trim, StationPalette::default().trim);
        assert!(station(&r).is_well_formed());
        assert_eq!(docking_ring_center(&r), Some(Vec2::ZERO));
    }

    #[test]
    fn validation_reports_each_problem() {
        let mut r = StationRecipe::kettle_dock(1);
        r.parts.push(Part::new(
            PartKind::Greenhouse {
                radius: 4.0,
                stage: 9,
            },
            Placement::polar(0.0, 119.0),
        ));
        r.parts[0].at.scale = f32::NAN;
        let errs = r.validate().unwrap_err();
        let text: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
        assert!(
            text.iter().any(|t| t.starts_with("part 0: at.scale")),
            "{text:?}"
        );
        assert!(text.iter().any(|t| t.contains("stage is 9")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("reaches")), "{text:?}");
        // Still builds without panicking.
        assert!(station(&r).is_well_formed());

        let empty = StationRecipe {
            name: "nothing".into(),
            seed: 0,
            palette: StationPalette::default(),
            faction: None,
            region: None,
            parts: vec![],
        };
        assert_eq!(empty.validate().unwrap_err().len(), 2);
    }

    #[test]
    fn lights_follow_lanterns() {
        let lights = station_lights(&StationRecipe::lantern_wharf());
        // 2 lanterns + 4 ring beacons.
        assert_eq!(lights.len(), 6);
        assert!(lights.iter().all(|l| l.position.is_finite()));
    }

    #[test]
    fn part_kind_names_match_serde() {
        let r = StationRecipe::lantern_wharf();
        let json = serde_json::to_value(&r).unwrap();
        for p in json["parts"].as_array().unwrap() {
            assert!(PART_KINDS.contains(&p["part"].as_str().unwrap()));
        }
    }
}
