//! Stations as recipes: a list of parametric parts plus a palette.
//!
//! A [`StationRecipe`] is plain serde data, so authored stations, the Kettle Dock
//! greenhouse stages and stations the game master invents later are all the same thing.
//! Validate a recipe with [`StationRecipe::validate`], then let the render crate build its meshes.
//!
//! Local axes: the station centre is the origin, the play plane is `z = 0`, +Z is up.
//! Each part sits at a polar [`Placement`]; in a part's own frame +X points away from the
//! station centre.

use crate::art_bible::ArtBible;
use crate::color::Color;
use crate::palette;
use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use std::fmt;

/// The colours a station is painted with. Parts pick slots from it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
            libm::cosf(a) * self.distance,
            libm::sinf(a) * self.distance,
            self.z,
        )) * Mat4::from_rotation_z(a + self.turn.to_radians())
            * Mat4::from_scale(Vec3::splat(self.scale))
    }
}

/// A station part and its parameters. In JSON the variant is the `"part"` field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
        let mut errs = self.validate().err().unwrap_or_default();
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
    let half_diagonal = |x: f32, y: f32| 0.5 * libm::hypotf(x, y);
    MARGIN
        * match *kind {
            DecalKind::Stripe { length, width, .. } => half_diagonal(length, width),
            // Each chevron bar is a `0.7w x 0.2w` box centred at `(x, ±0.22w)` with
            // `|x| <= length/2`, rotated about its own centre, so its farthest corner is
            // at most the centre's distance plus the bar's half-diagonal.
            DecalKind::Chevron { length, width } => {
                libm::hypotf(0.5 * length, 0.22 * width) + half_diagonal(0.7 * width, 0.2 * width)
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
            let at = p.at;
            let a = at.angle.to_radians();
            Some(Vec2::new(libm::cosf(a), libm::sinf(a)) * at.distance)
        }
        _ => None,
    })
}

/// The kind name of a part as the art bible and its budget name it (`"block"`).
pub fn kind_name(kind: &PartKind) -> &'static str {
    part_params(kind).0
}

impl StationRecipe {
    /// The recipe that draws a station at its build stage, by station ID: the five
    /// authored stations, and a small generic one for any other (the game master's
    /// sixth) until it carries its own.
    pub fn for_station(id: &str, stage: u8) -> Self {
        let stage = stage.min(3);
        match id {
            "station.lantern_wharf" => Self::lantern_wharf(),
            "station.kettle_dock" => Self::kettle_dock(stage),
            "station.brassmouth_exchange" => Self::brassmouth(),
            "station.cinder_hook" => Self::cinder_hook(),
            "station.glass_drift_observatory" => Self::glass_drift_observatory(),
            _ => {
                let mut r = Self::kettle_dock(stage);
                r.parts
                    .retain(|p| !matches!(p.kind, PartKind::Greenhouse { .. }) || stage > 0);
                r
            }
        }
    }
}

/// A named spot on a station where a part's module bolts on (issue #47, 3.1): a quay, a
/// pier slot, a rim berth. In station space, like a part's placement: the module's +X
/// points away from the centre.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub name: &'static str,
    pub at: Placement,
}

