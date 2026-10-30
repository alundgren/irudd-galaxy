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
    /// A rounded cargo capsule with a hatch on top.
    CargoPod { radius: f32, length: f32 },
    /// A mast, a jib and a hanging hook.
    Crane { height: f32, reach: f32 },
    /// A fan of flat solar petals around a hub.
    SolarPetal {
        length: f32,
        #[serde(default = "default_three")]
        count: u32,
    },
    /// A tall harbour light: post, cage and a warm lamp.
    LampMast { height: f32 },
    /// A glass dome on a rim.
    Dome { radius: f32 },
    /// A ring of little cabins.
    HabitatRing { radius: f32, tube: f32 },
    /// A navigation beacon with a glowing head.
    Beacon { height: f32 },
    /// A board that lies almost flat so it reads from above. `number` 0 draws no digits.
    Signage {
        width: f32,
        height: f32,
        #[serde(default)]
        number: u32,
    },
    /// Several leaning dishes on one base.
    AntennaCluster {
        height: f32,
        #[serde(default = "default_three")]
        count: u32,
    },
    /// A heavy arm ending in an open claw.
    DockingArm { length: f32 },
    /// An awning, tilted just enough to catch the light from above.
    Banner { width: f32, height: f32 },
    /// A gate. The faction motif decides round, square or pointed.
    Arch { width: f32, height: f32 },
    /// A tall tower, the observatory's landmark.
    Spire { height: f32, radius: f32 },
    /// A chunky building with a roof, a door and windows.
    Block { width: f32, depth: f32, height: f32 },
    /// An open frame of posts and braces.
    Scaffold { radius: f32, height: f32 },
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
fn default_three() -> u32 {
    3
}
fn default_bands() -> u32 {
    3
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

/// A mark painted on a station: stripes, a chevron, the faction sigil or a number.
///
/// Decals are geometry, slightly raised, so the inner-ink pass can outline them.
/// They lie in the play plane and read from above.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decal {
    #[serde(flatten)]
    pub kind: DecalKind,
    #[serde(default)]
    pub at: Placement,
    /// Overrides the decal's main colour. The backing stays hull-coloured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl Decal {
    pub fn new(kind: DecalKind, at: Placement) -> Self {
        Self {
            kind,
            at,
            color: None,
        }
    }
}

