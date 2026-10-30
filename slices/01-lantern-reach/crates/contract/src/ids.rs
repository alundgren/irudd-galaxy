//! Stable IDs.
//!
//! Every authored or live-created thing has a namespaced string ID such as
//! `station.kettle_dock`, `ware.ice` or `rock.shallows.ice.03`. Things a pack creates
//! live are namespaced by that pack, for example `gm.sixth_station`. Saves, scripts,
//! history facts and the CLI all speak these IDs, which is what lets progress survive
//! reloads and content changes.
//!
//! Syntax: dot-separated segments of `[a-z0-9_]`, the first starting with a letter,
//! at most [`MAX_ID_LEN`] bytes. Entity IDs need at least two segments; [`PackId`] and
//! [`FactKind`](crate::history::FactKind) allow one.
//!
//! Each kind has its own newtype so a `WareId` can't be passed where a `StationId` is
//! expected. They are ordered by their string, cheap to clone (a static `&str` or an
//! `Arc<str>`), and borrow as `&str` so `store.get("station.kettle_dock")` works.
//!
//! History facts and causes use sequence IDs instead ([`FactId`], [`CauseId`]): a
//! number inside the engine, written as `fact.42` / `cause.42` in JSON.

use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Longest accepted ID, in bytes.
pub const MAX_ID_LEN: usize = 128;

/// Checks the ID syntax. `const` so `from_static` can check at compile time.
pub const fn is_valid_id(s: &str, min_segments: usize) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_ID_LEN || !b[0].is_ascii_lowercase() {
        return false;
    }
    let mut segments = 1;
    let mut seg_len = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'.' {
            if seg_len == 0 {
                return false;
            }
            segments += 1;
            seg_len = 0;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' {
            seg_len += 1;
        } else {
            return false;
        }
        i += 1;
    }
    seg_len > 0 && segments >= min_segments
}

/// An ID that failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError {
    pub kind: &'static str,
    pub value: String,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid {} id {:?}: expected dot-separated [a-z0-9_] segments, like `station.kettle_dock`",
            self.kind, self.value
        )
    }
}

impl std::error::Error for IdError {}

/// Shared string storage for IDs: static for constants, `Arc` for everything loaded.
#[derive(Clone)]
enum IdStr {
    Static(&'static str),
    Shared(Arc<str>),
}

impl IdStr {
    fn as_str(&self) -> &str {
        match self {
            IdStr::Static(s) => s,
            IdStr::Shared(s) => s,
        }
    }
}

impl PartialEq for IdStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for IdStr {}
impl PartialOrd for IdStr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IdStr {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}
impl Hash for IdStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, kind = $kind:literal, min_segments = $min:literal, example = $example:literal) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(IdStr);

        impl $name {
            /// What this ID names, used in error messages.
            pub const KIND: &'static str = $kind;

            /// A compile-time constant ID. Panics (at compile time in `const` context)
            /// if the syntax is wrong.
            pub const fn from_static(s: &'static str) -> Self {
                assert!(is_valid_id(s, $min), "invalid id syntax");
                Self(IdStr::Static(s))
            }

            /// Parses and validates an ID.
            pub fn new(s: impl AsRef<str>) -> Result<Self, IdError> {
                let s = s.as_ref();
                if is_valid_id(s, $min) {
                    Ok(Self(IdStr::Shared(Arc::from(s))))
                } else {
                    Err(IdError { kind: $kind, value: s.to_owned() })
                }
            }

            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.as_str())
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }
        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, IdError> {
                Self::new(s)
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = Cow::<'de, str>::deserialize(d)?;
                Self::new(&*s).map_err(serde::de::Error::custom)
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                let pattern = if $min > 1 { ID_PATTERN } else { ID_PATTERN_ONE };
                json_schema!({
                    "type": "string",
                    "description": concat!("Stable ", $kind, " id, e.g. `", $example, "`."),
                    "pattern": pattern,
                    "maxLength": MAX_ID_LEN,
                    "examples": [$example],
                })
            }
        }
    };
}

/// JSON Schema pattern for IDs with two or more segments.
pub const ID_PATTERN: &str = r"^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$";
/// JSON Schema pattern for IDs with one or more segments.
pub const ID_PATTERN_ONE: &str = r"^[a-z][a-z0-9_]*(\.[a-z0-9_]+)*$";

string_id!(
    /// A region of space, e.g. Lantern Shallows.
    RegionId, kind = "region", min_segments = 2, example = "region.lantern_shallows");
string_id!(
    /// A lane linking two regions.
    LaneId, kind = "lane", min_segments = 2, example = "lane.shallows_cinder");
string_id!(
    /// A station.
    StationId, kind = "station", min_segments = 2, example = "station.kettle_dock");
string_id!(
    /// A commodity.
    WareId, kind = "ware", min_segments = 2, example = "ware.ice");
string_id!(
    /// One rock in a field.
    RockId, kind = "rock", min_segments = 2, example = "rock.shallows.ice.03");
string_id!(
    /// A kind of rock (what it yields, how it mines).
    RockTypeId, kind = "rock type", min_segments = 2, example = "rock_type.ice");
string_id!(
    /// A rock field, the authored source of rocks.
    FieldId, kind = "field", min_segments = 2, example = "field.shallows.ice");
