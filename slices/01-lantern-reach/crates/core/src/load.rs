//! Building a new world from authored TOML data (`packs/core/data/`).
//!
//! Each file in [`DATA_FILES`] is optional except `start.toml`. Unknown keys,
//! duplicate IDs and dangling references are errors, reported together.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_contract::data::*;
use galaxy_contract::money::cents_from_credits;
use galaxy_contract::prelude::*;
use galaxy_contract::world::WORLD_FORMAT_VERSION;
use serde::de::DeserializeOwned;

use crate::invariants::{Invariant, NumbersFinite, ReferencesResolve};
use crate::rng::SimRng;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("{file}: {message}")]
    Read { file: String, message: String },
    #[error("invalid data:\n  {}", .0.join("\n  "))]
    Invalid(Vec<String>),
}

fn read<T: DeserializeOwned + Default>(dir: &Path, file: &str) -> Result<T, DataError> {
    let path = dir.join(file);
    if !path.exists() {
        return Ok(T::default());
    }
    read_required(dir, file)
}

fn read_required<T: DeserializeOwned>(dir: &Path, file: &str) -> Result<T, DataError> {
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| DataError::Read {
        file: file.to_owned(),
        message: e.to_string(),
    })?;
    toml::from_str(&text).map_err(|e| DataError::Read {
        file: file.to_owned(),
        message: e.to_string(),
    })
}

/// Decimal credits from a data file as centicredits, or an error: not finite, finer than
/// a centicredit, or outside `0..=max`.
pub(crate) fn cents(credits: f64, max: i64, what: &str, errors: &mut Vec<String>) -> i64 {
    match cents_from_credits(credits).filter(|c| (0..=max).contains(c)) {
        Some(c) => c,
        None => {
            errors.push(format!(
                "{what}: {credits} isn't a whole number of centicredits in 0..={}",
                show_credits(max)
            ));
            0
        }
    }
}

/// A price in centicredits.
fn price(credits: f64, what: &str, errors: &mut Vec<String>) -> u32 {
    cents(credits, i64::from(u32::MAX), what, errors) as u32
}

fn ware(def: WareDef, errors: &mut Vec<String>) -> Ware {
    Ware {
        base_price: price(
            def.base_price,
            &format!("wares.toml: {} base_price", def.id),
            errors,
        ),
        id: def.id,
        name: def.name,
        tradeable: def.tradeable,
    }
}

fn collect<V: Record + Clone>(
    items: Vec<V>,
    file: &str,
    errors: &mut Vec<String>,
) -> Store<V::Id, V>
where
    V::Id: std::fmt::Display,
{
    let mut store = Store::new();
    for item in items {
        if let Some(old) = store.put(item) {
            errors.push(format!("{file}: duplicate id `{}`", old.id()));
        }
    }
    store
}

/// The cause name of [`craft_backfill`]'s batches.
pub const CRAFT_BACKFILL: &str = "craft_backfill";

/// Batches that add the named traders `dir` defines to a world that has none of them
/// yet (a save made before `craft.toml`): one per craft, each putting its hull (if the
/// world lacks it), its ship at home with orders to dock there, and its route policy,
/// caused by the system [`CRAFT_BACKFILL`]. A craft whose ship or policy the world
/// already has is left alone, and so is one whose faction, stations or wares the world
/// doesn't know. The result depends only on `dir` and the world, so it repeats exactly.
pub fn craft_backfill(dir: &Path, world: &WorldState) -> Result<Vec<CommandBatch>, DataError> {
    let fresh = load_world(dir, world.meta.seed)?;
    let mut batches = Vec::new();
    for (id, def) in &fresh.traders {
        if world.traders.contains_key(id) || world.ships.contains(id) {
            continue;
        }
        let known = world.factions.contains(&def.faction)
            && def
                .stations
                .iter()
                .chain(def.preferences.iter().map(|p| &p.station))
                .all(|s| world.stations.contains(s))
            && def.wares.iter().all(|w| world.wares.contains(w));
        let (Some(ship), Some(hull), true) =
            (fresh.ships.get(id), fresh.hulls.get(&def.hull), known)
        else {
            continue;
        };
        let mut batch = CommandBatch::new(Cause::system(
            CRAFT_BACKFILL,
            format!("{} joins a save made before named traders", def.name),
        ));
        if !world.hulls.contains(&def.hull) {
            batch = batch.with(Command::PutHull {
                id: def.hull.clone(),
                value: Some(Box::new(hull.clone())),
            });
        }
        batches.push(
            batch
                .with(Command::PutShip {
                    id: id.clone(),
                    value: Some(Box::new(ship.clone())),
                })
                .with(Command::PutTrader {
                    id: id.clone(),
                    value: Some(Box::new(def.clone())),
                }),
        );
    }
    Ok(batches)
}

/// The cause name of [`pacing_backfill`]'s batch.
pub const PACING_BACKFILL: &str = "pacing_backfill";

