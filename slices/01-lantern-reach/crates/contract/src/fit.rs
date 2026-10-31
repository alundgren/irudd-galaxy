//! The hullwright's fit model (#47 Phase 3): sockets on hulls, parts that fit them,
//! the power and mass budgets, the hull's parts locker, and parts on order from a
//! friend. Plain data and pure reads; `galaxy-progress` (`hullwright`) builds the
//! service batches and the room's read API on top.
//!
//! - **Parts are upgrades.** Every fittable thing is an
//!   [`UpgradeDef`](crate::progress::UpgradeDef) with a `socket`: control modules sit in
//!   the control rack (`socket.rack`), everything else in a socket that holds one part.
//!   `Player::fittings` stays the one record of what the flagship has fitted; a part's
//!   socket is its def's `socket`, so a hull never holds two parts in one socket.
//! - **Sockets are data** ([`SocketDef`], `ship_parts.toml`); each hull lists the ones
//!   it has, with its power cap and free mass ([`HullFit`], `hulls.toml`).
//! - **Power** caps what runs at once: a fit that would take the flagship over its
//!   hull's cap is refused, so whatever is fitted always runs.
//! - **Mass** past the hull's free allowance multiplies top speed and turn rate down
//!   ([`MassRules`]); [`crate::modifiers::for_ship`] folds it into `speed` and
//!   `turn_rate`, so flight reads it like any other modifier.
//! - **Control modules** use no budget and are never taken off.
//! - **The locker** belongs to the hull: a part taken off goes into the locker of the
//!   ship it came off ([`FitState::lockers`], keyed by ship). The flagship keeps its
//!   ship ID through prestige, so its locker moves with the hull's lineage, as fitted
//!   parts do.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::progress::{Requirement, UpgradeDef};
use crate::world::{Ship, WorldState};

/// The fit tables from the core pack's data. Saved with the world.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FitRules {
    pub sockets: BTreeMap<SocketId, SocketDef>,
    /// The sockets and budgets of each hull that takes parts.
    pub hulls: BTreeMap<HullId, HullFit>,
    /// One wharf per station at most: who fits there and which sockets they work on.
    pub wharves: BTreeMap<StationId, WharfDef>,
    /// Blueprints by the flag that marks them found.
    pub blueprints: BTreeMap<FlagId, BlueprintDef>,
    pub mass: MassRules,
}

impl FitRules {
    /// No sockets: a save from before the fit model (the game backfills it on open).
    pub fn is_empty(&self) -> bool {
        self.sockets.is_empty()
    }
}

/// A socket kind. A hull has at most one of each.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocketDef {
    pub id: SocketId,
    /// "Drill mount".
    pub name: String,
    /// Where it sits on the hull seen from above, for the cradle and the ship's look:
    /// `nose`, `utility` (behind the cockpit), `belly`, `tail` or `bay`.
    pub place: String,
    /// The control rack: holds every control module, uses no budget, and its modules
    /// are never taken off. Any other socket holds one part.
    #[serde(default)]
    pub rack: bool,
}

/// A hull's sockets and budgets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HullFit {
    pub hull: HullId,
    pub sockets: Vec<SocketId>,
    /// Power pips: the most the fitted parts may draw together.
    pub power: u32,
    /// Mass the hull carries without slowing down.
    pub free_mass: u32,
}

/// A wharf: a hullwright at a station and the sockets they work on. Its parts are the
/// upgrades whose `station` is this one (Ready, Make or Friends by their `source`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WharfDef {
    pub station: StationId,
    /// Who fits here.
    pub keeper: NpcId,
    /// "Orrin's cradle".
    pub name: String,
    /// The sockets this wharf fits and removes parts on. Control modules don't need a
    /// wharf: their seller fits them, as before.
    pub sockets: Vec<SocketId>,
}

/// A blueprint found in the world: its flag is set (with a `blueprint_found` fact) the
/// first time `found` holds. Making its part needs it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlueprintDef {
    /// The flag that marks it found (`blueprint.long_drill`).
    pub id: FlagId,
    /// "Long-throw drill blueprint".
    pub name: String,
    pub part: UpgradeId,
    /// Where it turns up, for the log line ("in the Cinder Belt").
    pub found_in: String,
    /// When it's found: activity, history or beats, never time.
    pub found: Requirement,
}

/// How mass past the free allowance slows the hull. Each unit over takes these shares
/// off top speed and turn rate, never below `floor`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MassRules {
    pub speed_per_unit: f64,
    pub turn_per_unit: f64,
    pub floor: f64,
}

impl Default for MassRules {
    fn default() -> Self {
        Self {
            speed_per_unit: 0.04,
            turn_per_unit: 0.06,
            floor: 0.5,
        }
    }
}