/// One decal. In JSON the variant is the `"decal"` field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "decal", rename_all = "snake_case")]
pub enum DecalKind {
    /// Alternating bands along +X.
    Stripe {
        length: f32,
        width: f32,
        #[serde(default = "default_bands")]
        bands: u32,
    },
    /// Repeated V marks along +X.
    Chevron { length: f32, width: f32 },
    /// The station's faction mark. Stations with no faction get the co-op's lantern.
    Sigil { size: f32 },
    /// One or two digits, seven-segment, laid flat.
    Number {
        #[serde(default)]
        value: u32,
        size: f32,
    },
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
    /// Stripes, sigils and numbers. Not structural, so they have their own budget.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decals: Vec<Decal>,
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
                PartKind::CargoPod { radius, length } => {
                    check("radius", radius, 0.6, 8.0);
                    check("length", length, 1.0, 20.0);
                }
                PartKind::Crane { height, reach } => {
                    check("height", height, 2.0, 30.0);
                    check("reach", reach, 2.0, 24.0);
                }
                PartKind::SolarPetal { length, count } => {
                    check("length", length, 2.0, 30.0);
                    check("count", count as f32, 2.0, 8.0);
                }
                PartKind::LampMast { height } => check("height", height, 2.0, 24.0),
                PartKind::Dome { radius } => check("radius", radius, 1.5, 20.0),
                PartKind::HabitatRing { radius, tube } => {
                    check("radius", radius, 4.0, 40.0);
                    check("tube", tube, 0.4, 4.0);
                }
                PartKind::Beacon { height } => check("height", height, 1.5, 20.0),
                PartKind::Signage {
                    width,
                    height,
                    number,
                } => {
                    check("width", width, 2.0, 16.0);
                    check("height", height, 1.2, 10.0);
                    check("number", number as f32, 0.0, 99.0);
                }
                PartKind::AntennaCluster { height, count } => {
                    check("height", height, 2.0, 24.0);
                    check("count", count as f32, 2.0, 6.0);
                }
                PartKind::DockingArm { length } => check("length", length, 3.0, 30.0),
                PartKind::Banner { width, height } => {
                    check("width", width, 2.0, 14.0);
                    check("height", height, 1.5, 10.0);
                }
                PartKind::Arch { width, height } => {
                    check("width", width, 3.0, 24.0);
                    check("height", height, 2.0, 16.0);
                }
                PartKind::Spire { height, radius } => {
                    check("height", height, 4.0, 36.0);
                    check("radius", radius, 0.6, 8.0);
                }
                PartKind::Block {
                    width,
                    depth,
                    height,
                } => {
                    check("width", width, 2.0, 20.0);
                    check("depth", depth, 2.0, 20.0);
                    check("height", height, 1.5, 16.0);
                }
                PartKind::Scaffold { radius, height } => {
                    check("radius", radius, 2.0, 24.0);
                    check("height", height, 1.5, 16.0);
                }
            }
            for message in placement_problems(at) {
                err(Some(i), message);
            }
            let reach = at.distance + part_reach(&part.kind) * at.scale;
            if reach.is_finite() && reach > MAX_STATION_RADIUS {
                err(
                    Some(i),
                    format!("reaches {reach:.1} from the centre; at most {MAX_STATION_RADIUS}"),
                );
            }
        }
        for (i, decal) in self.decals.iter().enumerate() {
            errs.extend(decal_placement_errors(decal, i));
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
        if self.decals.len() > bible.budget.max_decals {
            err(
                None,
                format!(
                    "{} decals; at most {}",
                    self.decals.len(),
                    bible.budget.max_decals
                ),
            );
        }
        for (i, decal) in self.decals.iter().enumerate() {
            let (kind, params) = decal_params(&decal.kind);
            for (name, value) in params {
                if let Some(message) = bible.proportion_issue(kind, name, value) {
                    err(None, format!("decal {i}: {message}"));
                }
            }
            if let Some(color) = decal.color
                && let Some(issue) = bible.check_paint(color)
            {
                err(None, format!("decal {i} paint {}", issue.message));
            }
            for e in decal_placement_errors(decal, i) {
                err(e.part, e.message);
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
        let parts = self
            .parts
            .iter()
            .map(|p| p.at.distance + part_reach(&p.kind) * p.at.scale);
        let decals = self
            .decals
            .iter()
            .map(|d| d.at.distance + decal_reach(&d.kind) * d.at.scale);
        parts.chain(decals).fold(0.0, f32::max)
    }

    /// Lantern Wharf: a homely cooperative harbour. The basin sits to the east; the
    /// houses, the garden and the lantern masts crowd the western quay.
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
                        radius: 6.5,
                        height: 5.0,
                        tiers: 2,
                        sides: 10,
                    },
                    Placement::default(),
                ),
                p(
                    DockingRing {
                        radius: 13.0,
                        tube: 1.7,
                        spokes: 3,
                        inner: 5.0,
                    },
                    Placement::polar(0.0, 14.0),
                ),
                p(
                    Block {
                        width: 7.0,
                        depth: 5.5,
                        height: 4.2,
                    },
                    Placement::polar(195.0, 10.0),
                ),
                p(
                    Greenhouse {
                        radius: 4.2,
                        stage: 2,
                    },
                    Placement::polar(140.0, 11.0),
                ),
                p(
                    CargoPod {
                        radius: 1.6,
                        length: 5.0,
                    },
                    Placement {
                        turn: 25.0,
                        ..Placement::polar(230.0, 12.0)
                    },
                ),
                p(
                    CargoPod {
                        radius: 1.3,
                        length: 3.8,
                    },
                    Placement::polar(255.0, 8.5),
                ),
                p(LampMast { height: 9.0 }, Placement::polar(110.0, 16.0)),
                p(LampMast { height: 6.5 }, Placement::polar(250.0, 15.0)),
                p(
                    Banner {
                        width: 6.0,
                        height: 3.4,
                    },
                    Placement {
                        z: 3.2,
                        turn: 15.0,
                        ..Placement::polar(195.0, 10.0)
                    },
                ),
                p(
                    Signage {
                        width: 6.0,
                        height: 3.0,
                        number: 1,
                    },
                    Placement {
                        z: 5.0,
                        ..Placement::polar(170.0, 6.0)
                    },
                ),
                p(DockingArm { length: 9.0 }, Placement::polar(18.0, 22.0)),
                p(DockingArm { length: 8.0 }, Placement::polar(340.0, 20.0)),
                p(Lantern { height: 3.0 }, Placement::polar(80.0, 8.0)),
            ],
            decals: vec![
                Decal::new(
                    DecalKind::Sigil { size: 3.4 },
                    Placement {
                        z: 5.6,
                        ..Placement::default()
                    },
                ),
                Decal::new(
                    DecalKind::Stripe {
                        length: 5.5,
                        width: 1.4,
                        bands: 4,
                    },
                    Placement {
                        z: 2.4,
                        turn: 25.0,
                        ..Placement::polar(230.0, 12.0)
                    },
                ),
            ],
        }
    }

    /// Kettle Dock: a greenhouse co-op. One house grows with `stage`; its sibling is
    /// already in bloom, and a glass dome sits on the hub like a kettle lid.
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
                        radius: 5.2,
                        height: 4.2,
                        tiers: 2,
                        sides: 8,
                    },
                    Placement::default(),
                ),
                p(
                    Greenhouse {
                        radius: 8.0,
                        stage: stage.min(3),
                    },
                    Placement::polar(20.0, 16.0),
                ),
                p(
                    Greenhouse {
                        radius: 5.0,
                        stage: 3,
                    },
                    Placement::polar(175.0, 14.0),
                ),
                p(
                    Dome { radius: 4.2 },
                    Placement {
                        z: 2.2,
                        ..Placement::polar(200.0, 2.0)
                    },
                ),
                p(
                    Tank {
                        radius: 1.5,
                        length: 5.0,
                        count: 2,
                    },
                    Placement::polar(95.0, 9.0),
                ),
                p(
                    DockingRing {
                        radius: 7.5,
                        tube: 1.4,
                        spokes: 0,
                        inner: 0.0,
                    },
                    Placement::polar(285.0, 20.0),
                ),
                p(LampMast { height: 7.0 }, Placement::polar(55.0, 8.0)),
                p(
                    Banner {
                        width: 5.0,
                        height: 3.0,
                    },
                    Placement {
                        z: 3.4,
                        ..Placement::polar(150.0, 8.0)
                    },
                ),
                p(
                    CargoPod {
                        radius: 1.4,
                        length: 4.2,
                    },
                    Placement::polar(120.0, 11.0),
                ),
                p(
                    Signage {
                        width: 5.0,
                        height: 2.6,
                        number: 3,
                    },
                    Placement {
                        z: 4.6,
                        ..Placement::polar(240.0, 7.0)
                    },
                ),
            ],
            decals: vec![Decal::new(
                DecalKind::Sigil { size: 2.8 },
                Placement {
                    z: 6.4,
                    ..Placement::polar(200.0, 2.0)
                },
            )],
        }
    }

    /// Brassmouth Exchange: a square counting-house around a small courtyard ring.
    /// Arches are the gates. The old wagon-wheel plan is gone.
    pub fn brassmouth() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        let mut parts = vec![
            p(
                Hub {
                    radius: 6.5,
                    height: 5.5,
                    tiers: 2,
                    sides: 8,
                },
                Placement::default(),
            ),
            p(
                DockingRing {
                    radius: 8.5,
                    tube: 1.5,
                    spokes: 4,
                    inner: 5.5,
                },
                Placement::default(),
            ),
        ];
        for a in [45.0, 135.0, 225.0, 315.0] {
            parts.push(p(
                Block {
                    width: 6.5,
                    depth: 6.5,
                    height: 5.0,
                },
                Placement::polar(a, 16.0),
            ));
        }
        for a in [0.0, 90.0, 180.0, 270.0] {
            parts.push(p(
                Arch {
                    width: 7.0,
                    height: 5.5,
                },
                Placement::polar(a, 16.0),
            ));
        }
        parts.push(p(DockingArm { length: 8.0 }, Placement::polar(15.0, 24.0)));
        parts.push(p(DockingArm { length: 8.0 }, Placement::polar(195.0, 24.0)));
        parts.push(p(
            Banner {
                width: 6.0,
                height: 3.2,
            },
            Placement {
                z: 4.2,
                turn: 90.0,
                ..Placement::polar(0.0, 16.0)
            },
        ));
        parts.push(p(
            Banner {
                width: 6.0,
                height: 3.2,
            },
            Placement {
                z: 4.2,
                turn: 90.0,
                ..Placement::polar(180.0, 16.0)
            },
        ));
        parts.push(p(
            Signage {
                width: 7.0,
                height: 3.4,
                number: 12,
            },
            Placement {
                z: 5.2,
                ..Placement::polar(90.0, 9.0)
            },
        ));
        parts.push(p(
            CargoPod {
                radius: 1.5,
                length: 5.0,
            },
            Placement::polar(55.0, 23.0),
        ));
        parts.push(p(
            CargoPod {
                radius: 1.4,
                length: 4.2,
            },
            Placement::polar(125.0, 23.0),
        ));
        parts.push(p(Beacon { height: 5.5 }, Placement::polar(270.0, 9.5)));
        parts.push(p(
            Antenna {
                height: 8.0,
                dish: true,
            },
            Placement {
                z: 4.0,
                ..Placement::polar(40.0, 3.0)
            },
        ));
        Self {
            name: "Brassmouth Exchange".into(),
            seed: 3,
            palette: StationPalette::from_faction(faction("faction.tollway_compact")),
            faction: Some("faction.tollway_compact".into()),
            region: Some("region.brassmouth_narrows".into()),
            parts,
            decals: vec![
                Decal::new(
                    DecalKind::Sigil { size: 3.0 },
                    Placement {
                        z: 6.4,
                        ..Placement::default()
                    },
                ),
                Decal::new(
                    DecalKind::Number {
                        value: 12,
                        size: 2.2,
                    },
                    Placement {
                        z: 1.6,
                        ..Placement::polar(270.0, 9.5)
                    },
                ),
            ],
        }
    }
}