/// A batch that adds the tunables `dir`'s `pacing.toml` defines and the world lacks (a
/// save made before they were authored), each at its authored value, caused by the
/// system [`PACING_BACKFILL`]. A tunable the world has, tuned or not, is left alone, so
/// saved overrides stay. `None` when nothing is missing. The result depends only on
/// `dir` and the world, so it repeats exactly.
pub fn pacing_backfill(dir: &Path, world: &WorldState) -> Result<Option<CommandBatch>, DataError> {
    let fresh = load_world(dir, world.meta.seed)?;
    let mut batch = CommandBatch::new(Cause::system(
        PACING_BACKFILL,
        "pacing tunables a save made before them lacks",
    ));
    for (id, tunable) in &fresh.progress.tunables {
        if !world.progress.tunables.contains_key(id) {
            batch = batch.with(Command::SetTunable {
                id: id.clone(),
                value: Some(Box::new(tunable.clone())),
            });
        }
    }
    Ok((!batch.commands.is_empty()).then_some(batch))
}

/// Reads `dir` and builds the starting world for `seed`.
pub fn load_world(dir: &Path, seed: u64) -> Result<WorldState, DataError> {
    let wares: WaresFile = read(dir, "wares.toml")?;
    let hulls: HullsFile = read(dir, "hulls.toml")?;
    let factions: FactionsFile = read(dir, "factions.toml")?;
    let regions: RegionsFile = read(dir, "regions.toml")?;
    let lanes: LanesFile = read(dir, "lanes.toml")?;
    let stations: StationsFile = read(dir, "stations.toml")?;
    let rocks: RocksFile = read(dir, "rocks.toml")?;
    let npcs: NpcsFile = read(dir, "npcs.toml")?;
    let ship_rules: ShipRules = read(dir, "ship_rules.toml")?;
    let economy: EconomyRules = read(dir, "economy.toml")?;
    let controls: ControlsFile = read(dir, "controls.toml")?;
    let upgrades: UpgradesFile = read(dir, "upgrades.toml")?;
    let traits: TraitsFile = read(dir, "traits.toml")?;
    let pacing: PacingFile = read(dir, "pacing.toml")?;
    let start: StartFile = read_required(dir, "start.toml")?;
    let craft: CraftFile = read(dir, "craft.toml")?;
    let ranks = hulls.rank;
    let (standing, regard, deeds) = (factions.standing, factions.regard, factions.deed);

    let mut errors = Vec::new();
    let mut world = WorldState {
        meta: WorldMeta {
            format: WORLD_FORMAT_VERSION,
            seed,
            tick: Tick(0),
        },
        wares: {
            let wares = wares
                .ware
                .into_iter()
                .map(|w| ware(w, &mut errors))
                .collect();
            collect(wares, "wares.toml", &mut errors)
        },
        hulls: collect(hulls.hull, "hulls.toml", &mut errors),
        factions: collect(factions.faction, "factions.toml", &mut errors),
        regions: collect(regions.region, "regions.toml", &mut errors),
        lanes: collect(lanes.lane, "lanes.toml", &mut errors),
        rock_types: collect(rocks.rock_type, "rocks.toml", &mut errors),
        fields: collect(rocks.field, "rocks.toml", &mut errors),
        npcs: collect(npcs.npc, "npcs.toml", &mut errors),
        ship_rules,
        economy: EconomyState {
            rules: economy,
            ..EconomyState::default()
        },
        ..WorldState::default()
    };

    for def in stations.station {
        build_station(&mut world, def, &mut errors);
    }
    generate_rocks(&mut world, &mut errors);
    place_player(&mut world, start.player, &mut errors);
    for def in craft.craft {
        let Some(home) = world.stations.get(&def.home) else {
            errors.push(format!(
                "craft.toml: {} has unknown home {}",
                def.id, def.home
            ));
            continue;
        };
        let ship = Ship {
            id: def.id.clone(),
            name: def.name.clone(),
            hull: def.hull.clone(),
            owner: Owner::Faction {
                faction: def.faction.clone(),
            },
            region: home.region.clone(),
            position: home.position,
            velocity: Vec2::ZERO,
            heading: 0.0,
            // The runner performs the first docking, opening the first menu.
            docked_at: None,
            hold: Hold::default(),
            orders: OrderQueue {
                orders: vec![Order::Dock {
                    station: def.home.clone(),
                }],
                ..OrderQueue::default()
            },
            activity: ShipActivity::default(),
        };
        if world.ships.contains(&def.id) || world.traders.contains_key(&def.id) {
            errors.push(format!("craft.toml: duplicate ship {}", def.id));
            continue;
        }
        world.ships.put(ship);
        world.traders.insert(def.id.clone(), def);
    }
    crate::load_progress::build(
        &mut world,
        crate::load_progress::Files {
            controls,
            upgrades,
            traits,
            pacing,
            ranks,
            standing,
            regard,
            deeds,
        },
        &mut errors,
    );

    if let Err(e) = ReferencesResolve.check(&world) {
        errors.push(e);
    }
    if let Err(e) = NumbersFinite.check(&world) {
        errors.push(format!("numbers must be finite: {e}"));
    }
    for (id, h) in &world.hulls {
        if h.hold_capacity == 0 {
            errors.push(format!("hulls.toml: {id} has no hold"));
        }
    }
    if errors.is_empty() {
        Ok(world)
    } else {
        Err(DataError::Invalid(errors))
    }
}