/// Where a part comes from: one tab of the wharf at the part's `station`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case", deny_unknown_fields)]
pub enum PartSource {
    /// Bought for its `cost` and fitted.
    #[default]
    Ready,
    /// Made from materials in the hold, once its blueprint is found. No credits.
    Make {
        blueprint: FlagId,
        materials: Vec<Material>,
    },
    /// Ordered for its `cost` from the wharf keeper's friend, who sends it after the
    /// player's next docking at the friend's station; it arrives in the locker.
    Friend { npc: NpcId, station: StationId },
}

impl PartSource {
    pub fn is_ready(&self) -> bool {
        matches!(self, PartSource::Ready)
    }
    /// The tab's name: "Ready", "Make" or "Friends".
    pub fn tab(&self) -> &'static str {
        match self {
            PartSource::Ready => "Ready",
            PartSource::Make { .. } => "Make",
            PartSource::Friend { .. } => "Friends",
        }
    }
}

/// Units of a ware a Make consumes from the hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub ware: WareId,
    pub units: u32,
}

/// The player's lockers and orders. Missing parts load empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FitState {
    /// Each hull's spare parts, by the ship they belong to.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub lockers: BTreeMap<ShipId, BTreeSet<UpgradeId>>,
    /// Parts on order from a friend, by part.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub orders: BTreeMap<UpgradeId, PartOrder>,
}

/// A part on its way from a friend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartOrder {
    pub part: UpgradeId,
    /// The locker it arrives in.
    pub ship: ShipId,
    /// Who sends it, and from where: it's sent after the flagship's next docking there.
    pub from: NpcId,
    pub station: StationId,
    /// Dockings after this fact count (`None`: all of history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<FactId>,
    /// Centicredits paid when it was ordered.
    pub paid: i64,
}

// --- Reads -------------------------------------------------------------------------

/// The flagship.
pub fn flagship(world: &WorldState) -> Option<&Ship> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|id| world.ships.get(id))
}

/// The upgrade, if it's a part or module that fits a socket.
pub fn part<'w>(world: &'w WorldState, id: &UpgradeId) -> Option<&'w UpgradeDef> {
    world
        .progress
        .rules
        .upgrades
        .get(id)
        .filter(|u| u.socket.is_some())
}

/// Whether `socket` is the control rack.
pub fn is_rack(world: &WorldState, socket: &SocketId) -> bool {
    world
        .progress
        .rules
        .fit
        .sockets
        .get(socket)
        .is_some_and(|s| s.rack)
}

/// Whether the upgrade is a control module (or an upgrade from before sockets): it
/// uses no budget and is never taken off.
pub fn is_module(world: &WorldState, id: &UpgradeId) -> bool {
    match world.progress.rules.upgrades.get(id) {
        Some(u) => u.socket.as_ref().is_none_or(|s| is_rack(world, s)),
        None => true,
    }
}

/// A hull's sockets and budgets, if it takes parts.
pub fn hull_fit<'w>(world: &'w WorldState, hull: &HullId) -> Option<&'w HullFit> {
    world.progress.rules.fit.hulls.get(hull)
}

/// The part in a single socket among `fittings`, if any (never a rack module).
pub fn occupant<'f>(
    world: &WorldState,
    fittings: &'f BTreeSet<UpgradeId>,
    socket: &SocketId,
) -> Option<&'f UpgradeId> {
    if is_rack(world, socket) {
        return None;
    }
    fittings
        .iter()
        .find(|u| part(world, u).and_then(|d| d.socket.as_ref()) == Some(socket))
}

/// What a set of fittings draws and weighs against a hull's budgets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub power_used: u32,
    pub power_cap: u32,
    pub mass: u32,
    pub free_mass: u32,
}

impl Budget {
    pub fn over_power(&self) -> bool {
        self.power_used > self.power_cap
    }
    /// Mass past the free allowance.
    pub fn excess_mass(&self) -> u32 {
        self.mass.saturating_sub(self.free_mass)
    }
}

/// `fittings` on `hull`: power drawn and mass carried by its parts (control modules
/// count too, at their own numbers, which the data keeps at zero).
pub fn budget(world: &WorldState, hull: &HullId, fittings: &BTreeSet<UpgradeId>) -> Budget {
    let (cap, free) = hull_fit(world, hull).map_or((0, 0), |h| (h.power, h.free_mass));
    let parts = fittings.iter().filter_map(|u| part(world, u));
    let mut b = Budget {
        power_cap: cap,
        free_mass: free,
        ..Budget::default()
    };
    for p in parts {
        b.power_used = b.power_used.saturating_add(p.power);
        b.mass = b.mass.saturating_add(p.mass);
    }
    b
}

/// The flagship's budget as fitted now.
pub fn flagship_budget(world: &WorldState) -> Option<Budget> {
    let ship = flagship(world)?;
    Some(budget(world, &ship.hull, &world.player.fittings))
}