impl StationRecipe {
    /// Cinder Hook: a rough mining camp. A scaffold and a crane lean over patched
    /// shacks. The basin is off to one side, not a ring around a tidy hub.
    pub fn cinder_hook() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        let patch = Color::hex(0xb98a5e);
        let rust = Color::hex(0x8a5a44);
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
                        radius: 5.5,
                        height: 4.0,
                        tiers: 2,
                        sides: 6,
                    },
                    Placement::default(),
                ),
                p(
                    Scaffold {
                        radius: 7.0,
                        height: 5.5,
                    },
                    Placement::polar(35.0, 14.0),
                ),
                p(
                    Crane {
                        height: 10.0,
                        reach: 11.0,
                    },
                    Placement {
                        turn: 190.0,
                        ..Placement::polar(35.0, 16.0)
                    },
                ),
                p(
                    Tank {
                        radius: 1.8,
                        length: 5.0,
                        count: 3,
                    },
                    Placement {
                        turn: 70.0,
                        ..Placement::polar(200.0, 11.0)
                    },
                )
                .colored(rust),
                p(
                    CargoPod {
                        radius: 1.7,
                        length: 4.5,
                    },
                    Placement::polar(155.0, 12.0),
                )
                .colored(patch),
                p(
                    Block {
                        width: 5.0,
                        depth: 4.2,
                        height: 3.2,
                    },
                    Placement::polar(250.0, 9.0),
                )
                .colored(patch),
                p(Beacon { height: 6.0 }, Placement::polar(300.0, 8.0)),
                p(
                    AntennaCluster {
                        height: 7.0,
                        count: 4,
                    },
                    Placement {
                        z: 2.2,
                        ..Placement::polar(115.0, 4.5)
                    },
                ),
                p(DockingArm { length: 10.0 }, Placement::polar(330.0, 14.0)),
                p(
                    DockingRing {
                        radius: 8.0,
                        tube: 1.4,
                        spokes: 2,
                        inner: 3.5,
                    },
                    Placement {
                        turn: 15.0,
                        ..Placement::polar(8.0, 18.0)
                    },
                ),
                p(Lantern { height: 2.4 }, Placement::polar(80.0, 7.5)),
                p(Lantern { height: 2.4 }, Placement::polar(190.0, 6.5)),
                p(
                    Signage {
                        width: 4.5,
                        height: 2.4,
                        number: 4,
                    },
                    Placement {
                        z: 3.6,
                        ..Placement::polar(250.0, 9.0)
                    },
                ),
            ],
            decals: vec![Decal::new(
                DecalKind::Chevron {
                    length: 6.0,
                    width: 1.6,
                },
                Placement {
                    z: 4.4,
                    ..Placement::polar(35.0, 14.0)
                },
            )],
        }
    }

    /// The Glass Drift Observatory: a spire, a great lens and a scholar's walk.
    /// The docking ring is off to the side so the lens, not a wheel, owns the silhouette.
    pub fn glass_drift_observatory() -> Self {
        use PartKind::*;
        let p = |kind, at| Part::new(kind, at);
        Self {
            name: "Glass Drift Observatory".into(),
            seed: 5,
            palette: StationPalette::from_faction(faction("faction.lamplighter_society")),
            faction: Some("faction.lamplighter_society".into()),
            region: Some("region.cinder_drift".into()),
            parts: vec![
                p(
                    Hub {
                        radius: 5.0,
                        height: 4.0,
                        tiers: 2,
                        sides: 10,
                    },
                    Placement::default(),
                ),
                p(
                    Spire {
                        height: 16.0,
                        radius: 1.8,
                    },
                    Placement {
                        z: 2.4,
                        ..Placement::default()
                    },
                ),
                p(Lens { radius: 8.0 }, Placement::polar(28.0, 18.0)),
                p(Dome { radius: 5.2 }, Placement::polar(200.0, 12.0)),
                p(
                    HabitatRing {
                        radius: 8.5,
                        tube: 1.15,
                    },
                    Placement::default(),
                ),
                p(
                    AntennaCluster {
                        height: 6.0,
                        count: 3,
                    },
                    Placement {
                        z: 3.0,
                        ..Placement::polar(140.0, 5.0)
                    },
                ),
                p(LampMast { height: 8.0 }, Placement::polar(110.0, 12.0)),
                p(LampMast { height: 6.0 }, Placement::polar(300.0, 11.0)),
                p(
                    DockingRing {
                        radius: 7.5,
                        tube: 1.3,
                        spokes: 4,
                        inner: 3.5,
                    },
                    Placement::polar(250.0, 22.0),
                ),
                p(
                    Banner {
                        width: 4.5,
                        height: 2.8,
                    },
                    Placement {
                        z: 4.5,
                        ..Placement::polar(175.0, 8.0)
                    },
                ),
                p(
                    Signage {
                        width: 5.2,
                        height: 2.8,
                        number: 8,
                    },
                    Placement {
                        z: 3.6,
                        ..Placement::polar(70.0, 7.0)
                    },
                ),
            ],
            decals: vec![Decal::new(
                DecalKind::Sigil { size: 2.6 },
                Placement {
                    z: 5.8,
                    ..Placement::polar(200.0, 12.0)
                },
            )],
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
        PartKind::CargoPod { radius, length } => {
            ("cargo_pod", vec![("radius", radius), ("length", length)])
        }
        PartKind::Crane { height, reach } => ("crane", vec![("height", height), ("reach", reach)]),
        PartKind::SolarPetal { length, count } => (
            "solar_petal",
            vec![("length", length), ("count", count as f32)],
        ),
        PartKind::LampMast { height } => ("lamp_mast", vec![("height", height)]),
        PartKind::Dome { radius } => ("dome", vec![("radius", radius)]),
        PartKind::HabitatRing { radius, tube } => {
            ("habitat_ring", vec![("radius", radius), ("tube", tube)])
        }
        PartKind::Beacon { height } => ("beacon", vec![("height", height)]),
        PartKind::Signage {
            width,
            height,
            number,
        } => (
            "signage",
            vec![
                ("width", width),
                ("height", height),
                ("number", number as f32),
            ],
        ),
        PartKind::AntennaCluster { height, count } => (
            "antenna_cluster",
            vec![("height", height), ("count", count as f32)],
        ),
        PartKind::DockingArm { length } => ("docking_arm", vec![("length", length)]),
        PartKind::Banner { width, height } => {
            ("banner", vec![("width", width), ("height", height)])
        }
        PartKind::Arch { width, height } => ("arch", vec![("width", width), ("height", height)]),
        PartKind::Spire { height, radius } => {
            ("spire", vec![("height", height), ("radius", radius)])
        }
        PartKind::Block {
            width,
            depth,
            height,
        } => (
            "block",
            vec![("width", width), ("depth", depth), ("height", height)],
        ),
        PartKind::Scaffold { radius, height } => {
            ("scaffold", vec![("radius", radius), ("height", height)])
        }
    }
}

fn decal_params(kind: &DecalKind) -> (&'static str, Vec<(&'static str, f32)>) {
    match *kind {
        DecalKind::Stripe {
            length,
            width,
            bands,
        } => (
            "stripe",
            vec![
                ("length", length),
                ("width", width),
                ("bands", bands as f32),
            ],
        ),
        DecalKind::Chevron { length, width } => {
            ("chevron", vec![("length", length), ("width", width)])
        }
        DecalKind::Sigil { size } => ("sigil", vec![("size", size)]),
        DecalKind::Number { value, size } => {
            ("number", vec![("value", value as f32), ("size", size)])
        }
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
        PartKind::CargoPod { radius, length } => length * 0.5 + radius,
        PartKind::Crane { height, reach } => height.max(reach),
        PartKind::SolarPetal { length, .. } => length,
        PartKind::LampMast { height } | PartKind::Beacon { height } => height * 0.35,
        PartKind::Dome { radius } => radius,
        PartKind::HabitatRing { radius, tube } => radius + tube,
        PartKind::Signage { width, height, .. } | PartKind::Banner { width, height } => {
            width.max(height) * 0.5
        }
        PartKind::AntennaCluster { height, .. } => height * 0.45,
        PartKind::DockingArm { length } => length,
        PartKind::Arch { width, height } => width.max(height) * 0.5,
        PartKind::Spire { radius, .. } => radius * 1.4,
        PartKind::Block { width, depth, .. } => width.max(depth) * 0.55,
        PartKind::Scaffold { radius, .. } => radius,
    }
}

