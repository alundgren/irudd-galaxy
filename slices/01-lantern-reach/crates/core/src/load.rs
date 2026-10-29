//! Building a new world from authored TOML data (`packs/core/data/`).
//!
//! Each file in [`DATA_FILES`] is optional except `start.toml`. Unknown keys,
//! duplicate IDs and dangling references are errors, reported together.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_contract::data::*;
use galaxy_contract::prelude::*;
use galaxy_contract::world::WORLD_FORMAT_VERSION;
use serde::de::DeserializeOwned;

use crate::invariants::{Invariant, ReferencesResolve};
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
    let start: StartFile = read_required(dir, "start.toml")?;

    let mut errors = Vec::new();
    let mut world = WorldState {
        meta: WorldMeta {
            format: WORLD_FORMAT_VERSION,
            seed,
            tick: Tick(0),
        },
        wares: collect(wares.ware, "wares.toml", &mut errors),
        hulls: collect(hulls.hull, "hulls.toml", &mut errors),
        factions: collect(factions.faction, "factions.toml", &mut errors),
        regions: collect(regions.region, "regions.toml", &mut errors),
        lanes: collect(lanes.lane, "lanes.toml", &mut errors),
        rock_types: collect(rocks.rock_type, "rocks.toml", &mut errors),
        fields: collect(rocks.field, "rocks.toml", &mut errors),
        npcs: collect(npcs.npc, "npcs.toml", &mut errors),
        ship_rules,
        ..WorldState::default()
    };

    for def in stations.station {
        build_station(&mut world, def, &mut errors);
    }
    generate_rocks(&mut world, &mut errors);
    place_player(&mut world, start.player, &mut errors);

    if let Err(e) = ReferencesResolve.check(&world) {
        errors.push(e);
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
        let base = l.base_price.unwrap_or(ware.base_price);
        let line = MarketLine {
            stock: l.stock,
            price: l.price.unwrap_or(base),
            base_price: base,
            min_price: l.min_price.unwrap_or(base / 2),
            max_price: l.max_price.unwrap_or(base.saturating_mul(2)),
            production: l.production,
            consumption: l.consumption,
        };
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
        hull: start.hull,
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
        credits: start.credits,
        ship: Some(start.ship),
        ..Player::default()
    };
}
