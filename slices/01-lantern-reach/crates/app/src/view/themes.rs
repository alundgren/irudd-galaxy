//! Station themes as data (#47, 4.1; the station concept's "Theming"): how a faction
//! builds, lights and peoples its stations. A market at a Compact exchange and one at a
//! Cooperative wharf work the same, but the theme decides the props and residents around
//! them, their shapes and their paint, and the calm caps on how much moves.
//!
//! View data only: the sim never reads it. It lives in the core pack's
//! `data/themes.toml`, parsed strictly (unknown keys and names are errors) and checked
//! against the world (factions, stations and craft hulls must exist). A station takes
//! the theme named for it under `[station."<id>"]`, else the theme of its owner's
//! faction, else `default`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use galaxy_contract::prelude::*;
use render::ArtBible;
use render::kit::life::{Motif, StockStyle};
use serde::Deserialize;

/// The file in a pack's `data/` folder.
pub const FILE: &str = "themes.toml";

/// The layout a station's life follows: where residents walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spine {
    /// Out along the struts to each module and back.
    Pier,
    /// Round the hub.
    Ring,
    /// Along the quay that the modules face.
    Quay,
    /// Close about a tall centre.
    Spire,
}

/// Ambient props a theme sets around its stations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prop {
    /// Lamp posts along the quay, lit one more per station pulse by the lamplighter.
    LampString,
    /// Stock as timber crates and barrels (one stock style per theme).
    CratePile,
    /// Stock as square brass bins.
    BrassBins,
    /// Stock as tall glass jars.
    GlassJars,
    /// Soft steam off the hub.
    Steam,
    /// Warm light pooled on greenhouse glass.
    GreenhouseGlow,
    /// The owner's banner on a pole.
    Banner,
    /// A row of signal lamps on the hub that brighten slowly in turn.
    SignalLamps,
}

impl Prop {
    fn stock(self) -> Option<StockStyle> {
        match self {
            Prop::CratePile => Some(StockStyle::Crates),
            Prop::BrassBins => Some(StockStyle::Bins),
            Prop::GlassJars => Some(StockStyle::Jars),
            _ => None,
        }
    }
}

/// Who walks a station's pier, and what their coat says they do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resident {
    Dockhand,
    Lamplighter,
    Grower,
    Clerk,
    Porter,
    Scholar,
    Welder,
}

impl Resident {
    pub const ALL: [Resident; 7] = [
        Resident::Dockhand,
        Resident::Lamplighter,
        Resident::Grower,
        Resident::Clerk,
        Resident::Porter,
        Resident::Scholar,
        Resident::Welder,
    ];
}

/// The ambient sounds a theme may name. Names only for now: no new audio is made for
/// them, and the existing cues keep playing as they do.
pub const SOUNDS: [&str; 10] = [
    "kettle_whistle",
    "hammering",
    "market_talk",
    "toll_bell",
    "ledger_scratch",
    "brass_chime",
    "lamp_hiss",
    "wind_chimes",
    "low_hum",
    "pages",
];

/// How much may move at one station at once. The life layer never draws more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calm {
    /// Residents walking (the crowd and the workers are counted separately).
    pub max_walkers: u32,
    /// Craft moving near the station: filler traffic only fills up to this with what
    /// the economy's own craft leave.
    pub max_moving_craft: u32,
    /// Steam puffs, sparks and other small glows.
    pub max_particles: u32,
    /// People in a crowd at the notice wall.
    #[serde(default = "default_crowd")]
    pub max_crowd: u32,
}

fn default_crowd() -> u32 {
    6
}

/// Hard ceilings on any theme's caps: a theme can ask for less, never more.
pub const CALM_CEILING: Calm = Calm {
    max_walkers: 16,
    max_moving_craft: 6,
    max_particles: 48,
    max_crowd: 10,
};

/// How a theme paints its props and residents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Paint {
    /// The station's own faction paint.
    Station,
    /// A faction's paint from the art bible.
    Faction(String),
}