/// A conservative bound on how far a decal's geometry reaches from its placement
/// point: the half-diagonal of its footprint (corners, not just the long side), with a
/// margin, so `validate` can promise the built mesh stays inside the station radius
/// whatever the decal's turn.
fn decal_reach(kind: &DecalKind) -> f32 {
    const MARGIN: f32 = 1.1;
    let half_diagonal = |x: f32, y: f32| 0.5 * x.hypot(y);
    MARGIN
        * match *kind {
            DecalKind::Stripe { length, width, .. } => half_diagonal(length, width),
            // Each chevron bar is a `0.7w x 0.2w` box centred at `(x, ±0.22w)` with
            // `|x| <= length/2`, rotated about its own centre, so its farthest corner is
            // at most the centre's distance plus the bar's half-diagonal.
            DecalKind::Chevron { length, width } => {
                (0.5 * length).hypot(0.22 * width) + half_diagonal(0.7 * width, 0.2 * width)
            }
            // The backing plate is `1.2 * size` square; the mark sits inside it.
            DecalKind::Sigil { size } => half_diagonal(1.2 * size, 1.2 * size),
            // The backing plate is `1.35 * size` by `1.4 * size`, or `2.3 * size` for
            // two digits (see `build_decal`).
            DecalKind::Number { value, size } => {
                let wide = if value >= 10 { 2.3 } else { 1.4 };
                half_diagonal(1.35 * size, wide * size)
            }
        }
}

/// Builds a station mesh from a recipe. Invalid values are clamped so this never panics,
/// but only a recipe that passes [`StationRecipe::validate`] is guaranteed to look right.
pub fn station(recipe: &StationRecipe) -> MeshData {
    let mut b = MeshBuilder::new();
    let pal = recipe.palette;
    let rng = Rng::new(recipe.seed);
    let motif = motif_of(recipe);
    let mark = mark_of(recipe);
    for (i, part) in recipe.parts.iter().take(MAX_PARTS).enumerate() {
        let mut r = rng.fork(i as u64 + 1);
        let at = sanitize_placement(&part.at);
        b.with(at.matrix(), |b| {
            build_part(b, &part.kind, part.color, &pal, motif, &mut r)
        });
    }
    let cap = ArtBible::bundled().budget.max_decals;
    for decal in recipe.decals.iter().take(cap) {
        let at = sanitize_placement(&decal.at);
        b.with(at.matrix(), |b| {
            build_decal(b, &decal.kind, decal.color, &pal, mark)
        });
    }
    b.build()
}

fn motif_of(recipe: &StationRecipe) -> &'static str {
    recipe
        .faction
        .as_deref()
        .and_then(|id| ArtBible::bundled().faction(id))
        .map(|f| f.motif.as_str())
        .unwrap_or("round")
}

fn mark_of(recipe: &StationRecipe) -> &'static str {
    recipe
        .faction
        .as_deref()
        .and_then(|id| ArtBible::bundled().faction(id))
        .map(|f| f.mark.as_str())
        .unwrap_or("lantern_kettle")
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
            PartKind::LampMast { height } => {
                push(Vec3::Z * clampf(height, 2.0, 24.0), pal.glow, 6.5)
            }
            PartKind::Beacon { height } => push(Vec3::Z * clampf(height, 1.5, 20.0), pal.glow, 5.0),
            PartKind::Spire { height, .. } => {
                push(Vec3::Z * clampf(height, 4.0, 36.0), pal.glow, 4.5)
            }
            _ => {}
        }
    }
    out
}

fn placement_problems(at: &Placement) -> Vec<String> {
    let mut out = Vec::new();
    let mut check = |name: &str, v: f32, lo: f32, hi: f32| {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            out.push(format!("{name} is {v}; expected {lo}..={hi}"));
        }
    };
    check("at.angle", at.angle, -720.0, 720.0);
    check("at.distance", at.distance, 0.0, MAX_STATION_RADIUS);
    check("at.z", at.z, -40.0, 40.0);
    check("at.turn", at.turn, -720.0, 720.0);
    check("at.scale", at.scale, 0.1, 5.0);
    out
}

fn decal_placement_errors(decal: &Decal, index: usize) -> Vec<RecipeError> {
    let mut errs: Vec<RecipeError> = placement_problems(&decal.at)
        .into_iter()
        .map(|message| RecipeError {
            part: None,
            message: format!("decal {index}: {message}"),
        })
        .collect();
    let reach = decal.at.distance + decal_reach(&decal.kind) * decal.at.scale;
    if reach.is_finite() && reach > MAX_STATION_RADIUS {
        errs.push(RecipeError {
            part: None,
            message: format!(
                "decal {index}: reaches {reach:.1} from the centre; at most {MAX_STATION_RADIUS}"
            ),
        });
    }
    errs
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
    motif: &str,
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
            motif,
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
        PartKind::CargoPod { radius, length } => cargo_pod(
            b,
            clampf(radius, 0.6, 8.0),
            clampf(length, 1.0, 20.0),
            color,
            pal,
        ),
        PartKind::Crane { height, reach } => crane(
            b,
            clampf(height, 2.0, 30.0),
            clampf(reach, 2.0, 24.0),
            color,
            pal,
        ),
        PartKind::SolarPetal { length, count } => {
            solar_petal(b, clampf(length, 2.0, 30.0), count.clamp(2, 8), color, pal)
        }
        PartKind::LampMast { height } => lamp_mast(b, clampf(height, 2.0, 24.0), color, pal, motif),
        PartKind::Dome { radius } => dome(b, clampf(radius, 1.5, 20.0), color, pal),
        PartKind::HabitatRing { radius, tube } => habitat_ring(
            b,
            clampf(radius, 4.0, 40.0),
            clampf(tube, 0.4, 4.0),
            color,
            pal,
        ),
        PartKind::Beacon { height } => beacon(b, clampf(height, 1.5, 20.0), color, pal, motif),
        PartKind::Signage {
            width,
            height,
            number,
        } => signage(
            b,
            clampf(width, 2.0, 16.0),
            clampf(height, 1.2, 10.0),
            number.min(99),
            color,
            pal,
        ),
        PartKind::AntennaCluster { height, count } => antenna_cluster(
            b,
            clampf(height, 2.0, 24.0),
            count.clamp(2, 6),
            color,
            pal,
            rng,
        ),
        PartKind::DockingArm { length } => docking_arm(b, clampf(length, 3.0, 30.0), color, pal),
        PartKind::Banner { width, height } => banner(
            b,
            clampf(width, 2.0, 14.0),
            clampf(height, 1.5, 10.0),
            color,
            pal,
            mark_from_motif(motif),
        ),
        PartKind::Arch { width, height } => arch(
            b,
            clampf(width, 3.0, 24.0),
            clampf(height, 2.0, 16.0),
            color,
            pal,
            motif,
        ),
        PartKind::Spire { height, radius } => spire(
            b,
            clampf(height, 4.0, 36.0),
            clampf(radius, 0.6, 8.0),
            color,
            pal,
            motif,
        ),
        PartKind::Block {
            width,
            depth,
            height,
        } => block(
            b,
            clampf(width, 2.0, 20.0),
            clampf(depth, 2.0, 20.0),
            clampf(height, 1.5, 16.0),
            color,
            pal,
            motif,
        ),
        PartKind::Scaffold { radius, height } => scaffold(
            b,
            clampf(radius, 2.0, 24.0),
            clampf(height, 1.5, 16.0),
            color,
            pal,
        ),
    }
}

