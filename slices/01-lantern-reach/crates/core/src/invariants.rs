//! Runtime invariants, checked after every command batch. A batch that breaks one is
//! undone and rejected.
//!
//! Add one by implementing [`Invariant`] and registering it with
//! `Sim::add_invariant`. Keep checks cheap: they run once per batch.

use galaxy_contract::prelude::*;

pub trait Invariant {
    fn name(&self) -> &'static str;
    /// `Err` with a short reason if the world breaks the rule.
    fn check(&self, world: &WorldState) -> Result<(), String>;
}

/// The engine's built-in invariants.
pub fn builtin() -> Vec<Box<dyn Invariant>> {
    vec![
        Box::new(KeysMatchIds),
        Box::new(ReferencesResolve),
        Box::new(HoldWithinCapacity),
        Box::new(RockWithinInitial),
        Box::new(PricesInBand),
        Box::new(NumbersFinite),
        Box::new(FitsHold),
    ]
}

/// No NaN or infinite numbers anywhere in the world: saves couldn't hold them.
pub struct NumbersFinite;

impl Invariant for NumbersFinite {
    fn name(&self) -> &'static str {
        "numbers_finite"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        crate::finite::check(w)
    }
}

/// Every record's `id` equals its store key.
pub struct KeysMatchIds;

fn keys_match<V: Record>(store: &Store<V::Id, V>, what: &str) -> Result<(), String>
where
    V::Id: std::fmt::Display,
{
    match store.iter().find(|(k, v)| *k != v.id()) {
        Some((k, v)) => Err(format!("{what} stored under `{k}` has id `{}`", v.id())),
        None => Ok(()),
    }
}

impl Invariant for KeysMatchIds {
    fn name(&self) -> &'static str {
        "keys_match_ids"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        keys_match(&w.regions, "region")?;
        keys_match(&w.lanes, "lane")?;
        keys_match(&w.stations, "station")?;
        keys_match(&w.markets, "market")?;
        keys_match(&w.wares, "ware")?;
        keys_match(&w.hulls, "hull")?;
        keys_match(&w.rock_types, "rock type")?;
        keys_match(&w.fields, "field")?;
        keys_match(&w.rocks, "rock")?;
        keys_match(&w.ships, "ship")?;
        keys_match(&w.factions, "faction")?;
        keys_match(&w.npcs, "npc")?;
        keys_match(&w.rumors, "rumor")?;
        keys_match(&w.station_parts, "station part")
    }
}

/// Every ID a record refers to exists.
pub struct ReferencesResolve;

fn exists<K: Ord + std::fmt::Display, V>(
    store: &Store<K, V>,
    id: &K,
    from: &dyn std::fmt::Display,
) -> Result<(), String> {
    if store.contains(id) {
        Ok(())
    } else {
        Err(format!("{from} refers to missing `{id}`"))
    }
}

fn maybe<K: Ord + std::fmt::Display, V>(
    store: &Store<K, V>,
    id: &Option<K>,
    from: &dyn std::fmt::Display,
) -> Result<(), String> {
    id.as_ref().map_or(Ok(()), |id| exists(store, id, from))
}

impl Invariant for ReferencesResolve {
    fn name(&self) -> &'static str {
        "references_resolve"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        for id in w.station_recipes.keys() {
            exists(&w.stations, id, &"station recipe")?;
        }
        for rock in &w.rock_tags {
            exists(&w.rocks, rock, &"rock lamp")?;
        }
        for (id, o) in &w.map_overlays {
            exists(&w.regions, &o.region, id)?;
            maybe(&w.factions, &o.owner, id)?;
        }
        for (id, b) in &w.survey_buoys {
            exists(&w.regions, &b.region, id)?;
            exists(&w.factions, &b.planted_by, id)?;
            if let Some(reward) = &b.reward {
                exists(&w.npcs, &reward.person, id)?;
                for faction in reward.standing.keys() {
                    exists(&w.factions, faction, id)?;
                }
            }
        }
        for (id, r) in &w.regions {
            maybe(&w.factions, &r.owner, id)?;
        }
        for (id, l) in &w.lanes {
            exists(&w.regions, &l.a, id)?;
            exists(&w.regions, &l.b, id)?;
        }
        for (id, s) in &w.stations {
            exists(&w.regions, &s.region, id)?;
            maybe(&w.factions, &s.owner, id)?;
        }
        for (id, p) in &w.station_parts {
            exists(&w.stations, &p.station, id)?;
            exists(&w.npcs, &p.keeper, id)?;
        }
        for (id, m) in &w.markets {
            exists(&w.stations, id, &"market")?;
            for ware in m.lines.keys() {
                exists(&w.wares, ware, id)?;
            }
        }
        for (id, t) in &w.rock_types {
            exists(&w.wares, &t.ware, id)?;
        }
        for (id, f) in &w.fields {
            exists(&w.regions, &f.region, id)?;
            exists(&w.rock_types, &f.rock_type, id)?;
        }
        for (id, r) in &w.rocks {
            exists(&w.fields, &r.field, id)?;
            exists(&w.rock_types, &r.rock_type, id)?;
        }
        for (id, s) in &w.ships {
            exists(&w.hulls, &s.hull, id)?;
            exists(&w.regions, &s.region, id)?;
            maybe(&w.stations, &s.docked_at, id)?;
            for ware in s.hold.cargo.keys() {
                exists(&w.wares, ware, id)?;
            }
            for c in s.hold.consigned.values() {
                exists(&w.wares, &c.ware, id)?;
                exists(&w.npcs, &c.owner, id)?;
                exists(&w.npcs, &c.to, id)?;
                exists(&w.stations, &c.station, id)?;
            }
            if let Some(t) = &s.activity.transit {
                exists(&w.lanes, &t.lane, id)?;
            }
            if let Some(m) = &s.activity.mining {
                exists(&w.rocks, &m.rock, id)?;
            }
            match &s.owner {
                Owner::Player => {}
                Owner::Npc { npc } => exists(&w.npcs, npc, id)?,
                Owner::Faction { faction } => exists(&w.factions, faction, id)?,
            }
        }
        for (id, n) in &w.npcs {
            maybe(&w.factions, &n.faction, id)?;
            maybe(&w.stations, &n.location, id)?;
        }
        for (id, t) in &w.traders {
            if id != &t.id
                || !w.ships.contains(id)
                || !w.hulls.contains(&t.hull)
                || !w.factions.contains(&t.faction)
                || !t.stations.contains(&t.home)
                || t.stations.len() < 2
                || t.max_units_per_ware == 0
                || t.max_wares_per_docking == 0
                || t.cost_per_distance < 0.0
                || !t.cost_per_distance.is_finite()
                || !t.return_home_bonus.is_finite()
                || t.stations.iter().any(|s| !w.stations.contains(s))
                || t.wares.iter().any(|s| !w.wares.contains(s))
                || t.preferences
                    .iter()
                    .any(|p| !w.stations.contains(&p.station) || !p.bonus.is_finite())
            {
                return Err(format!("invalid trader policy for {id}"));
            }
        }
        maybe(&w.ships, &w.player.ship, &"player")
    }
}