/// One theme, parsed and checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub id: String,
    /// The faction whose stations wear it.
    pub faction: Option<FactionId>,
    pub spine: Vec<Spine>,
    pub motif: Motif,
    pub paint: Paint,
    pub props: BTreeSet<Prop>,
    pub residents: Vec<Resident>,
    /// Hulls for filler traffic when no real craft is near.
    pub craft: Vec<HullId>,
    pub sounds: Vec<String>,
    /// The sign style: a faction mark from the art bible, or `plain`.
    pub signage: String,
    /// How many lamps the lamplighter tends.
    pub lamps: u32,
    pub calm: Calm,
}

impl Theme {
    /// The stock pile's style: the theme's stock prop, or crates.
    pub fn stock(&self) -> StockStyle {
        self.props
            .iter()
            .find_map(|p| p.stock())
            .unwrap_or_default()
    }

    pub fn has(&self, prop: Prop) -> bool {
        self.props.contains(&prop)
    }

    /// The main spine: where residents walk.
    pub fn walk(&self) -> Spine {
        self.spine.first().copied().unwrap_or(Spine::Quay)
    }

    /// Who lights the lamps: the theme's lamplighter, else its first resident.
    pub fn lamplighter(&self) -> Resident {
        if self.residents.contains(&Resident::Lamplighter) {
            Resident::Lamplighter
        } else {
            self.residents.first().copied().unwrap_or(Resident::Dockhand)
        }
    }

    /// Who works a scaffold: the theme's welder, else its first resident.
    pub fn builder(&self) -> Resident {
        if self.residents.contains(&Resident::Welder) {
            Resident::Welder
        } else {
            self.residents.first().copied().unwrap_or(Resident::Dockhand)
        }
    }

    /// The art bible faction whose colours paint this theme at `station`.
    pub fn paint_faction<'a>(&'a self, station: Option<&'a Station>) -> Option<&'a str> {
        match &self.paint {
            Paint::Faction(f) => Some(f.as_str()),
            Paint::Station => station.and_then(|s| s.owner.as_ref()).map(|f| f.as_str()),
        }
    }
}