/// Banners carry a small mark. The motif names a shape, not a sigil, so map the
/// three motifs back to the three marks the bible ships with.
fn mark_from_motif(motif: &str) -> &'static str {
    match motif {
        "square" => "toll_key",
        "pointed" => "open_lamp",
        _ => "lantern_kettle",
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

#[allow(clippy::too_many_arguments)]
fn hub(
    b: &mut MeshBuilder,
    radius: f32,
    height: f32,
    tiers: u32,
    sides: u32,
    hull: Color,
    pal: &StationPalette,
    motif: &str,
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
    // Cap: round, square or pointed, so a faction reads even on a shared hub.
    let top = z0 + height;
    let rc = radius * (1.0 - (tiers - 1) as f32 * 0.22) * 0.9;
    b.paint(pal.accent, 0.0);
    let cap_h = match motif {
        "square" => rc * 0.45,
        "pointed" => rc * 1.15,
        _ => rc * 0.4,
    };
    b.with(Mat4::from_translation(Vec3::Z * top), |b| {
        raised_cap(b, rc * 0.85, cap_h, motif, sides)
    });
    b.paint(pal.glow, 1.0);
    b.with(
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

/// A cap whose shape follows the faction motif: round, square or pointed.
fn raised_cap(b: &mut MeshBuilder, radius: f32, height: f32, motif: &str, sides: u32) {
    match motif {
        "square" => b.extrude(
            &[
                Vec2::new(-radius, -radius),
                Vec2::new(radius, -radius),
                Vec2::new(radius, radius),
                Vec2::new(-radius, radius),
            ],
            height,
            0.2,
        ),
        "pointed" => b.frustum(radius, 0.0, height, sides.clamp(5, 8)),
        _ => b.lathe(
            &[
                (radius, 0.0),
                (radius * 0.72, height * 0.45),
                (radius * 0.28, height * 0.82),
                (0.0, height),
            ],
            sides.clamp(6, 12),
            0.0,
        ),
    };
}

fn cargo_pod(
    b: &mut MeshBuilder,
    radius: f32,
    length: f32,
    color: Option<Color>,
    pal: &StationPalette,
) {
    let half = length * 0.5;
    b.with(Mat4::from_rotation_y(FRAC_PI_2), |b| {
        b.paint(color.unwrap_or(pal.hull), 0.0);
        b.lathe(
            &[
                (0.0, -half - radius * 0.45),
                (radius * 0.72, -half),
                (radius, -half + radius * 0.35),
                (radius, half - radius * 0.35),
                (radius * 0.72, half),
                (0.0, half + radius * 0.45),
            ],
            8,
            0.0,
        );
    });
    b.paint(pal.trim, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * radius * 0.92), |b| {
        b.cuboid(Vec3::new(length * 0.36, radius * 0.75, radius * 0.28))
    });
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * radius * 1.08), |b| {
        b.cuboid(Vec3::new(length * 0.14, radius * 0.3, radius * 0.16))
    });
}

fn crane(b: &mut MeshBuilder, height: f32, reach: f32, color: Option<Color>, pal: &StationPalette) {
    let mast = (height * 0.16).max(1.1);
    b.paint(color.unwrap_or(pal.trim), 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.4), |b| {
        b.cuboid(Vec3::new(mast * 2.4, mast * 2.4, 0.8))
    });
    b.frustum(mast * 0.5, mast * 0.28, height, 6);
    let tip = Vec3::Z * height;
    let end = Vec3::new(reach, 0.0, height * 0.86);
    strut(b, tip, end, mast * 0.55);
    strut(b, Vec3::Z * height * 0.55, end, mast * 0.32);
    let hook = Vec3::new(reach, 0.0, height * 0.28);
    strut(b, end, hook, (mast * 0.22).max(0.45));
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(hook), |b| {
        b.cuboid(Vec3::splat(mast * 0.75))
    });
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(hook + Vec3::Z * mast * 0.15), |b| {
        b.cuboid(Vec3::splat(mast * 0.28))
    });
}

fn solar_petal(
    b: &mut MeshBuilder,
    length: f32,
    count: u32,
    color: Option<Color>,
    pal: &StationPalette,
) {
    b.paint(pal.trim, 0.0);
    b.frustum(length * 0.1, length * 0.07, 0.7, 6);
    let paint = color.unwrap_or(pal.accent);
    for k in 0..count {
        let a = k as f32 / count as f32 * TAU;
        b.with(Mat4::from_rotation_z(a), |b| {
            b.paint(
                if k % 2 == 0 {
                    paint
                } else {
                    paint.lerp(pal.hull, 0.45)
                },
                0.08,
            );
            b.with(
                Mat4::from_translation(Vec3::new(length * 0.55, 0.0, 0.4)),
                |b| b.cuboid(Vec3::new(length * 0.82, length * 0.22, 0.22)),
            );
            b.paint(pal.hull, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(length * 0.55, 0.0, 0.55)),
                |b| b.cuboid(Vec3::new(length * 0.62, length * 0.05, 0.1)),
            );
        });
    }
}

fn lamp_mast(
    b: &mut MeshBuilder,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
    motif: &str,
) {
    b.paint(color.unwrap_or(pal.trim), 0.0);
    b.frustum(0.55, 0.32, height * 0.7, 6);
    let z = height * 0.7;
    let cage = (height * 0.14).max(0.8);
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU + TAU / 8.0;
        let p = Vec3::new(a.cos(), a.sin(), 0.0) * cage;
        strut(
            b,
            p + Vec3::Z * z,
            p + Vec3::Z * (z + cage * 1.7),
            cage * 0.28,
        );
    }
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(Vec3::Z * (z + cage * 0.65)), |b| {
        b.lathe(
            &[
                (cage * 0.42, 0.0),
                (cage * 0.55, cage * 0.45),
                (cage * 0.3, cage * 0.9),
            ],
            6,
            0.0,
        )
    });
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * (z + cage * 1.75)), |b| {
        raised_cap(b, cage * 0.9, cage, motif, 6)
    });
}

fn dome(b: &mut MeshBuilder, radius: f32, color: Option<Color>, pal: &StationPalette) {
    let base = radius * 0.18;
    b.paint(color.unwrap_or(pal.trim), 0.0);
    b.lathe(
        &[
            (radius * 0.9, 0.0),
            (radius * 1.04, base * 0.6),
            (radius * 0.92, base),
        ],
        12,
        0.0,
    );
    b.paint(pal.glass, 0.3);
    b.lathe(
        &[
            (radius * 0.94, base),
            (radius * 0.82, base + radius * 0.38),
            (radius * 0.48, base + radius * 0.72),
            (0.0, base + radius * 0.98),
        ],
        10,
        0.0,
    );
    b.paint(color.unwrap_or(pal.accent), 0.0);
    let rib = (radius * 0.07).max(0.35);
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU;
        let p = |t: f32| {
            let e = t * FRAC_PI_2;
            Vec3::new(
                a.cos() * e.cos() * radius * 0.98,
                a.sin() * e.cos() * radius * 0.98,
                base + e.sin() * radius * 0.95,
            )
        };
        for s in 0..4 {
            strut(b, p(s as f32 / 4.0), p((s + 1) as f32 / 4.0), rib);
        }
    }
}