/// No hold carries more than its hull allows, plus its hold modifiers.
pub struct HoldWithinCapacity;

impl Invariant for HoldWithinCapacity {
    fn name(&self) -> &'static str {
        "hold_within_capacity"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        for (id, s) in &w.ships {
            if !w.hulls.contains(&s.hull) {
                continue;
            }
            let capacity = galaxy_contract::modifiers::hold_capacity(w, s);
            if s.hold.total() > capacity {
                return Err(format!("{id} holds {} of {capacity}", s.hold.total()));
            }
        }
        Ok(())
    }
}

/// The fit model holds together: a single socket on the flagship holds one part, a
/// part is in one place at a time (fitted, one locker, or on order), every locker and
/// order belongs to a ship that exists, and the flagship's parts stay within its power
/// cap. The hullwright refuses a fit over the cap while planning; this catches two fits
/// planned on the same starting world in one tick, so the second batch is undone.
pub struct FitsHold;

impl Invariant for FitsHold {
    fn name(&self) -> &'static str {
        "fits_hold"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        use galaxy_contract::fit;
        let mut taken = std::collections::BTreeMap::new();
        for u in &w.player.fittings {
            if fit::is_module(w, u) {
                continue;
            }
            if let Some(socket) = fit::part(w, u).and_then(|d| d.socket.as_ref())
                && let Some(other) = taken.insert(socket, u)
            {
                return Err(format!("{socket} holds both {other} and {u}"));
            }
        }
        let mut seen: std::collections::BTreeSet<&UpgradeId> = w.player.fittings.iter().collect();
        for (ship, locker) in &w.progress.fit.lockers {
            exists(&w.ships, ship, &"locker")?;
            for u in locker {
                if !seen.insert(u) {
                    return Err(format!("{u} is in {ship}'s locker and somewhere else too"));
                }
            }
        }
        for (u, o) in &w.progress.fit.orders {
            exists(&w.ships, &o.ship, u)?;
            if !seen.insert(u) {
                return Err(format!("{u} is on order and the player has it already"));
            }
        }
        if let Some(b) = fit::flagship_budget(w)
            && b.power_used > b.power_cap
        {
            return Err(format!(
                "fitted parts draw {} power; the hull carries {}",
                b.power_used, b.power_cap
            ));
        }
        Ok(())
    }
}

/// Rocks never hold more than they started with.
pub struct RockWithinInitial;

impl Invariant for RockWithinInitial {
    fn name(&self) -> &'static str {
        "rock_within_initial"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        match w.rocks.values().find(|r| r.remaining > r.initial) {
            Some(r) => Err(format!("{} has {} of {}", r.id, r.remaining, r.initial)),
            None => Ok(()),
        }
    }
}

/// Market prices stay inside their bands.
pub struct PricesInBand;

impl Invariant for PricesInBand {
    fn name(&self) -> &'static str {
        "prices_in_band"
    }
    fn check(&self, w: &WorldState) -> Result<(), String> {
        for (station, m) in &w.markets {
            for (ware, l) in &m.lines {
                if l.min_price > l.max_price || l.price < l.min_price || l.price > l.max_price {
                    return Err(format!(
                        "{ware} at {station}: price {} outside {}..={}",
                        l.price, l.min_price, l.max_price
                    ));
                }
            }
        }
        Ok(())
    }
}