/// The anchors on a station's recipe, by station ID: the authored stations' own, kept
/// clear of their art, or for any other station a ring of eight pier slots just outside
/// its recipe's footprint (`pier.1` east, counter-clockwise).
pub fn anchors(station: &str, recipe: &StationRecipe) -> Vec<Anchor> {
    let a = |name, angle, distance| Anchor {
        name,
        at: Placement::polar(angle, distance),
    };
    match station {
        "station.lantern_wharf" => vec![
            a("quay.west", 185.0, 22.0),
            a("quay.north", 116.0, 23.0),
            a("pier.south", 262.0, 22.0),
            a("quay.northwest", 150.0, 24.0),
            a("pier.southwest", 222.0, 23.0),
        ],
        "station.kettle_dock" => vec![
            a("rim.north", 100.0, 21.0),
            a("rim.west", 215.0, 21.0),
            a("rim.east", 345.0, 24.0),
            a("rim.northwest", 140.0, 22.0),
        ],
        "station.cinder_hook" => vec![
            a("hook.north", 112.0, 21.0),
            a("hook.west", 222.0, 21.0),
            a("hook.south", 278.0, 21.0),
            a("hook.northwest", 160.0, 21.0),
        ],
        "station.brassmouth_exchange" => vec![
            a("row.north", 90.0, 27.0),
            a("row.southwest", 248.0, 27.0),
            a("row.southeast", 325.0, 27.0),
            a("row.west", 158.0, 28.0),
        ],
        "station.glass_drift_observatory" => vec![
            a("deck.north", 125.0, 22.0),
            a("deck.west", 168.0, 23.0),
            a("deck.east", 330.0, 23.0),
            a("deck.south", 290.0, 25.0),
        ],
        _ => {
            const PIER: [&str; 8] = [
                "pier.1", "pier.2", "pier.3", "pier.4", "pier.5", "pier.6", "pier.7", "pier.8",
            ];
            let d = (recipe.footprint() + 6.0).clamp(12.0, MAX_STATION_RADIUS - 12.0);
            PIER.iter()
                .enumerate()
                .map(|(i, name)| a(name, i as f32 * 45.0, d))
                .collect()
        }
    }
}

/// The module kinds a station part can be drawn as: kit part kinds that read from
/// above, each with the parameters its module is drawn with.
pub const MODULE_KINDS: [&str; 9] = [
    "block",
    "banner",
    "crane",
    "dome",
    "tank",
    "cargo_pod",
    "docking_arm",
    "greenhouse",
    "lamp_mast",
];

/// The kit part a module of `kind` is drawn as, `level` making it a little bigger (an
/// upgraded part). `None` for a kind that isn't a module kind.
pub fn module_part(kind: &str, level: u32) -> Option<PartKind> {
    let grow = 1.0 + 0.12 * level.saturating_sub(1).min(4) as f32;
    Some(match kind {
        "block" => PartKind::Block {
            width: 6.0 * grow,
            depth: 5.0 * grow,
            height: 3.6,
        },
        "banner" => PartKind::Banner {
            width: 7.0 * grow,
            height: 4.2,
        },
        "crane" => PartKind::Crane {
            height: 8.0,
            reach: 7.0 * grow,
        },
        "dome" => PartKind::Dome { radius: 3.6 * grow },
        "tank" => PartKind::Tank {
            radius: 1.6,
            length: 5.0 * grow,
            count: 2,
        },
        "cargo_pod" => PartKind::CargoPod {
            radius: 1.8,
            length: 5.0 * grow,
        },
        "docking_arm" => PartKind::DockingArm { length: 8.0 * grow },
        "greenhouse" => PartKind::Greenhouse {
            radius: 3.8 * grow,
            stage: 3,
        },
        "lamp_mast" => PartKind::LampMast { height: 7.0 },
        _ => return None,
    })
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    const AUTHORED: [&str; 5] = [
        "station.lantern_wharf",
        "station.kettle_dock",
        "station.cinder_hook",
        "station.brassmouth_exchange",
        "station.glass_drift_observatory",
    ];

    #[test]
    fn anchors_have_unique_names_and_sit_inside_the_station() {
        for id in AUTHORED.iter().chain(&["gm.sixth_station"]) {
            let recipe = StationRecipe::for_station(id, 0);
            let list = anchors(id, &recipe);
            assert!(list.len() >= 3, "{id}");
            let mut names: Vec<_> = list.iter().map(|a| a.name).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), list.len(), "{id}: duplicate anchor names");
            for a in &list {
                assert!(a.at.distance + 12.0 < MAX_STATION_RADIUS, "{id} {}", a.name);
            }
        }
    }

    #[test]
    fn every_module_kind_passes_the_art_bible_at_every_level() {
        let bible = ArtBible::bundled();
        for kind in MODULE_KINDS {
            for level in 1..=5 {
                let part = module_part(kind, level).unwrap();
                assert_eq!(kind_name(&part), kind);
                let (name, params) = part_params(&part);
                for (p, v) in params {
                    assert_eq!(bible.proportion_issue(name, p, v), None, "{kind} {p}");
                }
                assert!(bible.kind_cap(kind) >= 1, "{kind} has a budget");
            }
        }
        assert_eq!(module_part("spire", 1), None, "not a module kind");
    }
}