fn habitat_ring(
    b: &mut MeshBuilder,
    radius: f32,
    tube: f32,
    color: Option<Color>,
    pal: &StationPalette,
) {
    b.paint(color.unwrap_or(pal.trim), 0.0);
    let segs = ((radius * 0.7) as u32).clamp(12, 28);
    b.torus(radius, tube * 1.35, tube, segs, 6);
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU;
        let p = Vec3::new(a.cos(), a.sin(), 0.0) * radius + Vec3::Z * tube * 0.15;
        b.with(Mat4::from_translation(p) * Mat4::from_rotation_z(a), |b| {
            b.paint(
                if k % 2 == 0 {
                    pal.hull
                } else {
                    pal.hull.lerp(pal.accent, 0.35)
                },
                0.0,
            );
            b.cuboid(Vec3::new(tube * 2.2, tube * 1.7, tube * 1.45));
            b.paint(pal.glow, 1.0);
            b.with(
                Mat4::from_translation(Vec3::new(tube * 1.05, 0.0, tube * 0.1)),
                |b| b.cuboid(Vec3::new(0.22, tube * 0.7, tube * 0.4)),
            );
        });
    }
}

fn beacon(
    b: &mut MeshBuilder,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
    motif: &str,
) {
    b.paint(color.unwrap_or(pal.trim), 0.0);
    b.frustum(0.55, 0.32, height * 0.72, 5);
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.76), |b| {
        b.lathe(
            &[
                (0.0, -height * 0.1),
                (height * 0.14, 0.0),
                (0.0, height * 0.1),
            ],
            4,
            0.3,
        )
    });
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.9), |b| {
        raised_cap(b, 0.45, height * 0.18, motif, 6)
    });
}

fn signage(
    b: &mut MeshBuilder,
    width: f32,
    height: f32,
    number: u32,
    color: Option<Color>,
    pal: &StationPalette,
) {
    b.with(Mat4::from_rotation_y(-0.4), |b| {
        b.paint(color.unwrap_or(pal.hull), 0.0);
        b.cuboid(Vec3::new(height, width, 0.32));
        b.paint(pal.trim, 0.0);
        let t = (width.min(height) * 0.12).max(0.4);
        let z = 0.22;
        for x in [height * 0.5 - t * 0.5, -height * 0.5 + t * 0.5] {
            b.with(Mat4::from_translation(Vec3::new(x, 0.0, z)), |b| {
                b.cuboid(Vec3::new(t, width * 1.02, 0.16))
            });
        }
        for y in [width * 0.5 - t * 0.5, -width * 0.5 + t * 0.5] {
            b.with(Mat4::from_translation(Vec3::new(0.0, y, z)), |b| {
                b.cuboid(Vec3::new(height, t, 0.16))
            });
        }
        if number > 0 {
            b.paint(pal.accent, 0.0);
            digits(b, number, height * 0.55, 0.36);
        }
    });
}

fn antenna_cluster(
    b: &mut MeshBuilder,
    height: f32,
    count: u32,
    color: Option<Color>,
    pal: &StationPalette,
    rng: &mut Rng,
) {
    b.paint(pal.hull, 0.0);
    b.lathe(
        &[
            (height * 0.16, 0.0),
            (height * 0.22, 0.3),
            (height * 0.12, 0.5),
        ],
        8,
        0.0,
    );
    for k in 0..count {
        let a = k as f32 / count as f32 * TAU + 0.5;
        let h = height * (0.55 + 0.4 * k as f32 / count as f32);
        let lean = 0.22 + rng.range(0.0, 0.18);
        b.with(
            Mat4::from_translation(Vec3::new(a.cos(), a.sin(), 0.0) * height * 0.1 + Vec3::Z * 0.4)
                * Mat4::from_rotation_z(a)
                * Mat4::from_rotation_y(lean),
            |b| {
                b.paint(color.unwrap_or(pal.trim), 0.0);
                b.frustum(0.18, 0.08, h, 5);
                if k % 2 == 0 {
                    b.paint(pal.hull, 0.0);
                    b.with(
                        Mat4::from_translation(Vec3::Z * h * 0.62) * Mat4::from_rotation_y(0.9),
                        |b| {
                            b.lathe(
                                &[(0.0, 0.0), (h * 0.14, h * 0.04), (h * 0.26, h * 0.14)],
                                7,
                                0.0,
                            )
                        },
                    );
                }
                b.paint(pal.glow, 1.0);
                b.with(Mat4::from_translation(Vec3::Z * h), |b| {
                    b.cuboid(Vec3::splat(0.28))
                });
            },
        );
    }
}

fn docking_arm(b: &mut MeshBuilder, length: f32, color: Option<Color>, pal: &StationPalette) {
    let w = (length * 0.14).max(1.3);
    b.paint(color.unwrap_or(pal.hull), 0.0);
    b.with(
        Mat4::from_translation(Vec3::new(length * 0.4, 0.0, w * 0.28)),
        |b| b.cuboid(Vec3::new(length * 0.8, w, w * 0.5)),
    );
    b.paint(pal.trim, 0.0);
    let end = length * 0.86;
    for y in [-1.0, 1.0] {
        strut(
            b,
            Vec3::new(end, y * w * 0.32, w * 0.28),
            Vec3::new(length + w * 0.15, y * w * 0.8, w * 0.12),
            w * 0.36,
        );
    }
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(Vec3::new(end, 0.0, w * 0.7)), |b| {
        b.cuboid(Vec3::splat(w * 0.4))
    });
}

fn banner(
    b: &mut MeshBuilder,
    width: f32,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
    mark: &str,
) {
    b.paint(pal.trim, 0.0);
    b.cuboid(Vec3::new(0.6, width, 0.6));
    b.with(
        Mat4::from_translation(Vec3::new(height * 0.42, 0.0, height * 0.12))
            * Mat4::from_rotation_y(-0.55),
        |b| {
            b.paint(color.unwrap_or(pal.accent), 0.0);
            b.cuboid(Vec3::new(height, width * 0.9, 0.16));
            b.paint(pal.hull, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * 0.12), |b| {
                b.cuboid(Vec3::new(height * 0.2, width * 0.9, 0.1))
            });
            b.with(
                Mat4::from_translation(Vec3::new(-height * 0.12, 0.0, 0.18)),
                |b| sigil(b, mark, (height * 0.42).min(width * 0.45), pal),
            );
        },
    );
}

fn arch(
    b: &mut MeshBuilder,
    width: f32,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
    motif: &str,
) {
    b.paint(color.unwrap_or(pal.trim), 0.0);
    let hw = width * 0.5;
    let thick = (width * 0.11).max(0.9);
    let post = match motif {
        "square" => height * 0.82,
        "pointed" => height * 0.55,
        _ => height * 0.48,
    };
    strut(
        b,
        Vec3::new(0.0, -hw, 0.0),
        Vec3::new(0.0, -hw, post),
        thick,
    );
    strut(b, Vec3::new(0.0, hw, 0.0), Vec3::new(0.0, hw, post), thick);
    match motif {
        "square" => {
            strut(
                b,
                Vec3::new(0.0, -hw, post),
                Vec3::new(0.0, hw, post),
                thick * 1.15,
            );
        }
        "pointed" => {
            let peak = Vec3::Z * height;
            strut(b, Vec3::new(0.0, -hw, post), peak, thick);
            strut(b, Vec3::new(0.0, hw, post), peak, thick);
        }
        _ => {
            let steps = 4;
            let rise = (height - post).max(0.5);
            for s in 0..steps {
                let a0 = TAU * 0.5 * s as f32 / steps as f32;
                let a1 = TAU * 0.5 * (s + 1) as f32 / steps as f32;
                let p = |a: f32| Vec3::new(0.0, -hw * a.cos(), post + rise * a.sin());
                strut(b, p(a0), p(a1), thick);
            }
        }
    }
    b.paint(pal.glow, 1.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.9), |b| {
        b.cuboid(Vec3::splat(thick * 0.4))
    });
}