/// What mass past the free allowance leaves of top speed and turn rate:
/// `(speed, turn)` factors in `floor..=1`.
pub fn mass_factors(world: &WorldState, budget: &Budget) -> (f64, f64) {
    let m = &world.progress.rules.fit.mass;
    let over = f64::from(budget.excess_mass());
    let floor = m.floor.clamp(0.0, 1.0);
    let f = |per: f64| (1.0 - per.max(0.0) * over).clamp(floor, 1.0);
    (f(m.speed_per_unit), f(m.turn_per_unit))
}

/// A ship's locker, oldest part first by ID.
pub fn locker<'w>(world: &'w WorldState, ship: &ShipId) -> impl Iterator<Item = &'w UpgradeId> {
    world
        .progress
        .fit
        .lockers
        .get(ship)
        .into_iter()
        .flat_map(|l| l.iter())
}

/// Whether the player has this part already: fitted, in a locker, or on order.
pub fn owned(world: &WorldState, id: &UpgradeId) -> bool {
    world.player.fittings.contains(id)
        || world.progress.fit.lockers.values().any(|l| l.contains(id))
        || world.progress.fit.orders.contains_key(id)
}

/// A fitted part as the ship wears it, for drawing it in the cradle and in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FittedLook {
    pub socket: SocketId,
    /// The socket's `place` (`nose`, `utility`, `belly`, `tail`, `bay`).
    pub place: String,
    pub part: UpgradeId,
    /// The part's `look` (`lens`, `long_drill`, `pods`, `fins`, `seam_lamp` ...).
    pub look: String,
}

/// What the flagship wears, in socket order: every fitted part with a `look`. Other
/// ships wear nothing (fittings are the flagship's).
pub fn looks(world: &WorldState, ship: &ShipId) -> Vec<FittedLook> {
    if world.player.ship.as_ref() != Some(ship) {
        return Vec::new();
    }
    let sockets = &world.progress.rules.fit.sockets;
    let mut out: Vec<FittedLook> = world
        .player
        .fittings
        .iter()
        .filter_map(|u| {
            let d = part(world, u)?;
            let socket = d.socket.clone()?;
            Some(FittedLook {
                place: sockets.get(&socket)?.place.clone(),
                socket,
                part: u.clone(),
                look: d.look.clone()?,
            })
        })
        .collect();
    out.sort_by(|a, b| a.socket.cmp(&b.socket).then(a.part.cmp(&b.part)));
    out
}

/// One rock the seam lamp reads: what's left in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeamReading {
    pub rock: RockId,
    pub remaining: u32,
    pub initial: u32,
}

/// What a fitted seam lamp shows: each rock in the ship's region within the lamp's
/// range, nearest first. Empty without a lamp.
pub fn seam_readings(world: &WorldState, ship: &ShipId) -> Vec<SeamReading> {
    let Some(range) = crate::modifiers::for_ship(world, ship).seam_lamp else {
        return Vec::new();
    };
    let Some(s) = world.ships.get(ship) else {
        return Vec::new();
    };
    let mut near: Vec<(f32, SeamReading)> = world
        .rocks
        .values()
        .filter(|r| {
            world
                .fields
                .get(&r.field)
                .is_some_and(|f| f.region == s.region)
        })
        .map(|r| (r.position.distance(s.position), r))
        .filter(|(d, _)| *d <= range)
        .map(|(d, r)| {
            (
                d,
                SeamReading {
                    rock: r.id.clone(),
                    remaining: r.remaining,
                    initial: r.initial,
                },
            )
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.rock.cmp(&b.1.rock)));
    near.into_iter().map(|(_, r)| r).collect()
}

/// Which of the flagship's fitted parts can't come across to `to` at prestige: a
/// socket the new hull lacks, then, while the rest would draw more than its power
/// cap, the hungriest (by ID on a tie). They go to the locker instead.
pub fn left_at_prestige(world: &WorldState, to: &HullId) -> BTreeSet<UpgradeId> {
    let mut out = BTreeSet::new();
    let Some(fit) = hull_fit(world, to) else {
        return out;
    };
    let mut kept = BTreeSet::new();
    for u in &world.player.fittings {
        if is_module(world, u) {
            continue;
        }
        let has = part(world, u)
            .and_then(|d| d.socket.as_ref())
            .is_some_and(|s| fit.sockets.contains(s));
        if has {
            kept.insert(u.clone());
        } else {
            out.insert(u.clone());
        }
    }
    let draw = |u: &UpgradeId| part(world, u).map_or(0, |d| d.power);
    let mut used: u32 = kept.iter().map(draw).sum();
    while used > fit.power {
        let Some(top) = kept.iter().max_by_key(|u| (draw(u), (*u).clone())).cloned() else {
            break;
        };
        used -= draw(&top);
        kept.remove(&top);
        out.insert(top);
    }
    out
}