fn build_station(world: &mut WorldState, def: StationDef, errors: &mut Vec<String>) {
    let mut lines = BTreeMap::new();
    for l in def.market {
        let Some(ware) = world.wares.get(&l.ware) else {
            errors.push(format!(
                "stations.toml: {} sells unknown `{}`",
                def.id, l.ware
            ));
            continue;
        };
        let what = |field: &str| format!("stations.toml: {} {} {field}", def.id, l.ware);
        let base = l
            .base_price
            .map_or(ware.base_price, |p| price(p, &what("base_price"), errors));
        let mut price_or = |p: Option<f64>, field: &str, default: u32| {
            p.map_or(default, |p| price(p, &what(field), errors))
        };
        let line = MarketLine {
            stock: l.stock,
            price: price_or(l.price, "price", base),
            base_price: base,
            min_price: price_or(l.min_price, "min_price", base / 2),
            max_price: price_or(l.max_price, "max_price", base.saturating_mul(2)),
            production: l.production,
            consumption: l.consumption,
            role: l.role.unwrap_or(if l.production > 0 {
                MarketRole::Sells
            } else {
                MarketRole::Buys
            }),
            target: l.target,
            sat_per_unit: l.sat_per_unit,
            sat_recover: l.sat_recover,
            needs: l.needs,
            ..MarketLine::default()
        };
        if let Some(missing) = line.needs.iter().find(|w| !world.wares.contains(*w)) {
            errors.push(format!(
                "stations.toml: {} {} needs unknown `{missing}`",
                def.id, l.ware
            ));
        }
        if lines.insert(l.ware.clone(), line).is_some() {
            errors.push(format!(
                "stations.toml: {} lists `{}` twice",
                def.id, l.ware
            ));
        }
    }
    if !lines.is_empty() {
        world.markets.put(Market {
            station: def.id.clone(),
            lines,
        });
    }
    let station = Station {
        id: def.id,
        name: def.name,
        region: def.region,
        position: def.position,
        dock_radius: def.dock_radius,
        owner: def.owner,
        services: def.services,
        build: BuildState {
            stage: def.stage,
            parts: def.parts,
        },
    };
    if let Some(old) = world.stations.put(station) {
        errors.push(format!("stations.toml: duplicate id `{}`", old.id));
    }
}

/// Rocks come from fields with stable IDs `<rock_prefix>.NN`. Their layout is keyed by
/// the field ID, not the world seed, so every new game has the same authored layout.
fn generate_rocks(world: &mut WorldState, errors: &mut Vec<String>) {
    for field in world.fields.values() {
        if field.amount_min > field.amount_max || field.amount_max == 0 {
            errors.push(format!("rocks.toml: {} has a bad amount range", field.id));
            continue;
        }
        let mut rng = SimRng::for_key(0, field.id.as_str());
        for i in 1..=field.count {
            let id = match RockId::new(format!("{}.{i:02}", field.rock_prefix)) {
                Ok(id) => id,
                Err(e) => {
                    errors.push(format!("rocks.toml: {}: {e}", field.id));
                    break;
                }
            };
            let r = field.radius * rng.unit_f32().sqrt();
            let a = rng.range_f32(0.0, std::f32::consts::TAU);
            let amount = rng.range_u32(field.amount_min, field.amount_max);
            let rock = Rock {
                id,
                field: field.id.clone(),
                rock_type: field.rock_type.clone(),
                position: field.center + Vec2::new(r * a.cos(), r * a.sin()),
                remaining: amount,
                initial: amount,
                growth: RockGrowth::default(),
            };
            if let Some(old) = world.rocks.put(rock) {
                errors.push(format!("rocks.toml: rock id `{}` generated twice", old.id));
            }
        }
    }
}

fn place_player(world: &mut WorldState, start: StartDef, errors: &mut Vec<String>) {
    let Some(station) = world.stations.get(&start.station) else {
        errors.push(format!("start.toml: unknown station `{}`", start.station));
        return;
    };
    let ship = Ship {
        id: start.ship.clone(),
        name: start.ship_name,
        hull: start.hull.clone(),
        owner: Owner::Player,
        region: station.region.clone(),
        position: station.position,
        velocity: Vec2::ZERO,
        heading: 0.0,
        docked_at: Some(station.id.clone()),
        hold: Hold::default(),
        orders: OrderQueue::default(),
        activity: ShipActivity::default(),
    };
    world.ships.put(ship);
    world.player = Player {
        name: start.name,
        credits: cents(start.credits, i64::MAX / 2, "start.toml: credits", errors),
        ship: Some(start.ship),
        hull_records: BTreeMap::from([(start.hull, HullRecord::default())]),
        ..Player::default()
    };
}