fn spire(
    b: &mut MeshBuilder,
    height: f32,
    radius: f32,
    color: Option<Color>,
    pal: &StationPalette,
    motif: &str,
) {
    b.paint(color.unwrap_or(pal.hull), 0.0);
    b.lathe(
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
    b.paint(pal.glow, 1.0);
    let r = radius * 0.62;
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU;
        b.with(
            Mat4::from_translation(Vec3::new(a.cos() * r, a.sin() * r, height * 0.46))
                * Mat4::from_rotation_z(a),
            |b| b.cuboid(Vec3::new(0.22, radius * 0.28, height * 0.05)),
        );
    }
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.84), |b| {
        raised_cap(b, radius * 0.34, height * 0.18, motif, 6)
    });
}

fn block(
    b: &mut MeshBuilder,
    width: f32,
    depth: f32,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
    motif: &str,
) {
    b.paint(color.unwrap_or(pal.hull), 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.42), |b| {
        b.cuboid(Vec3::new(depth, width, height * 0.84))
    });
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.86), |b| {
        raised_cap(b, depth.min(width) * 0.42, height * 0.26, motif, 4)
    });
    b.paint(pal.trim, 0.0);
    b.with(
        Mat4::from_translation(Vec3::new(depth * 0.5, 0.0, height * 0.26)),
        |b| b.cuboid(Vec3::new(0.28, width * 0.22, height * 0.4)),
    );
    b.paint(pal.glow, 1.0);
    for y in [-0.28_f32, 0.28] {
        b.with(
            Mat4::from_translation(Vec3::new(depth * 0.5, y * width, height * 0.58)),
            |b| b.cuboid(Vec3::new(0.22, width * 0.14, height * 0.14)),
        );
    }
}

fn scaffold(
    b: &mut MeshBuilder,
    radius: f32,
    height: f32,
    color: Option<Color>,
    pal: &StationPalette,
) {
    b.paint(color.unwrap_or(pal.trim), 0.0);
    let posts = 5;
    let thick = (radius * 0.1).max(0.7);
    for k in 0..posts {
        let a = k as f32 / posts as f32 * TAU;
        let p = Vec3::new(a.cos(), a.sin(), 0.0) * radius * 0.82;
        strut(b, p, p + Vec3::Z * height, thick);
    }
    for level in [0.38, 0.78] {
        let z = height * level;
        for k in 0..posts {
            let a0 = k as f32 / posts as f32 * TAU;
            let a1 = (k + 1) as f32 / posts as f32 * TAU;
            let p = |a: f32| Vec3::new(a.cos(), a.sin(), 0.0) * radius * 0.82 + Vec3::Z * z;
            strut(b, p(a0), p(a1), thick * 0.75);
        }
    }
    b.paint(pal.hull, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height * 0.7), |b| {
        b.cuboid(Vec3::new(radius * 1.15, radius * 0.5, 0.3))
    });
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height), |b| {
        b.frustum(radius * 0.12, 0.0, radius * 0.4, 4)
    });
}

fn build_decal(
    b: &mut MeshBuilder,
    kind: &DecalKind,
    color: Option<Color>,
    pal: &StationPalette,
    mark: &str,
) {
    match *kind {
        DecalKind::Stripe {
            length,
            width,
            bands,
        } => {
            let bands = bands.clamp(2, 8).max(1);
            let band = length / bands as f32;
            let paint = color.unwrap_or(pal.accent);
            for k in 0..bands {
                b.paint(if k % 2 == 0 { paint } else { pal.hull }, 0.0);
                let x = -length * 0.5 + band * (k as f32 + 0.5);
                b.with(Mat4::from_translation(Vec3::new(x, 0.0, 0.12)), |b| {
                    b.cuboid(Vec3::new(band * 0.9, width, 0.16))
                });
            }
        }
        DecalKind::Chevron { length, width } => {
            let n = ((length / width.max(0.4)).round() as u32).clamp(2, 6);
            b.paint(color.unwrap_or(pal.accent), 0.0);
            for k in 0..n {
                let x = -length * 0.5 + length * (k as f32 + 0.5) / n as f32;
                for s in [-1.0_f32, 1.0] {
                    b.with(
                        Mat4::from_translation(Vec3::new(x, s * width * 0.22, 0.12))
                            * Mat4::from_rotation_z(s * 0.55),
                        |b| b.cuboid(Vec3::new(width * 0.7, width * 0.2, 0.16)),
                    );
                }
            }
        }
        DecalKind::Sigil { size } => {
            b.paint(pal.hull, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * 0.08), |b| {
                b.cuboid(Vec3::new(size * 1.2, size * 1.2, 0.14))
            });
            b.with(Mat4::from_translation(Vec3::Z * 0.22), |b| {
                sigil(b, mark, size * 0.7, pal)
            });
        }
        DecalKind::Number { value, size } => {
            b.paint(pal.hull, 0.0);
            let wide = if value >= 10 { size * 2.3 } else { size * 1.4 };
            b.with(Mat4::from_translation(Vec3::Z * 0.08), |b| {
                b.cuboid(Vec3::new(size * 1.35, wide, 0.14))
            });
            b.paint(color.unwrap_or(pal.trim), 0.0);
            digits(b, value.min(99), size, 0.22);
        }
    }
}

fn sigil(b: &mut MeshBuilder, mark: &str, size: f32, pal: &StationPalette) {
    let s = size.max(0.4);
    match mark {
        "toll_key" => {
            b.paint(pal.trim, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(-s * 0.28, 0.0, 0.0)),
                |b| b.torus(s * 0.2, s * 0.1, s * 0.1, 8, 4),
            );
            b.with(Mat4::from_translation(Vec3::new(s * 0.12, 0.0, 0.0)), |b| {
                b.cuboid(Vec3::new(s * 0.62, s * 0.14, s * 0.12))
            });
            for x in [0.28_f32, 0.42] {
                b.with(
                    Mat4::from_translation(Vec3::new(s * x, -s * 0.16, 0.0)),
                    |b| b.cuboid(Vec3::new(s * 0.1, s * 0.16, s * 0.12)),
                );
            }
        }
        "open_lamp" => {
            b.paint(pal.accent, 0.0);
            strut(
                b,
                Vec3::new(-s * 0.26, -s * 0.2, 0.0),
                Vec3::new(0.0, s * 0.28, 0.0),
                s * 0.12,
            );
            strut(
                b,
                Vec3::new(s * 0.26, -s * 0.2, 0.0),
                Vec3::new(0.0, s * 0.28, 0.0),
                s * 0.12,
            );
            b.with(Mat4::from_translation(Vec3::new(0.0, -s * 0.2, 0.0)), |b| {
                b.cuboid(Vec3::new(s * 0.64, s * 0.12, s * 0.1))
            });
            b.paint(pal.glow, 1.0);
            b.with(Mat4::from_translation(Vec3::Z * 0.06), |b| {
                b.lathe(&[(0.0, -s * 0.08), (s * 0.1, 0.0), (0.0, s * 0.08)], 4, 0.4)
            });
        }
        _ => {
            b.paint(pal.trim, 0.0);
            b.lathe(
                &[
                    (s * 0.1, 0.0),
                    (s * 0.3, s * 0.06),
                    (s * 0.28, s * 0.2),
                    (s * 0.14, s * 0.28),
                ],
                8,
                0.0,
            );
            b.paint(pal.glow, 1.0);
            b.with(
                Mat4::from_translation(Vec3::new(s * 0.2, s * 0.18, s * 0.12)),
                |b| {
                    b.lathe(
                        &[(s * 0.1, 0.0), (s * 0.15, s * 0.1), (s * 0.06, s * 0.2)],
                        6,
                        0.0,
                    )
                },
            );
            b.paint(pal.accent, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(s * 0.2, s * 0.18, s * 0.34)),
                |b| b.frustum(s * 0.08, 0.0, s * 0.14, 5),
            );
        }
    }
}