/// Every theme, and which station wears which.
#[derive(Clone, Debug, PartialEq)]
pub struct Themes {
    themes: BTreeMap<String, Theme>,
    stations: BTreeMap<StationId, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    theme: BTreeMap<String, ThemeDef>,
    #[serde(default)]
    station: BTreeMap<String, StationDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StationDef {
    theme: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeDef {
    #[serde(default)]
    faction: Option<String>,
    spine: Vec<Spine>,
    motif: String,
    paint: String,
    props: Vec<Prop>,
    residents: Vec<Resident>,
    craft: Vec<String>,
    sounds: Vec<String>,
    signage: String,
    #[serde(default = "default_lamps")]
    lamps: u32,
    calm: Calm,
}

fn default_lamps() -> u32 {
    8
}

/// The theme every station falls back to.
pub const DEFAULT: &str = "default";

impl Themes {
    /// Parses and checks a `themes.toml`. Every problem is listed, one per line.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = toml::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
        let mut errors = Vec::new();
        let bible = ArtBible::bundled();
        let mut themes = BTreeMap::new();
        let mut owners: BTreeMap<String, String> = BTreeMap::new();
        for (id, def) in file.theme {
            let mut err = |m: String| errors.push(format!("{FILE}: theme `{id}`: {m}"));
            let Some(motif) = Motif::parse(&def.motif) else {
                err(format!(
                    "motif `{}` isn't round, square or pointed",
                    def.motif
                ));
                continue;
            };
            let paint = if def.paint == "station" {
                Paint::Station
            } else if bible.faction(&def.paint).is_some() {
                Paint::Faction(def.paint.clone())
            } else {
                err(format!(
                    "paint `{}` is neither `station` nor a faction in the art bible",
                    def.paint
                ));
                Paint::Station
            };
            let marks: BTreeSet<&str> = bible.factions.values().map(|f| f.mark.as_str()).collect();
            if def.signage != "plain" && !marks.contains(def.signage.as_str()) {
                err(format!(
                    "signage `{}` is neither `plain` nor a faction mark in the art bible",
                    def.signage
                ));
            }
            if def.spine.is_empty() {
                err("spine needs at least one layout".into());
            }
            if def.residents.is_empty() {
                err("residents needs at least one kind".into());
            }
            if def.props.iter().filter(|p| p.stock().is_some()).count() > 1 {
                err("at most one stock style (crate_pile, brass_bins, glass_jars)".into());
            }
            for s in &def.sounds {
                if !SOUNDS.contains(&s.as_str()) {
                    err(format!("unknown sound `{s}` (known: {})", SOUNDS.join(", ")));
                }
            }
            if !(1..=16).contains(&def.lamps) {
                err(format!("lamps {} is outside 1..=16", def.lamps));
            }
            let c = def.calm;
            let ceil = CALM_CEILING;
            if c.max_walkers > ceil.max_walkers
                || c.max_moving_craft > ceil.max_moving_craft
                || c.max_particles > ceil.max_particles
                || c.max_crowd > ceil.max_crowd
            {
                err(format!("calm caps are above the ceiling {ceil:?}"));
            }
            let mut craft = Vec::new();
            for h in &def.craft {
                match h.parse::<HullId>() {
                    Ok(h) => craft.push(h),
                    Err(e) => err(format!("craft `{h}`: {e}")),
                }
            }
            let faction = match def.faction.as_deref().map(str::parse::<FactionId>) {
                None => None,
                Some(Ok(f)) => {
                    if let Some(other) = owners.insert(f.to_string(), id.clone()) {
                        err(format!("faction `{f}` already has theme `{other}`"));
                    }
                    Some(f)
                }
                Some(Err(e)) => {
                    err(format!("faction: {e}"));
                    None
                }
            };
            let theme = Theme {
                id: id.clone(),
                faction,
                spine: def.spine,
                motif,
                paint,
                props: def.props.into_iter().collect(),
                residents: def.residents,
                craft,
                sounds: def.sounds,
                signage: def.signage,
                lamps: def.lamps,
                calm: def.calm,
            };
            themes.insert(id, theme);
        }
        if !themes.contains_key(DEFAULT) {
            errors.push(format!("{FILE}: there is no `{DEFAULT}` theme"));
        }
        let mut stations = BTreeMap::new();
        for (station, def) in file.station {
            if !themes.contains_key(&def.theme) {
                errors.push(format!(
                    "{FILE}: station `{station}`: no theme `{}`",
                    def.theme
                ));
            }
            match station.parse::<StationId>() {
                Ok(s) => {
                    stations.insert(s, def.theme);
                }
                Err(e) => errors.push(format!("{FILE}: station `{station}`: {e}")),
            }
        }
        if errors.is_empty() {
            Ok(Self { themes, stations })
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Reads and checks `themes.toml` in a pack's data folder.
    pub fn load(data: &Path) -> Result<Self, String> {
        let path = data.join(FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{FILE}: {e}"))?;
        Self::parse(&text)
    }

    /// The core pack's themes, built in: what a view uses until it's given others.
    pub fn bundled() -> &'static Themes {
        static BUNDLED: OnceLock<Themes> = OnceLock::new();
        BUNDLED.get_or_init(|| {
            Self::parse(include_str!("../../../../packs/core/data/themes.toml"))
                .expect("the core pack's themes.toml is checked by its tests")
        })
    }

    /// Checks the references into the world: factions, stations and craft hulls.
    pub fn check(&self, world: &WorldState) -> Result<(), String> {
        let mut errors = Vec::new();
        for t in self.themes.values() {
            if let Some(f) = &t.faction
                && !world.factions.contains(f)
            {
                errors.push(format!("{FILE}: theme `{}`: no faction `{f}`", t.id));
            }
            for h in &t.craft {
                if !world.hulls.contains(h) {
                    errors.push(format!("{FILE}: theme `{}`: no hull `{h}`", t.id));
                }
            }
        }
        for s in self.stations.keys() {
            if !world.stations.contains(s) {
                errors.push(format!("{FILE}: no station `{s}`"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.themes.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Theme> {
        self.themes.values()
    }

    /// The theme `station` wears: its own, else its owner's, else the default.
    pub fn for_station(&self, world: &WorldState, station: &StationId) -> &Theme {
        let owner = world.stations.get(station).and_then(|s| s.owner.as_ref());
        self.stations
            .get(station)
            .and_then(|id| self.themes.get(id))
            .or_else(|| {
                let owner = owner?;
                self.themes
                    .values()
                    .find(|t| t.faction.as_ref() == Some(owner))
            })
            .or_else(|| self.themes.get(DEFAULT))
            .expect("parse checks there is a default theme")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_world() -> WorldState {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        galaxy_core::new_game(&dir, 1).unwrap().world
    }

    #[test]
    fn the_core_themes_load_and_check_against_the_world() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let themes = Themes::load(&dir).unwrap();
        assert_eq!(&themes, Themes::bundled());
        let world = core_world();
        themes.check(&world).unwrap();
        // One theme per faction in the slice, plus the default.
        for f in world.factions.keys() {
            assert!(
                themes.iter().any(|t| t.faction.as_ref() == Some(f)),
                "{f} has no theme"
            );
        }
        let pick = |s: &str| themes.for_station(&world, &s.parse().unwrap()).id.clone();
        assert_eq!(pick("station.lantern_wharf"), "wharf_cooperative");
        assert_eq!(pick("station.brassmouth_exchange"), "tollway_compact");
        assert_eq!(pick("station.glass_drift_observatory"), "lamplighter_society");
        // Cinder Hook is claimed by two factions and owned by neither: its own theme.
        assert_eq!(pick("station.cinder_hook"), "default");
        // A Compact market and a Cooperative one look different.
        let coop = themes.get("wharf_cooperative").unwrap();
        let compact = themes.get("tollway_compact").unwrap();
        assert_ne!(coop.stock(), compact.stock());
        assert_ne!(coop.motif, compact.motif);
        assert_ne!(coop.residents, compact.residents);
    }

    const GOOD: &str = r#"
[theme.default]
spine = ["quay"]
motif = "round"
paint = "station"
props = ["crate_pile"]
residents = ["dockhand"]
craft = []
sounds = []
signage = "plain"
calm = { max_walkers = 4, max_moving_craft = 1, max_particles = 8 }
"#;

    #[test]
    fn unknown_keys_and_names_are_errors() {
        Themes::parse(GOOD).unwrap();
        for (bad, why) in [
            (GOOD.replace("signage", "sign_style"), "unknown field"),
            (GOOD.replace("\"quay\"", "\"maze\""), "unknown variant"),
            (GOOD.replace("crate_pile", "crate_pyramid"), "unknown variant"),
            (GOOD.replace("dockhand", "pirate"), "unknown variant"),
            (GOOD.replace("\"round\"", "\"wobbly\""), "motif"),
            (GOOD.replace("\"station\"", "\"faction.nobody\""), "paint"),
            (GOOD.replace("\"plain\"", "\"skull\""), "signage"),
            (GOOD.replace("sounds = []", "sounds = [\"boom\"]"), "sound"),
            (
                GOOD.replace("max_particles = 8", "max_particles = 8, max_fun = 1"),
                "unknown field",
            ),
            (
                GOOD.replace("max_walkers = 4", "max_walkers = 400"),
                "ceiling",
            ),
            (
                GOOD.replace("[\"crate_pile\"]", "[\"crate_pile\", \"brass_bins\"]"),
                "stock style",
            ),
            (GOOD.replace("theme.default", "theme.other"), "no `default`"),
            (
                format!("{GOOD}\n[station.\"station.lantern_wharf\"]\ntheme = \"nope\"\n"),
                "no theme `nope`",
            ),
        ] {
            let e = Themes::parse(&bad).unwrap_err();
            assert!(e.contains(why), "{why}: {e}");
        }
    }

    #[test]
    fn dangling_world_references_are_errors() {
        let world = core_world();
        let text = GOOD.replace("craft = []", "craft = [\"hull.nothing\"]")
            + "\n[station.\"station.nowhere\"]\ntheme = \"default\"\n";
        let themes = Themes::parse(&text).unwrap();
        let e = themes.check(&world).unwrap_err();
        assert!(e.contains("hull.nothing") && e.contains("station.nowhere"), "{e}");
        let text = GOOD.replace("motif", "faction = \"faction.nobody\"\nmotif");
        let e = Themes::parse(&text).unwrap().check(&world).unwrap_err();
        assert!(e.contains("faction.nobody"), "{e}");
    }
}