string_id!(
    /// A ship: the player's, an automated craft or an NPC craft.
    ShipId, kind = "ship", min_segments = 2, example = "ship.player");
string_id!(
    /// A hull class, e.g. the Wren.
    HullId, kind = "hull", min_segments = 2, example = "hull.wren");
string_id!(
    /// A named person.
    NpcId, kind = "npc", min_segments = 2, example = "npc.ilse");
string_id!(
    /// A faction.
    FactionId, kind = "faction", min_segments = 2, example = "faction.wharf_cooperative");
string_id!(
    /// A rumor.
    RumorId, kind = "rumor", min_segments = 2, example = "rumor.far_belt_glassite");
string_id!(
    /// A station part from the runtime asset kit.
    PartId, kind = "part", min_segments = 2, example = "part.kettle.greenhouse_a");
string_id!(
    /// A per-save mechanic flag (control unlocks, script switches).
    FlagId, kind = "flag", min_segments = 2, example = "flag.nav_chip");
string_id!(
    /// A quest or directed goal.
    QuestId, kind = "quest", min_segments = 2, example = "quest.ice_for_kettle");
string_id!(
    /// A pilot trait.
    TraitId, kind = "trait", min_segments = 2, example = "trait.rich_face");
string_id!(
    /// A journey beat.
    BeatId, kind = "beat", min_segments = 2, example = "beat.first_delivery");
string_id!(
    /// An asset (sprite, mesh recipe, sound) by ID.
    AssetId, kind = "asset", min_segments = 2, example = "asset.station.kettle_dock");
string_id!(
    /// Any entity ID when the kind isn't known statically (history `other` subject).
    AnyId, kind = "entity", min_segments = 2, example = "quest.ice_for_kettle");
string_id!(
    /// What kind of history fact this is. Engine events use their event name (`dock`,
    /// `sell`, ...); packs add kinds namespaced by pack, e.g. `gm.kettle.stage_up`.
    FactKind, kind = "fact kind", min_segments = 1, example = "dock");
string_id!(
    /// A content pack: `core` (the authored backbone) or `gm` (the live layer).
    PackId, kind = "pack", min_segments = 1, example = "core");

macro_rules! seq_id {
    ($(#[$meta:meta])* $name:ident, prefix = $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn seq(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}.{}", $prefix, self.0)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }
        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, IdError> {
                s.strip_prefix(concat!($prefix, "."))
                    .and_then(|n| n.parse().ok())
                    .map($name)
                    .ok_or_else(|| IdError { kind: $prefix, value: s.to_owned() })
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = Cow::<'de, str>::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "pattern": concat!("^", $prefix, "\\.[0-9]+$"),
                    "examples": [concat!($prefix, ".42")],
                })
            }
        }
    };
}

seq_id!(
    /// A history fact, assigned in order by the engine.
    FactId, prefix = "fact");
seq_id!(
    /// A recorded cause, assigned in order by the engine.
    CauseId, prefix = "cause");
seq_id!(
    /// A line said on a legibility channel (dock talk, map note, chronicle).
    TalkId, prefix = "talk");

/// A simulation tick. The sim counts ticks; it never reads a clock.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

impl JsonSchema for Tick {
    fn schema_name() -> Cow<'static, str> {
        "Tick".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "minimum": 0,
            "description": "Simulation tick count (30 per second nominal; the sim never reads a clock).",
        })
    }
}

impl Tick {
    pub fn next(self) -> Tick {
        Tick(self.0 + 1)
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax() {
        assert!(is_valid_id("station.kettle_dock", 2));
        assert!(is_valid_id("rock.shallows.ice.03", 2));
        assert!(is_valid_id("gm.sixth_station", 2));
        assert!(is_valid_id("core", 1));
        assert!(!is_valid_id("core", 2));
        assert!(!is_valid_id("Station.kettle", 2));
        assert!(!is_valid_id("station..kettle", 2));
        assert!(!is_valid_id("station.kettle.", 2));
        assert!(!is_valid_id("1station.kettle", 2));
        assert!(!is_valid_id("station.kettle dock", 2));
        assert!(!is_valid_id("", 1));
    }

    #[test]
    fn static_and_shared_compare_equal() {
        const KETTLE: StationId = StationId::from_static("station.kettle_dock");
        let loaded = StationId::new("station.kettle_dock").unwrap();
        assert_eq!(KETTLE, loaded);
        assert!(StationId::new("station.a").unwrap() < StationId::new("station.b").unwrap());
        assert!(StationId::new("bad id").is_err());
    }

    #[test]
    fn serde_round_trip() {
        let id = WareId::new("ware.ice").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"ware.ice\"");
        let back: WareId = serde_json::from_str("\"ware.ice\"").unwrap();
        assert_eq!(back, id);
        assert!(serde_json::from_str::<WareId>("\"Ware Ice\"").is_err());

        let f = FactId(42);
        assert_eq!(serde_json::to_string(&f).unwrap(), "\"fact.42\"");
        assert_eq!(serde_json::from_str::<FactId>("\"fact.42\"").unwrap(), f);
        assert!(serde_json::from_str::<FactId>("\"cause.42\"").is_err());
    }
}