fn digits(b: &mut MeshBuilder, value: u32, size: f32, z: f32) {
    let value = value.min(99);
    let gap = size * 0.85;
    if value >= 10 {
        digit(b, value / 10, size, Vec3::new(-gap * 0.5, 0.0, z));
        digit(b, value % 10, size, Vec3::new(gap * 0.5, 0.0, z));
    } else {
        digit(b, value, size, Vec3::new(0.0, 0.0, z));
    }
}

fn digit(b: &mut MeshBuilder, n: u32, size: f32, at: Vec3) {
    // 0 top, 1 upper left, 2 upper right, 3 middle, 4 lower left, 5 lower right, 6 bottom.
    let segs: [bool; 7] = match n % 10 {
        0 => [true, true, true, false, true, true, true],
        1 => [false, false, true, false, false, true, false],
        2 => [true, false, true, true, true, false, true],
        3 => [true, false, true, true, false, true, true],
        4 => [false, true, true, true, false, true, false],
        5 => [true, true, false, true, false, true, true],
        6 => [true, true, false, true, true, true, true],
        7 => [true, false, true, false, false, true, false],
        8 => [true, true, true, true, true, true, true],
        _ => [true, true, true, true, false, true, true],
    };
    let w = size * 0.42;
    let h = size * 0.4;
    let t = (size * 0.16).max(0.22);
    let bars = [
        (0, Vec3::new(0.0, h, 0.0), Vec3::new(w * 2.0, t, t)),
        (3, Vec3::ZERO, Vec3::new(w * 2.0, t, t)),
        (6, Vec3::new(0.0, -h, 0.0), Vec3::new(w * 2.0, t, t)),
        (1, Vec3::new(-w, h * 0.5, 0.0), Vec3::new(t, h, t)),
        (2, Vec3::new(w, h * 0.5, 0.0), Vec3::new(t, h, t)),
        (4, Vec3::new(-w, -h * 0.5, 0.0), Vec3::new(t, h, t)),
        (5, Vec3::new(w, -h * 0.5, 0.0), Vec3::new(t, h, t)),
    ];
    b.with(Mat4::from_translation(at), |b| {
        for (i, pos, dims) in bars {
            if segs[i] {
                b.with(Mat4::from_translation(pos), |b| b.cuboid(dims));
            }
        }
    });
}

/// The part kinds a recipe can use, for tools and validation messages.
pub const PART_KINDS: [&str; 24] = [
    "hub",
    "docking_ring",
    "arm",
    "greenhouse",
    "antenna",
    "tank",
    "sail",
    "lantern",
    "lens",
    "cargo_pod",
    "crane",
    "solar_petal",
    "lamp_mast",
    "dome",
    "habitat_ring",
    "beacon",
    "signage",
    "antenna_cluster",
    "docking_arm",
    "banner",
    "arch",
    "spire",
    "block",
    "scaffold",
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
            decals: vec![],
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
            decals: vec![],
            parts: vec![],
        };
        assert_eq!(empty.validate().unwrap_err().len(), 2);
    }

    #[test]
    fn decal_placement_and_reach_are_rejected() {
        let bible = ArtBible::bundled();
        let both = |recipe: &StationRecipe| {
            vec![
                recipe.validate().unwrap_err(),
                recipe.validate_art(bible).unwrap_err(),
            ]
        };

        // Every field is inside its own range. The transformed reach is not.
        let mut far = StationRecipe::lantern_wharf();
        far.decals = vec![Decal::new(
            DecalKind::Sigil { size: 12.0 },
            Placement {
                distance: 120.0,
                scale: 5.0,
                ..Placement::default()
            },
        )];
        assert!(far.decals[0].at.scale <= 5.0);
        for errs in both(&far) {
            let text: Vec<_> = errs.iter().map(|e| e.to_string()).collect();
            assert!(
                text.iter()
                    .any(|t| t.contains("decal 0") && t.contains("reaches")),
                "{text:?}"
            );
        }

        // Re-check of #23: a two-digit plate at the edge. Its backing plate's corner
        // reaches about 121.3, past the 120 limit, so both checks must refuse it.
        let mut plate = StationRecipe::lantern_wharf();
        plate.decals = vec![Decal::new(
            DecalKind::Number {
                value: 42,
                size: 8.0,
            },
            Placement {
                distance: 112.0,
                turn: 90.0,
                ..Placement::default()
            },
        )];
        for errs in both(&plate) {
            let text: Vec<_> = errs.iter().map(|e| e.to_string()).collect();
            assert!(text.iter().any(|t| t.contains("reaches")), "{text:?}");
        }

        // Re-check 2 of #23: a short, wide chevron whose rotated bar corner reaches
        // about 120.08 at this placement.
        let mut chevron = StationRecipe::lantern_wharf();
        chevron.decals = vec![Decal::new(
            DecalKind::Chevron {
                length: 2.0,
                width: 8.0,
            },
            Placement {
                distance: 115.46,
                turn: -57.700665,
                ..Placement::default()
            },
        )];
        for errs in both(&chevron) {
            let text: Vec<_> = errs.iter().map(|e| e.to_string()).collect();
            assert!(text.iter().any(|t| t.contains("reaches")), "{text:?}");
        }

        let mut nan = StationRecipe::lantern_wharf();
        nan.decals = vec![Decal::new(
            DecalKind::Sigil { size: 2.0 },
            Placement {
                distance: f32::NAN,
                ..Placement::default()
            },
        )];
        for errs in both(&nan) {
            assert!(
                errs.iter()
                    .any(|e| e.to_string().contains("decal 0: at.distance")),
                "{errs:?}"
            );
        }

        let mut range = StationRecipe::lantern_wharf();
        range.decals = vec![Decal::new(
            DecalKind::Number {
                value: 4,
                size: 2.0,
            },
            Placement {
                scale: 9.0,
                z: 80.0,
                ..Placement::default()
            },
        )];
        for errs in both(&range) {
            let text = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("decal 0: at.scale"), "{text}");
            assert!(text.contains("decal 0: at.z"), "{text}");
        }
    }

    #[test]
    fn lights_follow_lanterns() {
        let lights = station_lights(&StationRecipe::lantern_wharf());
        // 1 lantern, 2 lamp masts, the greenhouse lamp and 4 ring beacons.
        assert_eq!(lights.len(), 8);
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
